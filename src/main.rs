use chrono::Local;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::process;

// ── OpenAI-compatible request/response types ────────────────────────

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
}

#[derive(Serialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: ResponseMessage,
}

#[derive(Deserialize)]
struct ResponseMessage {
    content: String,
}

// ── File I/O ────────────────────────────────────────────────────────

fn read_file(path: &str) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| format!("无法读取文件 `{}`: {}", path, e))
}

fn write_output(content: &str) -> Result<String, String> {
    let dir = "outputs";
    if !Path::new(dir).exists() {
        fs::create_dir_all(dir).map_err(|e| format!("无法创建目录 `{}`: {}", dir, e))?;
    }

    let ts = Local::now().format("%Y%m%d_%H%M%S");
    let filename = format!("{}/draft_{}.md", dir, ts);

    fs::write(&filename, content).map_err(|e| format!("无法写入文件 `{}`: {}", filename, e))?;

    Ok(filename)
}

// ── LLM API call (OpenAI-compatible) ────────────────────────────────

async fn call_llm(
    client: &Client,
    base_url: &str,
    api_key: &str,
    model: &str,
    system_prompt: &str,
    user_content: &str,
) -> Result<String, String> {
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
        .map_err(|e| format!("网络请求失败: {}", e))?;

    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("API 返回错误 (HTTP {}): {}", status, text));
    }

    let chat_resp: ChatResponse = resp
        .json()
        .await
        .map_err(|e| format!("解析 API 响应失败: {}", e))?;

    chat_resp
        .choices
        .first()
        .map(|c| c.message.content.clone())
        .ok_or_else(|| "API 返回了空的 choices 数组".to_string())
}

// ── Env helpers ─────────────────────────────────────────────────────

fn env_var(key: &str) -> Result<String, String> {
    std::env::var(key).map_err(|_| format!("环境变量 `{}` 未设置，请检查 .env 文件", key))
}

fn env_var_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

// ── Main ────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    // 1. 加载 .env
    if let Err(e) = dotenvy::dotenv() {
        eprintln!("[warn] 未加载 .env 文件: {} (将使用系统环境变量)", e);
    }

    // 2. 读取环境变量
    let api_key = match env_var("API_KEY") {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[error] {}", e);
            process::exit(1);
        }
    };
    let base_url = env_var_or("BASE_URL", "https://api.openai.com/v1");
    let model = env_var_or("MODEL", "gpt-4o");

    println!("[info] BASE_URL = {}", base_url);
    println!("[info] MODEL    = {}", model);

    // 3. 读取输入文件
    let idea = match read_file("inputs/idea_01.md") {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[error] {}", e);
            process::exit(1);
        }
    };
    let style = match read_file("styles/wechat_base.md") {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[error] {}", e);
            process::exit(1);
        }
    };

    if idea.trim().is_empty() {
        eprintln!("[error] inputs/idea_01.md 内容为空，请先写入你的创作素材");
        process::exit(1);
    }

    println!("[info] 已读取 idea ({} 字符) 和 style ({} 字符)", idea.len(), style.len());

    // 4. 调用 LLM
    let client = Client::new();
    println!("[info] 正在调用 LLM API ...");

    let result = match call_llm(&client, &base_url, &api_key, &model, &style, &idea).await {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[error] {}", e);
            process::exit(1);
        }
    };

    println!("[info] API 返回 {} 字符", result.len());

    // 5. 写入输出文件
    match write_output(&result) {
        Ok(path) => println!("[done] 已保存到 {}", path),
        Err(e) => {
            eprintln!("[error] {}", e);
            process::exit(1);
        }
    }
}
