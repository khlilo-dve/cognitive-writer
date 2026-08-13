use std::process;

use reqwest::Client;
use dialoguer::{Input, theme::ColorfulTheme};

use crate::io::write_file;
use crate::llm::{call_llm, new_spinner};

// ── Env helpers ─────────────────────────────────────────────────────

fn env_var(key: &str) -> Result<String, String> {
    std::env::var(key).map_err(|_| format!("环境变量 `{}` 未设置，请检查 .env 文件", key))
}

fn env_var_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

// ── Fatal ───────────────────────────────────────────────────────────

fn fatal(msg: impl std::fmt::Display) -> ! {
    eprintln!("[error] {}", msg);
    process::exit(1);
}

// ── Jina Reader + fallback strip-tags ───────────────────────────────

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

// ── Learn system prompt ─────────────────────────────────────────────

const LEARN_PROMPT_PATH: &str = "prompts/learn_style.md";

fn load_learn_prompt() -> Result<String, String> {
    let prompt = std::fs::read_to_string(LEARN_PROMPT_PATH).map_err(|e| {
        format!("无法读取逆向风格提取提示词 `{LEARN_PROMPT_PATH}`: {e}")
    })?;

    if prompt.trim().is_empty() {
        return Err(format!("逆向风格提取提示词 `{LEARN_PROMPT_PATH}` 为空"));
    }

    Ok(prompt)
}

// ── learn 子命令入口 ───────────────────────────────────────────────

pub async fn run_learn(url: &str) {
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

    // 2. 读取提示词并调用 LLM 逆向分析风格
    let system_prompt = load_learn_prompt().unwrap_or_else(|e| fatal(&e));
    let spinner = new_spinner("正在分析写作风格...");
    let style_analysis = call_llm(
        &client,
        &base_url,
        &api_key,
        &model,
        &system_prompt,
        &article_text,
    )
    .await
    .unwrap_or_else(|e| {
        spinner.finish_with_message("分析失败");
        fatal(&e)
    });
    spinner.finish_with_message("分析完成");
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
#[cfg(test)]
mod tests {
    use super::*;

    // ── strip_html_tags ─────────────────────────────────────────

    #[test]
    fn test_load_learn_prompt_from_external_file() {
        let prompt = load_learn_prompt().expect("external learn prompt should be readable");
        assert!(!prompt.trim().is_empty());
        assert!(prompt.contains("写作风格分析专家"));
        assert!(prompt.contains("不要评价文章质量"));
        // 新版契约：直接产出精简执行 spec，而非冗长分析报告
        assert!(prompt.contains("## 定位"));
        assert!(prompt.contains("## 禁止"));
        assert!(prompt.contains("不要引用原文"));
        assert!(prompt.contains("600 字"));
    }

    #[test]
    fn test_strip_html_simple_tag() {
        assert_eq!(strip_html_tags("<p>text</p>"), "text");
    }

    #[test]
    fn test_strip_html_nested_tags() {
        assert_eq!(strip_html_tags("<div><p>text</p></div>"), "text");
    }

    #[test]
    fn test_strip_html_plain_text_unchanged() {
        assert_eq!(strip_html_tags("plain text here"), "plain text here");
    }

    #[test]
    fn test_strip_html_removes_tag_with_attributes() {
        assert_eq!(strip_html_tags("<a href=\"url\">link</a>"), "link");
    }

    #[test]
    fn test_strip_html_compresses_consecutive_blank_lines() {
        let input = "line1\n\n\nline2";
        let result = strip_html_tags(input);
        // 三个空行被压缩，不应存在三连 \n
        assert!(!result.contains("\n\n\n"), "blank lines should be compressed");
    }

    #[test]
    fn test_strip_html_mixed_content() {
        let input = "<h1>Title</h1>\n<p>Paragraph with <b>bold</b> text.</p>";
        let result = strip_html_tags(input);
        assert!(result.contains("Title"));
        assert!(result.contains("bold"));
        assert!(result.contains("text"));
        assert!(!result.contains('<'));
        assert!(!result.contains('>'));
    }

    #[test]
    fn test_strip_html_empty_input() {
        let result = strip_html_tags("");
        assert_eq!(result, "");
    }

}
