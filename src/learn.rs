use dialoguer::{Input, theme::ColorfulTheme};
use reqwest::Client;

use crate::error::AppError;
use crate::llm::{call_llm, new_spinner};
use crate::styles::save_new_style;
use crate::utils::{env_var, env_var_or};

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

async fn fetch_readable_text(client: &Client, url: &str) -> Result<String, AppError> {
    let jina_url = format!("https://r.jina.ai/{}", url);
    println!("[info] 正在通过 Jina Reader 抓取 ...");

    let jina_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| AppError::Network(e.to_string()))?;

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
                .map_err(|e| AppError::Network(e.to_string()))?;
            if text.trim().is_empty() {
                return Err(AppError::Parse("Jina Reader 返回空内容".into()));
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

async fn fetch_fallback_plain(client: &Client, url: &str) -> Result<String, AppError> {
    let resp = client
        .get(url)
        .header(
            "User-Agent",
            "Mozilla/5.0 (compatible; cognitive-writer/0.3)",
        )
        .send()
        .await
        .map_err(|e| AppError::Network(e.to_string()))?;

    if !resp.status().is_success() {
        return Err(AppError::Network(format!(
            "HTTP 返回错误状态码: {}",
            resp.status()
        )));
    }

    let html_body = resp
        .text()
        .await
        .map_err(|e| AppError::Network(e.to_string()))?;

    let text = strip_html_tags(&html_body);
    if text.is_empty() {
        return Err(AppError::Parse("提取的正文内容为空".into()));
    }
    println!("[info] 降级抓取完成 (strip-tags), {} 字符", text.len());
    Ok(text)
}

// ── Learn system prompt ─────────────────────────────────────────────

const LEARN_PROMPT_PATH: &str = "prompts/learn_style.md";

fn load_learn_prompt() -> Result<String, AppError> {
    let prompt = std::fs::read_to_string(LEARN_PROMPT_PATH)
        .map_err(|e| AppError::FileRead(format!("{LEARN_PROMPT_PATH}: {e}")))?;

    if prompt.trim().is_empty() {
        return Err(AppError::FileRead(format!("{LEARN_PROMPT_PATH} 为空")));
    }

    Ok(prompt)
}

// ── learn 子命令入口 ───────────────────────────────────────────────

/// 提取阶段不读取 stdin；CLI 和 REPL 共用。
pub async fn extract_style(url: &str) -> Result<String, AppError> {
    let api_key = env_var("API_KEY")?;
    let base_url = env_var_or("BASE_URL", "https://api.openai.com/v1");
    let model = env_var_or("MODEL", "gpt-4o");

    println!("[info] BASE_URL = {}", base_url);
    println!("[info] MODEL    = {}", model);

    // 1. 通过 Jina Reader 抓取正文（失败时降级为 strip-tags）
    println!("[info] 目标 URL: {}", url);
    let client = Client::new();
    let article_text = fetch_readable_text(&client, url).await?;

    // 2. 读取提示词并调用 LLM 逆向分析风格
    let system_prompt = load_learn_prompt()?;
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
    .map_err(|e| {
        spinner.finish_with_message("分析失败");
        e
    })?;
    spinner.finish_with_message("分析完成");
    println!("[info] LLM 返回 {} 字符", style_analysis.len());

    if style_analysis.trim().is_empty() {
        return Err(AppError::Parse("提取风格为空".into()));
    }
    Ok(style_analysis)
}

/// CLI 交互包装；REPL 直接调用 extract_style，命名交由主循环。
pub async fn run_learn(url: &str) -> Result<(), AppError> {
    let content = extract_style(url).await?;
    loop {
        let name: String = Input::with_theme(&ColorfulTheme::default())
            .with_prompt("请为该风格命名（输入 /cancel 取消）")
            .interact_text()
            .map_err(|e| AppError::Parse(format!("输入读取失败: {e}")))?;
        if name.trim() == "/cancel" {
            return Ok(());
        }
        match save_new_style("styles", &name, &content) {
            Ok(path) => {
                println!("[done] 风格已保存 → {}", path.display());
                return Ok(());
            }
            Err(e) => eprintln!("[error] {e}"),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    // ── strip_html_tags ─────────────────────────────────────────

    #[test]
    fn test_load_learn_prompt_from_external_file() {
        let prompt = load_learn_prompt().expect("external learn prompt should be readable");
        assert!(!prompt.trim().is_empty());
        assert!(prompt.contains("写作风格逆向工程专家") || prompt.contains("写作风格分析专家"));
        assert!(prompt.contains("提炼机制而非总结内容") || prompt.contains("不要评价文章质量"));
        // 新版契约：直接产出精简执行 spec，而非冗长分析报告
        assert!(prompt.contains("## 定位"));
        assert!(prompt.contains("## 禁止"));
        assert!(prompt.contains("零污染抽象") || prompt.contains("不要引用原文"));
        assert!(prompt.contains("600 字") || prompt.contains("600"));
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
        assert!(
            !result.contains("\n\n\n"),
            "blank lines should be compressed"
        );
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
