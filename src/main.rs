use dialoguer::{Select, theme::ColorfulTheme};
use pulldown_cmark::{Options, Parser, html};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{self, Command, Stdio};

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

// ── Style selection ─────────────────────────────────────────────────

fn list_styles(dir: &str) -> Result<Vec<String>, String> {
    let mut styles = Vec::new();
    let entries = fs::read_dir(dir)
        .map_err(|e| format!("无法读取 {} 目录: {}", dir, e))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "md") {
            styles.push(path.display().to_string());
        }
    }

    styles.sort();
    if styles.is_empty() {
        return Err(format!("{} 目录下没有 .md 风格文件", dir));
    }
    Ok(styles)
}

fn select_style(styles: &[String]) -> Result<usize, String> {
    if styles.len() == 1 {
        let name = styles[0].strip_prefix("styles/").unwrap_or(&styles[0]);
        println!("[info] 使用唯一风格: {}", name);
        return Ok(0);
    }

    let labels: Vec<&str> = styles
        .iter()
        .map(|s| s.strip_prefix("styles/").unwrap_or(s))
        .collect();

    Select::with_theme(&ColorfulTheme::default())
        .with_prompt("请选择写作风格")
        .items(&labels)
        .default(0)
        .interact()
        .map_err(|e| format!("风格选择失败: {}", e))
}

// ── File I/O ────────────────────────────────────────────────────────

fn read_file(path: &str) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| format!("无法读取 `{}`: {}", path, e))
}

fn extract_idea_slug(idea: &str) -> String {
    let raw = idea
        .lines()
        .find_map(|l| l.strip_prefix("文章主题：").or_else(|| l.strip_prefix("文章主题:")))
        .or_else(|| {
            idea.lines()
                .find(|l| l.starts_with("# "))
                .map(|l| l.trim_start_matches('#').trim())
        })
        .unwrap_or("untitled");

    let slug: String = raw
        .trim()
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
        .collect();

    if slug.is_empty() {
        "untitled".to_string()
    } else {
        slug
    }
}

fn next_version(dir: &str, slug: &str) -> u32 {
    let prefix = format!("{}_v", slug);
    let mut max: u32 = 0;

    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if let Some(rest) = name.strip_prefix(&prefix) {
                // 同时匹配 .md 和 .html 后缀
                let num_str = rest
                    .strip_suffix(".md")
                    .or_else(|| rest.strip_suffix(".html"));
                if let Some(n) = num_str.and_then(|s| s.parse::<u32>().ok()) {
                    max = max.max(n);
                }
            }
        }
    }

    max + 1
}

fn write_file(path: &str, content: &str) -> Result<(), String> {
    let dir = Path::new(path).parent().unwrap_or(Path::new("."));
    if !dir.exists() {
        fs::create_dir_all(dir).map_err(|e| format!("无法创建目录 `{}`: {}", dir.display(), e))?;
    }
    fs::write(path, content).map_err(|e| format!("无法写入 `{}`: {}", path, e))
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
    let raw_body = resp.text().await.unwrap_or_default();

    if !status.is_success() {
        return Err(format!("API 返回错误 (HTTP {}): {}", status, raw_body));
    }

    let chat_resp: ChatResponse = serde_json::from_str(&raw_body).map_err(|e| {
        format!(
            "解析 API 响应失败: {}\n原始响应: {}",
            e,
            &raw_body[..raw_body.len().min(500)]
        )
    })?;

    chat_resp
        .choices
        .first()
        .map(|c| c.message.content.clone())
        .ok_or_else(|| {
            format!(
                "API 返回了空的 choices 数组\n原始响应: {}",
                &raw_body[..raw_body.len().min(500)]
            )
        })
}

// ── Markdown → WeChat HTML ──────────────────────────────────────────

fn md_to_wechat_html(markdown: &str) -> String {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TABLES);

    let parser = Parser::new_ext(markdown, opts);
    let mut raw = String::new();
    html::push_html(&mut raw, parser);

    // 微信编辑器会剥离 <style> 块，所有样式必须内联
    let styled = raw
        .replace(
            "<h1>",
            "<h1 style=\"font-size:22px;font-weight:bold;color:#1a1a1a;\
             margin:28px 0 14px;line-height:1.4;\">",
        )
        .replace(
            "<h2>",
            "<h2 style=\"font-size:18px;font-weight:bold;color:#1a1a1a;\
             margin:24px 0 12px;line-height:1.4;\
             border-left:4px solid #07c160;padding-left:10px;\">",
        )
        .replace(
            "<h3>",
            "<h3 style=\"font-size:16px;font-weight:bold;color:#333;\
             margin:20px 0 10px;line-height:1.4;\">",
        )
        .replace(
            "<p>",
            "<p style=\"margin:14px 0;line-height:2;color:#333;font-size:15px;\">",
        )
        .replace(
            "<strong>",
            "<strong style=\"font-weight:bold;color:#1a1a1a;\">",
        )
        .replace(
            "<em>",
            "<em style=\"font-style:italic;color:#555;\">",
        )
        .replace(
            "<blockquote>\n",
            "<blockquote style=\"margin:20px 0;padding:15px 20px;\
             background:#f7f7f7;border-left:4px solid #07c160;\
             color:#666;font-size:14px;line-height:1.8;\">\n",
        )
        .replace(
            "<ul>\n",
            "<ul style=\"margin:14px 0;padding-left:24px;\">\n",
        )
        .replace(
            "<ol>\n",
            "<ol style=\"margin:14px 0;padding-left:24px;\">\n",
        )
        .replace(
            "<li>",
            "<li style=\"margin:5px 0;line-height:1.8;color:#333;\">",
        )
        .replace(
            "<hr />",
            "<hr style=\"border:none;border-top:1px solid #eee;margin:24px 0;\" />",
        );

    format!(
        "<section style=\"max-width:677px;margin:0 auto;padding:16px;\
         font-family:-apple-system,BlinkMacSystemFont,'PingFang SC',\
         'Hiragino Sans GB','Microsoft YaHei',sans-serif;\
         font-size:15px;line-height:2;color:#333;\">\
         {styled}</section>"
    )
}

// ── Clipboard injection ─────────────────────────────────────────────

fn inject_clipboard(html_content: &str) -> Result<&'static str, String> {
    // 1. xclip (X11 / WSLg)
    if let Ok(mut child) = Command::new("xclip")
        .args(["-selection", "clipboard", "-t", "text/html"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        if let Some(ref mut stdin) = child.stdin {
            if stdin.write_all(html_content.as_bytes()).is_ok() {
                drop(child.stdin.take());
                if child.wait().map_or(false, |s| s.success()) {
                    return Ok("xclip");
                }
            }
        }
    }

    // 2. wl-copy (Wayland)
    if let Ok(mut child) = Command::new("wl-copy")
        .args(["--type", "text/html"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        if let Some(ref mut stdin) = child.stdin {
            if stdin.write_all(html_content.as_bytes()).is_ok() {
                drop(child.stdin.take());
                if child.wait().map_or(false, |s| s.success()) {
                    return Ok("wl-copy");
                }
            }
        }
    }

    // 3. clip.exe (WSL2 → Windows 剪贴板，纯文本回退)
    if let Ok(mut child) = Command::new("clip.exe")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        if let Some(ref mut stdin) = child.stdin {
            if stdin.write_all(html_content.as_bytes()).is_ok() {
                drop(child.stdin.take());
                if child.wait().map_or(false, |s| s.success()) {
                    return Ok("clip.exe (纯文本)");
                }
            }
        }
    }

    Err("未找到剪贴板工具 (xclip / wl-copy / clip.exe)".to_string())
}

// ── Env helpers ─────────────────────────────────────────────────────

fn env_var(key: &str) -> Result<String, String> {
    std::env::var(key).map_err(|_| format!("环境变量 `{}` 未设置，请检查 .env 文件", key))
}

fn env_var_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

// ── Main ────────────────────────────────────────────────────────────

fn fatal(msg: &str) -> ! {
    eprintln!("[error] {}", msg);
    process::exit(1);
}

#[tokio::main]
async fn main() {
    // 1. 加载 .env
    if let Err(e) = dotenvy::dotenv() {
        eprintln!("[warn] 未加载 .env: {} (将使用系统环境变量)", e);
    }

    // 2. 环境变量
    let api_key = env_var("API_KEY").unwrap_or_else(|e| fatal(&e));
    let base_url = env_var_or("BASE_URL", "https://api.openai.com/v1");
    let model = env_var_or("MODEL", "gpt-4o");

    println!("[info] BASE_URL = {}", base_url);
    println!("[info] MODEL    = {}", model);

    // 3. 风格选择
    let styles = list_styles("styles").unwrap_or_else(|e| fatal(&e));
    let idx = select_style(&styles).unwrap_or_else(|e| fatal(&e));
    let style_path = &styles[idx];
    let style = read_file(style_path).unwrap_or_else(|e| fatal(&e));

    println!(
        "[info] 风格: {}",
        style_path.strip_prefix("styles/").unwrap_or(style_path)
    );

    // 4. 读取素材
    let idea = read_file("inputs/idea_01.md").unwrap_or_else(|e| fatal(&e));
    if idea.trim().is_empty() {
        fatal("inputs/idea_01.md 内容为空，请先写入创作素材");
    }
    println!("[info] 素材 {} 字符 | 风格 {} 字符", idea.len(), style.len());

    // 5. 调用 LLM
    let client = Client::new();
    println!("[info] 正在调用 LLM ...");
    let markdown = call_llm(&client, &base_url, &api_key, &model, &style, &idea)
        .await
        .unwrap_or_else(|e| fatal(&e));
    println!("[info] LLM 返回 {} 字符", markdown.len());

    // 6. 双轨输出
    let slug = extract_idea_slug(&idea);
    let ver = next_version("outputs", &slug);
    let md_path = format!("outputs/{}_v{}.md", slug, ver);
    let html_path = format!("outputs/{}_v{}.html", slug, ver);

    // 6a. 存档 Markdown
    write_file(&md_path, &markdown).unwrap_or_else(|e| fatal(&e));
    println!("[done] Markdown → {}", md_path);

    // 6b. 转换 HTML + 存档
    let html = md_to_wechat_html(&markdown);
    write_file(&html_path, &html).unwrap_or_else(|e| fatal(&e));
    println!("[done] HTML     → {}", html_path);

    // 6c. 注入剪贴板
    match inject_clipboard(&html) {
        Ok(tool) => {
            println!("[done] 富文本已注入剪贴板 (via {})", tool);
            println!();
            println!("  文章已生成并存档，富文本已注入剪贴板，请直接前往微信粘贴 (Ctrl+V)");
        }
        Err(e) => {
            eprintln!("[warn] {}", e);
            println!();
            println!(
                "  文章已生成并存档。剪贴板不可用，请用浏览器打开 {} 后手动复制",
                html_path
            );
        }
    }
}
