//! Durable MiMo learning requests. Credentials and raw HTTP errors never enter checkpoints.
use crate::{
    error::AppError,
    learning_job::{CallProfile, LearningJob},
    learning_workspace_store as store,
    llm::{post_chat, request_body},
    voice_learning::sha256,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{future::Future, path::Path};

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct LearningResponse {
    pub content: String,
    pub model: String,
    pub finish_reason: String,
    pub usage: Option<Value>,
}

#[derive(Serialize, Deserialize)]
struct ResponseRecord {
    request: Value,
    attempt: u32,
    response: LearningResponse,
    response_sha256: String,
}

pub(crate) struct LearningClient {
    base: String,
    key: Option<String>,
    client: reqwest::Client,
}

fn error(message: &str) -> AppError {
    AppError::LearningWorkspace(message.into())
}

impl LearningClient {
    pub(crate) fn from_env() -> Result<Self, AppError> {
        Self::new(
            std::env::var("BASE_URL").unwrap_or_else(|_| "https://api.xiaomimimo.com/v1".into()),
            std::env::var("API_KEY").ok(),
        )
    }
    fn new(base: String, key: Option<String>) -> Result<Self, AppError> {
        let base = base.trim_end_matches('/').to_string();
        let url = reqwest::Url::parse(&base).map_err(|_| error("学习端点无效"))?;
        let official = url.scheme() == "https" && url.host_str() == Some("api.xiaomimimo.com");
        let local = matches!(
            url.host_str(),
            Some("127.0.0.1" | "localhost" | "[::1]" | "::1")
        ) && matches!(url.scheme(), "http" | "https");
        if (!official && !local)
            || url.path() != "/v1"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(error(
                "此学习入口使用MiMo官方按量端点；本地模拟端点仅供验证",
            ));
        }
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(600))
            .build()
            .map_err(|_| error("无法创建学习客户端"))?;
        Ok(Self { base, key, client })
    }
    pub(crate) fn profile(&self) -> CallProfile {
        CallProfile {
            model: "mimo-v2.6-pro".into(),
            endpoint_sha256: sha256(self.base.as_bytes()),
        }
    }
    pub(crate) fn preflight(&self) -> Result<(), AppError> {
        let key = self
            .key
            .as_ref()
            .filter(|k| !k.trim().is_empty())
            .ok_or_else(|| AppError::EnvVar("API_KEY".into()))?;
        if key.starts_with("tp-") || key.starts_with("ttp-") {
            return Err(error("学习需要普通按量API凭证"));
        }
        Ok(())
    }
    pub(crate) async fn response(&self, body: &Value) -> Result<LearningResponse, AppError> {
        self.preflight()?;
        let raw = post_chat(
            &self.client,
            &self.base,
            self.key.as_deref().unwrap_or(""),
            body,
        )
        .await
        .map_err(|_| error("模型请求未完成，响应不确定；已保留请求记录且没有自动重试"))?;
        let value: Value = serde_json::from_str(&raw)
            .map_err(|_| error("模型响应无法解析，用量未知，保留请求记录"))?;
        Ok(LearningResponse {
            content: value
                .pointer("/choices/0/message/content")
                .and_then(Value::as_str)
                .unwrap_or("")
                .into(),
            model: value["model"].as_str().unwrap_or("").into(),
            finish_reason: value
                .pointer("/choices/0/finish_reason")
                .and_then(Value::as_str)
                .unwrap_or("")
                .into(),
            usage: value.get("usage").filter(|u| !u.is_null()).cloned(),
        })
    }
    pub(crate) async fn call(
        &self,
        root: &Path,
        job: &LearningJob,
        stage: String,
        system: String,
        user: String,
        retry: bool,
    ) -> Result<String, AppError> {
        if job.profile != self.profile() {
            return Err(error("端点或模型配置变化，未复用任务也未发送请求"));
        }
        let format = matches!(stage.as_str(), "learn-analysis" | "learn-analysis-repair")
            .then(|| json!({"type":"json_object"}));
        let mut body = request_body(&job.profile.model, &system, &user, format.as_ref())?;
        body["max_completion_tokens"] = json!(16384);
        body["thinking"] = json!({"type":"disabled"});
        let request = json!({"schema":1,"endpoint_sha256":job.profile.endpoint_sha256,"body":body});
        call_with(
            root,
            &stage,
            request,
            retry,
            || async { self.preflight() },
            || self.response(&body),
        )
        .await
    }
}

/// The default sender is HTTP; research adapters can use the same journal with budgeted requests.
pub(crate) async fn call_with<P, PF, S, SF>(
    root: &Path,
    stage: &str,
    request: Value,
    retry: bool,
    preflight: P,
    send: S,
) -> Result<String, AppError>
where
    P: FnOnce() -> PF,
    PF: Future<Output = Result<(), AppError>>,
    S: FnOnce() -> SF,
    SF: Future<Output = Result<LearningResponse, AppError>>,
{
    if stage.is_empty()
        || !stage
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err(error("阶段标识无效"));
    }
    let dir = root.join("requests");
    let response_path = dir.join(format!("{stage}.response.json"));
    if response_path.exists() {
        let record: ResponseRecord = store::read(&response_path)?;
        if record.request != request || record.response_sha256 != response_hash(&record.response)? {
            return Err(error("学习请求身份变化或响应检查点损坏，未发送新请求"));
        }
        if !(1..=3).contains(&record.attempt)
            || store::read::<Value>(
                &dir.join(format!("{stage}-attempt-{}.intent.json", record.attempt)),
            )? != request
        {
            return Err(error("响应与请求意图不一致"));
        }
        return validate(&record.response, &request);
    }
    let mut prior = 0;
    for attempt in 1..=3 {
        let path = dir.join(format!("{stage}-attempt-{attempt}.intent.json"));
        if path.exists() {
            if attempt != prior + 1 || store::read::<Value>(&path)? != request {
                return Err(error("请求意图历史损坏"));
            }
            prior = attempt;
        }
    }
    if prior > 0 && !retry {
        return Err(error(
            "该阶段已发送但没有可复用响应；未自动重发，可显式resume --retry-uncertain",
        ));
    }
    if prior >= 3 {
        return Err(error("该阶段已到三次请求上限，未知用量记录保留"));
    }
    // Credential/configuration failures before sending must not become uncertain requests.
    preflight().await?;
    let attempt = prior + 1;
    store::publish_json(
        &dir.join(format!("{stage}-attempt-{attempt}.intent.json")),
        &request,
    )?;
    let response = send().await?;
    let record = ResponseRecord {
        request: request.clone(),
        attempt,
        response_sha256: response_hash(&response)?,
        response,
    };
    store::publish_json(&response_path, &record)?;
    validate(&record.response, &request)
}

fn response_hash(response: &LearningResponse) -> Result<String, AppError> {
    Ok(sha256(
        &serde_json::to_vec(response).map_err(|_| error("响应留证失败"))?,
    ))
}
fn validate(response: &LearningResponse, request: &Value) -> Result<String, AppError> {
    if response.model != request["body"]["model"].as_str().unwrap_or("")
        || response.finish_reason != "stop"
        || response.content.trim().is_empty()
        || response
            .usage
            .as_ref()
            .and_then(|u| u["completion_tokens"].as_u64())
            .is_some_and(|n| n > 16384)
    {
        return Err(error(
            "返回模型、完成状态或正文无效；响应已保留，未自动重抽",
        ));
    }
    Ok(response.content.clone())
}

pub(crate) fn statistics(root: &Path) -> Result<Value, AppError> {
    let entries = match std::fs::read_dir(root.join("requests")) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(json!({"responses":0,"uncertain_attempts":0}));
        }
        Err(e) => return Err(e.into()),
    };
    let (mut intents, mut responses, mut prompt, mut completion, mut known) =
        (0u64, 0u64, 0u64, 0u64, true);
    for entry in entries {
        let path = entry?.path();
        let file = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if file.ends_with(".intent.json") {
            intents += 1;
        }
        if file.ends_with(".response.json") {
            let record: ResponseRecord = store::read(&path)?;
            if record.response_sha256 != response_hash(&record.response)? {
                return Err(error("响应检查点损坏"));
            }
            responses += 1;
            match record.response.usage.as_ref().and_then(|u| {
                Some((
                    u["prompt_tokens"].as_u64()?,
                    u["completion_tokens"].as_u64()?,
                ))
            }) {
                Some((p, c)) => {
                    prompt = prompt.saturating_add(p);
                    completion = completion.saturating_add(c);
                }
                None => known = false,
            }
        }
    }
    Ok(
        json!({"responses":responses,"uncertain_attempts":intents.saturating_sub(responses),
        "known_prompt_tokens":known.then_some(prompt),"known_completion_tokens":known.then_some(completion)}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request() -> Value {
        json!({"body":{"model":"mimo-v2.6-pro"}})
    }
    fn response() -> LearningResponse {
        LearningResponse {
            content: "有效说明".into(),
            model: "mimo-v2.6-pro".into(),
            finish_reason: "stop".into(),
            usage: None,
        }
    }

    #[tokio::test]
    async fn unknown_requests_are_not_retried_implicitly_and_valid_cache_works_without_credentials()
    {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        assert!(
            call_with(
                root,
                "analysis",
                request(),
                false,
                || async { Ok(()) },
                || async { Err(error("模拟中断")) }
            )
            .await
            .is_err()
        );
        assert!(
            call_with(
                root,
                "analysis",
                request(),
                false,
                || async { panic!("不应重发") },
                || async { Ok(response()) }
            )
            .await
            .is_err()
        );
        call_with(
            root,
            "analysis",
            request(),
            true,
            || async { Ok(()) },
            || async { Ok(response()) },
        )
        .await
        .unwrap();
        call_with(
            root,
            "analysis",
            request(),
            false,
            || async { panic!("缓存不需要凭证") },
            || async { panic!("缓存不需网络") },
        )
        .await
        .unwrap();
        let stats = statistics(root).unwrap();
        assert_eq!(stats["uncertain_attempts"], 1);
        assert!(stats["known_prompt_tokens"].is_null());
        let changed = json!({"body":{"model":"other"}});
        assert!(
            call_with(
                root,
                "analysis",
                changed,
                false,
                || async { Ok(()) },
                || async { panic!("身份变化不请求") }
            )
            .await
            .is_err()
        );
    }

    #[tokio::test]
    async fn preflight_failure_has_no_intent_and_uncertain_attempts_are_bounded() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        assert!(
            call_with(
                root,
                "stage",
                request(),
                false,
                || async { Err(error("缺少凭证")) },
                || async { panic!("不应发送") }
            )
            .await
            .is_err()
        );
        assert!(!root.join("requests").exists());
        for _ in 0..3 {
            assert!(
                call_with(
                    root,
                    "stage",
                    request(),
                    true,
                    || async { Ok(()) },
                    || async { Err(error("中断")) }
                )
                .await
                .is_err()
            );
        }
        assert!(
            call_with(
                root,
                "stage",
                request(),
                true,
                || async { panic!("不能超限") },
                || async { Ok(response()) }
            )
            .await
            .is_err()
        );
        assert_eq!(statistics(root).unwrap()["uncertain_attempts"], 3);
    }
}
