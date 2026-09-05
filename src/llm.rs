use crate::error::AppError;
use indicatif::{ProgressBar, ProgressStyle};
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
    let raw_body = resp
        .text()
        .await
        .map_err(|e| AppError::Network(e.to_string()))?;

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
            raw_body.chars().take(500).collect::<String>()
        ))
    })?;

    chat_resp
        .choices
        .first()
        .map(|c| c.message.content.clone())
        .ok_or(AppError::EmptyChoices)
}

// ── Spinner ─────────────────────────────────────────────────────────

/// 创建一个带转圈动画的进度指示器
pub fn new_spinner(msg: &str) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::with_template("{spinner:.green} {msg}")
            .unwrap()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
    );
    pb.set_message(msg.to_string());
    pb.enable_steady_tick(std::time::Duration::from_millis(80));
    pb
}

// ── 泛型重试函数 ───────────────────────────────────────────────────

pub async fn with_retry<T, F, Fut>(max_retries: u32, label: &str, f: F) -> Result<T, AppError>
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
                    eprintln!(
                        "[warn] {} 第 {} 次失败: {}，2 秒后重试 ...",
                        label, attempt, e
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                }
                last_err = Some(e);
            }
        }
    }
    Err(AppError::Parse(format!(
        "{} 连续 {} 次失败: {}",
        label,
        max_retries,
        last_err.unwrap_or_else(|| AppError::Parse("with_retry: max_retries 不能为 0".to_string()))
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    fn local_response(response: String) -> (String, std::thread::JoinHandle<()>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let thread = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 8192];
            stream.read(&mut request).unwrap();
            stream.write_all(response.as_bytes()).unwrap();
        });
        (format!("http://{address}"), thread)
    }

    #[test]
    fn invalid_unicode_response_returns_error_without_panicking() {
        let body = "中".repeat(300);
        let (base, server) = local_response(format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ));
        let result = tokio::runtime::Runtime::new().unwrap().block_on(call_llm(
            &Client::new(),
            &base,
            "fixture",
            "fixture",
            "p",
            "i",
        ));
        assert!(matches!(result, Err(AppError::Parse(_))));
        server.join().unwrap();
    }

    #[test]
    fn interrupted_response_is_network_error_instead_of_empty_json() {
        let (base, server) = local_response(
            "HTTP/1.1 200 OK\r\nContent-Length: 1000\r\nConnection: close\r\n\r\n{partial".into(),
        );
        let result = tokio::runtime::Runtime::new().unwrap().block_on(call_llm(
            &Client::new(),
            &base,
            "fixture",
            "fixture",
            "p",
            "i",
        ));
        assert!(matches!(result, Err(AppError::Network(_))));
        server.join().unwrap();
    }
}
