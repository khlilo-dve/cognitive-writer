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
