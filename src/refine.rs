// ── refine 子命令 — 局部重绘 ─────────────────────────────────────
use pulldown_cmark::{html, Options, Parser as MdParser};
use reqwest::Client;
use std::process;

use crate::clipboard::inject_clipboard;
use crate::io::{extract_idea_slug, next_version, read_file, write_file};
use crate::llm::{call_llm, new_spinner, with_retry};


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

// ── 辅助函数 ─────────────────────────────────────────────────────────

fn md_to_wechat_html(markdown: &str) -> String {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TABLES);

    let parser = MdParser::new_ext(markdown, opts);
    let mut raw = String::new();
    html::push_html(&mut raw, parser);

    let raw = raw.replace("<hr />", "").replace("<hr>", "");

    format!("<section style=\"font-size:15px;line-height:2;color:#333;\">{raw}</section>")
}

fn env_var(key: &str) -> Result<String, String> {
    std::env::var(key).map_err(|_| format!("环境变量 `{}` 未设置，请检查 .env 文件", key))
}

fn env_var_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn fatal(msg: impl std::fmt::Display) -> ! {
    eprintln!("[error] {}", msg);
    process::exit(1);
}

// ── run_refine (CLI 模式入口) ──────────────────────────────────────

pub async fn run_refine(file: &str, no_clipboard: bool) {
    // 注意：不再调用 dotenvy::dotenv()，main() 已经调用过了

    let api_key = env_var("API_KEY").unwrap_or_else(|e| fatal(&e));
    let base_url = env_var_or("BASE_URL", "https://api.openai.com/v1");
    let model = env_var_or("MODEL", "gpt-4o");

    println!("[info] BASE_URL = {}", base_url);
    println!("[info] MODEL    = {}", model);

    // 2. 读取目标文件
    let mut content = read_file(file).unwrap_or_else(|e| fatal(&e));
    println!("[info] 已读取 {}, {} 字符", file, content.len());

    // 3. 解析 AI_EDIT 标记
    let edits = parse_ai_edits(&content).unwrap_or_else(|e| fatal(&e));
    let total = edits.len();
    println!("[info] 发现 {} 个 AI_EDIT 标记", total);

    // 4. 逐个 LLM 重写
    let client = Client::new();
    for (i, edit) in edits.iter().enumerate() {
        let idx = i + 1;
        let user_prompt = format!(
            "修改指令：{}\n\n原文本：\n{}",
            edit.instruction, edit.original
        );

        let label = format!("标记 {}/{}", idx, total);
        let spinner = new_spinner(&format!("正在处理标记 {}/{}...", idx, total));
        let rewritten = with_retry(3, &label, || {
            call_llm(
                &client,
                &base_url,
                &api_key,
                &model,
                REFINE_SYSTEM_PROMPT,
                &user_prompt,
            )
        })
        .await
        .unwrap_or_else(|e| {
            spinner.finish_with_message(format!("标记 {}/{} 失败", idx, total));
            fatal(&e)
        });
        spinner.finish_with_message(format!("标记 {}/{} 完成", idx, total));
        content = content.replacen(&edit.full_match, &rewritten, 1);
    }

    // 5. 版本命名 + 三轨输出
    let slug = extract_idea_slug(&content);
    let ver = next_version("outputs", &slug);
    let md_path = format!("outputs/{}_v{}.md", slug, ver);
    let html_path = format!("outputs/{}_v{}.html", slug, ver);

    write_file(&md_path, &content).unwrap_or_else(|e| fatal(&e));
    println!("[done] Markdown → {}", md_path);

    let html_fragment = md_to_wechat_html(&content);
    let html_doc = format!(
        "<!DOCTYPE html>\n<html>\n<head><meta charset=\"utf-8\"></head>\n<body>\n{}\n</body>\n</html>",
        html_fragment
    );
    write_file(&html_path, &html_doc).unwrap_or_else(|e| fatal(&e));
    println!("[done] HTML     → {}", html_path);

    if no_clipboard {
        println!("[info] 跳过剪贴板注入 (--no-clipboard)");
    } else {
        match inject_clipboard(&html_fragment) {
            Ok(tool) => {
                println!("[done] 富文本已注入剪贴板 (via {})", tool);
                println!();
                println!("  局部重绘完成，富文本已注入剪贴板，请直接前往微信粘贴 (Ctrl+V)");
            }
            Err(e) => {
                eprintln!("[warn] {}", e);
                println!();
                println!(
                    "  局部重绘完成。剪贴板不可用，请用浏览器打开 {} 后手动复制",
                    html_path
                );
            }
        }
    }
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
