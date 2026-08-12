use dialoguer::{Select, theme::ColorfulTheme};
use std::fs;
use std::path::Path;

// ── Style selection ─────────────────────────────────────────────────

pub fn list_styles(dir: &str) -> Result<Vec<String>, String> {
    let mut styles = Vec::new();
    let entries = fs::read_dir(dir)
        .map_err(|e| format!("无法读取 {} 目录: {}", dir, e))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "md") {
            styles.push(path.display().to_string());
        }
    }

    styles.sort();
    if styles.is_empty() {
        return Err(format!("{} 目录下没有 .md 风格文件", dir));
    }
    Ok(styles)
}

pub fn select_style(styles: &[String]) -> Result<usize, String> {
    if styles.len() == 1 {
        let name = styles[0].strip_prefix("styles/").unwrap_or(&styles[0]);
        println!("[info] 使用唯一风格: {}", name);
        return Ok(0);
    }

    let labels: Vec<&str> = styles
        .iter()
        .map(|s| s.strip_prefix("styles/").unwrap_or(s))
        .collect();

    Select::with_theme(&ColorfulTheme::default())
        .with_prompt("请选择写作风格")
        .items(&labels)
        .default(0)
        .interact()
        .map_err(|e| format!("风格选择失败: {}", e))
}

// ── File I/O ────────────────────────────────────────────────────────

pub fn read_file(path: &str) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| format!("无法读取 `{}`: {}", path, e))
}

pub fn extract_idea_slug(idea: &str) -> String {
    let raw = idea
        .lines()
        .find_map(|l| {
            let stripped = l.trim_start_matches('#').trim_start();
            stripped
                .strip_prefix("文章主题：")
                .or_else(|| stripped.strip_prefix("文章主题:"))
        })
        .or_else(|| {
            idea.lines()
                .find(|l| l.starts_with('#') && !l.trim_start_matches('#').is_empty())
                .map(|l| l.trim_start_matches('#').trim())
        })
        .unwrap_or("untitled");

    let slug: String = raw
        .trim()
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
        .collect();

    if slug.is_empty() {
        "untitled".to_string()
    } else {
        slug
    }
}

pub fn next_version(dir: &str, slug: &str) -> u32 {
    let prefix = format!("{}_v", slug);
    let mut max: u32 = 0;

    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if let Some(rest) = name.strip_prefix(&prefix) {
                // 同时匹配 .md 和 .html 后缀
                let num_str = rest
                    .strip_suffix(".md")
                    .or_else(|| rest.strip_suffix(".html"));
                if let Some(n) = num_str.and_then(|s| s.parse::<u32>().ok()) {
                    max = max.max(n);
                }
            }
        }
    }

    max + 1
}

pub fn write_file(path: &str, content: &str) -> Result<(), String> {
    let dir = Path::new(path).parent().unwrap_or(Path::new("."));
    if !dir.exists() {
        fs::create_dir_all(dir).map_err(|e| format!("无法创建目录 `{}`: {}", dir.display(), e))?;
    }
    fs::write(path, content).map_err(|e| format!("无法写入 `{}`: {}", path, e))
}

#[cfg(test)]
pub fn delete_file(path: &str) -> Result<(), String> {
    std::fs::remove_file(path)
        .map_err(|e| format!("无法删除 `{}`: {}", path, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── extract_idea_slug ──────────────────────────────────────

    #[test]
    fn test_extract_slug_article_theme_with_hash_header() {
        // ## 创作素材 行不会匹配（无文章主题：），第二行提取
        let input = "## 创作素材\n文章主题：分享一个高级的行动逻辑";
        let slug = extract_idea_slug(input);
        // 中文为 Unicode alphanumeric，仅空格被过滤
        assert!(slug.contains("分享一个高级的行动逻辑"));
    }

    #[test]
    fn test_extract_slug_simple_hash_heading() {
        let slug = extract_idea_slug("# 测试标题");
        assert_eq!(slug, "测试标题");
    }

    #[test]
    fn test_extract_slug_hash_article_theme_english() {
        // 空格被过滤 → helloworld
        let slug = extract_idea_slug("#文章主题：hello world");
        assert_eq!(slug, "helloworld");
    }

    #[test]
    fn test_extract_slug_empty_input() {
        let slug = extract_idea_slug("");
        assert_eq!(slug, "untitled");
    }

    #[test]
    fn test_extract_slug_plain_text_no_heading() {
        let slug = extract_idea_slug("这是一段没有标题的纯文本内容");
        assert_eq!(slug, "untitled");
    }

    #[test]
    fn test_extract_slug_article_theme_without_hash() {
        let slug = extract_idea_slug("文章主题：中文标题\n一些正文内容");
        assert_eq!(slug, "中文标题");
    }

    #[test]
    fn test_extract_slug_hash_article_theme_underscore() {
        let slug = extract_idea_slug("# 文章主题：Test_123");
        assert_eq!(slug, "Test_123");
    }

    #[test]
    fn test_extract_slug_only_special_chars() {
        // @ 不是 alphanumeric，全被过滤 → 空 slug → untitled
        let slug = extract_idea_slug("# @@@");
        assert_eq!(slug, "untitled");
    }

    #[test]
    fn test_extract_slug_article_theme_not_first_line() {
        let input = "一些前言说明\n第二行\n文章主题：真正的标题\n正文内容";
        let slug = extract_idea_slug(input);
        assert_eq!(slug, "真正的标题");
    }

    #[test]
    fn test_extract_slug_fallback_to_first_hash_heading() {
        // 没有 文章主题： 时，回退到第一个 # 开头的有内容行
        let input = "# 回退标题";
        let slug = extract_idea_slug(input);
        assert_eq!(slug, "回退标题");
    }

    // ── next_version ────────────────────────────────────────────

    #[test]
    fn test_next_version_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let dir_path = dir.path().to_str().unwrap();
        assert_eq!(next_version(dir_path, "test"), 1);
    }

    #[test]
    fn test_next_version_has_v1_md() {
        let dir = tempfile::tempdir().unwrap();
        let dir_path = dir.path().to_str().unwrap();
        std::fs::write(format!("{}/test_v1.md", dir_path), "content").unwrap();
        assert_eq!(next_version(dir_path, "test"), 2);
    }

    #[test]
    fn test_next_version_mixed_md_and_html() {
        let dir = tempfile::tempdir().unwrap();
        let dir_path = dir.path().to_str().unwrap();
        std::fs::write(format!("{}/test_v1.md", dir_path), "").unwrap();
        std::fs::write(format!("{}/test_v1.html", dir_path), "").unwrap();
        std::fs::write(format!("{}/test_v2.md", dir_path), "").unwrap();
        assert_eq!(next_version(dir_path, "test"), 3);
    }

    #[test]
    fn test_next_version_different_slugs_independent() {
        let dir = tempfile::tempdir().unwrap();
        let dir_path = dir.path().to_str().unwrap();
        std::fs::write(format!("{}/apple_v1.md", dir_path), "").unwrap();
        std::fs::write(format!("{}/apple_v2.md", dir_path), "").unwrap();
        // banana slug 不受 apple 文件影响
        assert_eq!(next_version(dir_path, "banana"), 1);
        assert_eq!(next_version(dir_path, "apple"), 3);
    }

    #[test]
    fn test_next_version_only_matches_md_and_html() {
        let dir = tempfile::tempdir().unwrap();
        let dir_path = dir.path().to_str().unwrap();
        std::fs::write(format!("{}/test_v1.txt", dir_path), "").unwrap();
        std::fs::write(format!("{}/test_v2.pdf", dir_path), "").unwrap();
        // .txt 和 .pdf 不计入
        assert_eq!(next_version(dir_path, "test"), 1);
    }

    #[test]
    fn test_next_version_nonexistent_dir_returns_1() {
        // 目录不存在时 fs::read_dir 返回 Err，fallback 到 max=0 → 返回 1
        assert_eq!(next_version("/nonexistent/path/for/testing", "test"), 1);
    }

    // ── delete_file ────────────────────────────────────────────

    #[test]
    fn test_delete_file_success() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test_delete.md");
        let path_str = file_path.to_str().unwrap();
        std::fs::write(path_str, "content").unwrap();
        assert!(file_path.exists());
        assert!(delete_file(path_str).is_ok());
        assert!(!file_path.exists());
    }

    #[test]
    fn test_delete_file_not_found() {
        let result = delete_file("/nonexistent/path/for/testing/delete_file_test.md");
        assert!(result.is_err());
    }
}
