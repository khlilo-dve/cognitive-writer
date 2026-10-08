//! Research requests share the existing HTTP transport, with explicit roles and durable costs.
use crate::{
    error::AppError,
    eval_store::write_json,
    llm::{post_chat, request_body},
    research_budget::{cost, Budget},
    research_dataset::{sha256, valid_id},
    research_receipt::{charge, parse_receipt, verify, Checkpoint},
};
use serde_json::{json, Value};
use std::{
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
};
use tokio::sync::{Mutex, Semaphore};

#[derive(Clone, Copy)]
pub(crate) enum Role {
    Analysis,
    Structured,
    FlashAnalysis,
    CandidateAnalysis,
    Writer,
}
impl Role {
    pub(crate) fn model(self) -> &'static str {
        match self {
            Self::Analysis | Self::Structured | Self::CandidateAnalysis => "mimo-v2.6-pro",
            Self::Writer | Self::FlashAnalysis => "mimo-v2.6-flash",
        }
    }
    fn max_output(self) -> u64 {
        match self {
            Self::Analysis | Self::Structured | Self::FlashAnalysis => 16384,
            Self::Writer => 4096,
            Self::CandidateAnalysis => 32768,
        }
    }
    fn request(self, system: &str, user: &str, json_mode: bool) -> Result<Value, AppError> {
        let format = json_mode.then(|| json!({"type":"json_object"}));
        let mut body = request_body(self.model(), system, user, format.as_ref())?;
        body["max_completion_tokens"] = json!(self.max_output());
        body["thinking"] =
            json!({"type":if matches!(self,Self::Analysis|Self::CandidateAnalysis){"enabled"}else{"disabled"}});
        Ok(body)
    }
}

pub(crate) struct ResearchStore {
    pub root: PathBuf,
    run_id: String,
    base: String,
    key: String,
    client: reqwest::Client,
    budget: Mutex<Budget>,
    permits: Semaphore,
    _lock: File,
}

impl ResearchStore {
    pub fn open(common: &Path, run_id: &str, pilot: bool) -> Result<Self, AppError> {
        if !valid_id(run_id) {
            return Err(AppError::Parse("run_id无效".into()));
        }
        let approval: Value = serde_json::from_slice(&std::fs::read(common.join("approval.json"))?)
            .map_err(|_| AppError::Parse("预算批准记录无效".into()))?;
        let total = approval["total_limit_cny"]
            .as_u64()
            .filter(|n| *n > 0 && *n <= 100)
            .ok_or_else(|| AppError::Parse("缺少已批准的研究总额".into()))?
            * 1_000_000;
        let run_limit = if pilot {
            approval["pilot_limit_cny"]
                .as_u64()
                .filter(|n| *n > 0 && *n <= 5)
                .ok_or_else(|| AppError::Parse("试跑预算无效".into()))?
                * 1_000_000
        } else {
            total
        };
        let base = std::env::var("BASE_URL").map_err(|_| AppError::EnvVar("BASE_URL".into()))?;
        let url = reqwest::Url::parse(&base).map_err(|_| AppError::Parse("模型端点无效".into()))?;
        if url.scheme() != "https"
            || url.host_str() != Some("api.xiaomimimo.com")
            || url.path().trim_end_matches('/') != "/v1"
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(AppError::Parse(
                "当前模型渠道未匹配已核价官方按量端点，未发送请求".into(),
            ));
        }
        let key = std::env::var("API_KEY").map_err(|_| AppError::EnvVar("API_KEY".into()))?;
        if key.trim().is_empty() || key.starts_with("tp-") || key.starts_with("ttp-") {
            return Err(AppError::Parse(
                "研究需要普通按量API凭证，不能使用Token Plan凭证".into(),
            ));
        }
        Self::build(common, run_id, total, run_limit, base, key)
    }

    fn build(
        common: &Path,
        run_id: &str,
        total: u64,
        run_limit: u64,
        base: String,
        key: String,
    ) -> Result<Self, AppError> {
        std::fs::create_dir_all(common)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(common.join(".execution.lock"))?;
        lock.try_lock()
            .map_err(|_| AppError::Parse("另一个研究进程正在运行，拒绝并发覆盖账本".into()))?;
        let root = common.join("runs").join(run_id);
        std::fs::create_dir_all(root.join("requests"))?;
        let budget = Budget::open(common.join("usage-ledger.json"), run_id, total, run_limit)?;
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(600))
            .build()
            .map_err(|_| AppError::Network("无法建立研究客户端".into()))?;
        Ok(Self {
            root,
            run_id: run_id.into(),
            base,
            key,
            client,
            budget: Mutex::new(budget),
            permits: Semaphore::new(3),
            _lock: lock,
        })
    }

    pub fn freeze(&self, inputs: &Value) -> Result<(), AppError> {
        let path = self.root.join("inputs.json");
        match std::fs::read(&path) {
            Ok(b) => {
                let prior: Value = serde_json::from_slice(&b)
                    .map_err(|_| AppError::Parse("运行输入损坏".into()))?;
                if prior != *inputs {
                    return Err(AppError::Parse("运行输入变化，必须使用新run_id".into()));
                }
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => write_json(&path, inputs),
            Err(e) => Err(e.into()),
        }
    }

    pub async fn call(
        &self,
        name: &str,
        role: Role,
        system: &str,
        user: &str,
        json_mode: bool,
    ) -> Result<String, AppError> {
        if !valid_id(name) {
            return Err(AppError::Parse("研究请求名无效".into()));
        }
        let body = role.request(system, user, json_mode)?;
        let request = json!({"schema":1,"endpoint_sha256":sha256(self.base.trim_end_matches('/').as_bytes()),"body":body});
        let mut identity = format!("{}/{name}", self.run_id);
        let path = self.root.join("requests").join(format!("{name}.json"));
        let _permit = self
            .permits
            .acquire()
            .await
            .map_err(|_| AppError::Network("研究并发控制关闭".into()))?;
        let receipt = match std::fs::read(&path) {
            Ok(b) => {
                let cached: Checkpoint = serde_json::from_slice(&b)
                    .map_err(|_| AppError::Parse("研究检查点损坏".into()))?;
                if cached.request != request {
                    return Err(AppError::Parse("研究请求身份变化，拒绝缓存复用".into()));
                }
                verify(&cached)?;
                if let Some(origin) = cached.reused_from {
                    identity = origin;
                }
                println!("[research resume] {name}");
                cached.receipt
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let input_upper = serde_json::to_vec(&body)
                    .map_err(|e| AppError::Parse(e.to_string()))?
                    .len() as u64
                    + 1024;
                identity = self.budget.lock().await.reserve_attempt(
                    &identity,
                    cost(role.model(), input_upper, role.max_output())?,
                )?;
                println!("[research request] {name} {}", role.model());
                let raw = post_chat(&self.client, &self.base, &self.key, &body)
                    .await
                    .map_err(|e| match e {
                        AppError::ApiError { status, .. } => AppError::ApiError {
                            status,
                            body: "研究请求失败，费用预留保留，端点和错误正文不记录".into(),
                        },
                        _ => AppError::Network("研究请求中断，费用预留保留".into()),
                    })?;
                let receipt = parse_receipt(&raw)?;
                let receipt_sha256 = sha256(
                    &serde_json::to_vec(&receipt).map_err(|e| AppError::Parse(e.to_string()))?,
                );
                write_json(
                    &path,
                    &Checkpoint {
                        request,
                        receipt: receipt.clone(),
                        receipt_sha256,
                        completed_at: chrono::Utc::now().to_rfc3339(),
                        reused_from: (identity != format!("{}/{name}", self.run_id))
                            .then(|| identity.clone()),
                    },
                )?;
                receipt
            }
            Err(e) => return Err(e.into()),
        };
        self.budget
            .lock()
            .await
            .settle(&identity, charge(role.model(), &receipt)?)?;
        if receipt.model != role.model()
            || receipt.finish_reason != "stop"
            || receipt.content.trim().is_empty()
        {
            return Err(AppError::Parse(
                "返回模型/完成状态/正文无效，响应与费用已保留".into(),
            ));
        }
        if receipt
            .usage
            .as_ref()
            .and_then(|u| u["completion_tokens"].as_u64())
            .is_some_and(|n| n > role.max_output())
        {
            return Err(AppError::Parse(
                "输出usage超过请求上限，已计费并停止".into(),
            ));
        }
        Ok(receipt.content)
    }

    pub async fn usage(&self) -> Value {
        let b = self.budget.lock().await;
        json!({"occupied_cny":b.occupied(false) as f64/1e6,"run_occupied_cny":b.occupied(true) as f64/1e6,
            "entries":b.ledger.entries,"pricing":"official domestic cache-miss upper estimate"})
    }

    pub async fn limit_run(&self, micro_cny: u64) -> Result<(), AppError> {
        self.budget.lock().await.limit_run(micro_cny)
    }

    pub async fn import_completed(&self, source_run: &str) -> Result<(), AppError> {
        if !valid_id(source_run) || source_run == self.run_id {
            return Err(AppError::Parse("复用来源run无效".into()));
        }
        let source = self
            .root
            .parent()
            .ok_or_else(|| AppError::Parse("运行目录无效".into()))?
            .join(source_run);
        let summary: Value = serde_json::from_slice(&std::fs::read(source.join("summary.json"))?)
            .map_err(|_| AppError::Parse("来源summary无效".into()))?;
        if summary["status"] != "complete" {
            return Err(AppError::Parse("只导入完整验证的来源run".into()));
        }
        let mut provenance = Vec::new();
        for entry in std::fs::read_dir(source.join("requests"))? {
            let path = entry?.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let bytes = std::fs::read(&path)?;
            let mut cp: Checkpoint = serde_json::from_slice(&bytes)
                .map_err(|_| AppError::Parse("来源检查点损坏".into()))?;
            verify(&cp)?;
            let model = cp.request["body"]["model"]
                .as_str()
                .ok_or_else(|| AppError::Parse("来源模型缺失".into()))?;
            if cp.receipt.model != model || cp.receipt.finish_reason != "stop" {
                return Err(AppError::Parse("来源响应未完成".into()));
            }
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .ok_or_else(|| AppError::Parse("来源任务名无效".into()))?;
            let origin = cp
                .reused_from
                .clone()
                .unwrap_or(format!("{source_run}/{name}"));
            self.budget
                .lock()
                .await
                .settle(&origin, charge(model, &cp.receipt)?)?;
            cp.reused_from = Some(origin);
            let target = self.root.join("requests").join(path.file_name().unwrap());
            if !target.exists() {
                write_json(&target, &cp)?;
            }
            provenance
                .push(json!({"source_run":source_run,"task":name,"source_sha256":sha256(&bytes)}));
        }
        provenance.sort_by_key(|v| v["task"].as_str().unwrap_or("").to_owned());
        write_json(&self.root.join("reused-checkpoints.json"), &provenance)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    #[test]
    fn structured_pro_changes_only_explicit_thinking_mode() {
        let mut old = Role::Analysis.request("system", "user", true).unwrap();
        let structured = Role::Structured.request("system", "user", true).unwrap();
        assert_eq!(old["thinking"]["type"], "enabled");
        assert_eq!(structured["thinking"]["type"], "disabled");
        old["thinking"] = structured["thinking"].clone();
        assert_eq!(old, structured);
        assert_eq!(
            Role::Writer.request("s", "u", false).unwrap()["model"],
            "mimo-v2.6-flash"
        );
    }
    #[tokio::test]
    async fn paid_call_is_resumable_and_preserves_metadata_without_reasoning() {
        let d = tempfile::tempdir().unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let thread = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut all = Vec::new();
            let mut buf = [0; 4096];
            loop {
                let n = stream.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                all.extend_from_slice(&buf[..n]);
                if let Some(end) = all.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&all[..end]);
                    let len: usize = headers
                        .lines()
                        .find_map(|s| {
                            s.to_lowercase()
                                .strip_prefix("content-length: ")
                                .and_then(|v| v.parse().ok())
                        })
                        .unwrap();
                    if all.len() >= end + 4 + len {
                        break;
                    }
                }
            }
            let raw = String::from_utf8(all).unwrap();
            assert!(raw.contains("max_completion_tokens"));
            assert!(raw.contains("enabled"));
            let response=json!({"model":"mimo-v2.6-pro","choices":[{"finish_reason":"stop","message":{"content":"ok","reasoning_content":"DO_NOT_PERSIST"}}],"usage":{"prompt_tokens":100,"completion_tokens":200,"completion_tokens_details":{"reasoning_tokens":150}}}).to_string();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response.len(),
                response
            )
            .unwrap();
        });
        let s = ResearchStore::build(
            d.path(),
            "test",
            1000000,
            1000000,
            base,
            "TEST_SECRET".into(),
        )
        .unwrap();
        // Simulate a killed process that reserved a request but saved no response.
        s.budget.lock().await.reserve("test/one", 2000).unwrap();
        assert_eq!(
            s.call("one", Role::Analysis, "system", "user", true)
                .await
                .unwrap(),
            "ok"
        );
        assert_eq!(
            s.call("one", Role::Analysis, "system", "user", true)
                .await
                .unwrap(),
            "ok"
        );
        assert!(s
            .call("one", Role::Writer, "system", "user", true)
            .await
            .is_err());
        assert_eq!(s.budget.lock().await.occupied(false), 3500);
        let saved = std::fs::read_to_string(s.root.join("requests/one.json")).unwrap();
        assert!(!saved.contains("DO_NOT_PERSIST"));
        assert!(!saved.contains("TEST_SECRET"));
        thread.join().unwrap();
        write_json(&s.root.join("summary.json"), &json!({"status":"complete"})).unwrap();
        let base = s.base.clone();
        drop(s);
        let reused = ResearchStore::build(
            d.path(),
            "next",
            1000000,
            1000000,
            base,
            "TEST_SECRET".into(),
        )
        .unwrap();
        reused.import_completed("test").await.unwrap();
        assert_eq!(
            reused
                .call("one", Role::Analysis, "system", "user", true)
                .await
                .unwrap(),
            "ok"
        );
        assert_eq!(reused.budget.lock().await.occupied(false), 3500);
        assert_eq!(reused.budget.lock().await.occupied(true), 0);
    }
    #[test]
    fn missing_usage_and_overlapping_processes_fail_closed() {
        assert!(charge(
            "mimo-v2.6-pro",
            &parse_receipt(r#"{"choices":[]}"#).unwrap()
        )
        .is_err());
        let d = tempfile::tempdir().unwrap();
        let first = ResearchStore::build(
            d.path(),
            "one",
            100,
            100,
            "http://unused".into(),
            "test".into(),
        )
        .unwrap();
        assert!(ResearchStore::build(
            d.path(),
            "two",
            100,
            100,
            "http://unused".into(),
            "test".into()
        )
        .is_err());
        drop(first);
        assert!(ResearchStore::build(
            d.path(),
            "two",
            100,
            100,
            "http://unused".into(),
            "test".into()
        )
        .is_ok());
    }
}
