//! Durable micro-CNY accounting. Uncertain requests continue consuming their reservation.
use crate::{error::AppError, eval_store::write_json};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct Charge {
    pub run_id: String,
    pub phase: String,
    pub reserved_micro: u64,
    pub actual_micro: Option<u64>,
    pub state: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct Ledger {
    pub entries: BTreeMap<String, Charge>,
}

pub(crate) struct Budget {
    path: PathBuf,
    pub ledger: Ledger,
    total_limit: u64,
    run_limit: u64,
    local_limit: Option<u64>,
    run_id: String,
    phase: String,
}

pub(crate) fn rates(model: &str) -> Result<(u64, u64), AppError> {
    match model {
        "mimo-v2.6-pro" => Ok((3, 6)),
        "mimo-v2.6-flash" => Ok((1, 2)),
        _ => Err(AppError::Parse("研究模型无已核定费率".into())),
    }
}

pub(crate) fn cost(model: &str, input: u64, output: u64) -> Result<u64, AppError> {
    let (i, o) = rates(model)?;
    input
        .checked_mul(i)
        .and_then(|v| output.checked_mul(o).and_then(|w| v.checked_add(w)))
        .ok_or_else(|| AppError::Parse("费用整数溢出".into()))
}

impl Budget {
    pub fn open(
        path: PathBuf,
        run_id: &str,
        total_limit: u64,
        run_limit: u64,
    ) -> Result<Self, AppError> {
        let ledger = match std::fs::read(&path) {
            Ok(b) => {
                serde_json::from_slice(&b).map_err(|_| AppError::Parse("费用账本损坏".into()))?
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ledger::default(),
            Err(e) => return Err(e.into()),
        };
        Ok(Self {
            path,
            ledger,
            total_limit,
            run_limit,
            local_limit: None,
            run_id: run_id.into(),
            phase: if run_limit < total_limit {
                "pilot"
            } else {
                "full"
            }
            .into(),
        })
    }

    pub fn occupied(&self, run_only: bool) -> u64 {
        self.ledger
            .entries
            .values()
            .filter(|c| !run_only || c.run_id == self.run_id)
            .map(|c| c.actual_micro.unwrap_or(c.reserved_micro))
            .fold(0u64, u64::saturating_add)
    }

    pub fn reserve(&mut self, id: &str, amount: u64) -> Result<(), AppError> {
        if self.ledger.entries.contains_key(id) {
            return Err(AppError::Parse(
                "请求已有费用记录但无可复用响应，保留预留并停止；先核验中断结果".into(),
            ));
        }
        let phase_used = self.phase_occupied();
        if amount > self.total_limit.saturating_sub(self.occupied(false))
            || amount > self.run_limit.saturating_sub(phase_used)
            || self
                .local_limit
                .is_some_and(|limit| amount > limit.saturating_sub(self.occupied(true)))
        {
            return Err(AppError::Parse("预算不足，未发送请求".into()));
        }
        let mut next = self.ledger.clone();
        next.entries.insert(
            id.into(),
            Charge {
                run_id: self.run_id.clone(),
                phase: self.phase.clone(),
                reserved_micro: amount,
                actual_micro: None,
                state: "reserved_or_uncertain".into(),
            },
        );
        write_json(&self.path, &next)?;
        self.ledger = next;
        Ok(())
    }

    pub fn limit_run(&mut self, amount: u64) -> Result<(), AppError> {
        if amount == 0 || amount > self.total_limit || self.occupied(true) > amount {
            return Err(AppError::Parse("单次运行预算上限无效或已超出".into()));
        }
        self.local_limit = Some(amount);
        Ok(())
    }

    /// Recovery is bounded; every uncertain attempt remains charged at its reserve.
    pub fn reserve_attempt(&mut self, logical_id: &str, amount: u64) -> Result<String, AppError> {
        for attempt in 1..=3 {
            let id = if attempt == 1 {
                logical_id.to_owned()
            } else {
                format!("{logical_id}/attempt-{attempt}")
            };
            if let Some(prior) = self.ledger.entries.get(&id) {
                if prior.actual_micro.is_some() {
                    return Err(AppError::Parse(
                        "已结算请求缺少响应文件，拒绝重新采样".into(),
                    ));
                }
                continue;
            }
            self.reserve(&id, amount)?;
            return Ok(id);
        }
        Err(AppError::Parse(
            "请求已达到三次传输尝试上限，未知费用仍保留".into(),
        ))
    }

    fn phase_occupied(&self) -> u64 {
        self.ledger
            .entries
            .values()
            .filter(|c| c.phase == self.phase)
            .map(|c| c.actual_micro.unwrap_or(c.reserved_micro))
            .fold(0u64, u64::saturating_add)
    }

    pub fn settle(&mut self, id: &str, actual: u64) -> Result<(), AppError> {
        let mut next = self.ledger.clone();
        let c = next
            .entries
            .get_mut(id)
            .ok_or_else(|| AppError::Parse("响应缺少费用预留".into()))?;
        if let Some(prior) = c.actual_micro {
            if prior != actual {
                return Err(AppError::Parse("费用结算与既有记录冲突".into()));
            }
        }
        c.actual_micro = Some(actual);
        c.state = "settled".into();
        write_json(&self.path, &next)?;
        self.ledger = next;
        if self.occupied(false) > self.total_limit
            || self.phase_occupied() > self.run_limit
            || self
                .local_limit
                .is_some_and(|limit| self.occupied(true) > limit)
        {
            return Err(AppError::Parse(
                "实际用量超出预留预算，已记账并停止后续请求".into(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_run_cap_counts_uncertain_and_settled_costs() {
        let d = tempfile::tempdir().unwrap();
        let mut b = Budget::open(d.path().join("ledger.json"), "pilot", 100, 100).unwrap();
        b.limit_run(30).unwrap();
        b.reserve("a", 20).unwrap();
        assert!(b.reserve("b", 11).is_err());
        b.reserve("b", 10).unwrap();
        b.settle("a", 5).unwrap();
        b.reserve("c", 15).unwrap();
        assert_eq!(b.occupied(true), 30);
        assert!(b.reserve("d", 1).is_err());
    }
    #[test]
    fn recovery_preserves_unknown_cost_and_is_bounded() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("ledger.json");
        let mut b = Budget::open(p.clone(), "run", 100, 100).unwrap();
        assert_eq!(b.reserve_attempt("run/task", 20).unwrap(), "run/task");
        let mut b = Budget::open(p, "run", 100, 100).unwrap();
        assert_eq!(
            b.reserve_attempt("run/task", 20).unwrap(),
            "run/task/attempt-2"
        );
        assert_eq!(b.occupied(false), 40);
        assert!(b.reserve_attempt("run/task", 70).is_err());
        b.reserve_attempt("run/task", 20).unwrap();
        assert!(b.reserve_attempt("run/task", 1).is_err());
        b.settle("run/task/attempt-2", 5).unwrap();
        assert_eq!(b.occupied(false), 45);
        assert!(b.reserve_attempt("run/task", 1).is_err());
    }
    #[test]
    fn uncertain_reservations_survive_and_limit_other_runs() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("ledger.json");
        let mut b = Budget::open(p.clone(), "one", 100, 80).unwrap();
        b.reserve("a", 70).unwrap();
        assert!(b.reserve("b", 11).is_err());
        let mut second = Budget::open(p.clone(), "two", 100, 100).unwrap();
        assert!(second.reserve("a", 1).is_err());
        assert!(second.reserve("c", 31).is_err());
        second.settle("a", 40).unwrap();
        second.reserve("c", 60).unwrap();
        assert_eq!(
            Budget::open(p, "three", 100, 100).unwrap().occupied(false),
            100
        );
    }
    #[test]
    fn billing_counts_output_once_and_refuses_overflow_or_unknown_model() {
        assert_eq!(cost("mimo-v2.6-pro", 100, 200).unwrap(), 1500);
        assert!(cost("unknown", 1, 1).is_err());
        assert!(cost("mimo-v2.6-pro", u64::MAX, 1).is_err());
    }
    #[test]
    fn pilot_limit_is_shared_across_run_ids() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("ledger.json");
        let mut one = Budget::open(p.clone(), "pilot-one", 100, 5).unwrap();
        one.reserve("a", 4).unwrap();
        one.settle("a", 3).unwrap();
        let mut two = Budget::open(p, "pilot-two", 100, 5).unwrap();
        assert!(two.reserve("b", 3).is_err());
        two.reserve("b", 2).unwrap();
        assert!(two.settle("b", 3).is_err());
        assert_eq!(two.occupied(false), 6);
    }
}
