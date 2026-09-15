//! 旧风格操作和审阅状态匹配规则，仅编译于测试。

use crate::intent::Intent;

#[cfg(test)]
#[allow(dead_code)]
/// 判断输入是否包含"学习风格"语义关键词。
fn has_learn_keywords(input: &str) -> bool {
    input.contains("学一下")
        || (input.contains("分析") && input.contains("文风"))
        || (input.contains("逆向") && input.contains("文风"))
}

#[cfg(test)]
#[allow(dead_code)]
/// 判断输入是否匹配"列出风格"模式。
fn matches_list_styles(input: &str) -> bool {
    (input.contains("风格库") && input.contains("有什么"))
        || (input.contains("有哪些") && input.contains("风格"))
        || (input.contains("列出") && input.contains("风格"))
}

#[cfg(test)]
#[allow(dead_code)]
/// 判断输入是否匹配"查看风格"模式。
fn matches_show_style(input: &str) -> bool {
    (input.contains("看看") && input.contains("风格"))
        || (input.contains("风格") && input.contains("详情"))
        || (input.contains("风格") && input.contains("摘要"))
}

#[cfg(test)]
#[allow(dead_code)]
/// 判断输入是否匹配"删除风格"模式。
fn matches_delete_style(input: &str) -> bool {
    (input.contains("删掉") && input.contains("风格"))
        || (input.contains("删除") && input.contains("风格"))
}

// ── Layer 1 提取辅助 ────────────────────────────────────────────────

#[cfg(test)]
#[allow(dead_code)]
/// 从输入中提取风格名称。
///
/// 按以下顺序尝试：
/// 1. "改风格XXX" → 风格名在"改风格"之后
/// 2. 已知动词前缀 + 风格名 + "风格"（如"看看鲁迅风格"→"鲁迅"）
/// 3. 回退：提取"风格"之前的全部文本
fn extract_style_name(input: &str) -> Option<String> {
    // 1. "改风格XXX" 模式 — 风格名在"改风格"之后
    if let Some(pos) = input.find("改风格") {
        let name = input[pos + "改风格".len()..].trim().to_string();
        if !name.is_empty() {
            return Some(name);
        }
    }

    // 2. 已知前缀 + 风格名 + "风格"
    let fengge_pos = input.find("风格")?;
    let before = &input[..fengge_pos];

    let prefixes = ["看看", "删掉", "删除", "整体", "换成"];
    for prefix in &prefixes {
        if let Some(pos) = before.rfind(prefix) {
            let name = before[pos + prefix.len()..].trim().to_string();
            if !name.is_empty() {
                return Some(name);
            }
        }
    }

    // 3. 回退：提取"风格"之前的全部文本（去空白）
    let before_trimmed = before.trim();
    if !before_trimmed.is_empty() {
        return Some(before_trimmed.to_string());
    }

    None
}

#[cfg(test)]
#[allow(dead_code)]
/// 在 WaitingForPublish 状态下匹配发布相关意图。
fn match_publish_intent(input: &str) -> Intent {
    if input.contains("只发网站") || input.contains("只部署网站") || input.contains("网站发布")
    {
        return Intent::PublishWebsiteOnly;
    }
    if input.contains("发布")
        || input.contains("推")
        || input.contains("上线")
        || input.contains("发")
    {
        return Intent::Publish;
    }
    if input.contains("等一下")
        || input.contains("再看看")
        || input.contains("先不")
        || input.contains("等等")
        || input.contains("hold")
    {
        return Intent::Hold;
    }
    Intent::Unknown {
        reason: "无法理解意图".to_string(),
    }
}

#[cfg(test)]
#[allow(dead_code)]
/// 在 WaitingForFulltext 状态下匹配正文修改相关意图。
fn match_fulltext_intent(input: &str) -> Intent {
    // 更换风格
    if ((input.contains("整体") && input.contains("风格"))
        || (input.contains("换成") && input.contains("风格"))
        || input.contains("改风格"))
        && let Some(style_name) = extract_style_name(input)
    {
        return Intent::ChangeStyle { style_name };
    }

    // 其余输入视为段落修改指令
    Intent::ModifySection {
        instruction: input.to_string(),
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_style_name_kankan() {
        assert_eq!(extract_style_name("看看鲁迅风格").unwrap(), "鲁迅");
    }

    #[test]
    fn test_extract_style_name_delete() {
        assert_eq!(extract_style_name("删掉测试风格").unwrap(), "测试");
    }

    #[test]
    fn test_extract_style_name_gaifengge() {
        assert_eq!(extract_style_name("改风格鲁迅").unwrap(), "鲁迅");
    }
}
