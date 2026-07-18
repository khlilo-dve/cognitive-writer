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
