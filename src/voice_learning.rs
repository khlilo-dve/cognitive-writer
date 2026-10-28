//! Reusable text-only single/multi article learner. Returned styles are reviewable drafts.
use crate::{error::AppError, spec_quality::validate_execution_spec};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{collections::HashSet, future::Future};

pub const METHOD: &str = "voice-core-v1";

pub(crate) fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[derive(Clone, Serialize)]
pub struct VoicePrompts {
    pub analysis: String,
    pub compile: String,
}

impl VoicePrompts {
    pub fn load() -> Result<Self, AppError> {
        Ok(Self {
            analysis: std::fs::read_to_string("prompts/learn_voice_analysis.md")?,
            compile: std::fs::read_to_string("prompts/learn_voice_compile.md")?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceIdentity {
    pub article: String,
    pub text_sha256: String,
    pub normalized_sha256: String,
}

#[derive(Clone, Serialize)]
pub(crate) struct Source {
    pub(crate) identity: SourceIdentity,
    pub(crate) text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceDraft {
    pub status: String,
    pub method: String,
    pub spec: String,
    pub analysis: String,
    pub sources: Vec<SourceIdentity>,
    /// Input set, method and prompt identity; model/request identity belongs to the caller's audit.
    pub fingerprint: String,
    pub warnings: Vec<String>,
    pub calls: usize,
}

pub(crate) fn sources(articles: &[String]) -> Result<Vec<Source>, AppError> {
    if !(1..=10).contains(&articles.len()) {
        return Err(AppError::Parse("风格学习需要1–10篇正文".into()));
    }
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for text in articles {
        let normalized = text
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>();
        if normalized.is_empty() {
            return Err(AppError::Parse("学习正文为空".into()));
        }
        let normalized_sha256 = sha256(normalized.as_bytes());
        if !seen.insert(normalized_sha256.clone()) {
            return Err(AppError::Parse("学习正文重复，不能重复计为多篇".into()));
        }
        result.push(Source {
            identity: SourceIdentity {
                article: String::new(),
                text_sha256: sha256(text.as_bytes()),
                normalized_sha256,
            },
            text: text.clone(),
        });
    }
    result.sort_by(|a, b| {
        a.identity
            .normalized_sha256
            .cmp(&b.identity.normalized_sha256)
    });
    for (index, source) in result.iter_mut().enumerate() {
        source.identity.article = format!("a{:02}", index + 1);
    }
    Ok(result)
}

pub(crate) fn validate(spec: &str, sources: &[Source]) -> Result<(), AppError> {
    validate_execution_spec(spec)?;
    let chars = spec.chars().collect::<Vec<_>>();
    if chars.windows(16).any(|window| {
        let piece = window.iter().collect::<String>();
        sources.iter().any(|source| source.text.contains(&piece))
    }) {
        return Err(AppError::Parse(
            "执行说明复制了原文连续片段，需隔离内容".into(),
        ));
    }
    Ok(())
}

/// The caller routes both stages to its configured analysis model and persists requests.
/// No input acquisition, provider credentials or style-library writes occur here.
pub async fn learn_voice_with<F, Fut>(
    articles: &[String],
    prompts: &VoicePrompts,
    mut call: F,
) -> Result<VoiceDraft, AppError>
where
    F: FnMut(String, String, String) -> Fut,
    Fut: Future<Output = Result<String, AppError>>,
{
    let sources = sources(articles)?;
    if prompts.analysis.trim().is_empty() || prompts.compile.trim().is_empty() {
        return Err(AppError::Parse("风格学习提示词为空".into()));
    }
    let input = json!({"articles":sources.iter().map(|source|
        json!({"article":source.identity.article,"text":source.text})).collect::<Vec<_>>()})
    .to_string();
    let analysis = call("analysis".into(), prompts.analysis.clone(), input).await?;
    if analysis.trim().is_empty() {
        return Err(AppError::Parse("声音分析为空".into()));
    }
    let original = json!({"article_count":sources.len(),"analysis":analysis}).to_string();
    let mut user = original.clone();
    let mut calls = 1;
    for attempt in 0..2 {
        let raw = call(format!("compile-{attempt}"), prompts.compile.clone(), user).await?;
        calls += 1;
        let spec = raw.trim();
        match validate(spec, &sources) {
            Ok(()) => {
                let mut warnings = vec![
                    "语义内容泄漏、过度概括与新题材执行仍需审阅；程序通过不代表自动采用。".into(),
                ];
                if sources.len() == 1 {
                    warnings.push("单篇结果只代表该文本可见的写法。".into());
                }
                if ["一到两次", "每篇必须", "最稳定", "一贯"]
                    .iter()
                    .any(|p| spec.contains(p))
                {
                    warnings.push(
                        "说明含范围或频率措辞，审阅时需核对是否有依据或只是反向禁令。".into(),
                    );
                }
                let identities = sources
                    .iter()
                    .map(|s| s.identity.clone())
                    .collect::<Vec<_>>();
                let fingerprint = sha256(
                    &serde_json::to_vec(&json!({"method":METHOD,"sources":identities,
                    "analysis_prompt_sha256":sha256(prompts.analysis.as_bytes()),
                    "compile_prompt_sha256":sha256(prompts.compile.as_bytes())}))
                    .map_err(|e| AppError::Parse(e.to_string()))?,
                );
                return Ok(VoiceDraft {
                    status: "draft_pending_semantic_review".into(),
                    method: METHOD.into(),
                    spec: spec.into(),
                    analysis,
                    sources: identities,
                    fingerprint,
                    warnings,
                    calls,
                });
            }
            Err(error) if attempt == 0 => {
                user = format!(
                    "原始任务：{original}\n上次候选：{spec}\n程序诊断：{error}\n只修复损坏格式或原文内容，重新输出完整Markdown，不增加规则。"
                )
            }
            Err(error) => return Err(error),
        }
    }
    Err(AppError::Parse("执行说明编译失败".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    #[test]
    fn canonical_set_order_and_duplicate_boundary() {
        let a = "第一篇真实正文".to_string();
        let b = "第二篇真实正文".to_string();
        let one = sources(&[a.clone(), b.clone()]).unwrap();
        let two = sources(&[b, a.clone()]).unwrap();
        assert_eq!(
            serde_json::to_value(one).unwrap(),
            serde_json::to_value(two).unwrap()
        );
        assert!(sources(&[a.clone(), format!(" {a}\n")]).is_err());
    }

    #[tokio::test]
    async fn supports_ten_but_rejects_eleven_before_model_call() {
        let prompts = VoicePrompts {
            analysis: "分析".into(),
            compile: "编译".into(),
        };
        let inputs = (0..10)
            .map(|i| format!("文章{i}，各自不同的正文。"))
            .collect::<Vec<_>>();
        let result = learn_voice_with(&inputs, &prompts, |stage, _, _| async move {
            Ok(if stage == "analysis" {
                "内部分析".into()
            } else {
                "依据材料选择叙述或判断，保持亲近而有分寸的语气。".into()
            })
        })
        .await
        .unwrap();
        assert_eq!(result.sources.len(), 10);
        assert_eq!(result.calls, 2);
        let mut too_many = inputs;
        too_many.push("第十一篇".into());
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&calls);
        assert!(
            learn_voice_with(&too_many, &prompts, move |_, _, _| {
                seen.fetch_add(1, Ordering::SeqCst);
                async { Ok("不应调用".into()) }
            })
            .await
            .is_err()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn copied_source_gets_only_one_repair() {
        let text = "这是足够长的一段独特原文内容用于验证不允许直接复制。".to_string();
        let prompts = VoicePrompts {
            analysis: "分析".into(),
            compile: "编译".into(),
        };
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&calls);
        let copied = text.clone();
        assert!(
            learn_voice_with(&[text], &prompts, move |stage, _, _| {
                seen.fetch_add(1, Ordering::SeqCst);
                let copied = copied.clone();
                async move {
                    Ok(if stage == "analysis" {
                        "分析".into()
                    } else {
                        copied
                    })
                }
            })
            .await
            .is_err()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }
}
