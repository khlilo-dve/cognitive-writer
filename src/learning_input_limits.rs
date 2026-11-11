//! Paid learning input budget, separate from stylistic completeness or output length.
use crate::error::AppError;
pub(crate) const MAX_TEXT_BYTES: usize = 256 * 1024;
pub(crate) fn validate(texts: &[String]) -> Result<(), AppError> {
    if texts
        .iter()
        .try_fold(0usize, |total, s| total.checked_add(s.len()))
        .is_none_or(|n| n > MAX_TEXT_BYTES)
    {
        return Err(AppError::LearningWorkspace("选定文章正文合计超过256KiB学习输入预算；请缩小文章范围或提供较短的完整文章，未截断也未发付费请求".into()));
    }
    Ok(())
}
fn error(message: &str) -> AppError {
    AppError::LearningWorkspace(message.into())
}
pub(crate) fn validate_request(
    request: &crate::learning_sources::SourceRequest,
) -> Result<(), AppError> {
    if request.sources.is_empty()
        || request.sources.len() > 32
        || request.note.trim().is_empty()
        || request
            .expected_articles
            .is_some_and(|n| !(1..=10).contains(&n))
    {
        return Err(error(
            "需要1–32项来源、学习目标note，明确独立篇数须为1–10；不会静默截取来源",
        ));
    }
    for input in &request.sources {
        if let crate::learning_sources::SourceInput::Url { url } = input {
            if let Ok(u) = reqwest::Url::parse(url) {
                if !u.username().is_empty() || u.password().is_some() {
                    return Err(error("来源URL不能含内嵌认证信息，未保存该请求"));
                }
            }
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn combined_unicode_byte_budget_checks_the_whole_set_without_shortening() {
        assert!(validate(&["文".repeat(MAX_TEXT_BYTES / 3)]).is_ok());
        let texts = vec!["a".repeat(MAX_TEXT_BYTES), "b".into()];
        assert!(validate(&texts).is_err());
        assert_eq!(texts[0].len(), MAX_TEXT_BYTES);
    }
}
