use reqwest::Client;

use crate::clipboard::inject_clipboard;
use crate::error::AppError;
use crate::io::{
    extract_idea_slug, list_styles, next_version, read_file, select_style, write_file,
};
use crate::llm::{call_llm, new_spinner, with_retry};
use crate::utils::{env_var, env_var_or, fatal, md_to_wechat_html};

// ── Prompt builders ──────────────────────────────────────────────────

/// 大纲生成 system prompt：注入风格执行指令，让大纲在结构层就贴合风格。
pub(crate) fn outline_system_prompt(style: &str) -> String {
    format!(
        "你是一个写作结构设计专家。\n\n\
         以下是本次文章的写作风格：核心偏好优先采用，条件策略仅在适用时使用；当前任务明确要求与事实边界优先：\n\n---\n{style}\n---\n\n\
         用户会给你一份创作素材，请输出一份【符合上述风格】的文章逻辑大纲。\n\n\
         要求：\n\
         1. 用 Markdown 层级列表（- / 1. 2. 3.）\n\
         2. 开头节点体现风格的破题方式，结尾节点体现风格的收束方式\n\
         3. 每个节点写清该段的【核心判断】和【支撑素材方向】，核心判断要符合风格的论证方式\n\
         4. 标注段落之间的逻辑衔接关系（递进/转折/并列/因果）\n\
         5. 控制在 300-500 字以内\n\
         6. 不要写正文，只输出大纲骨架"
    )
}

/// 正文渲染 system prompt：风格 spec 外包一层适用条件与事实边界。
/// 通用约束（只输出正文 / 不伪造事实 / 任务明确要求优先）放在代码里，
/// 不写进每个风格文件，避免重复与漂移。
pub(crate) fn style_system_prompt(style: &str) -> String {
    format!(
        "你是一名中文写作者。以下是本次写作风格：核心偏好优先采用，条件策略仅在适用时使用，不把单篇观察升级为每段必须执行：\n\n---\n{style}\n---\n\n\
         通用约束：\n\
         - 只输出 Markdown 正文，不复述风格规则，不输出思考、自检或写作过程\n\
         - 不虚构亲身经历、引用、精确数据或事实；可用明确标记为假设的例子解释\n\
         - 当前写作任务的明确要求与事实边界优先；风格内未规定的维度不强行补齐"
    )
}

/// 正文渲染 user prompt：大纲 + 素材 + 输出前自检要求。
pub(crate) fn render_prompt(outline: &str, idea: &str) -> String {
    format!(
        "以下是符合风格的逻辑大纲，请严格按此结构展开正文：\n\n---\n{}\n---\n\n原始素材：\n\n---\n{}\n---\n\n\
         请按大纲结构展开，输出完整的 Markdown 正文。输出前核对：开头破题、核心论证、段落节奏、句式人称、结尾收束是否符合给定风格，是否出现 AI 套话或素材外事实；核对后直接输出正文，不要输出核对过程。",
        outline, idea
    )
}

// ── REPL 模式：Pass 1 only — 生成风格化骨架 ─────────────────────────

pub async fn generate_outline(
    client: &Client,
    base_url: &str,
    api_key: &str,
    model: &str,
    style: &str,
    idea: &str,
) -> Result<String, AppError> {
    let system = outline_system_prompt(style);
    with_retry(3, "大纲生成", || {
        call_llm(client, base_url, api_key, model, &system, idea)
    })
    .await
}

// ── REPL 模式：Pass 2 only — 从骨架渲染全文 ──────────────────────

pub async fn render_fulltext(
    client: &Client,
    base_url: &str,
    api_key: &str,
    model: &str,
    style: &str,
    outline: &str,
    idea: &str,
) -> Result<String, AppError> {
    let system = style_system_prompt(style);
    let user = render_prompt(outline, idea);
    with_retry(3, "正文渲染", || {
        call_llm(client, base_url, api_key, model, &system, &user)
    })
    .await
}

// ── 骨架-渲染双通道（CLI 模式，含 spinner UI）─────────────────────

async fn generate_with_outline(
    client: &Client,
    base_url: &str,
    api_key: &str,
    model: &str,
    style: &str,
    idea: &str,
) -> Result<String, AppError> {
    // Pass 1 — 风格化骨架
    let spinner = new_spinner("正在生成大纲骨架...");
    let result = generate_outline(client, base_url, api_key, model, style, idea).await;
    spinner.finish_with_message(if result.is_ok() {
        "大纲生成完成"
    } else {
        "大纲生成失败"
    });
    let outline = result?;
    println!("[info] 大纲 {} 字符", outline.len());

    // Pass 2 — 渲染
    let spinner = new_spinner("正在渲染正文...");
    let result = render_fulltext(client, base_url, api_key, model, style, &outline, idea).await;
    spinner.finish_with_message(if result.is_ok() {
        "正文渲染完成"
    } else {
        "正文渲染失败"
    });
    let markdown = result?;
    println!("[info] 正文 {} 字符", markdown.len());

    Ok(markdown)
}

// ── generate 子命令入口（CLI 模式）────────────────────────────────

pub async fn run_generate(input: &str, no_clipboard: bool) {
    let api_key = env_var("API_KEY").unwrap_or_else(|e| fatal(&e));
    let base_url = env_var_or("BASE_URL", "https://api.openai.com/v1");
    let model = env_var_or("MODEL", "gpt-4o");

    println!("[info] BASE_URL = {}", base_url);
    println!("[info] MODEL    = {}", model);

    // 风格选择
    let styles = list_styles("styles").unwrap_or_else(|e| fatal(&e));
    let idx = select_style(&styles).unwrap_or_else(|e| fatal(&e));
    let style_path = &styles[idx];
    let style = read_file(style_path).unwrap_or_else(|e| fatal(&e));

    println!(
        "[info] 风格: {}",
        style_path.strip_prefix("styles/").unwrap_or(style_path)
    );
    println!("[info] 素材文件: {}", input);

    // 读取素材
    let idea = read_file(input).unwrap_or_else(|e| fatal(&e));
    if idea.trim().is_empty() {
        fatal(format!("{} 内容为空，请先写入创作素材", input));
    }
    println!(
        "[info] 素材 {} 字符 | 风格 {} 字符",
        idea.len(),
        style.len()
    );

    // 双通道生成：骨架 → 渲染
    let client = Client::new();
    let markdown = generate_with_outline(&client, &base_url, &api_key, &model, &style, &idea)
        .await
        .unwrap_or_else(|e| fatal(&e));

    // 双轨输出
    let slug = extract_idea_slug(&idea);
    let ver = next_version("outputs", &slug);
    let md_path = format!("outputs/{}_v{}.md", slug, ver);
    let html_path = format!("outputs/{}_v{}.html", slug, ver);

    // 存档 Markdown
    write_file(&md_path, &markdown).unwrap_or_else(|e| fatal(&e));
    println!("[done] Markdown → {}", md_path);

    // 转换 HTML + 存档
    let html_fragment = md_to_wechat_html(&markdown);
    let html_doc = format!(
        "<!DOCTYPE html>\n<html>\n<head><meta charset=\"utf-8\"></head>\n<body>\n{}\n</body>\n</html>",
        html_fragment
    );
    write_file(&html_path, &html_doc).unwrap_or_else(|e| fatal(&e));
    println!("[done] HTML     → {}", html_path);

    // 注入剪贴板 (CF_HTML 富文本格式)
    if no_clipboard {
        println!("[info] 跳过剪贴板注入 (--no-clipboard)");
    } else {
        match inject_clipboard(&html_fragment) {
            Ok(tool) => {
                println!("[done] 富文本已注入剪贴板 (via {})", tool);
                println!();
                println!("  文章已生成并存档，富文本已注入剪贴板，请直接前往微信粘贴 (Ctrl+V)");
            }
            Err(e) => {
                eprintln!("[warn] {}", e);
                println!();
                println!(
                    "  文章已生成并存档。剪贴板不可用，请用浏览器打开 {} 后手动复制",
                    html_path
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_outline_system_prompt_includes_style_and_structure() {
        let p = outline_system_prompt("## 定位\n理性分析");
        assert!(p.contains("写作结构设计专家"));
        assert!(p.contains("## 定位"));
        assert!(p.contains("理性分析"));
        assert!(p.contains("破题"));
        assert!(p.contains("收束"));
    }

    #[test]
    fn test_style_system_prompt_wraps_style_with_constraints() {
        let p = style_system_prompt("## 定位\n理性分析");
        assert!(p.contains("## 定位"));
        assert!(p.contains("只输出 Markdown 正文"));
        assert!(p.contains("条件策略仅在适用时使用"));
        assert!(p.contains("明确要求与事实边界优先"));
    }

    #[test]
    fn test_render_prompt_contains_outline_and_idea() {
        let p = render_prompt("## 大纲标题", "这是素材");
        assert!(p.contains("## 大纲标题"));
        assert!(p.contains("这是素材"));
        assert!(p.contains("不要输出核对过程"));
    }
}
