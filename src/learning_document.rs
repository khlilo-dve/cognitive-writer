//! Binary provenance and format dispatch; only derived text reaches the materials Agent.
use crate::{
    learning_source_fetch::{self, Fetched},
    learning_source_parse::{self as parse, PageKind},
    learning_sources::{Snapshot, SourceStatus},
    voice_learning::sha256,
};
use std::path::Path;
pub(crate) fn read(path: &Path, extension: &str) -> Fetched {
    let locator = path.display().to_string();
    let mut result = Fetched {
        status: SourceStatus::Failed,
        reason: String::new(),
        resolved_locator: locator.clone(),
        attempts: vec![],
        snapshots: vec![],
        extraction: None,
        binary: None,
    };
    let bytes = match learning_source_fetch::read_limited(path) {
        Ok(bytes) => bytes,
        Err(e) => {
            result.reason = e.to_string();
            return result;
        }
    };
    let parsed = if extension == "docx" {
        crate::learning_docx::extract(&bytes).map(|v| (v.body, v.incomplete, String::new()))
    } else {
        crate::learning_pdf::extract(&bytes).map(|v| {
            (
                v.body,
                false,
                format!(
                    "PDF共{}页；空白页{:?}；仅文字层，图片含义与多栏阅读顺序未认证",
                    v.pages, v.blank_pages
                ),
            )
        })
    };
    result.binary = Some(bytes);
    match parsed {
        Ok((text, incomplete, note)) => {
            let method = if extension == "docx" {
                "docx_visible_body_v1"
            } else {
                "pdf_text_layer_v1"
            };
            result.snapshots.push(Snapshot {
                method: method.into(),
                content_type: "text/markdown".into(),
                resolved_locator: locator.clone(),
                raw_sha256: sha256(text.as_bytes()),
                body: text.clone(),
            });
            if incomplete {
                result.status = SourceStatus::Unsupported;
                result.reason="DOCX含图片/嵌入对象或文本框，无法保证完整文章；请提供纯文字正文或去除非文章对象的文档".into();
            } else {
                let mut parsed = parse::text(
                    &text,
                    path.file_stem().and_then(|s| s.to_str()).unwrap_or("文档"),
                );
                parsed.method = method.into();
                if extension == "pdf" && parsed.kind != PageKind::NonArticle {
                    parsed.kind = PageKind::NeedsInspection;
                    parsed.reason = format!("{note}；待领域确认文章边界，不按页数计篇数");
                }
                result.status = match parsed.kind {
                    PageKind::Articles => SourceStatus::Extracted,
                    PageKind::NeedsInspection => SourceStatus::NeedsInspection,
                    PageKind::NonArticle => SourceStatus::NonArticle,
                };
                result.reason = parsed.reason.clone();
                result.extraction = Some(parsed);
            }
        }
        Err(e) => {
            result.reason = e.to_string();
            if result.reason.contains("OCR") || result.reason.contains("加密") {
                result.status = SourceStatus::Unsupported;
            }
        }
    }
    result.attempts.push(crate::learning_sources::Attempt {
        method: format!("file_{extension}"),
        http_status: None,
        outcome: result.reason.clone(),
    });
    result
}
