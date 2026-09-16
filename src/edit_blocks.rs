//! Markdown 块索引使用原文偏移；正文段落不包含标题、列表、引用和代码块。
use pulldown_cmark::{Event, Options, Parser, Tag};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct Block {
    pub id: usize,
    pub start: usize,
    pub end: usize,
    pub kind: String,
    pub paragraph: Option<usize>,
    pub heading_level: Option<u8>,
}

pub(crate) fn parser(text: &str) -> Parser<'_> {
    Parser::new_ext(text, Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH)
}

pub(crate) fn blocks(text: &str) -> Vec<Block> {
    let mut result = Vec::new();
    let mut depth = 0;
    let mut current = None;
    let mut paragraphs = 0;
    for (event, range) in parser(text).into_offset_iter() {
        match event {
            Event::Start(tag) => {
                if depth == 0 {
                    let (kind, level) = match tag {
                        Tag::Paragraph => ("正文段落", None),
                        Tag::Heading { level, .. } => ("标题", Some(level as u8)),
                        Tag::List(_) => ("列表", None),
                        Tag::BlockQuote(_) => ("引用", None),
                        Tag::CodeBlock(_) => ("代码块", None),
                        Tag::Table(_) => ("表格", None),
                        _ => ("其他内容块", None),
                    };
                    let paragraph = if kind == "正文段落" {
                        paragraphs += 1;
                        Some(paragraphs)
                    } else {
                        None
                    };
                    current = Some(Block {
                        id: result.len() + 1,
                        start: range.start,
                        end: range.end,
                        kind: kind.into(),
                        paragraph,
                        heading_level: level,
                    });
                }
                depth += 1;
            }
            Event::End(_) => {
                depth -= 1;
                if depth == 0 {
                    if let Some(mut block) = current.take() {
                        block.end = range.end;
                        while block.end > block.start
                            && matches!(text.as_bytes()[block.end - 1], b'\n' | b'\r')
                        {
                            block.end -= 1;
                        }
                        result.push(block);
                    }
                }
            }
            Event::Rule if depth == 0 => result.push(Block {
                id: result.len() + 1,
                start: range.start,
                end: range.end,
                kind: "分隔线".into(),
                paragraph: None,
                heading_level: None,
            }),
            _ => {}
        }
    }
    result
}

pub(crate) enum Location {
    Selected(Vec<usize>),
    Ask(Vec<usize>),
}

fn number(input: &str) -> Option<usize> {
    let s = input.trim();
    if let Ok(n) = s.parse::<usize>() {
        return Some(n);
    }
    let digit = |s: &str| match s {
        "零" => Some(0),
        "一" => Some(1),
        "二" | "两" => Some(2),
        "三" => Some(3),
        "四" => Some(4),
        "五" => Some(5),
        "六" => Some(6),
        "七" => Some(7),
        "八" => Some(8),
        "九" => Some(9),
        _ => None,
    };
    if let Some((a, b)) = s.split_once('十') {
        Some(
            if a.is_empty() { 1 } else { digit(a)? } * 10
                + if b.is_empty() { 0 } else { digit(b)? },
        )
    } else {
        digit(s)
    }
}

fn ordinals(instruction: &str) -> Option<Vec<usize>> {
    let mut result = Vec::new();
    let mut rest = instruction;
    while let Some((_, tail)) = rest.split_once('第') {
        let (nums, remaining) = tail.split_once('段')?;
        if let Some((a, b)) = nums.split_once(['到', '至', '-']) {
            let (a, b) = (number(a)?, number(b)?);
            if a == 0 || a > b || b - a > 100 {
                return None;
            }
            result.extend(a..=b);
        } else {
            for n in nums.split(['、', ',', '，', '和', '及']) {
                result.push(number(n)?);
            }
        }
        rest = remaining;
    }
    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

/// B 编号只在用户明确使用 B 前缀或追问回复时解释，避免将字数当作位置。
pub(crate) fn block_ids(input: &str, allow_bare: bool) -> Option<Vec<usize>> {
    let mut ids = Vec::new();
    for token in input.trim().split(['、', ',', '，', ' ', '\n']) {
        if token.is_empty() {
            continue;
        }
        let raw = token
            .strip_prefix(['B', 'b'])
            .or(if allow_bare { Some(token) } else { None })?;
        let id = raw.parse::<usize>().ok()?;
        if id == 0 {
            return None;
        }
        ids.push(id);
    }
    ids.sort_unstable();
    ids.dedup();
    if ids.is_empty() { None } else { Some(ids) }
}

pub(crate) fn locate(text: &str, instruction: &str) -> Location {
    let all = blocks(text);
    let ask = || Location::Ask(all.iter().map(|b| b.id).collect());
    if let Some(ids) = block_ids(instruction, false) {
        return if ids.iter().all(|id| all.iter().any(|b| b.id == *id)) {
            Location::Selected(ids)
        } else {
            ask()
        };
    }
    // 被提及的位置可能是排除项或参考项，不能把所有段落编号都当成修改授权。
    let compact: String = instruction.chars().filter(|c| !c.is_whitespace()).collect();
    if [
        "除了",
        "以外",
        "之外",
        "不要修改",
        "不要改",
        "别改",
        "不改",
        "不动",
        "别动",
        "段保持",
        "段保留",
    ]
    .iter()
    .any(|term| compact.contains(term))
        || compact.contains("开头") && compact.contains("结尾")
    {
        return ask();
    }
    if let Some(nums) = ordinals(instruction) {
        if nums.len() > 1 {
            let start = instruction.find('第').unwrap();
            let end = instruction.rfind('段').unwrap();
            let span = &instruction[start..end];
            if !span.chars().all(|c| {
                c.is_whitespace()
                    || "第段0123456789零一二三四五六七八九十两、,，和及与到至-".contains(c)
            }) {
                return ask();
            }
        }
        if compact.contains("最后") {
            return ask();
        }
    }
    // 引用原文定位；重复内容必须追问，不能以 find/replacen 默选第一处。
    let quoted = [('「', '」'), ('“', '”'), ('"', '"'), ('‘', '’')]
        .iter()
        .find_map(|(a, b)| {
            let start = instruction.find(*a)? + a.len_utf8();
            let end = start + instruction[start..].find(*b)?;
            if start == end {
                None
            } else {
                Some((&instruction[start..end], end + b.len_utf8()))
            }
        });
    let mut scope: Vec<&Block> = all.iter().collect();
    if let Some((quote, end)) = quoted {
        let matched: Vec<_> = all
            .iter()
            .filter(|b| text[b.start..b.end].contains(quote))
            .collect();
        let after = &instruction[end..];
        if after.contains('下') && matched.len() == 1 && matched[0].heading_level.is_some() {
            let heading = matched[0];
            scope = all
                .iter()
                .skip_while(|b| b.id <= heading.id)
                .take_while(|b| {
                    !b.heading_level
                        .is_some_and(|level| level <= heading.heading_level.unwrap())
                })
                .collect();
            if ordinals(after).is_none() {
                return Location::Ask(scope.iter().map(|b| b.id).collect());
            }
        } else if ordinals(instruction).is_none() {
            let ids = matched.iter().map(|b| b.id).collect();
            return if matched.len() == 1 {
                Location::Selected(ids)
            } else if matched.is_empty() {
                ask()
            } else {
                Location::Ask(ids)
            };
        }
    }
    let paragraphs: Vec<_> = scope.iter().filter(|b| b.paragraph.is_some()).collect();
    if let Some(nums) = ordinals(instruction) {
        let mut ids = Vec::new();
        for n in nums {
            let Some(block) = n.checked_sub(1).and_then(|i| paragraphs.get(i)) else {
                return ask();
            };
            ids.push(block.id);
        }
        ids.sort_unstable();
        ids.dedup();
        return Location::Selected(ids);
    }
    let ordinal = if instruction.contains("开头") || instruction.contains("第一段") {
        paragraphs.first()
    } else if instruction.contains("结尾") || instruction.contains("最后一段") {
        paragraphs.last()
    } else {
        None
    };
    match ordinal {
        Some(b) => Location::Selected(vec![b.id]),
        None => ask(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn raw_ranges_cover_markdown_blocks_without_reformatting() {
        let text = "# 标题\r\n\r\n第一段。\r\n\r\n- 条目\r\n- 第二项\r\n\r\n> 引用\r\n\r\n```rs\r\nx();\r\n```\r\n\r\n第二段。\r\n";
        let result = blocks(text);
        assert_eq!(result.iter().filter(|b| b.paragraph.is_some()).count(), 2);
        assert_eq!(result.len(), 6);
        assert_eq!(&text[result[1].start..result[1].end], "第一段。");
        assert_eq!(result[4].kind, "代码块");
        assert!(result.windows(2).all(|w| w[0].end <= w[1].start));
        assert!(matches!(locate(text, "第二段加案例"), Location::Selected(ids) if ids == vec![6]));
    }
    #[test]
    fn target_resolution_handles_duplicates_sections_and_multiple_paragraphs() {
        let text = "# 标题\n\n重复。\n\n## 小节\n\n重复。\n\n第三段。\n\n## 其他\n\n第四段。";
        assert!(matches!(locate(text, "把“重复。”改短"), Location::Ask(ids) if ids.len() == 2));
        assert!(
            matches!(locate(text, "“小节”下面第二段改短"), Location::Selected(ids) if ids == vec![5])
        );
        assert!(
            matches!(locate(text, "第2、4段改短"), Location::Selected(ids) if ids == vec![4,7])
        );
        assert!(
            matches!(locate(text, "第2至3段改短"), Location::Selected(ids) if ids == vec![4,5])
        );
        assert!(matches!(locate(text, "第99段改短"), Location::Ask(_)));
        assert!(matches!(locate(text, "整体润色"), Location::Ask(_)));
        assert_eq!(block_ids("B2, B4", false), Some(vec![2, 4]));
        assert_eq!(block_ids("800 字", true), None);
        for instruction in [
            "第二段不动，第一段加案例",
            "参考第二段重写第三段",
            "第二段加第一段里的例子",
            "除了第三段，其余改短",
            "第一段和最后一段调整",
        ] {
            assert!(
                matches!(locate(text, instruction), Location::Ask(_)),
                "{instruction}"
            );
        }
        assert!(
            matches!(locate(text, "第三段把“重复。”换掉"), Location::Selected(ids) if ids == vec![5])
        );
    }
}
