//! Transfer v2: independent semantic dimensions and deterministic article length.
use crate::error::AppError;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub(crate) const DIMENSIONS: [&str; 5] = [
    "style_fidelity",
    "source_independence",
    "factual_compliance",
    "naturalness",
    "task_completion",
];
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Scores {
    pub style_fidelity: f64,
    pub source_independence: f64,
    pub factual_compliance: f64,
    pub naturalness: f64,
    pub task_completion: f64,
}
impl Scores {
    pub fn values(&self) -> [f64; 5] {
        [
            self.style_fidelity,
            self.source_independence,
            self.factual_compliance,
            self.naturalness,
            self.task_completion,
        ]
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Leak {
    pub article_quote: String,
    pub source_quote: String,
    pub reason: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Fact {
    pub article_quote: String,
    pub reason: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Judgment {
    pub scores: Scores,
    pub reasons: BTreeMap<String, String>,
    pub source_leakage: Vec<Leak>,
    pub fact_violations: Vec<Fact>,
    pub task_issues: Vec<String>,
    pub uncertainties: Vec<String>,
}

pub(crate) fn length_check(body: &str) -> Value {
    let count = crate::spec_quality::character_count(body);
    json!({"characters":count,"min":600,"max":900,"within_range":(600..=900).contains(&count),
        "below":600_usize.saturating_sub(count),"above":count.saturating_sub(900),
        "policy":"non-whitespace Unicode scalars including title, punctuation and Markdown; diagnostic only"})
}

pub(crate) fn source_body(source: &str) -> &str {
    if source.starts_with("# 样文 ") {
        source.split_once('\n').map(|(_, s)| s.trim()).unwrap_or("")
    } else {
        source.trim()
    }
}

pub(crate) fn judge_input(source: &str, topic: &str, mode: &str, body: &str) -> String {
    json!({"reference":source_body(source),"task":topic,"fact_mode":mode,
        "mechanical_length":length_check(body),"article":body})
    .to_string()
}

pub(crate) fn parse_judgment(raw: &str, source: &str, body: &str) -> Result<Judgment, AppError> {
    let clean = raw
        .trim()
        .strip_prefix("```json")
        .and_then(|s| s.trim().strip_suffix("```"))
        .unwrap_or(raw)
        .trim();
    let j: Judgment = serde_json::from_str(clean)
        .map_err(|e| AppError::Parse(format!("Transfer v2 JSON无效: {e}")))?;
    if j.scores
        .values()
        .iter()
        .any(|v| !v.is_finite() || !(1.0..=5.0).contains(v))
        || j.reasons.len() != DIMENSIONS.len()
        || DIMENSIONS
            .iter()
            .any(|k| j.reasons.get(*k).is_none_or(|v| v.trim().is_empty()))
    {
        return Err(AppError::Parse(
            "五维分数须为1–5，且分别提供非空理由".into(),
        ));
    }
    let quoted = |text: &str, quote: &str| !quote.trim().is_empty() && text.contains(quote);
    for v in &j.source_leakage {
        if !quoted(body, &v.article_quote)
            || !quoted(source_body(source), &v.source_quote)
            || v.reason.trim().is_empty()
        {
            return Err(AppError::Parse(
                "污染证据必须是正文及参考样文可逐字定位的连续摘录，并说明内容关系".into(),
            ));
        }
    }
    for v in &j.fact_violations {
        if !quoted(body, &v.article_quote) || v.reason.trim().is_empty() {
            return Err(AppError::Parse(
                "事实违规必须引用正文可定位的连续摘录并说明权限依据".into(),
            ));
        }
    }
    if (j.scores.source_independence == 5.0) != j.source_leakage.is_empty()
        || (j.scores.factual_compliance == 5.0) != j.fact_violations.is_empty()
        || (j.scores.task_completion == 5.0) != j.task_issues.is_empty()
        || j.task_issues.iter().any(|s| s.trim().is_empty())
    {
        return Err(AppError::Parse(
            "独立性/事实/主题分与对应确定违规项不一致；无证据的怀疑应放uncertainties".into(),
        ));
    }
    Ok(j)
}

/// A failed citation/schema gets one diagnostic repair, never a silent successful score.
pub(crate) async fn grade_with<F, Fut>(
    source: &str,
    topic: &str,
    mode: &str,
    body: &str,
    mut call: F,
) -> Result<(Judgment, usize), AppError>
where
    F: FnMut(usize, String) -> Fut,
    Fut: std::future::Future<Output = Result<String, AppError>>,
{
    let original = judge_input(source, topic, mode, body);
    let mut user = original.clone();
    for attempt in 0..2 {
        let raw = call(attempt, user).await?;
        match parse_judgment(&raw,source,body) {
            Ok(j)=>return Ok((j,attempt+1)),
            Err(e) if attempt==0=>user=json!({"original_input":serde_json::from_str::<Value>(&original).unwrap(),
                "previous_response":raw,"validation_error":e.to_string(),
                "repair_instruction":"修正JSON或引用定位；保留有依据的判断，不为通过校验虚构证据。重新输出完整评分JSON。"}).to_string(),
            Err(e)=>return Err(e),
        }
    }
    Err(AppError::Parse("评分修复失败".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Value {
        json!({"scores":{"style_fidelity":4,"source_independence":5,"factual_compliance":5,"naturalness":4,"task_completion":5},
            "reasons":DIMENSIONS.into_iter().map(|s|(s,"依据")).collect::<BTreeMap<_,_>>(),
            "source_leakage":[],"fact_violations":[],"task_issues":[],"uncertainties":[]})
    }
    #[tokio::test]
    async fn ungrounded_evidence_is_repaired_once_without_inflating_the_score() {
        let mut bad = fixture();
        bad["scores"]["source_independence"] = json!(3);
        bad["source_leakage"] =
            json!([{"article_quote":"正文证据","source_quote":"不存在的证据","reason":"内容搬运"}]);
        let mut good = bad.clone();
        good["source_leakage"][0]["source_quote"] = json!("原文证据");
        let mut calls = 0;
        let (j, n) = grade_with(
            "原文证据",
            "主题",
            "strict",
            "正文证据",
            |attempt, user| {
                calls += 1;
                if attempt == 1 {
                    assert!(user.contains("validation_error"));
                    assert!(user.contains("不存在的证据"));
                }
                std::future::ready(Ok(if attempt == 0 {
                    bad.to_string()
                } else {
                    good.to_string()
                }))
            },
        )
        .await
        .unwrap();
        assert_eq!((n, calls), (2, 2));
        assert_eq!(j.scores.source_independence, 3.0);
        calls = 0;
        let result = grade_with(
            "原文证据",
            "主题",
            "strict",
            "正文证据",
            |_, _| {
                calls += 1;
                std::future::ready(Ok(bad.to_string()))
            },
        )
        .await;
        assert!(result.is_err());
        assert_eq!(calls, 2);
    }

    #[test]
    fn facts_do_not_automatically_reduce_style_source_or_task_scores() {
        let mut v = fixture();
        v["scores"]["factual_compliance"] = json!(3);
        v["fact_violations"] = json!([{"article_quote":"全国83%", "reason":"素材未提供数据"}]);
        let j = parse_judgment(&v.to_string(), "样文", "调查称全国83%如此。").unwrap();
        assert_eq!(j.scores.source_independence, 5.0);
        assert_eq!(j.scores.task_completion, 5.0);
        assert_eq!(j.scores.style_fidelity, 4.0);
    }
    #[test]
    fn rejects_unanchored_quotes_score_overflow_and_unsupported_deductions() {
        let mut v = fixture();
        v["scores"]["source_independence"] = json!(3);
        assert!(parse_judgment(&v.to_string(), "原文", "正文").is_err());
        v["source_leakage"] =
            json!([{"article_quote":"正文", "source_quote":"伪造原文", "reason":"相似"}]);
        assert!(parse_judgment(&v.to_string(), "原文", "正文").is_err());
        v["source_leakage"][0]["source_quote"] = json!("原文");
        assert!(parse_judgment(&v.to_string(), "原文", "正文").is_ok());
        v["scores"]["naturalness"] = json!(6);
        assert!(parse_judgment(&v.to_string(), "原文", "正文").is_err());
    }
    #[test]
    fn length_boundaries_are_separate_and_anonymous_input_excludes_labels() {
        for (n, ok) in [(599, false), (600, true), (900, true), (901, false)] {
            assert_eq!(length_check(&"字".repeat(n))["within_range"], ok);
        }
        assert_eq!(length_check("中 文\n😀")["characters"], 3);
        let input = judge_input("# 样文 1：作者标签\n\n真实正文", "任务", "creative", "文章");
        assert!(!input.contains("作者标签"));
        assert!(!input.contains("variant"));
        assert!(input.contains("真实正文"));
        assert!(input.contains("creative"));
        let j = parse_judgment(&fixture().to_string(), "样文", "短").unwrap();
        assert_eq!(j.scores.task_completion, 5.0);
    }
}
