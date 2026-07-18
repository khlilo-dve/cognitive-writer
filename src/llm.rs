use crate::error::AppError;
use reqwest::Client;
use serde::{Deserialize, Serialize};

// ── OpenAI-compatible request/response types ────────────────────────

#[derive(Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
}

#[derive(Serialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

#[derive(Deserialize)]
pub struct ChatResponse {
    pub choices: Vec<Choice>,
}

#[derive(Deserialize)]
pub struct Choice {
    pub message: ResponseMessage,
}

#[derive(Deserialize)]
pub struct ResponseMessage {
    pub content: String,
}

// ── LLM API call (OpenAI-compatible) ────────────────────────────────

pub async fn call_llm(
    client: &Client,
    base_url: &str,
    api_key: &str,
    model: &str,
    system_prompt: &str,
    user_content: &str,
) -> Result<String, AppError> {
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));

    let body = ChatRequest {
        model: model.to_string(),
        messages: vec![
            Message {
                role: "system".to_string(),
                content: system_prompt.to_string(),
            },
            Message {
                role: "user".to_string(),
                content: user_content.to_string(),
            },
        ],
    };

    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| AppError::Network(format!("{}", e)))?;

    let status = resp.status();
    let raw_body = resp.text().await.unwrap_or_default();

    if !status.is_success() {
        return Err(AppError::ApiError {
            status: status.as_u16(),
            body: raw_body,
        });
    }

    let chat_resp: ChatResponse = serde_json::from_str(&raw_body).map_err(|e| {
        AppError::Parse(format!(
            "{}\n原始响应: {}",
            e,
            &raw_body[..raw_body.len().min(500)]
        ))
    })?;

    chat_resp
        .choices
        .first()
        .map(|c| c.message.content.clone())
        .ok_or(AppError::EmptyChoices)
}

// ── 泛型重试函数 ───────────────────────────────────────────────────

pub async fn with_retry<T, F, Fut>(
    max_retries: u32,
    label: &str,
    f: F,
) -> Result<T, AppError>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<T, AppError>>,
{
    let mut last_err: Option<AppError> = None;
    for attempt in 1..=max_retries {
        match f().await {
            Ok(result) => return Ok(result),
            Err(e) => {
                if attempt < max_retries {
                    eprintln!("[warn] {} 第 {} 次失败: {}，2 秒后重试 ...", label, attempt, e);
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                }
                last_err = Some(e);
            }
        }
    }
    Err(last_err.unwrap_or_else(|| {
        AppError::Parse("with_retry: max_retries 不能为 0".to_string())
    }))
}
