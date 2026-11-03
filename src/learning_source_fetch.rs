//! Bounded file/HTTP readers. No model, environment credentials or user interaction.
use crate::{
    error::AppError,
    learning_source_parse::{self as parse, Extraction, PageKind},
    learning_sources::{Attempt, MAX_BYTES, Snapshot, SourceInput, SourceStatus},
    voice_learning::sha256,
};
use std::{io::Read, path::Path, time::Duration};

pub(crate) struct Fetched {
    pub status: SourceStatus,
    pub reason: String,
    pub resolved_locator: String,
    pub attempts: Vec<Attempt>,
    pub snapshots: Vec<Snapshot>,
    pub extraction: Option<Extraction>,
}
fn empty(locator: String, status: SourceStatus, reason: &str) -> Fetched {
    Fetched {
        status,
        reason: reason.into(),
        resolved_locator: locator,
        attempts: vec![],
        snapshots: vec![],
        extraction: None,
    }
}
pub(crate) fn read_limited(path: &Path) -> Result<Vec<u8>, AppError> {
    if !std::fs::metadata(path)?.is_file() {
        return Err(AppError::FileRead(
            "需要普通文件，不能读取目录或流设备".into(),
        ));
    }
    let file = std::fs::File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(AppError::FileRead(
            "需要普通文件，不能读取目录或流设备".into(),
        ));
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > MAX_BYTES {
        return Err(AppError::FileRead("输入超过8MiB，未截断处理".into()));
    }
    Ok(bytes)
}
fn utf8(bytes: Vec<u8>) -> Result<String, AppError> {
    String::from_utf8(bytes)
        .map_err(|_| AppError::Parse("输入不是有效UTF-8，请提供UTF-8正文".into()))
}
fn snapshot(method: &str, content_type: &str, locator: &str, body: String) -> Snapshot {
    Snapshot {
        method: method.into(),
        content_type: content_type.into(),
        resolved_locator: locator.into(),
        raw_sha256: sha256(body.as_bytes()),
        body,
    }
}
fn finish(mut result: Fetched, extraction: Extraction) -> Fetched {
    result.status = match extraction.kind {
        PageKind::Articles => SourceStatus::Extracted,
        PageKind::NeedsInspection => SourceStatus::NeedsInspection,
        PageKind::NonArticle => SourceStatus::NonArticle,
    };
    result.reason = extraction.reason.clone();
    result.extraction = Some(extraction);
    result
}
fn local(input: &SourceInput, base: &Path) -> Fetched {
    let (locator, method, body) = match input {
        SourceInput::Text { label, text } => (
            label.clone(),
            "provided_text",
            Ok(text.clone()).and_then(|s| {
                if s.len() > MAX_BYTES {
                    Err(AppError::Parse("输入超过8MiB，未截断处理".into()))
                } else {
                    Ok(s)
                }
            }),
        ),
        SourceInput::File { path } => {
            let target = if Path::new(path).is_absolute() {
                Path::new(path).to_path_buf()
            } else {
                base.join(path)
            };
            if !matches!(
                target
                    .extension()
                    .and_then(|s| s.to_str())
                    .map(str::to_ascii_lowercase)
                    .as_deref(),
                Some("txt" | "md" | "markdown")
            ) {
                return empty(
                    target.display().to_string(),
                    SourceStatus::Unsupported,
                    "当前采集入口支持UTF-8 TXT/MD；可提供文本文件或可读文章链接",
                );
            }
            (
                target.display().to_string(),
                "file_utf8",
                read_limited(&target).and_then(utf8),
            )
        }
        SourceInput::Url { .. } => unreachable!(),
    };
    let mut result = empty(locator.clone(), SourceStatus::Failed, "");
    match body {
        Ok(body) => {
            let extracted = parse::text(&body, &locator);
            result.attempts.push(Attempt {
                method: method.into(),
                http_status: None,
                outcome: "received".into(),
            });
            result
                .snapshots
                .push(snapshot(method, "text/plain", &locator, body));
            finish(result, extracted)
        }
        Err(error) => {
            result.reason = error.to_string();
            result.attempts.push(Attempt {
                method: method.into(),
                http_status: None,
                outcome: result.reason.clone(),
            });
            result
        }
    }
}
struct HttpBody {
    status: u16,
    content_type: String,
    locator: String,
    body: Option<String>,
}
async fn http(client: &reqwest::Client, url: &str) -> Result<HttpBody, AppError> {
    let mut response = client
        .get(url)
        .header("Accept", "text/html, text/markdown, text/plain")
        .header("User-Agent", "cognitive-writer/source-input-v1")
        .send()
        .await
        .map_err(|_| AppError::Network("来源网络读取失败，未记录认证或错误URL".into()))?;
    let status = response.status().as_u16();
    let locator = response.url().to_string();
    if !response.status().is_success() {
        return Ok(HttpBody {
            status,
            content_type: String::new(),
            locator,
            body: None,
        });
    }
    if response
        .content_length()
        .is_some_and(|n| n > MAX_BYTES as u64)
    {
        return Err(AppError::Parse("HTTP正文超过8MiB，未截断处理".into()));
    }
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|s| s.to_str().ok())
        .unwrap_or("")
        .to_string();
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| AppError::Network("来源响应中断，未交付部分正文".into()))?
    {
        if bytes.len().saturating_add(chunk.len()) > MAX_BYTES {
            return Err(AppError::Parse("HTTP正文超过8MiB，未截断处理".into()));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(HttpBody {
        status,
        content_type,
        locator,
        body: Some(utf8(bytes)?),
    })
}

pub(crate) async fn fetch(
    input: &SourceInput,
    base: &Path,
    reader_prefix: Option<&str>,
) -> Fetched {
    let SourceInput::Url { url } = input else {
        return local(input, base);
    };
    let parsed = match reqwest::Url::parse(url) {
        Ok(u)
            if matches!(u.scheme(), "http" | "https")
                && u.username().is_empty()
                && u.password().is_none() =>
        {
            u
        }
        _ => {
            return empty(
                String::new(),
                SourceStatus::Failed,
                "需要无内嵌认证信息的HTTP/HTTPS文章URL",
            );
        }
    };
    let mut result = empty(parsed.to_string(), SourceStatus::Failed, "");
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
    {
        Ok(c) => c,
        Err(_) => {
            result.reason = "无法创建来源读取客户端".into();
            return result;
        }
    };
    let mut urls = vec![("direct_http", parsed.to_string())];
    if let Some(prefix) = reader_prefix {
        urls.push(("reader_http", format!("{prefix}{parsed}")));
    }
    for (method, target) in urls {
        let fetched = http(&client, &target).await;
        match fetched {
            Ok(body) => {
                let status = body.status;
                result.attempts.push(Attempt {
                    method: method.into(),
                    http_status: Some(status),
                    outcome: if body.body.is_some() {
                        "received".into()
                    } else {
                        format!("HTTP {status}")
                    },
                });
                let Some(raw) = body.body else {
                    result.reason = format!("来源返回HTTP {status}，未取得正文");
                    if status < 500 && status != 429 {
                        break;
                    }
                    continue;
                };
                let extraction = if method == "reader_http" {
                    Ok(parse::reader(&raw))
                } else if body.content_type.contains("html")
                    || (body.content_type.is_empty() && raw.trim_start().starts_with('<'))
                {
                    parse::html(&raw, &body.locator)
                } else if body.content_type.starts_with("text/plain")
                    || body.content_type.starts_with("text/markdown")
                {
                    let mut p = parse::text(&raw, &parsed.to_string());
                    p.kind = PageKind::NeedsInspection;
                    p.reason = "URL返回文本候选，待领域确认文章边界".into();
                    Ok(p)
                } else {
                    Err(AppError::Parse(
                        "来源未返回受支持的HTML或文本正文类型".into(),
                    ))
                };
                result
                    .snapshots
                    .push(snapshot(method, &body.content_type, &body.locator, raw));
                match extraction {
                    Ok(extraction) => {
                        let done = extraction.kind != PageKind::NeedsInspection
                            || !matches!(
                                extraction.method.as_str(),
                                "unresolved" | "html_unconfirmed_body"
                            );
                        if !extraction.articles.is_empty() || result.extraction.is_none() {
                            result = finish(result, extraction);
                        }
                        if done {
                            break;
                        }
                    }
                    Err(error) => {
                        result.reason = error.to_string();
                    }
                }
            }
            Err(error) => {
                result.attempts.push(Attempt {
                    method: method.into(),
                    http_status: None,
                    outcome: error.to_string(),
                });
                result.reason = error.to_string();
                // Complete input/size/encoding errors are not transient network failures.
                if !matches!(error, AppError::Network(_)) {
                    break;
                }
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    fn server(
        responses: Vec<(&'static str, &'static str)>,
    ) -> (String, std::thread::JoinHandle<usize>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let join = std::thread::spawn(move || {
            let mut n = 0;
            for (status, body) in responses {
                let (mut s, _) = listener.accept().unwrap();
                s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                let mut buf = [0; 4096];
                s.read(&mut buf).unwrap();
                write!(s,"HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
                n += 1;
            }
            n
        });
        (url, join)
    }
    #[tokio::test]
    async fn http_success_is_not_article_success_and_login_does_not_retry() {
        let (url, join) = server(vec![("200 OK", "<form><input type=password></form>")]);
        let got = fetch(
            &SourceInput::Url { url: url.clone() },
            Path::new("."),
            Some(&format!("{url}/reader/")),
        )
        .await;
        assert_eq!(got.status, SourceStatus::NonArticle);
        assert_eq!(got.attempts.len(), 1);
        assert_eq!(got.snapshots.len(), 1);
        assert_eq!(join.join().unwrap(), 1);
        let (url, join) = server(vec![(
            "200 OK",
            "<article><h1>标题</h1><p>摘要。</p><a href='/full'>阅读全文</a></article>",
        )]);
        let got = fetch(
            &SourceInput::Url { url: url.clone() },
            Path::new("."),
            Some(&format!("{url}/reader/")),
        )
        .await;
        assert_eq!(got.status, SourceStatus::NeedsInspection);
        assert_eq!(got.attempts.len(), 1);
        assert_eq!(join.join().unwrap(), 1);
    }
    #[tokio::test]
    async fn one_distinct_fallback_after_server_error_preserves_both_attempts() {
        let (url, join) = server(vec![
            ("503 Unavailable", "busy"),
            (
                "200 OK",
                "Title: 原文\nURL Source: https://source.org\nMarkdown Content:\n正文。",
            ),
        ]);
        let got = fetch(
            &SourceInput::Url { url: url.clone() },
            Path::new("."),
            Some(&format!("{url}/reader/")),
        )
        .await;
        assert_eq!(got.status, SourceStatus::NeedsInspection);
        assert_eq!(got.attempts.len(), 2);
        assert_eq!(got.extraction.unwrap().articles[0].text, "正文。");
        assert_eq!(join.join().unwrap(), 2);
    }
    #[test]
    fn invalid_encoding_oversize_and_unsupported_format_are_not_truncated() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("bad.txt"), [0xff]).unwrap();
        assert!(
            local(
                &SourceInput::File {
                    path: "bad.txt".into()
                },
                dir.path()
            )
            .reason
            .contains("UTF-8")
        );
        std::fs::write(dir.path().join("large.md"), vec![b'x'; MAX_BYTES + 1]).unwrap();
        assert!(
            local(
                &SourceInput::File {
                    path: "large.md".into()
                },
                dir.path()
            )
            .reason
            .contains("8MiB")
        );
        assert_eq!(
            local(
                &SourceInput::File {
                    path: "input.pdf".into()
                },
                dir.path()
            )
            .status,
            SourceStatus::Unsupported
        );
    }
}
