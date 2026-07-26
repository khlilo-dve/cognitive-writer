use crate::error::AppError;
use std::path::Path;
use std::process::Command;

// ── extract_summary (内部函数) ────────────────────────────────────────

/// 从 Markdown 正文提取摘要
///
/// 1. 跳过 `#` 开头的标题行
/// 2. 跳过空行
/// 3. 取第一个有内容的段落的文本
/// 4. 截断到 max_chars，如果截断在词中间则回退到最近空格
/// 5. 如果正文超过 max_chars 则末尾加 `...`
fn extract_summary(markdown: &str, max_chars: usize) -> String {
    for line in markdown.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let char_count = trimmed.chars().count();
        if char_count <= max_chars {
            return trimmed.to_string();
        }

        // 截断到 max_chars 字符
        let truncated: String = trimmed.chars().take(max_chars).collect();

        // 英文按词边界截断：找到最后一个空格，在此处截断
        if let Some(last_space) = truncated.rfind(' ') {
            return format!("{}...", &truncated[..last_space]);
        }

        return format!("{}...", truncated);
    }
    String::new()
}

// ── write_mdx_draft ───────────────────────────────────────────────────

/// 将文章写入个人网站项目的 MDX 草稿文件
///
/// # Arguments
/// * `website_path` - 网站项目根路径
/// * `taxonomy` - 内容分类目录（"signal" / "node" / "pow"）
/// * `slug` - 文件名 slug（不含扩展名）
/// * `title` - 文章标题
/// * `markdown_body` - 文章正文（Markdown 格式）
///
/// # Returns
/// 成功时返回写入的完整文件路径
pub fn write_mdx_draft(
    website_path: &str,
    taxonomy: &str,
    slug: &str,
    title: &str,
    markdown_body: &str,
) -> Result<String, AppError> {
    // 检查网站路径是否存在
    let root = Path::new(website_path);
    if !root.is_dir() {
        return Err(AppError::Website(format!(
            "网站项目路径不存在: {}",
            website_path
        )));
    }

    // 获取当前日期
    let date = chrono::Local::now().format("%Y-%m-%d").to_string();

    // 提取摘要（前 150 字符）
    let summary = extract_summary(markdown_body, 150);
    // 转义摘要和标题中的双引号，防止 YAML frontmatter 解析错误
    let escaped_summary = summary.replace('"', "\\\"");
    let escaped_title = title.replace('"', "\\\"");

    // 构造 MDX 内容
    let mdx_content = format!(
        "---\ntitle: \"{}\"\ndate: \"{}\"\nsummary: \"{}\"\n---\n\n{}",
        escaped_title, date, escaped_summary, markdown_body
    );

    // 确保目录存在
    let content_dir = root.join("content").join(taxonomy);
    std::fs::create_dir_all(&content_dir).map_err(|e| {
        AppError::Website(format!(
            "无法创建目录 {}: {}",
            content_dir.display(),
            e
        ))
    })?;

    // 写入文件
    let file_path = content_dir.join(format!("{}.mdx", slug));
    std::fs::write(&file_path, &mdx_content).map_err(|e| {
        AppError::Website(format!(
            "无法写入文件 {}: {}",
            file_path.display(),
            e
        ))
    })?;

    Ok(file_path.display().to_string())
}

// ── publish_to_website ────────────────────────────────────────────────

/// 通过 git commit + push 发布网站
///
/// # Arguments
/// * `website_path` - 网站项目根路径
/// * `slug` - 要发布的文章 slug（不带路径和扩展名）
///
/// # Returns
/// 成功时返回网站 URL（如 "https://khlilo.xyz/signal/{slug}"）
pub fn publish_to_website(
    website_path: &str,
    slug: &str,
) -> Result<String, AppError> {
    // 检查网站路径是否存在
    let root = Path::new(website_path);
    if !root.is_dir() {
        return Err(AppError::Website(format!(
            "网站项目路径不存在: {}",
            website_path
        )));
    }

    let file_path = format!("content/signal/{}.mdx", slug);

    // 1. git add
    run_git(website_path, &["add", &file_path])?;

    // 2. git commit（"nothing to commit" 不算错误）
    let commit_output = run_git_raw(website_path, &["commit", "-m", &format!("post: {}", slug)]);
    match commit_output {
        Ok(_) => {}
        Err(e) => {
            // 区分 "nothing to commit" 和其他 git 错误
            let err_str = e.to_string();
            if err_str.contains("nothing to commit") {
                // 没有变更需要提交，继续执行 push（no-op）
            } else {
                return Err(AppError::Git(err_str));
            }
        }
    }

    // 3. git push
    run_git(website_path, &["push", "origin", "main"])?;

    Ok(format!("https://khlilo.xyz/signal/{}", slug))
}

// ── Git helpers ───────────────────────────────────────────────────────

/// 执行 git 命令，成功返回 stdout，失败返回 AppError::Git
fn run_git(working_dir: &str, args: &[&str]) -> Result<String, AppError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(working_dir)
        .args(args)
        .output()
        .map_err(|e| AppError::Git(format!("无法执行 git: {}", e)))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(AppError::Git(format!(
            "git {} 失败: {}",
            args.join(" "),
            stderr
        )))
    }
}

/// 执行 git 命令，返回原始错误信息（用于区分 "nothing to commit"）
fn run_git_raw(
    working_dir: &str,
    args: &[&str],
) -> Result<String, AppError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(working_dir)
        .args(args)
        .output()
        .map_err(|e| AppError::Git(format!("无法执行 git: {}", e)))?;

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();

    if output.status.success() {
        Ok(stdout)
    } else {
        // 合并 stdout 和 stderr，便于上层区分 "nothing to commit"
        let combined = if stdout.is_empty() {
            stderr
        } else if stderr.is_empty() {
            stdout
        } else {
            format!("{}\n{}", stdout, stderr)
        };
        Err(AppError::Git(combined))
    }
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── extract_summary ──────────────────────────────────────────

    #[test]
    fn test_extract_summary_skips_headings() {
        let input = "# Title\n\nContent here";
        assert_eq!(extract_summary(input, 150), "Content here");
    }

    #[test]
    fn test_extract_summary_truncates() {
        let input = "A".repeat(200);
        let result = extract_summary(&input, 100);
        assert!(result.ends_with("..."));
        // 对于全 A 无空格的文本，直接截断到 100 字符 + ...
        assert_eq!(result.len(), 103); // 100 chars + "..."
    }

    #[test]
    fn test_extract_summary_truncates_at_word_boundary() {
        // "Hello world t" = 12 chars, 在 "world" 后的空格处截断
        let input = "Hello world this is a test sentence for word boundary truncation";
        let result = extract_summary(input, 12);
        assert_eq!(result, "Hello world...");
    }

    #[test]
    fn test_extract_summary_empty() {
        assert_eq!(extract_summary("", 150), "");
    }

    #[test]
    fn test_extract_summary_no_headings_empty_lines() {
        // 只有标题和空行，没有正文内容
        let input = "# Title\n## Subtitle\n\n\n";
        assert_eq!(extract_summary(input, 150), "");
    }

    #[test]
    fn test_extract_summary_no_truncation_needed() {
        let input = "Short paragraph.";
        assert_eq!(extract_summary(input, 150), "Short paragraph.");
    }

    // ── write_mdx_draft ──────────────────────────────────────────

    #[test]
    fn test_write_mdx_draft_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let website_path = dir.path().to_str().unwrap();

        let result = write_mdx_draft(
            website_path,
            "signal",
            "test-post",
            "测试文章",
            "这是文章正文的第一段。这是更多的内容。",
        );
        assert!(result.is_ok());
        let file_path = result.unwrap();
        assert!(file_path.ends_with("test-post.mdx"));

        // 验证文件存在
        let mdx_path = std::path::Path::new(&file_path);
        assert!(mdx_path.exists());

        // 读取内容并验证
        let content = std::fs::read_to_string(&file_path).unwrap();
        assert!(content.contains("title: \"测试文章\""));
        assert!(content.contains("summary:"));
        assert!(content.contains("---"));
        assert!(content.contains("这是文章正文的第一段"));
    }

    #[test]
    fn test_write_mdx_draft_content_has_frontmatter() {
        let dir = tempfile::tempdir().unwrap();
        let website_path = dir.path().to_str().unwrap();

        let result = write_mdx_draft(
            website_path,
            "node",
            "my-article",
            "My Title",
            "Some body content here.",
        );
        assert!(result.is_ok());
        let file_path = result.unwrap();
        let content = std::fs::read_to_string(&file_path).unwrap();

        // 验证 frontmatter 字段存在
        assert!(content.contains("---\ntitle:"));
        assert!(content.contains("date:"));
        assert!(content.contains("summary:"));

        // frontmatter 应以第一个 --- 开头
        assert!(content.starts_with("---\n"));
    }

    #[test]
    fn test_write_mdx_draft_creates_taxonomy_dir() {
        let dir = tempfile::tempdir().unwrap();
        let website_path = dir.path().to_str().unwrap();

        // taxonomy 目录尚不存在
        let taxonomy_dir = std::path::Path::new(website_path)
            .join("content")
            .join("pow");
        assert!(!taxonomy_dir.exists());

        let result = write_mdx_draft(
            website_path,
            "pow",
            "pow-post",
            "POW 标题",
            "# Heading\n\n这是正文内容。",
        );
        assert!(result.is_ok());

        // 验证目录被创建
        assert!(taxonomy_dir.exists());
        assert!(taxonomy_dir.is_dir());

        // 验证文件存在
        let mdx_file = taxonomy_dir.join("pow-post.mdx");
        assert!(mdx_file.exists());
    }

    #[test]
    fn test_write_mdx_draft_nonexistent_website_path() {
        let result = write_mdx_draft(
            "/nonexistent/path/for/testing/12345",
            "signal",
            "slug",
            "Title",
            "Body",
        );
        assert!(result.is_err());
        let err = result.unwrap_err();
        let err_msg = err.to_string();
        assert!(err_msg.contains("网站项目路径不存在"));
    }

    #[test]
    fn test_write_mdx_draft_escapes_quotes_in_title() {
        let dir = tempfile::tempdir().unwrap();
        let website_path = dir.path().to_str().unwrap();

        let result = write_mdx_draft(
            website_path,
            "signal",
            "quote-test",
            "Title with \"double quotes\"",
            "Body content.",
        );
        assert!(result.is_ok());
        let file_path = result.unwrap();
        let content = std::fs::read_to_string(&file_path).unwrap();

        // 双引号应该被转义
        assert!(content.contains("title: \"Title with \\\"double quotes\\\"\""));
    }

    // ── publish_to_website ───────────────────────────────────────

    #[test]
    fn test_publish_to_website_nonexistent_path() {
        let result = publish_to_website("/nonexistent/path/12345", "test-slug");
        assert!(result.is_err());
        let err = result.unwrap_err();
        let err_msg = err.to_string();
        assert!(err_msg.contains("网站项目路径不存在"));
    }
}
