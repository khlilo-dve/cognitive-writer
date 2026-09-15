//! 首次生成的风格候选与本地选择；不把描述性命中当作用户确认。
use crate::error::AppError;
use crate::style_names::{extract_description, match_key};

pub(crate) struct StyleOption {
    pub name: String,
    pub description: String,
    preview: String,
}

pub(crate) fn read_options(dir: &str) -> Result<Vec<StyleOption>, AppError> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(AppError::FileRead(format!("无法读取风格目录 {dir}: {e}"))),
    };
    let mut options = Vec::new();
    for entry in entries {
        let entry =
            entry.map_err(|e| AppError::FileRead(format!("无法读取风格目录 {dir}: {e}")))?;
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "md") {
            let content = std::fs::read_to_string(&path)
                .map_err(|e| AppError::FileRead(format!("无法读取 {}: {e}", path.display())))?;
            let name = path.file_stem().and_then(|s| s.to_str()).ok_or_else(|| {
                AppError::FileRead(format!("风格文件名不是 UTF-8: {}", path.display()))
            })?;
            options.push(StyleOption {
                name: name.into(),
                description: extract_description(&content),
                preview: content.chars().take(200).collect(),
            });
        }
    }
    options.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(options)
}

/// 精确名称优先；拼音归一化也必须返回全部候选，不能取第一个。
pub(crate) fn named_matches(options: &[StyleOption], query: &str) -> Vec<String> {
    let query = query.trim();
    if query.is_empty() {
        return Vec::new();
    }
    if let Some(option) = options.iter().find(|o| o.name == query) {
        return vec![option.name.clone()];
    }
    let key = match_key(query);
    options
        .iter()
        .filter(|o| match_key(&o.name) == key)
        .map(|o| o.name.clone())
        .collect()
}

pub(crate) fn suggestions(options: &[StyleOption], query: &str) -> Vec<String> {
    let named = named_matches(options, query);
    if !named.is_empty() {
        return named;
    }
    if query.trim().is_empty() {
        return Vec::new();
    }
    options
        .iter()
        .filter(|o| o.preview.contains(query.trim()))
        .map(|o| o.name.clone())
        .collect()
}

pub(crate) fn selection_query(input: &str) -> &str {
    let input = input.trim().trim_end_matches(['。', '！', '!', '.', '吧']);
    let stripped = ["我想用", "我选择", "我选", "选择", "请用", "就用", "用"]
        .iter()
        .find_map(|prefix| input.strip_prefix(prefix))
        .unwrap_or(input)
        .trim();
    stripped.strip_suffix("风格").unwrap_or(stripped).trim()
}

pub(crate) fn is_list_request(input: &str) -> bool {
    matches!(
        input.trim(),
        "/styles"
            | "有哪些风格"
            | "有什么风格"
            | "列出风格"
            | "风格列表"
            | "风格库里有什么"
            | "我的风格库有什么"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_preserves_ambiguity_and_does_not_choose_from_description() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["轻辩", "清变", "另一个"] {
            std::fs::write(
                dir.path().join(format!("{name}.md")),
                "# 规则\n犀利，短句。\n",
            )
            .unwrap();
        }
        let options = read_options(dir.path().to_str().unwrap()).unwrap();
        assert_eq!(named_matches(&options, "轻辩"), vec!["轻辩"]);
        assert_eq!(named_matches(&options, "qingbian").len(), 2);
        assert!(named_matches(&options, "犀利").is_empty());
        assert_eq!(suggestions(&options, "犀利").len(), 3);
        assert!(suggestions(&options, "").is_empty());
        assert!(named_matches(&options, "../轻辩").is_empty());
    }

    #[test]
    fn empty_library_is_distinct_from_read_failure() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("styles");
        assert!(read_options(path.to_str().unwrap()).unwrap().is_empty());
        std::fs::write(&path, "blocked").unwrap();
        assert!(matches!(
            read_options(path.to_str().unwrap()),
            Err(AppError::FileRead(_))
        ));
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("broken.md"), [0xff]).unwrap();
        assert!(matches!(
            read_options(path.to_str().unwrap()),
            Err(AppError::FileRead(_))
        ));
    }

    #[test]
    fn local_selection_accepts_name_and_short_phrases() {
        for input in [
            "轻辩",
            "用轻辩",
            "我选轻辩风格",
            "请用轻辩风格。",
            "就用轻辩吧",
        ] {
            assert_eq!(selection_query(input), "轻辩");
        }
        assert_eq!(selection_query("2"), "2");
    }
}
