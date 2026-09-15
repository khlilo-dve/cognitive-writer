//! 风格名称校验、拼音归一化与摘要提取。
use crate::error::AppError;
use pinyin::ToPinyin;
use std::path::{Path, PathBuf};

/// Extract the first non-empty, non-heading paragraph from a Markdown style
/// file as a one-line description (capped at 120 chars).
pub(crate) fn extract_description(content: &str) -> String {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with('#') {
            continue;
        }
        // Find the byte position after the first sentence-ending punctuation.
        let end_byte = trimmed
            .char_indices()
            .find(|(_, c)| matches!(c, '。' | '！' | '？'))
            .map(|(idx, c)| idx + c.len_utf8())
            .unwrap_or(trimmed.len());

        let desc = &trimmed[..end_byte];
        let char_count = desc.chars().count();
        if char_count > 120 {
            let truncated: String = desc.chars().take(120).collect();
            return format!("{}...", truncated);
        }
        return desc.to_string();
    }
    String::new()
}

/// Check whether `query` contains time-related keywords used to indicate
/// "the most recent style".
pub(crate) fn is_recent_query(query: &str) -> bool {
    let keywords = ["刚学", "最近", "上次"];
    keywords.iter().any(|kw| query.contains(kw))
}

/// 将用户提供的风格名限制为单个文件名，防止读写操作逃出 styles 目录。
pub(crate) fn style_file_path(styles_dir: &str, name: &str) -> Result<PathBuf, AppError> {
    let name = name.trim();
    let components: Vec<_> = Path::new(name).components().collect();
    if name.is_empty()
        || name.contains(['/', '\\', '\0'])
        || !matches!(components.as_slice(), [std::path::Component::Normal(_)])
    {
        return Err(AppError::FileRead(format!(
            "无效的风格名称 `{name}`：名称必须是单个文件名"
        )));
    }

    Ok(Path::new(styles_dir).join(format!("{name}.md")))
}

/// 把名称归一化为可用于精确比对的键：中文转无声调拼音，ASCII 转小写，其余原样。
///
/// 这样「轻辩」与「qingbian」得到同一个键，而「qingbain」保持不同，实现
/// 中文名 ↔ 拼音文件名的精确跨编码匹配，同时不引入模糊匹配。
pub(crate) fn match_key(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.trim().chars() {
        if let Some(py) = c.to_pinyin() {
            out.push_str(py.plain());
        } else {
            out.extend(c.to_lowercase());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_match_key_chinese_to_pinyin() {
        assert_eq!(match_key("轻辩"), "qingbian");
    }

    #[test]
    fn test_match_key_ascii_lowercased() {
        assert_eq!(match_key("QingBian"), "qingbian");
    }

    #[test]
    fn test_match_key_mixed_chinese_and_ascii() {
        // 中文转拼音、英文小写，用于混合名
        assert_eq!(match_key("轻bian"), "qingbian");
    }

    #[test]
    fn test_extract_description_basic() {
        let input = "# 标题\n这是描述。\n第二段。";
        assert_eq!(extract_description(input), "这是描述。");
    }

    #[test]
    fn test_extract_description_skips_empty_lines() {
        let input = "\n\n# 标题\n\n这是描述内容。";
        assert_eq!(extract_description(input), "这是描述内容。");
    }

    #[test]
    fn test_extract_description_no_description() {
        let input = "# 只有标题\n## 二级\n### 三级";
        assert_eq!(extract_description(input), "");
    }

    #[test]
    fn test_extract_description_empty_input() {
        assert_eq!(extract_description(""), "");
    }

    #[test]
    fn test_extract_description_long_first_sentence() {
        let long = "a".repeat(150) + "。";
        let input = format!("# Title\n{long}");
        let desc = extract_description(&input);
        assert!(desc.len() <= 123); // 120 + "..."
        assert!(desc.ends_with("..."));
    }
}
