use dialoguer::{Input, theme::ColorfulTheme};
use reqwest::Client;

use crate::error::AppError;
use crate::llm::{call_llm_with_format, json_response_format, new_spinner};
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
            println!("[info] Jina Reader 抓取成功, {} 字符", text.chars().count());
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
    println!(
        "[info] 降级抓取完成 (strip-tags), {} 字符",
        text.chars().count()
    );
    Ok(text)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LearningMethod {
    Legacy,
    DirectFree,
    TwoStage,
}

fn learning_method() -> Result<LearningMethod, AppError> {
    parse_learning_method(std::env::var("STYLE_LEARNING_METHOD").as_deref().ok())
}

fn parse_learning_method(value: Option<&str>) -> Result<LearningMethod, AppError> {
    match value.unwrap_or("direct_free") {
        "legacy" => Ok(LearningMethod::Legacy),
        "direct_free" => Ok(LearningMethod::DirectFree),
        "two_stage" => Ok(LearningMethod::TwoStage),
        _ => Err(AppError::EnvVar(
            "STYLE_LEARNING_METHOD须为legacy、direct_free或two_stage".into(),
        )),
    }
}

/// Shared raw-text learning entry for CLI/REPL and legacy batch evaluators.
pub async fn extract_style_text(
    client: &Client,
    base_url: &str,
    api_key: &str,
    model: &str,
    article: &str,
) -> Result<String, AppError> {
    let root = std::path::Path::new("outputs/style-learning").join(format!(
        "{}-{}",
        chrono::Utc::now().format("%Y%m%dT%H%M%S%9fZ"),
        std::process::id()
    ));
    extract_style_text_at(
        client,
        base_url,
        api_key,
        model,
        article,
        &root,
        learning_method()?,
    )
    .await
}

fn write_audit(path: &std::path::Path, value: &impl serde::Serialize) -> Result<(), AppError> {
    let content =
        serde_json::to_string_pretty(value).map_err(|e| AppError::Parse(e.to_string()))?;
    crate::io::write_file_atomic(path, &content)
}

async fn extract_style_text_at(
    client: &Client,
    base_url: &str,
    api_key: &str,
    model: &str,
    article: &str,
    root: &std::path::Path,
    method: LearningMethod,
) -> Result<String, AppError> {
    std::fs::create_dir_all(root)?;
    let audit = root;
    let call = |stage: String, system: String, user: String| async move {
        let path = audit.join(format!("{stage}.json"));
        let format = if matches!(method, LearningMethod::Legacy) {
            None
        } else {
            json_response_format(model)
        };
        write_audit(
            &path,
            &serde_json::json!({
                "status":"pending", "model":model, "system":system, "user":user, "response_format":format.as_ref()
            }),
        )?;
        let response = call_llm_with_format(
            client,
            base_url,
            api_key,
            model,
            &system,
            &user,
            format.as_ref(),
        )
        .await;
        write_audit(
            &path,
            &serde_json::json!({
                "status":if response.is_ok() {"received"} else {"failed"},
                "model":model, "system":system, "user":user, "response_format":format.as_ref(),
                "response":response.as_ref().ok(),
                "error":response.as_ref().err().map(|_| "模型请求失败，未记录端点或HTTP正文")
            }),
        )?;
        response
    };
    let result = match method {
        LearningMethod::Legacy => crate::style_learning::extract_legacy_with(article, call).await,
        LearningMethod::DirectFree => {
            let prompt = std::fs::read_to_string("prompts/learn_style_direct.md")?;
            crate::style_learning::extract_direct_with(article, &prompt, call).await
        }
        LearningMethod::TwoStage => {
            let prompts = crate::style_learning::LearningPrompts::load()?;
            crate::style_learning::extract_with(article, &prompts, call).await
        }
    };
    match result {
        Ok(learned) => {
            write_audit(&root.join("result.json"), &learned)?;
            for warning in &learned.warnings {
                eprintln!("[warn] {warning}");
            }
            println!("[info] 学习记录 → {}", root.display());
            Ok(learned.spec)
        }
        Err(error) => {
            write_audit(
                &root.join("result.json"),
                &serde_json::json!({
                    "status":"failed", "message":"学习未通过或模型调用失败；阶段响应保留用于诊断，未保存正式风格"
                }),
            )?;
            eprintln!("[info] 未保存正式风格；阶段记录 → {}", root.display());
            Err(error)
        }
    }
}

// ── learn 子命令入口 ───────────────────────────────────────────────

/// 提取阶段不读取 stdin；CLI 和 REPL 共用。
pub async fn extract_style(url: &str) -> Result<String, AppError> {
    let api_key = env_var("API_KEY")?;
    let base_url = env_var_or("BASE_URL", "https://api.openai.com/v1");
    let model = env_var_or("MODEL", "gpt-4o");

    println!("[info] MODEL    = {}", model);

    // 1. 通过 Jina Reader 抓取正文（失败时降级为 strip-tags）
    println!("[info] 目标 URL: {}", url);
    let client = Client::new();
    let article_text = fetch_readable_text(&client, url).await?;

    let spinner = new_spinner("正在学习风格并校验结果...");
    let result = extract_style_text(&client, &base_url, &api_key, &model, &article_text).await;
    spinner.finish_with_message(if result.is_ok() {
        "分析完成"
    } else {
        "分析失败"
    });
    result
}

/// CLI 交互包装；REPL 直接调用 extract_style，命名交由主循环。
pub async fn run_learn(url: &str) -> Result<(), AppError> {
    let content = extract_style(url).await?;
    crate::spec_quality::validate_execution_spec(&content)?;
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

    #[test]
    fn default_learning_and_explicit_compatibility() {
        assert_eq!(
            parse_learning_method(None).unwrap(),
            LearningMethod::DirectFree
        );
        for (name, method) in [
            ("legacy", LearningMethod::Legacy),
            ("direct_free", LearningMethod::DirectFree),
            ("two_stage", LearningMethod::TwoStage),
        ] {
            assert_eq!(parse_learning_method(Some(name)).unwrap(), method);
        }
        assert!(parse_learning_method(Some("unknown")).is_err());
    }

    #[tokio::test]
    async fn live_text_entry_saves_only_execution_text_and_audits_without_credentials() {
        use std::io::{Read, Write};
        let analysis = serde_json::json!({"features":[{"observation":"描述", "evidence":["这是原文。"],
                "rule":"用短句收束。", "condition":"判断结束时", "support":"local"}], "uncertain":[]}).to_string();
        let compiled =
            serde_json::json!({"spec":"先铺陈判断，再用短句收束。", "evidence_insufficient":true})
                .to_string();
        for (method, model, responses) in [
            (
                LearningMethod::TwoStage,
                "fixture-model",
                vec![analysis, compiled.clone()],
            ),
            (
                parse_learning_method(None).unwrap(),
                "mimo-v2.6-pro",
                vec![compiled],
            ),
            (
                LearningMethod::Legacy,
                "mimo-v2.6-pro",
                vec!["先铺陈判断，再用短句收束。".to_string()],
            ),
        ] {
            let expected_calls = responses.len();
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let addr = listener.local_addr().unwrap();
            let server = std::thread::spawn(move || {
                let mut requests = Vec::new();
                for response in responses {
                    let (mut stream, _) = listener.accept().unwrap();
                    stream
                        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                        .unwrap();
                    let mut bytes = Vec::new();
                    loop {
                        let mut chunk = [0; 4096];
                        let n = stream.read(&mut chunk).unwrap();
                        assert!(n > 0);
                        bytes.extend_from_slice(&chunk[..n]);
                        if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                            let headers = String::from_utf8_lossy(&bytes[..end]).to_lowercase();
                            let len: usize = headers
                                .lines()
                                .find_map(|l| l.strip_prefix("content-length: "))
                                .unwrap()
                                .parse()
                                .unwrap();
                            if bytes.len() >= end + 4 + len {
                                requests.push(
                                    serde_json::from_slice::<serde_json::Value>(&bytes[end + 4..])
                                        .unwrap(),
                                );
                                break;
                            }
                        }
                    }
                    let body = serde_json::json!({"choices":[{"message":{"content":response}}]})
                        .to_string();
                    write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    )
                    .unwrap();
                }
                requests
            });
            let dir = tempfile::tempdir().unwrap();
            let spec = extract_style_text_at(
                &Client::new(),
                &format!("http://{addr}"),
                "fixture-secret",
                model,
                "这是原文。",
                dir.path(),
                method,
            )
            .await
            .unwrap();
            assert_eq!(spec, "先铺陈判断，再用短句收束。");
            let requests = server.join().unwrap();
            assert_eq!(requests.len(), expected_calls);
            for request in &requests {
                if matches!(method, LearningMethod::DirectFree) {
                    assert_eq!(
                        request["response_format"],
                        serde_json::json!({"type":"json_object"})
                    );
                } else {
                    assert!(request.get("response_format").is_none());
                }
            }
            if expected_calls == 2 {
                assert!(
                    !requests[1]["messages"][1]["content"]
                        .as_str()
                        .unwrap()
                        .contains("这是原文")
                );
            } else {
                assert_eq!(requests[0]["messages"][1]["content"], "这是原文。");
            }
            for entry in std::fs::read_dir(dir.path()).unwrap() {
                let text = std::fs::read_to_string(entry.unwrap().path()).unwrap();
                assert!(!text.contains("fixture-secret"));
                assert!(!text.contains(&addr.to_string()));
            }
            assert!(dir.path().join("result.json").exists());
        }
    }

    // ── strip_html_tags ─────────────────────────────────────────

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
