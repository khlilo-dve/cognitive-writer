//! 旧关键词解析的提取规则，仅编译于测试，保留兼容性回归覆盖。

#[cfg(test)]
#[allow(dead_code)]
/// 从输入中提取首个 URL（https?:// 开头的 token）。
///
/// 先尝试以空白分隔的 token，再尝试嵌入式 URL（无前置空白）。
fn extract_url(input: &str) -> Option<String> {
    // 优先：空白分隔的独立 token
    for token in input.split_whitespace() {
        if token.starts_with("https://") || token.starts_with("http://") {
            return Some(token.to_string());
        }
    }

    // 回退：嵌入式 URL（如"学一下https://example.com"）
    for proto in &["https://", "http://"] {
        if let Some(pos) = input.find(proto) {
            let rest = &input[pos..];
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            return Some(rest[..end].to_string());
        }
    }

    None
}

#[cfg(test)]
#[allow(dead_code)]
/// 尝试提取"写一篇/选题 + 用 + 风格"模式，返回 `(topic, style_name)`。
///
/// 匹配规则：
/// - 包含"写一篇"或"选题"
/// - 包含"用"
/// - 包含"风格"
/// - topic 在关键词和"用"之间，style_name 在"用"和"风格"之间
///
/// 使用两阶段匹配以避免多段落素材中的误匹配：
/// 1. 仅在第一段（指令行）中尝试匹配
/// 2. 回退到全文匹配（保底）
fn extract_generate(input: &str) -> Option<(String, String)> {
    // 阶段 1：仅匹配第一段（指令行）
    let first_para = input.split("\n\n").next().unwrap_or(input);
    if let Some(result) = try_extract(first_para) {
        return Some(result);
    }

    // 阶段 2：回退到全文匹配（保底）
    try_extract(input)
}

#[cfg(test)]
#[allow(dead_code)]
/// 尝试从给定文本中提取 generate 的 (topic, style_name)。
///
/// 使用 find（首次匹配）而非 rfind（末次匹配）定位"风格"，
/// 因为指令行中风格关键词只出现一次。
fn try_extract(text: &str) -> Option<(String, String)> {
    let keyword = if text.contains("写一篇") {
        "写一篇"
    } else if text.contains("选题") {
        "选题"
    } else {
        return None;
    };

    if !text.contains('用') || !text.contains("风格") {
        return None;
    }

    let kw_pos = text.find(keyword)?;
    let after_kw = &text[kw_pos + keyword.len()..];

    // find（首次匹配）：指令行中风格关键词只出现一次
    let fengge_pos = after_kw.find("风格")?;
    let before_fengge = &after_kw[..fengge_pos];

    // rfind 保留：topic 中可能含"用"字（如"如何用AI写作"）
    let yong_pos = before_fengge.rfind('用')?;

    let topic = after_kw[..yong_pos].trim().to_string();
    let style_name = after_kw[yong_pos + "用".len()..fengge_pos]
        .trim()
        .to_string();

    if topic.is_empty() || style_name.is_empty() {
        return None;
    }

    Some((topic, style_name))
}

#[cfg(test)]
#[allow(dead_code)]
/// 尝试提取"重写 path.md 改成 instruction"模式，返回 `(path, instruction)`。
fn extract_update(input: &str) -> Option<(String, String)> {
    if !input.contains("重写") || !input.contains(".md") || !input.contains("改成") {
        return None;
    }

    let path = extract_md_path(input)?;
    let gaicheng_pos = input.find("改成")?;
    let instruction = input[gaicheng_pos + "改成".len()..].trim().to_string();

    if instruction.is_empty() {
        return None;
    }

    Some((path, instruction))
}

#[cfg(test)]
#[allow(dead_code)]
/// 尝试提取"path.md 改"模式（非"重写...改成"路径），返回 `(path, instruction)`。
fn extract_refine(input: &str) -> Option<(String, String)> {
    if !input.contains(".md") || !input.contains('改') {
        return None;
    }

    // 排除已由 extract_update 处理的模式
    if input.contains("重写") && input.contains("改成") {
        return None;
    }

    let path = extract_md_path(input)?;
    let path_pos = input.find(&path)?;
    let after_path = input[path_pos + path.len()..].trim();

    // 去除前缀"改"后剩余为修改指令
    let instruction = after_path.trim_start_matches('改').trim().to_string();

    if instruction.is_empty() {
        return None;
    }

    Some((path, instruction))
}

#[cfg(test)]
#[allow(dead_code)]
/// 从输入中提取 .md 文件路径（空白分隔的 token）。
fn extract_md_path(input: &str) -> Option<String> {
    for token in input.split_whitespace() {
        if token.contains(".md") {
            return Some(token.to_string());
        }
    }
    None
}

// ── Layer 2: 状态相关的意图匹配 ─────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_url_standalone_https() {
        let url = extract_url("https://example.com/article").unwrap();
        assert_eq!(url, "https://example.com/article");
    }

    #[test]
    fn test_extract_url_standalone_http() {
        let url = extract_url("http://example.com").unwrap();
        assert_eq!(url, "http://example.com");
    }

    #[test]
    fn test_extract_url_embedded_no_space() {
        let url = extract_url("学一下https://example.com的风格").unwrap();
        assert_eq!(url, "https://example.com的风格");
    }

    #[test]
    fn test_extract_url_in_middle_of_text() {
        let url = extract_url("看看 https://example.com 这篇文章").unwrap();
        assert_eq!(url, "https://example.com");
    }

    #[test]
    fn test_extract_url_no_url() {
        assert!(extract_url("写一篇文章").is_none());
    }

    #[test]
    fn test_extract_generate_basic() {
        let (topic, style) = extract_generate("写一篇关于AI的文章用鲁迅风格").unwrap();
        assert_eq!(topic, "关于AI的文章");
        assert_eq!(style, "鲁迅");
    }

    #[test]
    fn test_extract_generate_xuanti() {
        let (topic, style) = extract_generate("选题关于Rust用张三风格").unwrap();
        assert_eq!(topic, "关于Rust");
        assert_eq!(style, "张三");
    }

    #[test]
    fn test_extract_generate_no_match() {
        assert!(extract_generate("写一篇文章").is_none());
        assert!(extract_generate("用鲁迅风格写一篇").is_none());
    }

    #[test]
    fn test_extract_generate_with_multi_paragraph_material() {
        // 多段落素材不应污染指令行的 topic/style 提取
        let input = "写一篇关于AI Agent的文章，用轻辩风格\n\n以下是参考素材：\n第一段：某个用强化学习训练Agent的实验表明...\n第二段：关于写作风格的讨论可以参考...";
        let (topic, style) = extract_generate(input).unwrap();
        assert_eq!(topic, "关于AI Agent的文章，");
        assert_eq!(style, "轻辩");
    }

    #[test]
    fn test_extract_generate_with_material_containing_style_keyword() {
        // 素材中包含“风格”字眼，不应误导解析
        let input = "写一篇关于编程的文章用张三风格\\n\\n参考：你的写作风格应该注意...";
        let (topic, style) = extract_generate(input).unwrap();
        assert_eq!(topic, "关于编程的文章");
        assert_eq!(style, "张三");
    }

    #[test]
    fn test_extract_generate_single_line_still_works() {
        // 单行输入回归测试
        let (topic, style) = extract_generate("写一篇关于AI的文章用鲁迅风格").unwrap();
        assert_eq!(topic, "关于AI的文章");
        assert_eq!(style, "鲁迅");
        // 选题模式单行回归
        let (topic, style) = extract_generate("选题关于Rust用张三风格").unwrap();
        assert_eq!(topic, "关于Rust");
        assert_eq!(style, "张三");
    }

    #[test]
    fn test_extract_update() {
        let (path, inst) = extract_update("重写 outputs/test_v1.md 改成更正式的语气").unwrap();
        assert_eq!(path, "outputs/test_v1.md");
        assert_eq!(inst, "更正式的语气");
    }

    #[test]
    fn test_extract_update_no_match() {
        assert!(extract_update("修改 outputs/test_v1.md 的语气").is_none());
    }

    #[test]
    fn test_extract_refine() {
        let (path, inst) = extract_refine("outputs/test_v1.md 改更简洁").unwrap();
        assert_eq!(path, "outputs/test_v1.md");
        assert_eq!(inst, "更简洁");
    }

    #[test]
    fn test_extract_refine_excludes_update() {
        // 同时满足 update 和 refine 时，refine 应返回 None
        assert!(extract_refine("重写 outputs/test_v1.md 改成更正式").is_none());
    }
}
