use crate::error::AppError;
use serde::{Deserialize, Serialize};
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Evidence {
    article_quote: String,
    reference: String,
    reference_quote: String,
    reason: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AuthorJudgment {
    pub choice: String,
    pub fidelity_a: f64,
    pub fidelity_b: f64,
    pub confidence: String,
    pub reason: String,
    pub evidence: Vec<Evidence>,
    pub uncertainties: Vec<String>,
}
pub(crate) fn parse(raw: &str, a: &str, b: &str, body: &str) -> Result<AuthorJudgment, AppError> {
    let j: AuthorJudgment =
        serde_json::from_str(raw).map_err(|e| AppError::Parse(format!("作者匹配JSON: {e}")))?;
    if !["A", "B", "both", "neither", "uncertain"].contains(&j.choice.as_str())
        || !["low", "medium", "high"].contains(&j.confidence.as_str())
        || j.reason.trim().is_empty()
        || [j.fidelity_a, j.fidelity_b]
            .iter()
            .any(|v| !v.is_finite() || !(1.0..=5.0).contains(v))
    {
        return Err(AppError::Parse("作者匹配选项/评分/解释无效".into()));
    }
    for e in &j.evidence {
        let reference = match e.reference.as_str() {
            "A" => a,
            "B" => b,
            _ => return Err(AppError::Parse("作者证据参考标签无效".into())),
        };
        if e.article_quote.trim().is_empty()
            || e.reference_quote.trim().is_empty()
            || !body.contains(&e.article_quote)
            || !reference.contains(&e.reference_quote)
            || e.reason.trim().is_empty()
        {
            return Err(AppError::Parse("作者匹配引文无法逐字定位".into()));
        }
    }
    if ["A", "B"].contains(&j.choice.as_str()) && j.evidence.is_empty() {
        return Err(AppError::Parse("明确作者匹配必须提供定位证据".into()));
    }
    Ok(j)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn uncertainty_is_valid_but_fabricated_attribution_is_not() {
        let raw = r#"{"choice":"uncertain","fidelity_a":2,"fidelity_b":2,"confidence":"low","reason":"缺乏可区分证据","evidence":[],"uncertainties":["参考不足"]}"#;
        assert!(parse(raw, "参考甲", "参考乙", "正文").is_ok());
        assert!(parse(
            &raw.replace("uncertain\"", "A\""),
            "参考甲",
            "参考乙",
            "正文"
        )
        .is_err());
    }
}
