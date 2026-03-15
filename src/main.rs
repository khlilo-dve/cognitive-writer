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

    // 微信编辑器会剥离 <style> 标签，所以直接移除 <hr> 标签
    let raw = raw.replace("<hr />", "").replace("<hr>", "");

    // 极简样式：只保留基本排版和加粗，不加任何装饰性 CSS
    format!(
        "<section style=\"font-size:15px;line-height:2;color:#333;\">{raw}</section>"
    )
}

// ── Clipboard injection ─────────────────────────────────────────────

/// 构建 Windows CF_HTML 格式：带字节偏移量的标准头部 + HTML 片段
/// 规范参考: https://learn.microsoft.com/en-us/windows/win32/dataxchg/html-clipboard-format
fn build_cf_html(fragment: &str) -> String {
    let pre = "<html>\r\n<head><meta charset=\"utf-8\"></head>\r\n<body>\r\n<!--StartFragment-->";
    let post = "<!--EndFragment-->\r\n</body>\r\n</html>";

    // 头部模板长度固定（全 ASCII，len() == 字节数）
    let header_len = "Version:0.9\r\nStartHTML:0000000000\r\nEndHTML:0000000000\r\nStartFragment:0000000000\r\nEndFragment:0000000000\r\n".len();

    let start_html = header_len;
    let start_frag = start_html + pre.len();
    let end_frag = start_frag + fragment.len(); // Rust .len() 返回字节数，CF_HTML 要求字节偏移
    let end_html = end_frag + post.len();

    format!(
        "Version:0.9\r\nStartHTML:{:010}\r\nEndHTML:{:010}\r\nStartFragment:{:010}\r\nEndFragment:{:010}\r\n{}{}{}",
        start_html, end_html, start_frag, end_frag, pre, fragment, post
    )
}

/// WSL2 → Windows: 通过 PowerShell + .NET System.Windows.Forms 写入 CF_HTML
fn inject_cf_html_powershell(html_fragment: &str) -> Result<&'static str, String> {
    let cf_html = build_cf_html(html_fragment);

    // 写入临时文件（UTF-8 无 BOM，Rust 默认行为）
    let tmp = "/tmp/cw_clipboard.html";
    fs::write(tmp, &cf_html).map_err(|e| format!("临时文件写入失败: {}", e))?;

    // 转换为 Windows 路径
    let wslpath_out = Command::new("wslpath")
        .args(["-w", tmp])
        .output()
        .map_err(|e| format!("wslpath 失败: {}", e))?;

    if !wslpath_out.status.success() {
        let _ = fs::remove_file(tmp);
        return Err("wslpath 路径转换失败".to_string());
    }

    let win_path = String::from_utf8_lossy(&wslpath_out.stdout).trim().to_string();

    // PowerShell 脚本：用 ReadAllBytes 读取原始 UTF-8 字节 → MemoryStream → CF_HTML
    // 关键：绕过 .NET String(UTF-16) 转换，避免系统默认编码(GBK)截断多字节中文
    let ps_script = format!(
        concat!(
            "Add-Type -AssemblyName System.Windows.Forms; ",
            "$bytes = [System.IO.File]::ReadAllBytes('{}'); ",
            "$ms = New-Object System.IO.MemoryStream(,$bytes); ",
            "$d = New-Object System.Windows.Forms.DataObject; ",
            "$d.SetData([System.Windows.Forms.DataFormats]::Html, $ms); ",
            "[System.Windows.Forms.Clipboard]::SetDataObject($d, $true)"
        ),
        win_path.replace('\'', "''")
    );

    let out = Command::new("powershell.exe")
        .args(["-sta", "-NoProfile", "-Command", &ps_script])
        .output()
        .map_err(|e| format!("powershell.exe 执行失败: {}", e))?;

    let _ = fs::remove_file(tmp);

    if out.status.success() {
        Ok("CF_HTML (PowerShell)")
    } else {
        Err(format!(
            "PowerShell 剪贴板设置失败: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// 通过 stdin pipe 向 CLI 工具写入数据
fn pipe_to_cmd(cmd: &str, args: &[&str], data: &[u8]) -> Result<(), ()> {
    let mut child = Command::new(cmd)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| ())?;

    let stdin = child.stdin.as_mut().ok_or(())?;
    stdin.write_all(data).map_err(|_| ())?;
    drop(child.stdin.take());
    child.wait().map_err(|_| ())?;
    Ok(())
}

fn inject_clipboard(html_content: &str) -> Result<&'static str, String> {
    // 1. WSL2 / Windows: CF_HTML via PowerShell（真正的富文本，最优路径）
    if let Ok(tool) = inject_cf_html_powershell(html_content) {
        return Ok(tool);
    }

    // 2. Linux X11: xclip -selection clipboard -t text/html
    if pipe_to_cmd(
        "xclip",
        &["-selection", "clipboard", "-t", "text/html"],
        html_content.as_bytes(),
    )
    .is_ok()
    {
        return Ok("xclip (text/html)");
    }

    // 3. Wayland: wl-copy --type text/html
    if pipe_to_cmd(
        "wl-copy",
        &["--type", "text/html"],
        html_content.as_bytes(),
    )
    .is_ok()
    {
        return Ok("wl-copy (text/html)");
    }

    Err("未找到可用的剪贴板工具 (powershell.exe / xclip / wl-copy)".to_string())
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

    // 5. 调用 LLM (自动重试，最多 3 次)
    let client = Client::new();
    println!("[info] 正在调用 LLM ...");
    let mut markdown = String::new();
    let max_retries = 3;
    for attempt in 1..=max_retries {
        match call_llm(&client, &base_url, &api_key, &model, &style, &idea).await {
            Ok(content) => {
                markdown = content;
                break;
            }
            Err(e) => {
                if attempt < max_retries {
                    eprintln!("[warn] 第 {} 次请求失败: {}，2 秒后重试 ...", attempt, e);
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                } else {
                    fatal(&format!("连续 {} 次请求均失败: {}", max_retries, e));
                }
            }
        }
    }
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
    let html_fragment = md_to_wechat_html(&markdown);
    let html_doc = format!(
        "<!DOCTYPE html>\n<html>\n<head><meta charset=\"utf-8\"></head>\n<body>\n{}\n</body>\n</html>",
        html_fragment
    );
    write_file(&html_path, &html_doc).unwrap_or_else(|e| fatal(&e));
    println!("[done] HTML     → {}", html_path);

    // 6c. 注入剪贴板 (CF_HTML 富文本格式)
    match inject_clipboard(&html_fragment) {
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
