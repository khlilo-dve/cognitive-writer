//! Evidence-anchored representation views and deterministic per-axis overlap.
use crate::error::AppError;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

pub(crate) const DIMENSIONS: [&str; 9] = [
    "language",
    "voice",
    "rhythm",
    "information",
    "narration",
    "constraints",
    "title",
    "opening",
    "ending",
];
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Feature {
    pub id: String,
    pub dimension: String,
    pub rule: String,
    pub condition: String,
    pub quote: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct View {
    pub features: Vec<Feature>,
    pub uncertainties: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Link {
    pub left: String,
    pub right: String,
    pub reason: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Comparison {
    pub matches: Vec<Link>,
    pub conflicts: Vec<Link>,
    pub uncertain_left: Vec<String>,
    pub uncertain_right: Vec<String>,
    pub notes: Vec<String>,
}

pub(crate) fn parse_view(raw: &str, spec: &str) -> Result<View, AppError> {
    let v: View =
        serde_json::from_str(raw).map_err(|e| AppError::Parse(format!("比较视图JSON: {e}")))?;
    if v.features.is_empty() || v.features.len() > 40 {
        return Err(AppError::Parse("特征数须为1–40，不填充缺失维度".into()));
    }
    let mut ids = HashSet::new();
    for f in &v.features {
        if f.id.is_empty()
            || !ids.insert(&f.id)
            || !DIMENSIONS.contains(&f.dimension.as_str())
            || f.rule.trim().is_empty()
            || f.condition.trim().is_empty()
            || f.quote.chars().count() < 6
            || !spec.contains(&f.quote)
        {
            return Err(AppError::Parse(format!(
                "特征{}重复/维度不合法/缺内容/quote无法逐字定位",
                f.id
            )));
        }
    }
    Ok(v)
}

pub(crate) fn parse_comparison(raw: &str, a: &View, b: &View) -> Result<Comparison, AppError> {
    let c: Comparison =
        serde_json::from_str(raw).map_err(|e| AppError::Parse(format!("比较JSON: {e}")))?;
    let left: HashMap<_, _> = a.features.iter().map(|f| (f.id.as_str(), f)).collect();
    let right: HashMap<_, _> = b.features.iter().map(|f| (f.id.as_str(), f)).collect();
    let mut used_a = HashSet::new();
    let mut used_b = HashSet::new();
    for p in c.matches.iter().chain(&c.conflicts) {
        let fa = left
            .get(p.left.as_str())
            .ok_or_else(|| AppError::Parse("A侧id不存在".into()))?;
        let fb = right
            .get(p.right.as_str())
            .ok_or_else(|| AppError::Parse("B侧id不存在".into()))?;
        if fa.dimension != fb.dimension
            || !used_a.insert(p.left.as_str())
            || !used_b.insert(p.right.as_str())
            || p.reason.trim().is_empty()
        {
            return Err(AppError::Parse("关系跨维度、重复计数或缺少解释".into()));
        }
    }
    for id in &c.uncertain_left {
        if !left.contains_key(id.as_str()) || !used_a.insert(id.as_str()) {
            return Err(AppError::Parse("A侧未知项无效或重复".into()));
        }
    }
    for id in &c.uncertain_right {
        if !right.contains_key(id.as_str()) || !used_b.insert(id.as_str()) {
            return Err(AppError::Parse("B侧未知项无效或重复".into()));
        }
    }
    Ok(c)
}

pub(crate) fn metrics(a: &View, b: &View, c: &Comparison) -> Vec<Value> {
    DIMENSIONS.iter().map(|d|{
        let aa:HashSet<_>=a.features.iter().filter(|f|f.dimension==*d).map(|f|f.id.as_str()).collect();
        let bb:HashSet<_>=b.features.iter().filter(|f|f.dimension==*d).map(|f|f.id.as_str()).collect();
        let n=c.matches.iter().filter(|p|aa.contains(p.left.as_str())).count();
        let conflicts=c.conflicts.iter().filter(|p|aa.contains(p.left.as_str())).count();
        let denominator=aa.len()+bb.len()-n;
        json!({"dimension":d,"left_count":aa.len(),"right_count":bb.len(),"matched":n,"conflicts":conflicts,
            "overlap":if denominator==0 {None} else {Some(n as f64/denominator as f64)},
            "coverage":if aa.is_empty()&&bb.is_empty(){"neither"}else if aa.is_empty()||bb.is_empty(){"one_sided"}else{"both"},
            "uncertain_left":c.uncertain_left.iter().filter(|id|aa.contains(id.as_str())).count(),
            "uncertain_right":c.uncertain_right.iter().filter(|id|bb.contains(id.as_str())).count()})
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn view() -> View {
        View {
            features: vec![Feature {
                id: "f1".into(),
                dimension: "voice".into(),
                rule: "保持距离".into(),
                condition: "未限定".into(),
                quote: "保持平静的叙述距离".into(),
            }],
            uncertainties: vec![],
        }
    }
    #[test]
    fn unknown_dimensions_are_not_perfect_matches_and_conflicts_stay_separate() {
        let a = view();
        let c = Comparison {
            matches: vec![],
            conflicts: vec![Link {
                left: "f1".into(),
                right: "f1".into(),
                reason: "相同条件反向".into(),
            }],
            uncertain_left: vec![],
            uncertain_right: vec![],
            notes: vec![],
        };
        let c = parse_comparison(&serde_json::to_string(&c).unwrap(), &a, &a).unwrap();
        let m = metrics(&a, &a, &c);
        assert!(m.iter().find(|r| r["dimension"] == "title").unwrap()["overlap"].is_null());
        let v = m.iter().find(|r| r["dimension"] == "voice").unwrap();
        assert_eq!(v["conflicts"], 1);
        assert_eq!(v["overlap"], 0.0);
    }
    #[test]
    fn fabricated_quotes_and_many_to_one_matches_are_rejected() {
        let a = view();
        assert!(parse_view(&serde_json::to_string(&a).unwrap(), "不同原文").is_err());
        parse_view(&serde_json::to_string(&a).unwrap(), "保持平静的叙述距离").unwrap();
        let c = Comparison {
            matches: vec![
                Link {
                    left: "f1".into(),
                    right: "f1".into(),
                    reason: "同义".into(),
                },
                Link {
                    left: "f1".into(),
                    right: "f1".into(),
                    reason: "重复".into(),
                },
            ],
            conflicts: vec![],
            uncertain_left: vec![],
            uncertain_right: vec![],
            notes: vec![],
        };
        assert!(parse_comparison(&serde_json::to_string(&c).unwrap(), &a, &a).is_err());
    }
}
