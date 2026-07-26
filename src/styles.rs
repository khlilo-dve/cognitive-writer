use std::fs;
use std::path::Path;
use std::time::SystemTime;

use crate::error::AppError;

/// 风格摘要
pub struct StyleSummary {
    pub filename: String,     // e.g. "qingbian.md"
    pub display_name: String, // e.g. "qingbian" (without extension)
    pub description: String,  // first non-empty non-heading paragraph
}

// ── private helpers ────────────────────────────────────────────────────

/// Extract the first non-empty, non-heading paragraph from a Markdown style
/// file as a one-line description (capped at 120 chars).
fn extract_description(content: &str) -> String {
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
fn is_recent_query(query: &str) -> bool {
    let keywords = ["刚学", "最近", "上次"];
    keywords.iter().any(|kw| query.contains(kw))
}

/// Find the most recently modified .md file in the given directory.
/// Returns `(display_name, file_content)`.
fn find_most_recent(styles_dir: &str) -> Result<(String, String), AppError> {
    let dir_entries =
        fs::read_dir(styles_dir).map_err(|e| AppError::NoStyles(format!("无法读取风格目录: {e}")))?;

    let mut best: Option<(SystemTime, String, String)> = None;

    for entry in dir_entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "md") {
            let metadata = fs::metadata(&path).map_err(|e| {
                AppError::FileRead(format!("无法读取文件元数据 `{}`: {e}", path.display()))
            })?;
            let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            let content = fs::read_to_string(&path)
                .map_err(|e| AppError::FileRead(format!("无法读取 `{}`: {e}", path.display())))?;
            let display = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string();
            match &best {
                Some((t, _, _)) if modified <= *t => {}
                _ => best = Some((modified, display, content)),
            }
        }
    }

    best.map(|(_, d, c)| (d, c))
        .ok_or_else(|| AppError::NoStyles(format!("{styles_dir} 目录下没有 .md 风格文件")))
}

// ── public API ─────────────────────────────────────────────────────────

/// Match a style file based on user query with three-tier priority:
///
/// 1. Exact filename match  (query "qingbian" → `styles/qingbian.md`)
/// 2. Content keyword match (scans first 200 chars of every .md file)
/// 3. Most-recently-modified (when query contains "刚学"/"最近"/"上次")
///
/// Returns `(display_name, file_content)` on success.
pub fn fuzzy_match_style(
    styles_dir: &str,
    query: &str,
) -> Result<(String, String), AppError> {
    let dir_entries =
        fs::read_dir(styles_dir).map_err(|e| AppError::NoStyles(format!("无法读取风格目录: {e}")))?;

    // Collect .md entries
    let mut md_files: Vec<(String, String)> = Vec::new(); // (display_name, full_path_string)
    for entry in dir_entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "md") {
            let display = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string();
            md_files.push((display, path.display().to_string()));
        }
    }

    if md_files.is_empty() {
        return Err(AppError::NoStyles(format!(
            "{styles_dir} 目录下没有 .md 风格文件"
        )));
    }

    // Priority 1: exact filename match
    let exact_path = Path::new(styles_dir).join(format!("{query}.md"));
    if exact_path.exists() {
        let content = fs::read_to_string(&exact_path)
            .map_err(|e| AppError::FileRead(format!("无法读取 `{}`: {e}", exact_path.display())))?;
        return Ok((query.to_string(), content));
    }

    // Priority 3 (checked before 2 as it can short-circuit): recent query
    if is_recent_query(query) {
        return find_most_recent(styles_dir);
    }

    // Priority 2: content keyword match (first 200 chars)
    for (display, path_str) in &md_files {
        let content = fs::read_to_string(path_str).map_err(|e| {
            AppError::FileRead(format!("无法读取 `{path_str}`: {e}"))
        })?;
        let preview: String = content.chars().take(200).collect();
        if preview.contains(query) {
            return Ok((display.clone(), content));
        }
    }

    Err(AppError::NoStyles(format!(
        "未找到匹配 '{query}' 的风格文件"
    )))
}

/// List all styles with one-line descriptions.
///
/// Returns `Err(NoStyles)` when the directory is empty or contains no .md
/// files.
pub fn list_styles_with_desc(styles_dir: &str) -> Result<Vec<StyleSummary>, AppError> {
    let entries =
        fs::read_dir(styles_dir).map_err(|e| AppError::NoStyles(format!("无法读取风格目录: {e}")))?;

    let mut results: Vec<StyleSummary> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "md") {
            let filename = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown.md")
                .to_string();
            let display_name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string();
            let content = fs::read_to_string(&path).map_err(|e| {
                AppError::FileRead(format!("无法读取 `{}`: {e}", path.display()))
            })?;
            let description = extract_description(&content);
            results.push(StyleSummary {
                filename,
                display_name,
                description,
            });
        }
    }

    if results.is_empty() {
        return Err(AppError::NoStyles(format!(
            "{styles_dir} 目录下没有 .md 风格文件"
        )));
    }

    results.sort_by(|a, b| a.display_name.cmp(&b.display_name));
    Ok(results)
}

/// Read and return the full content of a style file by its display name
/// (without `.md` extension).
pub fn show_style_detail(styles_dir: &str, name: &str) -> Result<String, AppError> {
    let path = Path::new(styles_dir).join(format!("{name}.md"));
    fs::read_to_string(&path).map_err(|e| {
        AppError::FileRead(format!("无法读取风格文件 `{}`: {e}", path.display()))
    })
}

/// Delete a style file by its display name (without `.md` extension).
///
/// The caller is responsible for performing a confirmation check before
/// calling this function.
pub fn delete_style(styles_dir: &str, name: &str) -> Result<(), AppError> {
    let path = Path::new(styles_dir).join(format!("{name}.md"));
    fs::remove_file(&path)
        .map_err(|e| AppError::FileRead(format!("无法删除风格文件 `{}`: {e}", path.display())))
}

// ── tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    /// Helper: create a temporary styles directory populated with the given
    /// (filename, content) pairs.
    fn setup_temp_styles(files: &[(&str, &str)]) -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        for (name, content) in files {
            let path = dir.path().join(name);
            let mut f = fs::File::create(&path).unwrap();
            f.write_all(content.as_bytes()).unwrap();
        }
        let dir_str = dir.path().to_str().unwrap().to_string();
        (dir, dir_str)
    }

    // ── fuzzy_match_style ──────────────────────────────────────────

    #[test]
    fn test_fuzzy_match_exact_filename() {
        let (_dir, dir_str) = setup_temp_styles(&[
            ("qingbian.md", "# 轻辩风格\n这是轻辩的描述内容。"),
            ("other.md", "# Other\n无关内容。"),
        ]);

        let (name, content) = fuzzy_match_style(&dir_str, "qingbian").unwrap();
        assert_eq!(name, "qingbian");
        assert!(content.contains("轻辩风格"));
    }

    #[test]
    fn test_fuzzy_match_content_keyword() {
        let (_dir, dir_str) = setup_temp_styles(&[
            ("style_a.md", "# 风格A\n这是一篇关于轻辩的文章。"),
            ("style_b.md", "# 风格B\n普通内容。"),
        ]);

        // "轻辩" is not an exact filename match, but exists in style_a.md content
        let (name, _content) = fuzzy_match_style(&dir_str, "轻辩").unwrap();
        assert_eq!(name, "style_a");
    }

    #[test]
    fn test_fuzzy_match_not_found() {
        let (_dir, dir_str) = setup_temp_styles(&[
            ("only.md", "# Only\n没有匹配的内容。"),
        ]);

        let result = fuzzy_match_style(&dir_str, "不存在的关键词");
        assert!(result.is_err());
    }

    #[test]
    fn test_fuzzy_match_recent_keyword() {
        let (_dir, dir_str) = setup_temp_styles(&[
            ("alpha.md", "# Alpha\n第一个文件。"),
            ("beta.md", "# Beta\n第二个文件。"),
        ]);

        // "最近" triggers most-recently-modified logic
        let result = fuzzy_match_style(&dir_str, "最近用过的风格");
        // Either file could be "more recent" depending on filesystem timing;
        // we just verify it returns Ok (finds something).
        assert!(result.is_ok());
    }

    #[test]
    fn test_fuzzy_match_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let dir_str = dir.path().to_str().unwrap().to_string();
        let result = fuzzy_match_style(&dir_str, "anything");
        assert!(result.is_err());
    }

    // ── list_styles_with_desc ───────────────────────────────────────

    #[test]
    fn test_list_styles_with_desc() {
        let (_dir, dir_str) = setup_temp_styles(&[
            (
                "style_a.md",
                "# 风格A\n这是风格A的第一段描述内容。\n## 二级标题\n更多内容。",
            ),
            (
                "style_b.md",
                "# 风格B\n风格B的简介在这里。",
            ),
        ]);

        let summaries = list_styles_with_desc(&dir_str).unwrap();
        assert_eq!(summaries.len(), 2);
        // sorted by display_name
        assert_eq!(summaries[0].display_name, "style_a");
        assert_eq!(summaries[1].display_name, "style_b");
        assert_eq!(summaries[0].filename, "style_a.md");
        assert!(summaries[0].description.contains("风格A"));
        assert!(summaries[1].description.contains("风格B"));
    }

    #[test]
    fn test_list_styles_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let dir_str = dir.path().to_str().unwrap().to_string();
        let result = list_styles_with_desc(&dir_str);
        assert!(result.is_err());
    }

    #[test]
    fn test_list_styles_no_md_files() {
        let dir = tempfile::tempdir().unwrap();
        let dir_str = dir.path().to_str().unwrap().to_string();
        let _f = fs::File::create(format!("{dir_str}/notes.txt")).unwrap();
        let result = list_styles_with_desc(&dir_str);
        assert!(result.is_err());
    }

    // ── show_style_detail ───────────────────────────────────────────

    #[test]
    fn test_show_style_detail() {
        let (_dir, dir_str) = setup_temp_styles(&[
            (
                "mystyle.md",
                "# My Style\n完整的风格描述内容在这里。\n\n更多段落。",
            ),
        ]);

        let content = show_style_detail(&dir_str, "mystyle").unwrap();
        assert!(content.contains("My Style"));
        assert!(content.contains("完整的风格描述"));
        assert!(content.contains("更多段落"));
    }

    #[test]
    fn test_show_style_detail_not_found() {
        let (_dir, dir_str) = setup_temp_styles(&[("exists.md", "# Exists")]);
        let result = show_style_detail(&dir_str, "nonexistent");
        assert!(result.is_err());
    }

    // ── delete_style ────────────────────────────────────────────────

    #[test]
    fn test_delete_style() {
        let (_dir, dir_str) = setup_temp_styles(&[
            ("todelete.md", "# To Delete\n待删除。"),
            ("keep.md", "# Keep\n保留。"),
        ]);

        delete_style(&dir_str, "todelete").unwrap();

        let path = std::path::Path::new(&dir_str).join("todelete.md");
        assert!(!path.exists());

        let keep_path = std::path::Path::new(&dir_str).join("keep.md");
        assert!(keep_path.exists());
    }

    #[test]
    fn test_delete_style_not_found() {
        let (_dir, dir_str) = setup_temp_styles(&[("exists.md", "# Exists")]);
        let result = delete_style(&dir_str, "nonexistent");
        assert!(result.is_err());
    }

    // ── extract_description ─────────────────────────────────────────

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
