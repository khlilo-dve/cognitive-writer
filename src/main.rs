mod clipboard;
mod error;
mod io;
mod llm;
mod refine;

use clap::{Parser, Subcommand};
use dialoguer::{Input, theme::ColorfulTheme};
use pulldown_cmark::{Options, Parser as MdParser, html};
use reqwest::Client;
use std::process;

use crate::clipboard::inject_clipboard;
use crate::error::AppError;
use crate::io::{extract_idea_slug, list_styles, next_version, read_file, select_style, write_file};
use crate::llm::{call_llm, with_retry};
use crate::refine::{parse_ai_edits, REFINE_SYSTEM_PROMPT};

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
    Generate {
        /// 素材文件路径 (默认: inputs/idea_01.md)
        #[arg(short, long, default_value = "inputs/idea_01.md")]
        input: String,
    },
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

fn fatal(msg: impl std::fmt::Display) -> ! {
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
    match cli.command.unwrap_or(Commands::Generate {
        input: "inputs/idea_01.md".to_string(),
    }) {
        Commands::Generate { input } => run_generate(&input).await,
        Commands::Learn { url } => run_learn(&url).await,
        Commands::Refine { file } => run_refine(&file).await,
    }
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

// ── 骨架-渲染双通道 ─────────────────────────────────────────────────

async fn generate_with_outline(
    client: &Client,
    base_url: &str,
    api_key: &str,
    model: &str,
    style: &str,
    idea: &str,
) -> Result<String, AppError> {
    // Pass 1 — 骨架
    println!("[info] Pass 1: 正在生成大纲骨架 ...");
    let outline = with_retry(3, "大纲生成", || {
        call_llm(client, base_url, api_key, model, OUTLINE_SYSTEM_PROMPT, idea)
    })
    .await?;
    println!("[info] 大纲生成完成, {} 字符", outline.len());

    // Pass 2 — 渲染
    println!("[info] Pass 2: 正在按大纲渲染正文 ...");
    let render_prompt = format!(
        "以下是文章的逻辑大纲，请严格按此结构展开正文：\n\n---\n{}\n---\n\n原始素材：\n\n---\n{}\n---\n\n请根据大纲结构和原始素材，输出完整的 Markdown 正文。",
        outline, idea
    );

    let markdown = with_retry(3, "正文渲染", || {
        call_llm(client, base_url, api_key, model, style, &render_prompt)
    })
    .await?;
    println!("[info] 正文渲染完成, {} 字符", markdown.len());

    Ok(markdown)
}

// ── generate 子命令 ─────────────────────────────────────────────────

async fn run_generate(input: &str) {
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
    .unwrap_or_else(|e| fatal(&e));

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

async fn run_refine(file: &str) {
    // 注意：不再调用 dotenvy::dotenv()，main() 已经调用过了

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

        let label = format!("标记 {}/{}", idx, total);
        let rewritten = with_retry(3, &label, || {
            call_llm(
                &client,
                &base_url,
                &api_key,
                &model,
                REFINE_SYSTEM_PROMPT,
                &user_prompt,
            )
        })
        .await
        .unwrap_or_else(|e| fatal(&e));

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
