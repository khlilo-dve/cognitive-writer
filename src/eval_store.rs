//! Versioned evaluation requests and resumable checkpoints. Never persist credentials.
use std::path::{Path, PathBuf};

use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::llm::{call_llm, with_retry};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Request {
    schema: u32,
    endpoint_fingerprint: String,
    model: String,
    system: String,
    user: String,
}

#[derive(Serialize, Deserialize)]
struct Checkpoint {
    request: Request,
    response: String,
    completed_at: String,
}

pub(crate) struct Store {
    pub root: PathBuf,
    pub model: String,
    client: Client,
    permits: tokio::sync::Semaphore,
    base: String,
    key: String,
}

// Stable identity only, not a cryptographic integrity/security primitive.
fn fingerprint(value: &str) -> String {
    let hash = value.bytes().fold(0xcbf29ce484222325_u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
    });
    format!("{hash:016x}")
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}

pub(crate) fn write_json(path: &Path, value: &impl Serialize) -> Result<(), AppError> {
    let json = serde_json::to_vec_pretty(value).map_err(|e| AppError::Parse(e.to_string()))?;
    let pending = path.with_extension("pending");
    std::fs::write(&pending, json)?;
    std::fs::rename(pending, path)?;
    Ok(())
}

fn load(path: &Path, request: &Request) -> Result<Option<String>, AppError> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let stored: Checkpoint = serde_json::from_slice(&bytes)
        .map_err(|_| AppError::Parse(format!("检查点损坏: {}", path.display())))?;
    if &stored.request != request {
        return Err(AppError::Parse(format!(
            "请求已变化，不能复用 {}；请使用新的 EVAL_RUN_ID",
            path.display()
        )));
    }
    Ok(Some(stored.response))
}

fn safe_error(error: AppError) -> AppError {
    match error {
        AppError::ApiError { status, .. } => AppError::ApiError {
            status,
            body: "评测请求失败，响应正文未记录（避免泄漏凭据）".into(),
        },
        AppError::Network(_) => AppError::Network("连接失败或请求超时（端点已隐藏）".into()),
        AppError::EmptyChoices => AppError::EmptyChoices,
        AppError::Parse(message) => AppError::Parse(
            message
                .lines()
                .next()
                .unwrap_or("无法解析模型响应")
                .chars()
                .take(160)
                .collect(),
        ),
        _ => AppError::Parse("模型响应无效；未保存为完成检查点".into()),
    }
}

fn bounded_setting(key: &str, default: u64, min: u64, max: u64) -> Result<u64, AppError> {
    let value = match std::env::var(key) {
        Ok(raw) => raw
            .parse::<u64>()
            .map_err(|_| AppError::EnvVar(format!("{key} 必须为整数")))?,
        Err(std::env::VarError::NotPresent) => default,
        Err(_) => return Err(AppError::EnvVar(format!("{key} 编码无效"))),
    };
    if !(min..=max).contains(&value) {
        return Err(AppError::EnvVar(format!("{key} 必须在 {min}–{max} 之间")));
    }
    Ok(value)
}

impl Store {
    pub fn freeze_inputs(&self, inputs: &serde_json::Value) -> Result<(), AppError> {
        let path = self.root.join("inputs.json");
        match std::fs::read(&path) {
            Ok(bytes) => {
                let prior: serde_json::Value = serde_json::from_slice(&bytes)
                    .map_err(|_| AppError::Parse("评测输入快照损坏".into()))?;
                if prior != *inputs {
                    return Err(AppError::Parse(
                        "输入或提示词已变化，请使用新的 EVAL_RUN_ID".into(),
                    ));
                }
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => write_json(&path, inputs),
            Err(e) => Err(e.into()),
        }
    }

    pub fn from_env() -> Result<Self, AppError> {
        // This is the explicit online evaluation entry, never used by offline tests.
        dotenvy::dotenv().ok();
        let run_id = std::env::var("EVAL_RUN_ID")
            .unwrap_or_else(|_| chrono::Utc::now().format("%Y%m%dT%H%M%S%6fZ").to_string());
        if !valid_name(&run_id) {
            return Err(AppError::Parse(
                "EVAL_RUN_ID 只能包含字母、数字、下划线和连字符".into(),
            ));
        }
        let key = std::env::var("API_KEY").map_err(|_| AppError::EnvVar("API_KEY".into()))?;
        if key.trim().is_empty() {
            return Err(AppError::EnvVar("API_KEY".into()));
        }
        let root = Path::new("eval/runs").join(run_id);
        std::fs::create_dir_all(&root)?;
        Ok(Self {
            root,
            key,
            permits: tokio::sync::Semaphore::new(
                bounded_setting("EVAL_CONCURRENCY", 3, 1, 3)? as usize
            ),
            model: std::env::var("MODEL").unwrap_or_else(|_| "gpt-4o".into()),
            base: std::env::var("BASE_URL").unwrap_or_else(|_| "https://api.openai.com/v1".into()),
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(bounded_setting(
                    "EVAL_TIMEOUT_SECONDS",
                    180,
                    30,
                    900,
                )?))
                .build()
                .map_err(|_| AppError::Network("无法构建评测客户端".into()))?,
        })
    }

    pub async fn call(
        &self,
        name: &str,
        system: &str,
        user: &str,
        validate: fn(&str) -> Result<(), AppError>,
    ) -> Result<String, AppError> {
        if !valid_name(name) {
            return Err(AppError::Parse("无效的评测任务名".into()));
        }
        let request = Request {
            schema: 1,
            endpoint_fingerprint: fingerprint(self.base.trim_end_matches('/')),
            model: self.model.clone(),
            system: system.into(),
            user: user.into(),
        };
        let path = self.root.join(format!("{name}.json"));
        if let Some(response) = load(&path, &request)? {
            validate(&response)?;
            println!("[resume] {name}");
            return Ok(response);
        }
        let _permit = self
            .permits
            .acquire()
            .await
            .map_err(|_| AppError::Network("评测并发控制已关闭".into()))?;
        println!("[request] {name}");
        let response = with_retry(3, name, || async {
            let response = call_llm(
                &self.client,
                &self.base,
                &self.key,
                &self.model,
                system,
                user,
            )
            .await
            .map_err(safe_error)?;
            // Validation errors contain schema diagnostics, not raw HTTP responses.
            if let Err(error) = validate(&response) {
                write_json(
                    &self.root.join(format!("rejected-{name}.json")),
                    &serde_json::json!({
                        "status":"rejected", "at":chrono::Utc::now().to_rfc3339(),
                        "validation_error":error.to_string(), "model_output":response,
                    }),
                )?;
                return Err(error);
            }
            Ok(response)
        })
        .await?;
        write_json(
            &path,
            &Checkpoint {
                request,
                response: response.clone(),
                completed_at: chrono::Utc::now().to_rfc3339(),
            },
        )?;
        println!("[saved] {name}");
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> Request {
        Request {
            schema: 1,
            endpoint_fingerprint: fingerprint("endpoint"),
            model: "test".into(),
            system: "prompt".into(),
            user: "input".into(),
        }
    }

    #[test]
    fn checkpoint_reuses_exact_request_and_rejects_drift_or_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("job.json");
        let req = request();
        assert!(load(&path, &req).unwrap().is_none());
        write_json(
            &path,
            &Checkpoint {
                request: req.clone(),
                response: "answer".into(),
                completed_at: "now".into(),
            },
        )
        .unwrap();
        assert_eq!(load(&path, &req).unwrap().unwrap(), "answer");
        for field in ["model", "system", "user", "endpoint_fingerprint"] {
            let mut json = serde_json::to_value(&req).unwrap();
            json[field] = serde_json::json!("changed");
            assert!(load(&path, &serde_json::from_value(json).unwrap()).is_err());
        }
        std::fs::write(&path, b"{partial").unwrap();
        assert!(load(&path, &req).is_err());
    }

    #[test]
    fn paths_cannot_escape_run_directory() {
        for invalid in ["", ".", "..", "../escape", "a/b", "a\\b", "/tmp", "x.json"] {
            assert!(!valid_name(invalid));
        }
        assert!(valid_name("20260905T120000Z-candidate_1"));
    }

    #[test]
    fn cached_call_works_without_network_and_freeze_rejects_changed_inputs() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store {
            root: dir.path().into(),
            model: "test".into(),
            base: "invalid-endpoint".into(),
            key: "fixture-secret".into(),
            client: Client::new(),
            permits: tokio::sync::Semaphore::new(3),
        };
        let inputs = serde_json::json!({"prompt":"original"});
        store.freeze_inputs(&inputs).unwrap();
        store.freeze_inputs(&inputs).unwrap();
        assert!(
            store
                .freeze_inputs(&serde_json::json!({"prompt":"changed"}))
                .is_err()
        );
        let request = Request {
            endpoint_fingerprint: fingerprint("invalid-endpoint"),
            ..request()
        };
        write_json(
            &dir.path().join("cached.json"),
            &Checkpoint {
                request,
                response: "cached answer".into(),
                completed_at: "now".into(),
            },
        )
        .unwrap();
        let answer = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(store.call("cached", "prompt", "input", |_| Ok(())))
            .unwrap();
        assert_eq!(answer, "cached answer");
        assert!(
            !std::fs::read_to_string(dir.path().join("cached.json"))
                .unwrap()
                .contains("fixture-secret")
        );
    }

    #[test]
    fn errors_hide_credentials_and_response_bodies() {
        let error = safe_error(AppError::ApiError {
            status: 401,
            body: "test-secret".into(),
        });
        assert!(!error.to_string().contains("test-secret"));
        assert!(error.to_string().contains("401"));
        assert!(
            !safe_error(AppError::Network("test-secret".into()))
                .to_string()
                .contains("test-secret")
        );
    }

    #[test]
    fn failed_validation_never_creates_checkpoint() {
        // A local HTTP fixture exercises the online boundary without external network or credentials.
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            for _ in 0..3 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut bytes = [0; 8192];
                stream.read(&mut bytes).unwrap();
                let body = r#"{"choices":[{"message":{"content":""}}]}"#;
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .unwrap();
            }
        });
        let dir = tempfile::tempdir().unwrap();
        let store = Store {
            root: dir.path().into(),
            model: "test".into(),
            permits: tokio::sync::Semaphore::new(3),
            client: Client::new(),
            base: format!("http://{addr}"),
            key: "fixture-only".into(),
        };
        let result = tokio::runtime::Runtime::new().unwrap().block_on(store.call(
            "invalid",
            "p",
            "i",
            |s| {
                if s.is_empty() {
                    Err(AppError::Parse("empty".into()))
                } else {
                    Ok(())
                }
            },
        ));
        assert!(result.is_err());
        assert!(!dir.path().join("invalid.json").exists());
        server.join().unwrap();
    }
}
