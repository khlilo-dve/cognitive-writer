//! Deterministic Spec length policy, independent of model estimates.
pub fn character_count(spec: &str) -> usize {
    spec.chars().filter(|c| !c.is_whitespace()).count()
}

/// Historical compact policy retained for frozen calibration.
pub fn conciseness_score(count: usize) -> f64 {
    match count {
        400..=600 => 5.0,
        300..=399 | 601..=700 => 4.0,
        200..=299 | 701..=850 => 3.0,
        100..=199 | 851..=1000 => 2.0,
        _ => 1.0,
    }
}

pub const DETAILED_MIN: usize = 1500;
pub const DETAILED_POLICY: &str = "detailed-v1";

/// Normalize only a complete, explicitly labelled Markdown outer wrapper.
pub(crate) fn execution_markdown(raw: &str) -> &str {
    let trimmed = raw.trim();
    for opening in ["```markdown\n", "```md\n"] {
        if let Some(inner) = trimmed
            .strip_prefix(opening)
            .and_then(|s| s.strip_suffix("\n```"))
        {
            return inner.trim();
        }
    }
    trimmed
}

/// Length sufficiency only; redundancy and usefulness require separate review.
pub fn detailed_length_score(count: usize) -> f64 {
    match count {
        0..=499 => 1.0,
        500..=999 => 2.0,
        1000..=1249 => 3.0,
        1250..=1499 => 4.0,
        _ => 5.0,
    }
}

/// Free-form execution Markdown; length target is scored separately, not padded into validity.
pub fn validate_execution_spec(spec: &str) -> Result<(), crate::error::AppError> {
    let count = character_count(spec);
    if count == 0
        || spec.contains("```")
        || spec.contains("~~~")
        || spec.trim_start().starts_with('{')
        || serde_json::from_str::<serde_json::Value>(spec).is_ok()
        || !spec.lines().any(|l| {
            let l = l.trim();
            !l.is_empty() && !l.starts_with('#') && l.chars().any(char::is_alphanumeric)
        })
    {
        return Err(crate::error::AppError::Parse(
            "风格执行文本为空、只有标题、含 JSON/代码围栏".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_format_accepts_unheaded_rules_but_rejects_broken_envelopes() {
        for valid in [
            "先给判断，再用短问句限定。",
            "## 节奏\n短句收束。",
            "- 先叙述，再停顿。",
        ] {
            assert!(validate_execution_spec(valid).is_ok());
        }
        for invalid in [
            "",
            "## 节奏",
            "```markdown\n规则\n```",
            "{}",
            r#"["规则"]"#,
            r#""规则""#,
            "---",
        ] {
            assert!(validate_execution_spec(invalid).is_err());
        }
        assert!(validate_execution_spec(&"字".repeat(6500)).is_ok());
    }

    #[test]
    fn detailed_length_rewards_sufficiency_without_penalizing_long_specs() {
        for (n, score) in [
            (0, 1.0),
            (499, 1.0),
            (500, 2.0),
            (999, 2.0),
            (1000, 3.0),
            (1249, 3.0),
            (1250, 4.0),
            (1499, 4.0),
            (1500, 5.0),
            (2000, 5.0),
            (5000, 5.0),
            (6500, 5.0),
        ] {
            assert_eq!(detailed_length_score(n), score);
        }
    }

    #[test]
    fn counts_unicode_scalars_and_markdown_but_not_unicode_whitespace() {
        assert_eq!(character_count("## 标题\n- 中，A😀\t\r\n\u{3000}\u{a0}"), 9);
        assert_eq!(character_count(" \n\t\u{2003}"), 0);
    }

    #[test]
    fn every_length_boundary_including_underlength_is_scored() {
        for (count, score) in [
            (0, 1.0),
            (99, 1.0),
            (100, 2.0),
            (199, 2.0),
            (200, 3.0),
            (299, 3.0),
            (300, 4.0),
            (399, 4.0),
            (400, 5.0),
            (600, 5.0),
            (601, 4.0),
            (700, 4.0),
            (701, 3.0),
            (850, 3.0),
            (851, 2.0),
            (1000, 2.0),
            (1001, 1.0),
            (usize::MAX, 1.0),
        ] {
            assert_eq!(conciseness_score(count), score, "length {count}");
        }
    }
}
