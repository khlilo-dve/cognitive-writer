//! 全文输入、目标块输出；程序按允许的原文范围应用修改。
use crate::edit_blocks::{blocks, parser};
use crate::error::AppError;
use pulldown_cmark::Event;
use serde::Deserialize;
use std::collections::BTreeSet;

pub(crate) const SYSTEM_PROMPT: &str = r#"你是文章局部编辑器。用户提供完整原文、内容块索引、允许修改的块编号、修改指令和风格要求。
完整原文供理解上下文，不是要求输出全文。只改 allowed_ids 指定的块，保持上下文衔接及原有风格。
只输出 JSON：{"edits":[{"id":2,"replacement":"该块修改后的完整 Markdown 内容"}]}。
每个允许的 id 必须且只能出现一次；禁止增加其他 id 或输出整篇文章。id 是块编号，不是正文段落序号。
replacement 仅包含该块，不能包含周围未修改内容。允许目标段落内部增加案例或拆成多段。
保持 Markdown 结构闭合，不能通过未闭合代码块、HTML、列表等影响块外内容。不要解释或加 JSON 代码围栏。"#;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Response {
    edits: Vec<Edit>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Edit {
    id: usize,
    replacement: String,
}

pub(crate) fn request(article: &str, ids: &[usize], instruction: &str, style: &str) -> String {
    serde_json::json!({
        "article": article, "blocks": blocks(article), "allowed_ids": ids,
        "instruction": instruction, "style": style,
    })
    .to_string()
}

fn invalid(reason: &str) -> AppError {
    AppError::Parse(format!("局部修改校验失败：{reason}；原文未改动"))
}

// 比较 Markdown 事件可发现跨块代码围栏、引用和引用式链接的外溢影响。
fn signature(text: &str, start: usize, end: usize) -> Vec<String> {
    parser(text)
        .into_offset_iter()
        .filter(|(_, range)| range.start >= start && range.start < end && range.end <= end + 2)
        .map(|(event, _)| format!("{event:?}"))
        .collect()
}

pub(crate) fn apply(article: &str, allowed: &[usize], raw: &str) -> Result<String, AppError> {
    let all = blocks(article);
    let allowed_set: BTreeSet<_> = allowed.iter().copied().collect();
    if allowed_set.is_empty()
        || allowed_set.len() != allowed.len()
        || allowed.iter().any(|id| !all.iter().any(|b| b.id == *id))
    {
        return Err(invalid("修改目标不存在或重复"));
    }
    let response: Response = serde_json::from_str(raw.trim())
        .map_err(|_| invalid("响应必须是包含 edits 的有效 JSON"))?;
    let mut seen = BTreeSet::new();
    for edit in &response.edits {
        if !allowed_set.contains(&edit.id) || !seen.insert(edit.id) {
            return Err(invalid("响应包含未授权或重复的块编号"));
        }
        if edit.replacement.trim() == article.trim() && allowed.len() < all.len() {
            return Err(invalid("模型返回了整篇原文，而非指定内容块"));
        }
        if edit.replacement.trim().is_empty() {
            return Err(invalid("替换内容为空"));
        }
        if parser(&edit.replacement)
            .any(|event| matches!(event, Event::Html(_) | Event::InlineHtml(_)))
        {
            return Err(invalid(
                "替换内容含 HTML，请使用 Markdown，以免影响其他段落",
            ));
        }
        let probe = format!("{}\n\nCognitiveWriterBoundaryProbe", edit.replacement);
        if !blocks(&probe)
            .last()
            .is_some_and(|b| &probe[b.start..b.end] == "CognitiveWriterBoundaryProbe")
        {
            return Err(invalid("替换内容的 Markdown 结构未闭合"));
        }
    }
    if seen != allowed_set {
        return Err(invalid("响应遗漏目标块"));
    }
    let mut edits = response.edits;
    edits.sort_by_key(|e| e.id);
    let mut output = String::new();
    let mut cursor = 0;
    let mut replacements = Vec::new();
    for edit in edits {
        let block = all.iter().find(|b| b.id == edit.id).unwrap();
        output.push_str(&article[cursor..block.start]);
        let start = output.len();
        // 块外的换行和空行全部取自原文。
        let replacement = edit.replacement.trim_end_matches(['\r', '\n']);
        output.push_str(replacement);
        replacements.push((block.start, block.end, start, output.len()));
        cursor = block.end;
    }
    output.push_str(&article[cursor..]);
    let rebuilt = blocks(&output);
    for (_, _, new_start, new_end) in &replacements {
        // 输出中编辑范围必须形成完整的顶层块，不能并入邻块。
        let contained: Vec<_> = rebuilt
            .iter()
            .filter(|b| b.start >= *new_start && b.end <= *new_end)
            .collect();
        if contained.is_empty()
            || contained.first().unwrap().start != *new_start
            || contained.last().unwrap().end != *new_end
        {
            return Err(invalid("替换内容未形成独立、闭合的 Markdown 内容块"));
        }
        if rebuilt.iter().any(|b| {
            b.start < *new_start && b.end > *new_start || b.start < *new_end && b.end > *new_end
        }) {
            return Err(invalid("替换内容跨越了允许的边界"));
        }
    }
    for block in all.iter().filter(|b| !allowed_set.contains(&b.id)) {
        let delta: isize = replacements
            .iter()
            .filter(|(_, end, _, _)| *end <= block.start)
            .map(|(a, b, c, d)| (d - c) as isize - (b - a) as isize)
            .sum();
        let start = block
            .start
            .checked_add_signed(delta)
            .ok_or_else(|| invalid("偏移无效"))?;
        let end = block
            .end
            .checked_add_signed(delta)
            .ok_or_else(|| invalid("偏移无效"))?;
        if !rebuilt
            .iter()
            .any(|b| b.start == start && b.end == end && b.kind == block.kind)
            || output.get(start..end) != article.get(block.start..block.end)
            || signature(article, block.start, block.end) != signature(&output, start, end)
        {
            return Err(invalid("替换影响了未选择内容的原文或 Markdown 结构"));
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn response(edits: &[(usize, &str)]) -> String {
        serde_json::json!({"edits":edits.iter().map(|(id,text)| serde_json::json!({"id":id,"replacement":text})).collect::<Vec<_>>()}).to_string()
    }
    #[test]
    fn replaces_only_selected_duplicate_and_keeps_original_bytes() {
        let article = "# 标题\r\n\r\n重复正文。\r\n\r\n重复正文。\r\n\r\n> 引用 **不变**\r\n\r\n```rs\r\nx();\r\n```\r\n";
        let actual = apply(article, &[3], &response(&[(3, "新正文。\n\n增加案例。")])).unwrap();
        assert_eq!(
            actual,
            "# 标题\r\n\r\n重复正文。\r\n\r\n新正文。\n\n增加案例。\r\n\r\n> 引用 **不变**\r\n\r\n```rs\r\nx();\r\n```\r\n"
        );
    }
    #[test]
    fn multiple_blocks_apply_together_or_reject_entire_response() {
        let article = "# T\n\nA\n\nB\n\nC\n";
        assert_eq!(
            apply(article, &[2, 4], &response(&[(4, "新 C"), (2, "新 A")])).unwrap(),
            "# T\n\n新 A\n\nB\n\n新 C\n"
        );
        for edits in [
            vec![(2, "X")],
            vec![(2, "X"), (4, "")],
            vec![(2, "X"), (2, "Y")],
            vec![(2, "X"), (4, "Z"), (3, "越界")],
        ] {
            assert!(apply(article, &[2, 4], &response(&edits)).is_err());
        }
        assert!(apply(article, &[2], "返回整篇文章").is_err());
        assert!(apply(article, &[2], r#"{"edits":[],"fulltext":"X"}"#).is_err());
    }
    #[test]
    fn rejects_markdown_that_changes_neighbors() {
        let article = "# T\n\nA\n\nB\n";
        for replacement in ["```rust\nunclosed", "<div>\nnot closed", "[x]: /link"] {
            assert!(
                apply(article, &[2], &response(&[(2, replacement)])).is_err(),
                "{replacement}"
            );
        }
        // Same source text outside the edit can still render differently if a link definition changes.
        let article = "A\n\n[外部][x]\n";
        assert!(
            apply(
                article,
                &[1],
                &response(&[(1, "段落\n\n[x]: https://example.com")])
            )
            .is_err()
        );
    }
    #[test]
    fn prompt_contains_full_article_and_only_authorized_ids() {
        let article = "# 标题\n\n目标。\n\n上下文。";
        let req: serde_json::Value =
            serde_json::from_str(&request(article, &[2], "加案例", "短句")).unwrap();
        assert_eq!(req["article"], article);
        assert_eq!(req["allowed_ids"], serde_json::json!([2]));
        assert_eq!(req["style"], "短句");
    }

    #[test]
    fn lists_quotes_code_and_tables_can_be_replaced_without_touching_neighbors() {
        let article = "# T\n\n- a\n- b\n\n> old\n\n```rs\nx();\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n尾段。";
        let all = blocks(article);
        for (id, replacement) in [
            (2, "- 新条目\n- 新条目二"),
            (3, "> 新引用"),
            (4, "```rs\ny();\n```"),
            (5, "| a | b |\n|---|---|\n| 3 | 4 |"),
        ] {
            let b = &all[id - 1];
            let expected = format!(
                "{}{}{}",
                &article[..b.start],
                replacement,
                &article[b.end..]
            );
            assert_eq!(
                apply(article, &[id], &response(&[(id, replacement)])).unwrap(),
                expected
            );
        }
    }
}
