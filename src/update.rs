// ── update 子命令 — 根据指令重写文章 ─────────────────────────────

use dialoguer::Input;
use dialoguer::theme::ColorfulTheme;
use pulldown_cmark::{Options, Parser as MdParser, html};
use reqwest::Client;
use std::process;

use crate::clipboard::inject_clipboard;
use crate::io::{extract_idea_slug, next_version, read_file, write_file};
use crate::llm::{call_llm, new_spinner, with_retry};

// ── Update system prompt ────────────────────────────────────────────

const UPDATE_SYSTEM_PROMPT: &str = r#"你是严苛的文本编辑。用户会给你一篇完整的文章和一条修改指令，你需要根据修改指令重写整篇文章。

要求：
1. 保持原文的整体结构和段落数量
2. 只修改与指令相关的部分，其余内容保持不变
3. 直接输出完整的修改后 Markdown，不要任何解释、说明或代码块包裹
4. 输出必须是可发布的最终版本"#;

// ── 辅助函数 ──────────────────────────────────────────────────────

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
    format!("<section style=\"font-size:15px;line-height:2;color:#333;\">{raw}</section>")
}

fn env_var(key: &str) -> Result<String, String> {
    std::env::var(key).map_err(|_| format!("环境变量 `{}` 未设置，请检查 .env 文件", key))
}

fn env_var_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn fatal(msg: impl std::fmt::Display) -> ! {
    eprintln!("[error] {}", msg);
    process::exit(1);
}

// ── update 子命令入口 ─────────────────────────────────────────────

pub async fn run_update(file: &str, instruction: Option<String>, no_clipboard: bool) {
    let api_key = env_var("API_KEY").unwrap_or_else(|e| fatal(&e));
    let base_url = env_var_or("BASE_URL", "https://api.openai.com/v1");
    let model = env_var_or("MODEL", "gpt-4o");

    println!("[info] BASE_URL = {}", base_url);
    println!("[info] MODEL    = {}", model);

    // 1. 读取目标文件
    let content = read_file(file).unwrap_or_else(|e| fatal(&e));
    if content.trim().is_empty() {
        fatal("目标文件内容为空，无法重写");
    }
    println!("[info] 已读取 {}, {} 字符", file, content.len());

    // 2. 获取修改指令
    let instruction = instruction.unwrap_or_else(|| {
        Input::<String>::with_theme(&ColorfulTheme::default())
            .with_prompt("请输入修改指令")
            .interact_text()
            .unwrap_or_else(|e| fatal(&format!("输入读取失败: {}", e)))
    });
    let instruction = instruction.trim();
    if instruction.is_empty() {
        fatal("修改指令不能为空");
    }

    // 3. 拼接 prompt 并调用 LLM 重写
    let user_prompt = format!(
        "以下是需要修改的文章全文：\n\n---\n{}\n---\n\n修改指令：{}\n\n请根据修改指令输出修改后的完整 Markdown 正文。",
        content, instruction
    );

    let client = Client::new();
    let spinner = new_spinner("正在重写文章...");
    let rewritten = with_retry(3, "文章重写", || {
        call_llm(
            &client,
            &base_url,
            &api_key,
            &model,
            UPDATE_SYSTEM_PROMPT,
            &user_prompt,
        )
    })
    .await
    .unwrap_or_else(|e| {
        spinner.finish_with_message("重写失败");
        fatal(&e)
    });
    spinner.finish_with_message("重写完成");
    println!("[info] 重写完成, {} 字符", rewritten.len());

    // 4. 版本管理 + 三轨输出
    let slug = extract_idea_slug(&rewritten);
    let ver = next_version("outputs", &slug);
    let md_path = format!("outputs/{}_v{}.md", slug, ver);
    let html_path = format!("outputs/{}_v{}.html", slug, ver);

    // Markdown 归档
    write_file(&md_path, &rewritten).unwrap_or_else(|e| fatal(&e));
    println!("[done] Markdown → {}", md_path);

    // HTML 转换 + 归档
    let html_fragment = md_to_wechat_html(&rewritten);
    let html_doc = format!(
        "<!DOCTYPE html>\n<html>\n<head><meta charset=\"utf-8\"></head>\n<body>\n{}\n</body>\n</html>",
        html_fragment
    );
    write_file(&html_path, &html_doc).unwrap_or_else(|e| fatal(&e));
    println!("[done] HTML     → {}", html_path);

    // 剪贴板注入
    if no_clipboard {
        println!("[info] 已跳过剪贴板注入 (--no-clipboard)");
        println!();
        println!(
            "  文章已重写并存档，请用浏览器打开 {} 后手动复制",
            html_path
        );
    } else {
        match inject_clipboard(&html_fragment) {
            Ok(tool) => {
                println!("[done] 富文本已注入剪贴板 (via {})", tool);
                println!();
                println!("  文章已重写并存档，富文本已注入剪贴板，请直接前往微信粘贴 (Ctrl+V)");
            }
            Err(e) => {
                eprintln!("[warn] {}", e);
                println!();
                println!(
                    "  文章已重写并存档。剪贴板不可用，请用浏览器打开 {} 后手动复制",
                    html_path
                );
            }
        }
    }
}
