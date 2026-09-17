//! Evidence-backed single-article extraction. The execution Markdown has no fixed headings.
use crate::error::AppError;
use crate::spec_quality::{character_count, validate_execution_spec};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::future::Future;

#[derive(Clone, Serialize)]
pub(crate) struct LearningPrompts {
    pub analyze: String,
    pub compile: String,
}
impl LearningPrompts {
    pub fn load() -> Result<Self, AppError> {
        Ok(Self {
            analyze: std::fs::read_to_string("prompts/analyze_style.md")?,
            compile: std::fs::read_to_string("prompts/learn_style.md")?,
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Support {
    Local,
    Repeated,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Feature {
    observation: String,
    evidence: Vec<String>,
    rule: String,
    condition: String,
    support: Support,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Analysis {
    features: Vec<Feature>,
    uncertain: Vec<String>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Compiled {
    spec: String,
    evidence_insufficient: bool,
}
#[derive(Debug, Serialize)]
pub(crate) struct LearnedStyle {
    pub spec: String,
    pub analysis: Option<Analysis>,
    pub evidence_insufficient: bool,
    pub warnings: Vec<String>,
    pub calls: usize,
}

fn parse_analysis(raw: &str, source: &str) -> Result<Analysis, AppError> {
    let parsed: Analysis = serde_json::from_str(raw.trim())
        .map_err(|e| AppError::Parse(format!("分析 JSON 无效: {e}")))?;
    if parsed.features.is_empty() {
        return Err(AppError::Parse("没有有证据的风格特征".into()));
    }
    for (i, f) in parsed.features.iter().enumerate() {
        let distinct: std::collections::HashSet<_> = f.evidence.iter().collect();
        if [&f.observation, &f.rule, &f.condition]
            .iter()
            .any(|s| s.trim().is_empty())
            || f.evidence.is_empty()
            || f.evidence
                .iter()
                .any(|e| e.trim().is_empty() || !source.contains(e.as_str()))
            || (matches!(f.support, Support::Repeated) && distinct.len() < 2)
        {
            return Err(AppError::Parse(format!(
                "特征 {} 缺字段、证据无法定位或重复支持不足",
                i + 1
            )));
        }
    }
    Ok(parsed)
}

fn compiler_input(analysis: &Analysis) -> String {
    // Whitelist fields. Neither original excerpts nor observation/source/uncertain text crosses this boundary.
    json!({"features": analysis.features.iter().map(|f| json!({
        "rule":f.rule, "condition":f.condition, "support":f.support
    })).collect::<Vec<_>>()} )
    .to_string()
}

fn parse_compiled(raw: &str) -> Result<Compiled, AppError> {
    let mut compiled: Compiled = serde_json::from_str(raw.trim())
        .map_err(|e| AppError::Parse(format!("编写 JSON 无效: {e}")))?;
    compiled.spec = compiled.spec.trim().to_string();
    validate_execution_spec(&compiled.spec)?;
    Ok(compiled)
}

/// Shared by live learning and resumable eval. At most one diagnostic repair per stage.
pub(crate) async fn extract_with<F, Fut>(
    source: &str,
    prompts: &LearningPrompts,
    mut call: F,
) -> Result<LearnedStyle, AppError>
where
    F: FnMut(String, String, String) -> Fut,
    Fut: Future<Output = Result<String, AppError>>,
{
    if source.trim().is_empty()
        || prompts.analyze.trim().is_empty()
        || prompts.compile.trim().is_empty()
    {
        return Err(AppError::Parse("样文或学习提示词为空".into()));
    }
    let mut calls = 0;
    let mut input = source.to_string();
    let mut analysis = None;
    for attempt in 0..2 {
        let raw = call(
            format!("analysis_{attempt}"),
            prompts.analyze.clone(),
            input,
        )
        .await?;
        calls += 1;
        match parse_analysis(&raw, source) {
            Ok(value) => {
                analysis = Some(value);
                break;
            }
            Err(e) if attempt == 0 => {
                input = format!(
                    "原始样文：\n{source}\n\n上次输出：\n{raw}\n\n程序诊断：{e}\n重新输出完整分析 JSON，只使用可定位的真实证据。"
                )
            }
            Err(e) => return Err(e),
        }
    }
    let analysis = analysis.ok_or_else(|| AppError::Parse("证据分析失败".into()))?;
    let clean_input = compiler_input(&analysis);
    input = clean_input.clone();
    for attempt in 0..2 {
        let raw = call(
            format!("compile_{attempt}"),
            prompts.compile.clone(),
            input.clone(),
        )
        .await?;
        calls += 1;
        match parse_compiled(&raw) {
            Ok(compiled) => {
                let count = character_count(&compiled.spec);
                let outside = !(400..=600).contains(&count);
                if outside && attempt == 0 && !(count < 400 && compiled.evidence_insufficient) {
                    input = format!(
                        "特征清单：{clean_input}\n上次输出：{raw}\n程序统计 spec 为 {count} 个非空白字符，目标400–600。仅根据已有证据修订；不足时标记证据不足，不凑字。输出完整 JSON。"
                    );
                    continue;
                }
                let mut warnings = Vec::new();
                if outside {
                    warnings.push(format!("风格为 {count} 个非空白字符，未达400–600目标，长度评分已扣分；未强行填充或截断。"));
                }
                if compiled.evidence_insufficient {
                    warnings.push("样文证据有限，已保留有依据的规则。".into());
                }
                return Ok(LearnedStyle {
                    spec: compiled.spec,
                    analysis: Some(analysis),
                    evidence_insufficient: compiled.evidence_insufficient,
                    warnings,
                    calls,
                });
            }
            Err(e) if attempt == 0 => {
                input = format!(
                    "特征清单：{clean_input}\n上次输出：{raw}\n程序诊断：{e}\n重新输出完整 JSON，spec 只放可执行 Markdown。"
                )
            }
            Err(e) => return Err(e),
        }
    }
    Err(AppError::Parse("风格编写失败".into()))
}

/// Controlled one-call alternative: the same execution validator and bounded repair budget.
pub(crate) async fn extract_direct_with<F, Fut>(
    source: &str,
    prompt: &str,
    mut call: F,
) -> Result<LearnedStyle, AppError>
where
    F: FnMut(String, String, String) -> Fut,
    Fut: Future<Output = Result<String, AppError>>,
{
    if source.trim().is_empty() || prompt.trim().is_empty() {
        return Err(AppError::Parse("样文或学习提示词为空".into()));
    }
    let mut input = source.to_string();
    for attempt in 0..2 {
        let raw = call(format!("direct_{attempt}"), prompt.to_string(), input).await?;
        match parse_compiled(&raw) {
            Ok(compiled) => {
                let count = character_count(&compiled.spec);
                let outside = !(400..=600).contains(&count);
                if outside && attempt == 0 && !(count < 400 && compiled.evidence_insufficient) {
                    input = format!(
                        "原始样文：{source}\n上次输出：{raw}\n程序统计 spec 为 {count} 个非空白字符，目标400–600。仅补充有证据的规则，不凑字；重新输出完整 JSON。"
                    );
                    continue;
                }
                let mut warnings = Vec::new();
                if outside {
                    warnings.push(format!("风格为 {count} 个非空白字符，未达400–600目标，长度评分已扣分；未强行填充或截断。"));
                }
                if compiled.evidence_insufficient {
                    warnings.push("样文证据有限，已保留有依据的规则。".into());
                }
                return Ok(LearnedStyle {
                    spec: compiled.spec,
                    analysis: None,
                    evidence_insufficient: compiled.evidence_insufficient,
                    warnings,
                    calls: attempt + 1,
                });
            }
            Err(e) if attempt == 0 => {
                input = format!(
                    "原始样文：{source}\n上次输出：{raw}\n程序诊断：{e}\n修复后输出完整JSON，spec只放执行指令。"
                )
            }
            Err(e) => return Err(e),
        }
    }
    Err(AppError::Parse("风格提取失败".into()))
}

/// Frozen fallback remains available when an experiment fails the adoption gate.
pub(crate) async fn extract_legacy_with<F, Fut>(
    source: &str,
    mut call: F,
) -> Result<LearnedStyle, AppError>
where
    F: FnMut(String, String, String) -> Fut,
    Fut: Future<Output = Result<String, AppError>>,
{
    if source.trim().is_empty() {
        return Err(AppError::Parse("样文为空".into()));
    }
    let prompt = include_str!("../eval/fixtures/learn_style_single_v1.md");
    let mut input = source.to_string();
    for attempt in 0..2 {
        let spec = call(format!("legacy_{attempt}"), prompt.into(), input)
            .await?
            .trim()
            .to_string();
        match validate_execution_spec(&spec) {
            Ok(()) => {
                let count = character_count(&spec);
                let warnings = if (400..=600).contains(&count) {
                    vec![]
                } else {
                    vec![format!(
                        "风格为{count}个非空白字符，未达400–600目标，长度评分扣分。"
                    )]
                };
                return Ok(LearnedStyle {
                    spec,
                    analysis: None,
                    evidence_insufficient: false,
                    warnings,
                    calls: attempt + 1,
                });
            }
            Err(e) if attempt == 0 => {
                input = format!(
                    "样文：{source}\n上次输出：{spec}\n程序诊断：{e}\n仅输出修正后的风格Markdown。"
                )
            }
            Err(e) => return Err(e),
        }
    }
    Err(AppError::Parse("风格提取失败".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn analysis() -> String {
        json!({"features":[{"observation":"秘密观察", "evidence":["秘密原句"],
            "rule":"先限定判断的条件，再补一句短问句。", "condition":"展开观点时", "support":"local"}],
            "uncertain":["秘密未观察项"]}).to_string()
    }
    fn prompts() -> LearningPrompts {
        LearningPrompts {
            analyze: "analyze".into(),
            compile: "compile".into(),
        }
    }
    #[test]
    fn rejects_fabricated_evidence_and_false_repeated_support() {
        assert!(parse_analysis(&analysis(), "其他文本").is_err());
        assert!(parse_analysis(&analysis().replace("local", "repeated"), "秘密原句").is_err());
        let parsed = parse_analysis(&analysis(), "秘密原句").unwrap();
        let input = compiler_input(&parsed);
        assert!(!input.contains("秘密"));
        assert!(input.contains("短问句"));
    }
    #[tokio::test]
    async fn frozen_fallback_keeps_plain_markdown_and_rejects_invalid_output() {
        let mut calls = 0;
        let result = extract_legacy_with("原文", |stage, _, _| {
            calls += 1;
            assert!(stage.starts_with("legacy_"));
            std::future::ready(Ok(if calls == 1 {
                "```bad```".into()
            } else {
                "用短句收束。".into()
            }))
        })
        .await
        .unwrap();
        assert_eq!(calls, 2);
        assert_eq!(result.spec, "用短句收束。");
        assert!(result.analysis.is_none());
        assert_eq!(result.warnings.len(), 1);
    }

    #[tokio::test]
    async fn direct_variant_uses_same_validator_and_repair_limit() {
        let mut calls = 0;
        let result = extract_direct_with("样文", "prompt", |stage, _, _| {
            assert!(stage.starts_with("direct_"));
            calls += 1;
            std::future::ready(Ok(if calls == 1 {
                "{}".into()
            } else {
                json!({"spec":"先给判断，再交代条件。","evidence_insufficient":true}).to_string()
            }))
        })
        .await
        .unwrap();
        assert_eq!(calls, 2);
        assert_eq!(result.calls, 2);
        assert!(result.analysis.is_none());
        assert!(!result.warnings.is_empty());
    }

    #[tokio::test]
    async fn invalid_evidence_never_reaches_compiler_and_empty_input_makes_no_call() {
        let mut calls = 0;
        let result = extract_with("其他原文", &prompts(), |stage, _, _| {
            assert!(stage.starts_with("analysis_"));
            calls += 1;
            std::future::ready(Ok(analysis()))
        })
        .await;
        assert!(result.is_err());
        assert_eq!(calls, 2);
        let result = extract_with("", &prompts(), |_, _, _| {
            calls += 1;
            std::future::ready(Ok(analysis()))
        })
        .await;
        assert!(result.is_err());
        assert_eq!(calls, 2);
    }

    #[tokio::test]
    async fn repairs_invalid_analysis_and_preserves_short_evidence_limited_spec() {
        let mut steps = vec![
            "{}".into(),
            analysis(),
            json!({"spec":"- 先限定判断条件，再用短问句回应。", "evidence_insufficient":true})
                .to_string(),
        ]
        .into_iter();
        let mut requests = Vec::new();
        let result = extract_with("秘密原句", &prompts(), |name, _, user| {
            requests.push((name, user));
            std::future::ready(Ok(steps.next().unwrap()))
        })
        .await
        .unwrap();
        assert_eq!(result.calls, 3);
        assert_eq!(requests[1].0, "analysis_1");
        assert!(!requests[2].1.contains("秘密"));
        assert!(result.evidence_insufficient);
        assert!(!result.warnings.is_empty());
    }
    #[tokio::test]
    async fn length_repair_is_bounded_and_never_truncates_valid_rules() {
        let original = "表达".repeat(350);
        let mut steps = vec![
            analysis(),
            json!({"spec":original,"evidence_insufficient":false}).to_string(),
            json!({"spec":original,"evidence_insufficient":false}).to_string(),
        ]
        .into_iter();
        let result = extract_with("秘密原句", &prompts(), |_, _, _| {
            std::future::ready(Ok(steps.next().unwrap()))
        })
        .await
        .unwrap();
        assert_eq!(result.spec, original);
        assert_eq!(result.calls, 3);
        assert_eq!(result.warnings.len(), 1);
    }
    #[tokio::test]
    async fn invalid_compilation_stops_after_one_repair() {
        let mut count = 0;
        let result = extract_with("秘密原句", &prompts(), |_, _, _| {
            count += 1;
            std::future::ready(Ok(if count == 1 { analysis() } else { "{}".into() }))
        })
        .await;
        assert!(result.is_err());
        assert_eq!(count, 3);
    }
}
