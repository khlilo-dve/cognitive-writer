use clap::{Parser, Subcommand};
use dialoguer::{Input, Select, theme::ColorfulTheme};
use pulldown_cmark::{Options, Parser as MdParser, html};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{self, Command, Stdio};

// ── CLI routing (clap derive) ───────────────────────────────────────

#[derive(Parser)]
#[command(name = "cognitive-writer", version, about = "AI 写作 + 风格逆向工具")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// 生成文章（默认行为）
    Generate,
    /// 从 URL 逆向提取写作风格
    Learn {
        /// 目标文章的 URL
        url: String,
    },
    /// 局部重绘：解析 <AI_EDIT> 标记并调用 LLM 重写
    Refine {
        /// 目标 Markdown 文件路径
        file: String,
    },
}

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
        .find_map(|l| {
            let stripped = l.trim_start_matches('#').trim_start();
            stripped
                .strip_prefix("文章主题：")
                .or_else(|| stripped.strip_prefix("文章主题:"))
        })
        .or_else(|| {
            idea.lines()
                .find(|l| l.starts_with('#') && !l.trim_start_matches('#').is_empty())
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

    let parser = MdParser::new_ext(markdown, opts);
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

// ── Jina Reader + fallback strip-tags ────────────────────────────────

fn strip_html_tags(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(ch),
            _ => {}
        }
    }
    // 压缩连续空行
    let mut prev_blank = false;
    result
        .lines()
        .filter(|line| {
            let blank = line.trim().is_empty();
            if blank && prev_blank {
                return false;
            }
            prev_blank = blank;
            true
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

async fn fetch_readable_text(client: &Client, url: &str) -> Result<String, String> {
    let jina_url = format!("https://r.jina.ai/{}", url);
    println!("[info] 正在通过 Jina Reader 抓取 ...");

    let jina_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("创建 HTTP Client 失败: {}", e))?;

    let resp = jina_client
        .get(&jina_url)
        .header("Accept", "text/markdown")
        .header("User-Agent", "cognitive-writer/0.3")
        .send()
        .await;

    match resp {
        Ok(r) if r.status().is_success() => {
            let text = r
                .text()
                .await
                .map_err(|e| format!("读取 Jina 响应体失败: {}", e))?;
            if text.trim().is_empty() {
                return Err("Jina Reader 返回空内容".to_string());
            }
            println!("[info] Jina Reader 抓取成功, {} 字符", text.len());
            Ok(text)
        }
        Ok(r) => {
            eprintln!(
                "[warn] Jina Reader 返回 HTTP {}, fallback 到直接抓取",
                r.status()
            );
            fetch_fallback_plain(client, url).await
        }
        Err(e) => {
            eprintln!("[warn] Jina Reader 请求失败: {}, fallback 到直接抓取", e);
            fetch_fallback_plain(client, url).await
        }
    }
}

async fn fetch_fallback_plain(client: &Client, url: &str) -> Result<String, String> {
    let resp = client
        .get(url)
        .header("User-Agent", "Mozilla/5.0 (compatible; cognitive-writer/0.3)")
        .send()
        .await
        .map_err(|e| format!("HTTP 请求失败: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("HTTP 返回错误状态码: {}", resp.status()));
    }

    let html_body = resp
        .text()
        .await
        .map_err(|e| format!("读取响应体失败: {}", e))?;

    let text = strip_html_tags(&html_body);
    if text.is_empty() {
        return Err("提取的正文内容为空".to_string());
    }
    println!("[info] 降级抓取完成 (strip-tags), {} 字符", text.len());
    Ok(text)
}

// ── Main ────────────────────────────────────────────────────────────

fn fatal(msg: &str) -> ! {
    eprintln!("[error] {}", msg);
    process::exit(1);
}

#[tokio::main]
async fn main() {
    // 加载 .env
    if let Err(e) = dotenvy::dotenv() {
        eprintln!("[warn] 未加载 .env: {} (将使用系统环境变量)", e);
    }

    let cli = Cli::parse();
    match cli.command.unwrap_or(Commands::Generate) {
        Commands::Generate => run_generate().await,
        Commands::Learn { url } => run_learn(&url).await,
        Commands::Refine { file } => run_refine(&file).await,
    }
}

// ── 骨架-渲染双通道 ─────────────────────────────────────────────────

async fn generate_with_outline(
    client: &Client,
    base_url: &str,
    api_key: &str,
    model: &str,
    style: &str,
    idea: &str,
) -> Result<String, String> {
    // Pass 1 — 骨架
    println!("[info] Pass 1: 正在生成大纲骨架 ...");
    let outline = {
        let max_retries = 3;
        let mut result = Err("未执行".to_string());
        for attempt in 1..=max_retries {
            match call_llm(client, base_url, api_key, model, OUTLINE_SYSTEM_PROMPT, idea).await {
                Ok(content) => {
                    result = Ok(content);
                    break;
                }
                Err(e) => {
                    if attempt < max_retries {
                        eprintln!("[warn] 大纲生成第 {} 次失败: {}，2 秒后重试 ...", attempt, e);
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    } else {
                        return Err(format!("大纲生成连续 {} 次失败: {}", max_retries, e));
                    }
                }
            }
        }
        result?
    };
    println!("[info] 大纲生成完成, {} 字符", outline.len());

    // Pass 2 — 渲染
    println!("[info] Pass 2: 正在按大纲渲染正文 ...");
    let render_prompt = format!(
        "以下是文章的逻辑大纲，请严格按此结构展开正文：\n\n---\n{}\n---\n\n原始素材：\n\n---\n{}\n---\n\n请根据大纲结构和原始素材，输出完整的 Markdown 正文。",
        outline, idea
    );

    let markdown = {
        let max_retries = 3;
        let mut result = Err("未执行".to_string());
        for attempt in 1..=max_retries {
            match call_llm(client, base_url, api_key, model, style, &render_prompt).await {
                Ok(content) => {
                    result = Ok(content);
                    break;
                }
                Err(e) => {
                    if attempt < max_retries {
                        eprintln!("[warn] 正文渲染第 {} 次失败: {}，2 秒后重试 ...", attempt, e);
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    } else {
                        return Err(format!("正文渲染连续 {} 次失败: {}", max_retries, e));
                    }
                }
            }
        }
        result?
    };
    println!("[info] 正文渲染完成, {} 字符", markdown.len());

    Ok(markdown)
}

// ── generate 子命令 ─────────────────────────────────────────────────

async fn run_generate() {
    let api_key = env_var("API_KEY").unwrap_or_else(|e| fatal(&e));
    let base_url = env_var_or("BASE_URL", "https://api.openai.com/v1");
    let model = env_var_or("MODEL", "gpt-4o");

    println!("[info] BASE_URL = {}", base_url);
    println!("[info] MODEL    = {}", model);

    // 风格选择
    let styles = list_styles("styles").unwrap_or_else(|e| fatal(&e));
    let idx = select_style(&styles).unwrap_or_else(|e| fatal(&e));
    let style_path = &styles[idx];
    let style = read_file(style_path).unwrap_or_else(|e| fatal(&e));

    println!(
        "[info] 风格: {}",
        style_path.strip_prefix("styles/").unwrap_or(style_path)
    );

    // 读取素材
    let idea = read_file("inputs/idea_01.md").unwrap_or_else(|e| fatal(&e));
    if idea.trim().is_empty() {
        fatal("inputs/idea_01.md 内容为空，请先写入创作素材");
    }
    println!("[info] 素材 {} 字符 | 风格 {} 字符", idea.len(), style.len());

    // 双通道生成：骨架 → 渲染
    let client = Client::new();
    let markdown = generate_with_outline(&client, &base_url, &api_key, &model, &style, &idea)
        .await
        .unwrap_or_else(|e| fatal(&e));

    // 双轨输出
    let slug = extract_idea_slug(&idea);
    let ver = next_version("outputs", &slug);
    let md_path = format!("outputs/{}_v{}.md", slug, ver);
    let html_path = format!("outputs/{}_v{}.html", slug, ver);

    // 存档 Markdown
    write_file(&md_path, &markdown).unwrap_or_else(|e| fatal(&e));
    println!("[done] Markdown → {}", md_path);

    // 转换 HTML + 存档
    let html_fragment = md_to_wechat_html(&markdown);
    let html_doc = format!(
        "<!DOCTYPE html>\n<html>\n<head><meta charset=\"utf-8\"></head>\n<body>\n{}\n</body>\n</html>",
        html_fragment
    );
    write_file(&html_path, &html_doc).unwrap_or_else(|e| fatal(&e));
    println!("[done] HTML     → {}", html_path);

    // 注入剪贴板 (CF_HTML 富文本格式)
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

// ── Outline system prompt (Pass 1: 骨架) ────────────────────────────

const OUTLINE_SYSTEM_PROMPT: &str = r#"你是一个写作结构设计专家。
用户会给你一份创作素材，你需要输出一份文章逻辑大纲。

要求：
1. 用 Markdown 层级列表（- / 1. 2. 3.）
2. 每个节点写清该段的【核心论点】和【支撑素材/案例方向】
3. 标注段落之间的逻辑衔接关系（递进/转折/并列/因果）
4. 控制在 300-500 字以内
5. 不要写正文，只输出骨架"#;

// ── learn 子命令 ────────────────────────────────────────────────────

const LEARN_SYSTEM_PROMPT: &str = r#"你是一个写作风格分析专家。用户会给你一篇完整的文章正文，你需要逆向分析该文章的写作风格，并输出一份可以直接作为 system prompt 使用的「风格指令文档」。

输出要求：
1. 用 Markdown 格式
2. 涵盖以下维度（如果文章中体现了的话）：
   - 整体风格定位（如"理性 + 隐喻"、"口语化 + 犀利"等）
   - 标题策略（标题长度、是否用问句/反问/数字等）
   - 开头模式（故事切入、金句开头、直接观点等）
   - 段落节奏（长短交替、短段密集等）
   - 句式特征（长句/短句偏好、排比、设问等）
   - 论证手法（类比、举例、数据引用、反直觉等）
   - 情绪基调（冷静、激昂、反讽、温暖等）
   - 结尾策略（升华、行动号召、开放式提问等）
   - 用词偏好（口语/书面、中英混用、领域术语等）
   - 读者互动方式（如果有的话）
3. 每个维度给出具体的示例句子或段落片段作为佐证
4. 最后给出一段可直接作为 system prompt 的「风格复刻指令」

注意：不要评价文章质量，只做风格提取和描述。"#;

async fn run_learn(url: &str) {
    let api_key = env_var("API_KEY").unwrap_or_else(|e| fatal(&e));
    let base_url = env_var_or("BASE_URL", "https://api.openai.com/v1");
    let model = env_var_or("MODEL", "gpt-4o");

    println!("[info] BASE_URL = {}", base_url);
    println!("[info] MODEL    = {}", model);

    // 1. 通过 Jina Reader 抓取正文（失败时降级为 strip-tags）
    println!("[info] 目标 URL: {}", url);
    let client = Client::new();
    let article_text = fetch_readable_text(&client, url)
        .await
        .unwrap_or_else(|e| fatal(&e));

    // 3. 调用 LLM 逆向分析风格
    println!("[info] 正在调用 LLM 分析写作风格 ...");
    let style_analysis = call_llm(
        &client,
        &base_url,
        &api_key,
        &model,
        LEARN_SYSTEM_PROMPT,
        &article_text,
    )
    .await
    .unwrap_or_else(|e| fatal(&format!("LLM 调用失败: {}", e)));

    println!("[info] LLM 返回 {} 字符", style_analysis.len());

    // 4. 用户输入文件名
    let name: String = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("请为该风格命名（将保存为 styles/<name>.md）")
        .validate_with(|input: &String| -> Result<(), String> {
            let s = input.trim();
            if s.is_empty() {
                return Err("名称不能为空".to_string());
            }
            if s.contains('/') || s.contains('\\') || s.contains('\0') {
                return Err("名称不能包含路径分隔符".to_string());
            }
            Ok(())
        })
        .interact_text()
        .unwrap_or_else(|e| fatal(&format!("输入读取失败: {}", e)));

    let name = name.trim();

    // 5. 保存风格文件
    let path = format!("styles/{}.md", name);
    write_file(&path, &style_analysis).unwrap_or_else(|e| fatal(&e));
    println!("[done] 风格已保存 → {}", path);
    println!();
    println!("  现在可以用 `cargo run -- generate` 选择该风格来生成文章");
}

// ── refine 子命令 — 局部重绘 ─────────────────────────────────────

const REFINE_SYSTEM_PROMPT: &str = r#"你是一个严苛的文本重构引擎。
请根据用户给出的修改指令，重写指定的文本片段。

要求：
1. 只输出重写后的文本
2. 不要带有任何解释、问候或 Markdown 代码块包裹
3. 必须与原有的行文风格无缝衔接
4. 严格遵循修改指令的要求"#;

struct AiEdit {
    instruction: String,
    original: String,
    full_match: String,
}

fn parse_ai_edits(content: &str) -> Result<Vec<AiEdit>, String> {
    let mut edits = Vec::new();
    let mut pos = 0;

    loop {
        // 1. 找开标签起始
        let open_start = match content[pos..].find("<AI_EDIT ") {
            Some(i) => pos + i,
            None => break,
        };

        // 2. 提取 instruction="..."
        let inst_key = "instruction=\"";
        let inst_start = match content[open_start..].find(inst_key) {
            Some(i) => open_start + i + inst_key.len(),
            None => return Err(format!("AI_EDIT 标签缺少 instruction 属性 (位置 {})", open_start)),
        };
        let inst_end = match content[inst_start..].find('"') {
            Some(i) => inst_start + i,
            None => return Err(format!("instruction 属性引号未闭合 (位置 {})", inst_start)),
        };
        let instruction = content[inst_start..inst_end].to_string();

        // 3. 找开标签结束 '>'
        let tag_end = match content[inst_end..].find('>') {
            Some(i) => inst_end + i + 1, // +1 跳过 '>' 本身
            None => return Err(format!("AI_EDIT 开标签未闭合 (位置 {})", open_start)),
        };

        // 4. 找闭标签
        let close_tag = "</AI_EDIT>";
        let close_start = match content[tag_end..].find(close_tag) {
            Some(i) => tag_end + i,
            None => return Err(format!("未找到匹配的 </AI_EDIT> (位置 {})", open_start)),
        };
        let close_end = close_start + close_tag.len();

        let original = content[tag_end..close_start].to_string();
        let full_match = content[open_start..close_end].to_string();

        edits.push(AiEdit {
            instruction,
            original,
            full_match,
        });

        pos = close_end;
    }

    if edits.is_empty() {
        return Err("未找到 AI_EDIT 标记".to_string());
    }
    Ok(edits)
}

async fn run_refine(file: &str) {
    // 1. 加载环境配置
    if let Err(e) = dotenvy::dotenv() {
        eprintln!("[warn] 未加载 .env: {} (将使用系统环境变量)", e);
    }

    let api_key = env_var("API_KEY").unwrap_or_else(|e| fatal(&e));
    let base_url = env_var_or("BASE_URL", "https://api.openai.com/v1");
    let model = env_var_or("MODEL", "gpt-4o");

    println!("[info] BASE_URL = {}", base_url);
    println!("[info] MODEL    = {}", model);

    // 2. 读取目标文件
    let mut content = read_file(file).unwrap_or_else(|e| fatal(&e));
    println!("[info] 已读取 {}, {} 字符", file, content.len());

    // 3. 解析 AI_EDIT 标记
    let edits = parse_ai_edits(&content).unwrap_or_else(|e| fatal(&e));
    let total = edits.len();
    println!("[info] 发现 {} 个 AI_EDIT 标记", total);

    // 4. 逐个 LLM 重写
    let client = Client::new();
    for (i, edit) in edits.iter().enumerate() {
        let idx = i + 1;
        let user_prompt = format!(
            "修改指令：{}\n\n原文本：\n{}",
            edit.instruction, edit.original
        );

        let mut rewritten = Err("未执行".to_string());
        for attempt in 1..=3 {
            match call_llm(&client, &base_url, &api_key, &model, REFINE_SYSTEM_PROMPT, &user_prompt).await {
                Ok(text) => {
                    rewritten = Ok(text);
                    break;
                }
                Err(e) => {
                    if attempt < 3 {
                        eprintln!("[warn] 标记 {}/{} 第 {} 次失败: {}，2 秒后重试 ...", idx, total, attempt, e);
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    } else {
                        fatal(&format!("标记 {}/{} 连续 3 次失败: {}", idx, total, e));
                    }
                }
            }
        }
        let rewritten = rewritten.unwrap_or_else(|e| fatal(&e));
        content = content.replacen(&edit.full_match, &rewritten, 1);
        println!("[info] 标记 {}/{} 替换完成", idx, total);
    }

    // 5. 版本命名 + 三轨输出
    let slug = extract_idea_slug(&content);
    let ver = next_version("outputs", &slug);
    let md_path = format!("outputs/{}_v{}.md", slug, ver);
    let html_path = format!("outputs/{}_v{}.html", slug, ver);

    write_file(&md_path, &content).unwrap_or_else(|e| fatal(&e));
    println!("[done] Markdown → {}", md_path);

    let html_fragment = md_to_wechat_html(&content);
    let html_doc = format!(
        "<!DOCTYPE html>\n<html>\n<head><meta charset=\"utf-8\"></head>\n<body>\n{}\n</body>\n</html>",
        html_fragment
    );
    write_file(&html_path, &html_doc).unwrap_or_else(|e| fatal(&e));
    println!("[done] HTML     → {}", html_path);

    match inject_clipboard(&html_fragment) {
        Ok(tool) => {
            println!("[done] 富文本已注入剪贴板 (via {})", tool);
            println!();
            println!("  局部重绘完成，富文本已注入剪贴板，请直接前往微信粘贴 (Ctrl+V)");
        }
        Err(e) => {
            eprintln!("[warn] {}", e);
            println!();
            println!(
                "  局部重绘完成。剪贴板不可用，请用浏览器打开 {} 后手动复制",
                html_path
            );
        }
    }
}
