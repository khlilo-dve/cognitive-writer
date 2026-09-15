//! 无网络调用的确认、取消与空输入预检。

use crate::intent::{Intent, SessionState};

/// 解析用户输入，返回结构化 Intent（快速预检模式）。
///
/// 本函数仅做 0 成本快速预检
///（confirm/cancel/empty），其余情况返回 Unknown 由调用方走 LLM 分类。
/// 旧关键词提取函数仅保留在测试模块中，生产路径不提供复杂意图离线降级。
pub fn parse_intent(input: &str, _state: &SessionState) -> Intent {
    let input = input.trim();
    if input.is_empty() {
        return Intent::Unknown {
            reason: "输入为空".to_string(),
        };
    }

    // ── 0 成本快速预检：确认 / 取消 ──
    if is_confirm(input) {
        return Intent::Confirm;
    }
    if is_cancel(input) {
        return Intent::Cancel;
    }

    // ── 其余返回 Unknown，由调用方走 LLM 分类 ──
    Intent::Unknown {
        reason: "无法理解意图".to_string(),
    }
}

/// 判断输入是否为确认语义（大小写不敏感）。
pub fn is_confirm(input: &str) -> bool {
    let s = input.trim();
    const CONFIRM_ZH: &[&str] = &[
        "确认",
        "没问题",
        "继续",
        "就这样",
        "好",
        "可以",
        "行",
        "对",
        "是的",
        "嗯",
    ];
    if CONFIRM_ZH.contains(&s) {
        return true;
    }
    let lower = s.to_lowercase();
    const CONFIRM_EN: &[&str] = &["ok", "yes", "y"];
    CONFIRM_EN.contains(&lower.as_str())
}

/// 判断输入是否为取消语义（大小写不敏感）。
pub fn is_cancel(input: &str) -> bool {
    let s = input.trim();
    const CANCEL_ZH: &[&str] = &["算了", "取消", "不写了", "不要了", "换一个", "放弃"];
    if CANCEL_ZH.contains(&s) {
        return true;
    }
    let lower = s.to_lowercase();
    const CANCEL_EN: &[&str] = &["cancel", "no", "n"];
    CANCEL_EN.contains(&lower.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_confirm_chinese() {
        assert!(is_confirm("确认"));
        assert!(is_confirm("好"));
        assert!(is_confirm("可以"));
        assert!(is_confirm("没问题"));
    }

    #[test]
    fn test_confirm_english_case_insensitive() {
        assert!(is_confirm("OK"));
        assert!(is_confirm("ok"));
        assert!(is_confirm("Yes"));
        assert!(is_confirm("y"));
    }

    #[test]
    fn test_not_confirm() {
        assert!(!is_confirm("不好"));
        assert!(!is_confirm("maybe"));
        assert!(!is_confirm(""));
    }

    #[test]
    fn test_cancel_chinese() {
        assert!(is_cancel("算了"));
        assert!(is_cancel("取消"));
        assert!(is_cancel("不写了"));
    }

    #[test]
    fn test_cancel_english() {
        assert!(is_cancel("cancel"));
        assert!(is_cancel("CANCEL"));
        assert!(is_cancel("no"));
    }

    #[test]
    fn test_not_cancel() {
        assert!(!is_cancel("继续"));
        assert!(!is_cancel("yes"));
        assert!(!is_cancel(""));
    }

    #[test]
    fn test_parse_url_is_learn() {
        // v3.1: parse_intent 已裁剪，URL 检测由 LLM 分类处理
        let result = parse_intent("https://example.com", &SessionState::Idle);
        assert!(matches!(result, Intent::Unknown { .. }));
    }

    #[test]
    fn test_parse_list_styles() {
        // v3.1: parse_intent 已裁剪，风格查询由 LLM 分类处理
        let result = parse_intent("风格库里有什么", &SessionState::Idle);
        assert!(matches!(result, Intent::Unknown { .. }));
    }

    #[test]
    fn test_parse_show_style() {
        // v3.1: parse_intent 已裁剪，风格详情由 LLM 分类处理
        let result = parse_intent("看看鲁迅风格", &SessionState::Idle);
        assert!(matches!(result, Intent::Unknown { .. }));
    }

    #[test]
    fn test_parse_delete_style() {
        // v3.1: parse_intent 已裁剪，删除风格由 LLM 分类处理
        let result = parse_intent("删掉测试风格", &SessionState::Idle);
        assert!(matches!(result, Intent::Unknown { .. }));
    }

    #[test]
    fn test_parse_generate() {
        // v3.1: parse_intent 已裁剪，生成由 LLM 分类处理
        let result = parse_intent("写一篇关于AI的文章用鲁迅风格", &SessionState::Idle);
        assert!(matches!(result, Intent::Unknown { .. }));
    }

    #[test]
    fn test_parse_update() {
        // v3.1: parse_intent 已裁剪，重写由 LLM 分类处理
        let result = parse_intent("重写 outputs/test.md 改成更正式", &SessionState::Idle);
        assert!(matches!(result, Intent::Unknown { .. }));
    }

    #[test]
    fn test_parse_refine() {
        // v3.1: parse_intent 已裁剪，局部修改由 LLM 分类处理
        let result = parse_intent("outputs/test.md 改一下第二段", &SessionState::Idle);
        assert!(matches!(result, Intent::Unknown { .. }));
    }

    #[test]
    fn test_idle_unknown_on_unmatched() {
        let result = parse_intent("随便说点什么", &SessionState::Idle);
        assert!(matches!(result, Intent::Unknown { .. }));
    }

    #[test]
    fn test_confirm_in_waiting_state() {
        let result = parse_intent("ok", &SessionState::WaitingForOutline);
        assert!(matches!(result, Intent::Confirm));
    }

    #[test]
    fn test_cancel_in_waiting_state() {
        let result = parse_intent("算了", &SessionState::WaitingForFulltext);
        assert!(matches!(result, Intent::Cancel));
    }

    #[test]
    fn test_waiting_for_publish_website_only() {
        // v3.1: parse_intent 已裁剪，发布选项由 LLM 分类处理
        let result = parse_intent("只发网站", &SessionState::WaitingForPublish);
        assert!(matches!(result, Intent::Unknown { .. }));
    }

    #[test]
    fn test_waiting_for_publish_publish() {
        // v3.1: parse_intent 已裁剪，发布由 LLM 分类处理
        let result = parse_intent("发布吧", &SessionState::WaitingForPublish);
        assert!(matches!(result, Intent::Unknown { .. }));
    }

    #[test]
    fn test_waiting_for_publish_hold() {
        // v3.1: parse_intent 已裁剪，暂缓由 LLM 分类处理
        let result = parse_intent("等一下", &SessionState::WaitingForPublish);
        assert!(matches!(result, Intent::Unknown { .. }));
    }

    #[test]
    fn test_waiting_for_outline_modify() {
        // v3.1: parse_intent 已裁剪，大纲修改由 LLM 分类处理
        let result = parse_intent("第二部分太长了", &SessionState::WaitingForOutline);
        assert!(matches!(result, Intent::Unknown { .. }));
    }

    #[test]
    fn test_waiting_for_fulltext_change_style() {
        // v3.1: parse_intent 已裁剪，换风格由 LLM 分类处理
        let result = parse_intent("换成鲁迅风格", &SessionState::WaitingForFulltext);
        assert!(matches!(result, Intent::Unknown { .. }));
    }

    #[test]
    fn test_waiting_for_fulltext_modify_section() {
        // v3.1: parse_intent 已裁剪，段落修改由 LLM 分类处理
        let result = parse_intent("第三段需要更多案例", &SessionState::WaitingForFulltext);
        assert!(matches!(result, Intent::Unknown { .. }));
    }

    #[test]
    fn test_empty_input_is_unknown() {
        let result = parse_intent("   ", &SessionState::WaitingForOutline);
        assert!(matches!(result, Intent::Unknown { .. }));
    }

    #[test]
    fn test_url_priority_over_generate() {
        // v3.1: parse_intent 已裁剪，URL 优先检测由 LLM 分类处理
        let result = parse_intent("写一篇用鲁迅风格 https://example.com", &SessionState::Idle);
        assert!(matches!(result, Intent::Unknown { .. }));
    }

    #[test]
    fn test_show_style_before_delete() {
        // v3.1: parse_intent 已裁剪，风格查询由 LLM 分类处理
        let result = parse_intent("删掉看看的风格", &SessionState::Idle);
        assert!(matches!(result, Intent::Unknown { .. }));
    }

    #[test]
    fn test_update_before_refine() {
        // v3.1: parse_intent 已裁剪，重写由 LLM 分类处理
        let result = parse_intent("重写 a.md 改成更正式", &SessionState::Idle);
        assert!(matches!(result, Intent::Unknown { .. }));
    }

    #[test]
    fn test_parse_intent_returns_unknown_for_complex_input() {
        // v3.1: 复杂自然语言输入在关键词 parser 下返回 Unknown
        // （实际上由 LLM 分类处理，parse_intent 只是 fallback）
        let result = parse_intent(
            "帮我整一篇关于认知偏差的，要那种犀利点的感觉",
            &SessionState::Idle,
        );
        assert!(matches!(result, Intent::Unknown { .. }));
    }
}
