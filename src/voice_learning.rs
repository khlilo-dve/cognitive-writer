//! Reusable text-only single/multi article learner. Returned styles are reviewable drafts.
use crate::{error::AppError, spec_quality::validate_execution_spec};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{collections::HashSet, future::Future};

pub const METHOD: &str = "voice-core-v6";

pub(crate) fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[derive(Clone, Serialize, Deserialize)]
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceAnalysis {
    analysis: String,
    evidence: Vec<AnalysisEvidence>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AnalysisEvidence {
    article: String,
    quote: String,
}
fn grounded_analysis(raw: &str, sources: &[Source]) -> Result<String, AppError> {
    let report: SourceAnalysis = serde_json::from_str(raw.trim())
        .map_err(|_| AppError::Parse("分析需要包含analysis与evidence的JSON对象".into()))?;
    if report.analysis.trim().is_empty() {
        return Err(AppError::Parse("声音分析为空".into()));
    }
    let mut covered = HashSet::new();
    for (index, evidence) in report.evidence.into_iter().enumerate() {
        let source = sources
            .iter()
            .find(|s| s.identity.article == evidence.article)
            .ok_or_else(|| {
                AppError::Parse(format!(
                    "第{}条分析引用了未知来源{}",
                    index + 1,
                    evidence.article
                ))
            })?;
        if evidence.quote.trim().is_empty() || !source.text.contains(&evidence.quote) {
            return Err(AppError::Parse(format!(
                "第{}条分析引文不在来源{}原文中，不能继续编译",
                index + 1,
                evidence.article
            )));
        }
        covered.insert(evidence.article);
    }
    if sources
        .iter()
        .any(|s| !covered.contains(&s.identity.article))
    {
        return Err(AppError::Parse("分析缺少某篇来源的可定位原文依据".into()));
    }
    Ok(report.analysis)
}

fn repair_context(raw: &str, sources: &[Source]) -> Vec<serde_json::Value> {
    let Ok(report) = serde_json::from_str::<SourceAnalysis>(raw.trim()) else {
        return vec![];
    };
    report.evidence.into_iter().enumerate().filter_map(|(index,e)|{
        let source=sources.iter().find(|s|s.identity.article==e.article)?;
        if !e.quote.trim().is_empty() && source.text.contains(&e.quote){return None;}
        let query=e.quote.chars().collect::<Vec<_>>();
        let mut excerpt=String::new();
        for (qstart,window) in query.windows(8).enumerate(){
            let part=window.iter().collect::<String>();
            if let Some(byte)=source.text.find(&part){
                let at=source.text[..byte].chars().count();let start=at.saturating_sub(qstart+16);
                excerpt=source.text.chars().skip(start).take(query.len().min(240)+32).collect();break;
            }
        }
        Some(json!({"evidence_index":index+1,"article":e.article,"invalid_quote":e.quote,"source_excerpt_candidate":excerpt}))
    }).collect()
}

/// The caller persists requests; analysis/analysis-repair return source-grounded JSON, compilation returns Markdown.
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
    let raw = call("analysis".into(), prompts.analysis.clone(), input.clone()).await?;
    let (analysis, analysis_calls) = match grounded_analysis(&raw, &sources) {
        Ok(analysis) => (analysis, 1),
        Err(error) => {
            let repair=json!({"original_input":serde_json::from_str::<serde_json::Value>(&input).map_err(|e|AppError::Parse(e.to_string()))?,
                "repair_context":repair_context(&raw,&sources),"invalid_analysis":raw,"diagnosis":error.to_string(),"instruction":"逐项修复错误引用。候选原文片段只帮助定位，必须核对原文；可改选较短的连续真实摘引，并同步调整不成立的分析。输出完整analysis和evidence，不重复旧错误，不编造证据。"}).to_string();
            let fixed = call("analysis-repair".into(), prompts.analysis.clone(), repair).await?;
            (grounded_analysis(&fixed, &sources)?, 2)
        }
    };
    let original = json!({"article_count":sources.len(),"analysis":analysis}).to_string();
    let mut user = original.clone();
    let mut calls = analysis_calls;
    for attempt in 0..2 {
        let raw = call(format!("compile-{attempt}"), prompts.compile.clone(), user).await?;
        calls += 1;
        let spec = crate::spec_quality::execution_markdown(&raw);
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
        let result = learn_voice_with(&inputs, &prompts, |stage, _, user| async move {
            let input: serde_json::Value = serde_json::from_str(&user).unwrap();
            if stage == "analysis" {
                assert_eq!(input["articles"].as_array().unwrap().len(), 10);
            } else {
                assert_eq!(input["analysis"], "内部分析");
                assert!(input.get("articles").is_none());
                assert_eq!(input["article_count"], 10);
            }
            Ok(if stage == "analysis" {
                json!({"analysis":"内部分析","evidence":input["articles"].as_array().unwrap().iter().map(|a|json!({"article":a["article"],"quote":a["text"]})).collect::<Vec<_>>()}).to_string()
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
                        json!({"analysis":"分析","evidence":[{"article":"a01","quote":copied}]})
                            .to_string()
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
    #[tokio::test]
    async fn unrelated_source_quotes_get_one_repair_and_never_reach_compilation() {
        let articles = vec!["我想念旧时的声音，也承认记忆模糊。".into()];
        let prompts = VoicePrompts {
            analysis: "证据分析".into(),
            compile: "编译".into(),
        };
        let mut stages = Vec::new();
        let failed=learn_voice_with(&articles,&prompts,|stage,_,_|{
            stages.push(stage.clone());async move{Ok(json!({"analysis":"无关的骨骼史料分析","evidence":[{"article":"a01","quote":"这里从事骨骼分拣工作"}]}).to_string())}
        }).await;
        assert!(failed.is_err());
        assert_eq!(stages, vec!["analysis", "analysis-repair"]);
        let sources = sources(&articles).unwrap();
        assert!(
            grounded_analysis(
                &json!({"analysis":"内容","evidence":[]}).to_string(),
                &sources
            )
            .is_err()
        );
        assert!(
            grounded_analysis(
                &json!({"analysis":"内容","evidence":[{"article":"a99","quote":"声音"}]})
                    .to_string(),
                &sources
            )
            .is_err()
        );
        assert_eq!(grounded_analysis(&json!({"analysis":"第一人称承载情绪与限定","evidence":[{"article":"a01","quote":"我想念旧时的声音"}]}).to_string(),&sources).unwrap(),"第一人称承载情绪与限定");
    }
    #[tokio::test]
    async fn a_valid_source_repair_continues_without_exposing_evidence_to_compiler() {
        let mut stages = Vec::new();
        let result=learn_voice_with(&["这段原文含有明确的感受与判断。".into()],&VoicePrompts{analysis:"来源分析".into(),compile:"编译".into()},|stage,_,user|{
            stages.push(stage.clone());async move{
                if stage=="analysis" {Ok(json!({"analysis":"错文","evidence":[{"article":"a01","quote":"别的文章"}]}).to_string())}
                else if stage=="analysis-repair" {Ok(json!({"analysis":"原文感受与判断并存","evidence":[{"article":"a01","quote":"明确的感受与判断"}]}).to_string())}
                else {let input:serde_json::Value=serde_json::from_str(&user).unwrap();assert_eq!(input["analysis"],"原文感受与判断并存");assert!(input.get("evidence").is_none());assert!(input.get("articles").is_none());Ok("依据当前材料兼顾感受和判断，保留必要的限定。".into())}
            }
        }).await.unwrap();
        assert_eq!(stages, vec!["analysis", "analysis-repair", "compile-0"]);
        assert_eq!(result.calls, 3);
    }
    #[test]
    fn repair_context_uses_actual_source_without_accepting_approximate_quotes() {
        let articles = vec!["引言。现在的年轻人可能不知道，这句话另有语境。结尾。".into()];
        let sources = sources(&articles).unwrap();
        let raw=json!({"analysis":"描述","evidence":[{"article":"a01","quote":"现在的人可能不知道，这句话另有语境。"}]}).to_string();
        assert!(grounded_analysis(&raw, &sources).is_err());
        let hints = repair_context(&raw, &sources);
        assert_eq!(hints.len(), 1);
        assert!(
            hints[0]["source_excerpt_candidate"]
                .as_str()
                .unwrap()
                .contains("现在的年轻人")
        );
        assert!(grounded_analysis(&raw, &sources).is_err());
    }
}
