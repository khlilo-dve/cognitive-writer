//! Source-grounded scope revision; a model review never promotes a draft automatically.
use crate::{
    error::AppError,
    spec_quality::execution_markdown,
    voice_learning::{VoiceDraft, sha256, sources, validate},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::future::Future;

pub const METHOD: &str = "voice-reviewed-v3";

#[derive(Clone, Serialize, Deserialize)]
pub struct RevisionPrompts {
    pub review: String,
    pub revise: String,
}

impl RevisionPrompts {
    pub fn load() -> Result<Self, AppError> {
        Ok(Self {
            review: std::fs::read_to_string("prompts/review_voice_scope.md")?,
            revise: std::fs::read_to_string("prompts/revise_voice_scope.md")?,
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VoiceRevision {
    pub parent_spec_sha256: String,
    /// Audit-only; may contain source excerpts and must not be injected into the Writer.
    pub review: String,
    pub revision_calls: usize,
    pub result: VoiceDraft,
}

/// Re-read the original sources and a candidate, without the original analysis or seeded defects.
/// The caller persists requests, routes analysis models and decides whether a draft may be adopted.
pub async fn revise_voice_with<F, Fut>(
    articles: &[String],
    draft: &VoiceDraft,
    prompts: &RevisionPrompts,
    mut call: F,
) -> Result<VoiceRevision, AppError>
where
    F: FnMut(String, String, String) -> Fut,
    Fut: Future<Output = Result<String, AppError>>,
{
    let sources = sources(articles)?;
    let identities = sources
        .iter()
        .map(|s| s.identity.clone())
        .collect::<Vec<_>>();
    if identities != draft.sources {
        return Err(AppError::Parse("修订原文与候选来源身份不一致".into()));
    }
    if prompts.review.trim().is_empty() || prompts.revise.trim().is_empty() {
        return Err(AppError::Parse("风格修订提示词为空".into()));
    }
    crate::spec_quality::validate_execution_spec(&draft.spec)?;
    let input = json!({"articles":sources.iter().map(|source|
        json!({"article":source.identity.article,"text":source.text})).collect::<Vec<_>>(),
        "candidate":draft.spec})
    .to_string();
    let review = call("review".into(), prompts.review.clone(), input).await?;
    if review.trim().is_empty() {
        return Err(AppError::Parse("原文范围审阅为空".into()));
    }
    let original = json!({"article_count":sources.len(),
        "articles":sources.iter().map(|source|json!({"article":source.identity.article,"text":source.text})).collect::<Vec<_>>(),
        "candidate":draft.spec,
        "source_review":review})
    .to_string();
    let mut user = original.clone();
    for attempt in 0..2 {
        let raw = call(format!("revise-{attempt}"), prompts.revise.clone(), user).await?;
        let spec = execution_markdown(&raw);
        match validate(spec, &sources) {
            Ok(()) => {
                let parent_spec_sha256 = sha256(draft.spec.as_bytes());
                let fingerprint = sha256(
                    &serde_json::to_vec(&json!({
                        "method":METHOD,"sources":identities,"parent_fingerprint":draft.fingerprint,
                        "parent_spec_sha256":parent_spec_sha256,
                        "review_prompt_sha256":sha256(prompts.review.as_bytes()),
                        "revise_prompt_sha256":sha256(prompts.revise.as_bytes())
                    }))
                    .map_err(|e| AppError::Parse(e.to_string()))?,
                );
                let revision_calls = attempt + 2;
                let mut result = draft.clone();
                result.method = METHOD.into();
                result.spec = spec.into();
                result.fingerprint = fingerprint;
                result.calls += revision_calls;
                result.warnings = vec![
                    "已经模型原文复读和范围修订，仍是待审草稿；没有独立人评或自动采用结论。".into(),
                    "模型审阅中的引文、反例及修订是否恰当仍需核对。".into(),
                ];
                if spec != raw.trim() {
                    result.warnings.push(
                        "已移除模型完整Markdown代码围栏外包装；内部正文仍经过原有校验。".into(),
                    );
                }
                if sources.len() == 1 {
                    result
                        .warnings
                        .push("单篇仅说明本篇可见写法，不代表长期习惯。".into());
                }
                result.status = "draft_pending_semantic_review".into();
                return Ok(VoiceRevision {
                    parent_spec_sha256,
                    review,
                    revision_calls,
                    result,
                });
            }
            Err(error) if attempt == 0 => {
                user = format!(
                    "原始任务：{original}\n上次候选：{spec}\n程序诊断：{error}\n只修复格式或原文内容复制，重新输出完整Markdown。"
                );
            }
            Err(error) => return Err(error),
        }
    }
    Err(AppError::Parse("风格范围修订失败".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voice_learning::{VoicePrompts, learn_voice_with};
    use std::sync::{Arc, Mutex};

    #[test]
    fn only_complete_markdown_packaging_is_normalized() {
        assert_eq!(
            execution_markdown("```markdown\n# 说明\n\n正文\n```"),
            "# 说明\n\n正文"
        );
        for raw in [
            "```json\n{}\n```",
            "前言\n```markdown\n正文\n```",
            "```markdown\n正文",
            "```md\n正文```",
        ] {
            assert_eq!(execution_markdown(raw), raw);
        }
        assert!(
            crate::spec_quality::validate_execution_spec(execution_markdown(
                "```md\n```rust\ncode\n```\n```"
            ))
            .is_err()
        );
    }

    async fn draft(texts: &[String]) -> VoiceDraft {
        learn_voice_with(
            texts,
            &VoicePrompts {
                analysis: "分析".into(),
                compile: "编译".into(),
            },
            |stage, _, _| async move {
                Ok(if stage == "analysis" {
                    "不应传给复读者的内部分析"
                } else {
                    "先说明有据的细节，再表达判断。"
                }
                .into())
            },
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn source_mismatch_fails_before_calls_and_input_order_can_change() {
        let texts = vec!["甲篇独特正文".into(), "乙篇独特正文".into()];
        let original = draft(&texts).await;
        let prompts = RevisionPrompts {
            review: "复读".into(),
            revise: "修订".into(),
        };
        assert!(
            revise_voice_with(
                &["另一个来源".into()],
                &original,
                &prompts,
                |_, _, _| async { panic!("不得调用") }
            )
            .await
            .is_err()
        );
        let seen = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&seen);
        let revision = revise_voice_with(
            &[texts[1].clone(), texts[0].clone()],
            &original,
            &prompts,
            move |stage, _, user| {
                captured.lock().unwrap().push((stage.clone(), user));
                async move {
                    Ok(if stage == "review" {
                        "原文只支持局部路线，应保留不同起笔。"
                    } else {
                        "依材料选择细节或判断起笔，不强制固定顺序。"
                    }
                    .into())
                }
            },
        )
        .await
        .unwrap();
        let captured = seen.lock().unwrap();
        assert!(!captured[0].1.contains("不应传给复读者的内部分析"));
        assert!(captured[0].1.contains("甲篇独特正文"));
        assert!(captured[1].1.contains("甲篇独特正文"));
        assert!(!captured[1].1.contains("不应传给复读者的内部分析"));
        assert_eq!(original.spec, "先说明有据的细节，再表达判断。");
        assert_eq!(revision.result.sources, original.sources);
        assert_eq!(revision.result.status, "draft_pending_semantic_review");
        assert_eq!(revision.revision_calls, 2);
        assert_ne!(revision.result.fingerprint, original.fingerprint);
    }

    #[tokio::test]
    async fn copied_revision_is_bounded_and_empty_review_stops() {
        let texts = vec!["这是一段足够长的来源正文用于核验候选不可抄写原文内容。".into()];
        let original = draft(&texts).await;
        let prompts = RevisionPrompts {
            review: "复读".into(),
            revise: "修订".into(),
        };
        let seen = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&seen);
        let copied = texts[0].clone();
        assert!(
            revise_voice_with(&texts, &original, &prompts, move |stage, _, _| {
                captured.lock().unwrap().push(stage.clone());
                let copied = copied.clone();
                async move {
                    Ok(if stage == "review" {
                        "保留内容隔离。".into()
                    } else {
                        copied
                    })
                }
            })
            .await
            .is_err()
        );
        assert_eq!(
            seen.lock().unwrap().as_slice(),
            ["review", "revise-0", "revise-1"]
        );
        assert!(
            revise_voice_with(&texts, &original, &prompts, |stage, _, _| async move {
                assert_eq!(stage, "review");
                Ok(" ".into())
            })
            .await
            .is_err()
        );
    }
}
