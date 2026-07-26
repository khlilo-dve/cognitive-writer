use reqwest::Client;
use std::process;

use pulldown_cmark::{Options, Parser as MdParser, html};

use crate::clipboard::inject_clipboard;
use crate::error::AppError;
use crate::io::{extract_idea_slug, list_styles, next_version, read_file, select_style, write_file};
use crate::llm::{call_llm, new_spinner, with_retry};

// ── Env helpers ─────────────────────────────────────────────────────

fn env_var(key: &str) -> Result<String, String> {
    std::env::var(key).map_err(|_| format!("环境变量 `{}` 未设置，请检查 .env 文件", key))
}

fn env_var_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
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

// ── Fatal ──────────────────────────────────────────────────────────

fn fatal(msg: impl std::fmt::Display) -> ! {
    eprintln!("[error] {}", msg);
    process::exit(1);
}

// ── Outer system prompt (Pass 1: 骨架) ────────────────────────────

const OUTLINE_SYSTEM_PROMPT: &str = r#"你是一个写作结构设计专家。
用户会给你一份创作素材，你需要输出一份文章逻辑大纲。

要求：
1. 用 Markdown 层级列表（- / 1. 2. 3.）
2. 每个节点写清该段的【核心论点】和【支撑素材/案例方向】
3. 标注段落之间的逻辑衔接关系（递进/转折/并列/因果）
4. 控制在 300-500 字以内
5. 不要写正文，只输出骨架"#;

// ── REPL 模式：Pass 1 only — 生成骨架 ────────────────────────────

pub async fn generate_outline(
    client: &Client,
    base_url: &str,
    api_key: &str,
    model: &str,
    idea: &str,
) -> Result<String, AppError> {
    with_retry(3, "大纲生成", || {
        call_llm(client, base_url, api_key, model, OUTLINE_SYSTEM_PROMPT, idea)
    })
    .await
}

// ── REPL 模式：Pass 2 only — 从骨架渲染全文 ──────────────────────

pub async fn render_fulltext(
    client: &Client,
    base_url: &str,
    api_key: &str,
    model: &str,
    style: &str,
    outline: &str,
    idea: &str,
) -> Result<String, AppError> {
    let render_prompt = format!(
        "以下是文章的逻辑大纲，请严格按此结构展开正文：\n\n---\n{}\n---\n\n原始素材：\n\n---\n{}\n---\n\n请根据大纲结构和原始素材，输出完整的 Markdown 正文。",
        outline, idea
    );

    with_retry(3, "正文渲染", || {
        call_llm(client, base_url, api_key, model, style, &render_prompt)
    })
    .await
}

// ── 骨架-渲染双通道（CLI 模式，含 spinner UI）─────────────────────

async fn generate_with_outline(
    client: &Client,
    base_url: &str,
    api_key: &str,
    model: &str,
    style: &str,
    idea: &str,
) -> Result<String, AppError> {
    // Pass 1 — 骨架
    let spinner = new_spinner("正在生成大纲骨架...");
    let outline = with_retry(3, "大纲生成", || {
        call_llm(client, base_url, api_key, model, OUTLINE_SYSTEM_PROMPT, idea)
    })
    .await?;
    spinner.finish_with_message("大纲生成完成");
    println!("[info] 大纲 {} 字符", outline.len());

    // Pass 2 — 渲染
    let render_prompt = format!(
        "以下是文章的逻辑大纲，请严格按此结构展开正文：\n\n---\n{}\n---\n\n原始素材：\n\n---\n{}\n---\n\n请根据大纲结构和原始素材，输出完整的 Markdown 正文。",
        outline, idea
    );

    let spinner = new_spinner("正在渲染正文...");
    let markdown = with_retry(3, "正文渲染", || {
        call_llm(client, base_url, api_key, model, style, &render_prompt)
    })
    .await?;
    spinner.finish_with_message("正文渲染完成");
    println!("[info] 正文 {} 字符", markdown.len());

    Ok(markdown)
}

// ── generate 子命令入口（CLI 模式）────────────────────────────────

pub async fn run_generate(input: &str, no_clipboard: bool) {
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
    println!("[info] 素材文件: {}", input);

    // 读取素材
    let idea = read_file(input).unwrap_or_else(|e| fatal(&e));
    if idea.trim().is_empty() {
        fatal(&format!("{} 内容为空，请先写入创作素材", input));
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
    if no_clipboard {
        println!("[info] 跳过剪贴板注入 (--no-clipboard)");
    } else {
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
}
