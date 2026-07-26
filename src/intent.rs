//! 意图解析器 — 将自然语言输入分类为结构化 Intent。
//!
//! 纯字符串匹配实现，不依赖 NLP 库。支持多轮对话状态机，
//! 两层匹配结构：第一层状态无关高置信度模式，第二层按状态分流。

// ── SessionState ─────────────────────────────────────────────────────

/// 多轮对话的会话状态。
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub enum SessionState {
    /// 空闲状态，等待用户发起新操作。
    Idle,
    /// 等待用户对大纲骨架的反馈。
    WaitingForOutline,
    /// 等待用户对正文的反馈。
    WaitingForFulltext,
    /// 等待用户确认发布。
    WaitingForPublish,
}

// ── Intent ───────────────────────────────────────────────────────────

/// 从用户输入解析出的结构化意图。
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum Intent {
    /// 生成文章：指定主题 + 风格名称。
    Generate { topic: String, style_name: String },
    /// 从 URL 学习写作风格。
    Learn { url: String },
    /// 列出所有可用风格。
    ListStyles,
    /// 查看指定风格的详情。
    ShowStyle { name: String },
    /// 删除指定风格。
    DeleteStyle { name: String },
    /// 局部重绘：修改 .md 文件中的特定段落。
    RefineFile { path: String, instruction: String },
    /// 整体重写：根据指令重写 .md 文件。
    UpdateFile { path: String, instruction: String },
    /// 确认当前待处理操作。
    Confirm,
    /// 取消当前待处理操作。
    Cancel,
    /// 修改大纲：携带修改指令。
    ModifyOutline { instruction: String },
    /// 修改特定段落：携带修改指令。
    ModifySection { instruction: String },
    /// 更换写作风格。
    ChangeStyle { style_name: String },
    /// 发布文章（公众号 + 网站）。
    Publish,
    /// 仅发布到网站。
    PublishWebsiteOnly,
    /// 暂缓/搁置当前操作。
    Hold,
    /// 无法识别的意图。
    Unknown,
}

// ── Public API ───────────────────────────────────────────────────────

/// 解析用户输入，返回结构化 Intent。
///
/// `state` 参数用于状态相关的匹配逻辑（确认/取消/特定状态的指令）。
/// 匹配分两层：
#[allow(dead_code)]
/// 1. 状态无关的高置信度模式（URL、生成、风格管理等）
/// 2. 状态相关匹配（确认、取消、特定状态的指令）
pub fn parse_intent(input: &str, state: &SessionState) -> Intent {
    let input = input.trim();
    if input.is_empty() {
        return Intent::Unknown;
    }

    // ═══════════════════════════════════════════════════════════════
    // Layer 1: 状态无关的高置信度模式（按优先级 1→8）
    // ═══════════════════════════════════════════════════════════════

    // 1. URL 检测 — 任意包含 https?:// 的输入视为 Learn
    if let Some(url) = extract_url(input) {
        return Intent::Learn { url };
    }

    // 2. 学习关键词 + URL（补充匹配：学一下/分析文风/逆向文风）
    if has_learn_keywords(input) && (input.contains("http://") || input.contains("https://")) {
        if let Some(url) = extract_url(input) {
            return Intent::Learn { url };
        }
    }

    // 3. ListStyles — 列出风格
    if matches_list_styles(input) {
        return Intent::ListStyles;
    }

    // 4. ShowStyle — 查看风格详情
    if matches_show_style(input) {
        if let Some(name) = extract_style_name(input) {
            return Intent::ShowStyle { name };
        }
    }

    // 5. DeleteStyle — 删除风格
    if matches_delete_style(input) {
        if let Some(name) = extract_style_name(input) {
            return Intent::DeleteStyle { name };
        }
    }

    // 6. Generate — 写一篇/选题 + 风格
    if let Some((topic, style_name)) = extract_generate(input) {
        return Intent::Generate { topic, style_name };
    }

    // 7. UpdateFile — 重写 + .md + 改成
    if let Some((path, instruction)) = extract_update(input) {
        return Intent::UpdateFile { path, instruction };
    }

    // 8. RefineFile — .md + 改（不含重写关键词）
    if let Some((path, instruction)) = extract_refine(input) {
        return Intent::RefineFile { path, instruction };
    }

    // ═══════════════════════════════════════════════════════════════
    // Layer 2: 状态相关匹配
    // ═══════════════════════════════════════════════════════════════

    // Idle 状态下未匹配到任何 Layer 1 模式 → Unknown
    if *state == SessionState::Idle {
        return Intent::Unknown;
    }

    // 确认/取消 — 所有非 Idle 状态下通用
    if is_confirm(input) {
        return Intent::Confirm;
    }

    if is_cancel(input) {
        return Intent::Cancel;
    }

    // 按具体状态分流
    match state {
        SessionState::WaitingForPublish => match_publish_intent(input),
        SessionState::WaitingForOutline => {
            Intent::ModifyOutline { instruction: input.to_string() }
        }
        SessionState::WaitingForFulltext => match_fulltext_intent(input),
        SessionState::Idle => {
            // 已在上面处理，编译需要 exhaustive match
            Intent::Unknown
        }
    }
}

// ── Layer 1 模式检测辅助 ────────────────────────────────────────────

/// 判断输入是否包含"学习风格"语义关键词。
fn has_learn_keywords(input: &str) -> bool {
    input.contains("学一下")
        || (input.contains("分析") && input.contains("文风"))
        || (input.contains("逆向") && input.contains("文风"))
}

/// 判断输入是否匹配"列出风格"模式。
fn matches_list_styles(input: &str) -> bool {
    (input.contains("风格库") && input.contains("有什么"))
        || (input.contains("有哪些") && input.contains("风格"))
        || (input.contains("列出") && input.contains("风格"))
}

/// 判断输入是否匹配"查看风格"模式。
fn matches_show_style(input: &str) -> bool {
    (input.contains("看看") && input.contains("风格"))
        || (input.contains("风格") && input.contains("详情"))
        || (input.contains("风格") && input.contains("摘要"))
}

/// 判断输入是否匹配"删除风格"模式。
fn matches_delete_style(input: &str) -> bool {
    (input.contains("删掉") && input.contains("风格"))
        || (input.contains("删除") && input.contains("风格"))
}

// ── Layer 1 提取辅助 ────────────────────────────────────────────────

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

/// 判断输入是否为确认语义（大小写不敏感）。
fn is_confirm(input: &str) -> bool {
    let s = input.trim();
    const CONFIRM_ZH: &[&str] = &[
        "没问题", "继续", "就这样", "好", "可以", "行", "对", "是的", "嗯",
    ];
    if CONFIRM_ZH.contains(&s) {
        return true;
    }
    let lower = s.to_lowercase();
    const CONFIRM_EN: &[&str] = &["ok", "yes", "y"];
    CONFIRM_EN.contains(&lower.as_str())
}

/// 判断输入是否为取消语义（大小写不敏感）。
fn is_cancel(input: &str) -> bool {
    let s = input.trim();
    const CANCEL_ZH: &[&str] = &[
        "算了", "取消", "不写了", "不要了", "换一个", "放弃",
    ];
    if CANCEL_ZH.contains(&s) {
        return true;
    }
    let lower = s.to_lowercase();
    const CANCEL_EN: &[&str] = &["cancel", "no", "n"];
    CANCEL_EN.contains(&lower.as_str())
}

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
    let style_name = after_kw[yong_pos + "用".len()..fengge_pos].trim().to_string();

    if topic.is_empty() || style_name.is_empty() {
        return None;
    }

    Some((topic, style_name))
}

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

/// 在 WaitingForPublish 状态下匹配发布相关意图。
fn match_publish_intent(input: &str) -> Intent {
    if input.contains("只发网站") || input.contains("只部署网站") || input.contains("网站发布") {
        return Intent::PublishWebsiteOnly;
    }
    if input.contains("发布") || input.contains("推") || input.contains("上线") || input.contains("发")
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
    Intent::Unknown
}

/// 在 WaitingForFulltext 状态下匹配正文修改相关意图。
fn match_fulltext_intent(input: &str) -> Intent {
    // 更换风格
    if (input.contains("整体") && input.contains("风格"))
        || (input.contains("换成") && input.contains("风格"))
        || input.contains("改风格")
    {
        if let Some(style_name) = extract_style_name(input) {
            return Intent::ChangeStyle { style_name };
        }
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

    // ── extract_url ─────────────────────────────────────────────

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

    // ── is_confirm ─────────────────────────────────────────────

    #[test]
    fn test_confirm_chinese() {
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

    // ── is_cancel ──────────────────────────────────────────────

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

    // ── extract_generate ───────────────────────────────────────

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

    // ── extract_update ─────────────────────────────────────────

    #[test]
    fn test_extract_update() {
        let (path, inst) =
            extract_update("重写 outputs/test_v1.md 改成更正式的语气").unwrap();
        assert_eq!(path, "outputs/test_v1.md");
        assert_eq!(inst, "更正式的语气");
    }

    #[test]
    fn test_extract_update_no_match() {
        assert!(extract_update("修改 outputs/test_v1.md 的语气").is_none());
    }

    // ── extract_refine ─────────────────────────────────────────

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

    // ── extract_style_name ─────────────────────────────────────

    #[test]
    fn test_extract_style_name_kankan() {
        assert_eq!(
            extract_style_name("看看鲁迅风格").unwrap(),
            "鲁迅"
        );
    }

    #[test]
    fn test_extract_style_name_delete() {
        assert_eq!(
            extract_style_name("删掉测试风格").unwrap(),
            "测试"
        );
    }

    #[test]
    fn test_extract_style_name_gaifengge() {
        assert_eq!(
            extract_style_name("改风格鲁迅").unwrap(),
            "鲁迅"
        );
    }

    // ── parse_intent: Layer 1 ──────────────────────────────────

    #[test]
    fn test_parse_url_is_learn() {
        let result = parse_intent("https://example.com", &SessionState::Idle);
        assert!(matches!(result, Intent::Learn { .. }));
    }

    #[test]
    fn test_parse_list_styles() {
        let result = parse_intent("风格库里有什么", &SessionState::Idle);
        assert!(matches!(result, Intent::ListStyles));
    }

    #[test]
    fn test_parse_show_style() {
        let result = parse_intent("看看鲁迅风格", &SessionState::Idle);
        assert!(matches!(result, Intent::ShowStyle { .. }));
    }

    #[test]
    fn test_parse_delete_style() {
        let result = parse_intent("删掉测试风格", &SessionState::Idle);
        assert!(matches!(result, Intent::DeleteStyle { .. }));
    }

    #[test]
    fn test_parse_generate() {
        let result = parse_intent("写一篇关于AI的文章用鲁迅风格", &SessionState::Idle);
        assert!(matches!(result, Intent::Generate { .. }));
    }

    #[test]
    fn test_parse_update() {
        let result =
            parse_intent("重写 outputs/test.md 改成更正式", &SessionState::Idle);
        assert!(matches!(result, Intent::UpdateFile { .. }));
    }

    #[test]
    fn test_parse_refine() {
        let result = parse_intent("outputs/test.md 改一下第二段", &SessionState::Idle);
        assert!(matches!(result, Intent::RefineFile { .. }));
    }

    // ── parse_intent: Layer 2 ──────────────────────────────────

    #[test]
    fn test_idle_unknown_on_unmatched() {
        let result = parse_intent("随便说点什么", &SessionState::Idle);
        assert!(matches!(result, Intent::Unknown));
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
        let result = parse_intent("只发网站", &SessionState::WaitingForPublish);
        assert!(matches!(result, Intent::PublishWebsiteOnly));
    }

    #[test]
    fn test_waiting_for_publish_publish() {
        let result = parse_intent("发布吧", &SessionState::WaitingForPublish);
        assert!(matches!(result, Intent::Publish));
    }

    #[test]
    fn test_waiting_for_publish_hold() {
        let result = parse_intent("等一下", &SessionState::WaitingForPublish);
        assert!(matches!(result, Intent::Hold));
    }

    #[test]
    fn test_waiting_for_outline_modify() {
        let result = parse_intent("第二部分太长了", &SessionState::WaitingForOutline);
        assert!(matches!(result, Intent::ModifyOutline { .. }));
    }

    #[test]
    fn test_waiting_for_fulltext_change_style() {
        let result = parse_intent("换成鲁迅风格", &SessionState::WaitingForFulltext);
        assert!(matches!(result, Intent::ChangeStyle { .. }));
    }

    #[test]
    fn test_waiting_for_fulltext_modify_section() {
        let result = parse_intent("第三段需要更多案例", &SessionState::WaitingForFulltext);
        assert!(matches!(result, Intent::ModifySection { .. }));
    }

    #[test]
    fn test_empty_input_is_unknown() {
        let result = parse_intent("   ", &SessionState::WaitingForOutline);
        assert!(matches!(result, Intent::Unknown));
    }

    // ── Priority: URL beats other patterns ─────────────────────

    #[test]
    fn test_url_priority_over_generate() {
        let result = parse_intent(
            "写一篇用鲁迅风格 https://example.com",
            &SessionState::Idle,
        );
        assert!(matches!(result, Intent::Learn { .. }));
    }

    // ── Priority: ShowStyle before DeleteStyle ──────────────────

    #[test]
    fn test_show_style_before_delete() {
        let result = parse_intent("删掉看看的风格", &SessionState::Idle);
        assert!(matches!(result, Intent::ShowStyle { .. }));
    }

    // ── Priority: UpdateFile before RefineFile ──────────────────

    #[test]
    fn test_update_before_refine() {
        let result = parse_intent("重写 a.md 改成更正式", &SessionState::Idle);
        assert!(matches!(result, Intent::UpdateFile { .. }));
    }
}
