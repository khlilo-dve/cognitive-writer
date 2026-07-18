// ── refine 子命令 — 局部重绘 ─────────────────────────────────────

pub const REFINE_SYSTEM_PROMPT: &str = r#"你是一个严苛的文本重构引擎。
请根据用户给出的修改指令，重写指定的文本片段。

要求：
1. 只输出重写后的文本
2. 不要带有任何解释、问候或 Markdown 代码块包裹
3. 必须与原有的行文风格无缝衔接
4. 严格遵循修改指令的要求"#;

pub struct AiEdit {
    pub instruction: String,
    pub original: String,
    pub full_match: String,
}

pub fn parse_ai_edits(content: &str) -> Result<Vec<AiEdit>, String> {
    let mut edits = Vec::new();
    let mut pos = 0;

    loop {
        // 1. 找开标签起始
        let open_start = match content[pos..].find("<AI_EDIT ") {
            Some(i) => pos + i,
            None => break,
        };

        // 2. 提取 instruction="..."
        let inst_key = "instruction=\"";
        let inst_start = match content[open_start..].find(inst_key) {
            Some(i) => open_start + i + inst_key.len(),
            None => return Err(format!("AI_EDIT 标签缺少 instruction 属性 (位置 {})", open_start)),
        };
        let inst_end = match content[inst_start..].find('"') {
            Some(i) => inst_start + i,
            None => return Err(format!("instruction 属性引号未闭合 (位置 {})", inst_start)),
        };
        let instruction = content[inst_start..inst_end].to_string();

        // 3. 找开标签结束 '>'
        let tag_end = match content[inst_end..].find('>') {
            Some(i) => inst_end + i + 1, // +1 跳过 '>' 本身
            None => return Err(format!("AI_EDIT 开标签未闭合 (位置 {})", open_start)),
        };

        // 4. 找闭标签
        let close_tag = "</AI_EDIT>";
        let close_start = match content[tag_end..].find(close_tag) {
            Some(i) => tag_end + i,
            None => return Err(format!("未找到匹配的 </AI_EDIT> (位置 {})", open_start)),
        };
        let close_end = close_start + close_tag.len();

        let original = content[tag_end..close_start].to_string();
        let full_match = content[open_start..close_end].to_string();

        edits.push(AiEdit {
            instruction,
            original,
            full_match,
        });

        pos = close_end;
    }

    if edits.is_empty() {
        return Err("未找到 AI_EDIT 标记".to_string());
    }
    Ok(edits)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_ai_edits ──────────────────────────────────────────

    #[test]
    fn test_parse_single_ai_edit() {
        let content = "<AI_EDIT instruction=\"修复语法错误\">原始文本内容</AI_EDIT>";
        let edits = parse_ai_edits(content).unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].instruction, "修复语法错误");
        assert_eq!(edits[0].original, "原始文本内容");
    }

    #[test]
    fn test_parse_multiple_ai_edits() {
        let content = concat!(
            "<AI_EDIT instruction=\"第一个修改\">文本A</AI_EDIT>",
            "中间无关内容",
            "<AI_EDIT instruction=\"第二个修改\">文本B</AI_EDIT>"
        );
        let edits = parse_ai_edits(content).unwrap();
        assert_eq!(edits.len(), 2);
        assert_eq!(edits[0].instruction, "第一个修改");
        assert_eq!(edits[0].original, "文本A");
        assert_eq!(edits[1].instruction, "第二个修改");
        assert_eq!(edits[1].original, "文本B");
    }

    #[test]
    fn test_parse_ai_edit_multiline_original() {
        let content = "<AI_EDIT instruction=\"重构段落\">第一行\n第二行\n第三行</AI_EDIT>";
        let edits = parse_ai_edits(content).unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].original, "第一行\n第二行\n第三行");
    }

    #[test]
    fn test_parse_ai_edits_no_tag_is_error() {
        let result = parse_ai_edits("没有任何标记的普通文本");
        assert!(result.is_err());
        { let err_msg = result.err().unwrap(); assert!(err_msg.contains("未找到")); };
    }

    #[test]
    fn test_parse_ai_edits_missing_instruction_is_error() {
        // <AI_EDIT> 不带 instruction 属性
        let result = parse_ai_edits("<AI_EDIT >文本</AI_EDIT>");
        assert!(result.is_err());
        { let err_msg = result.err().unwrap(); assert!(err_msg.contains("instruction")); };
    }

    #[test]
    fn test_parse_ai_edits_unclosed_tag_is_error() {
        let result = parse_ai_edits("<AI_EDIT instruction=\"测试\">缺少闭合标签");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_ai_edits_instruction_with_inner_single_quotes() {
        // instruction 属性值含单引号
        let content = "<AI_EDIT instruction=\"把'你好'改成'您好'\">你好</AI_EDIT>";
        let edits = parse_ai_edits(content).unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].instruction, "把'你好'改成'您好'");
        assert_eq!(edits[0].original, "你好");
    }

    #[test]
    fn test_parse_ai_edits_full_match_includes_tags() {
        let content = "<AI_EDIT instruction=\"测试\">原文</AI_EDIT>";
        let edits = parse_ai_edits(content).unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].full_match, "<AI_EDIT instruction=\"测试\">原文</AI_EDIT>");
    }
}
