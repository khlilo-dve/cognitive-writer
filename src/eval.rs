//! 两套 eval：风格提示词（生成端）+ learn 提取提示词（提取端）。
//!
//! 手动运行（需 API_KEY + 网络）：
//!   cargo test --lib -- --ignored eval --nocapture

use reqwest::Client;

use crate::error::AppError;
use crate::generate::{generate_outline, render_fulltext};
use crate::llm::call_llm;

// ── eval 1：风格提示词（生成端）───────────────────────────────────────

/// 评测风格提示词：同一主题 + 同一风格 → 生成大纲 + 正文，输出风格贴合度指标。
pub async fn eval_style_prompt(
    client: &Client,
    base_url: &str,
    api_key: &str,
    model: &str,
    style: &str,
    idea: &str,
) -> Result<(), AppError> {
    let outline = generate_outline(client, base_url, api_key, model, style, idea).await?;
    let fulltext = render_fulltext(client, base_url, api_key, model, style, &outline, idea).await?;

    // 具体指标
    let outline_len = outline.chars().count();
    let fulltext_len = fulltext.chars().count();
    let paras: Vec<&str> = fulltext.lines().filter(|l| !l.trim().is_empty()).collect();
    let avg_para_len = if paras.is_empty() {
        0.0
    } else {
        fulltext_len as f64 / paras.len() as f64
    };
    let forbidden = ["众所周知", "总而言之", "值得注意的是", "毋庸置疑", "综上所述"];
    let forbidden_hits = forbidden.iter().filter(|w| fulltext.contains(**w)).count();
    let question_marks = fulltext.matches('？').count();

    println!("\n===== eval 1 风格提示词（生成端）=====");
    println!("[指标] 大纲字符数: {}", outline_len);
    println!("[指标] 正文字符数: {}", fulltext_len);
    println!("[指标] 正文段落数: {}", paras.len());
    println!("[指标] 平均段落字数: {:.1}", avg_para_len);
    println!("[指标] 禁用 AI 套话命中数: {}", forbidden_hits);
    println!("[指标] 问号（设问/反问）数: {}", question_marks);
    println!("--- 大纲 ---\n{}\n--- 正文 ---\n{}", outline, fulltext);

    Ok(())
}

// ── eval 2：learn 提取提示词（提取端）─────────────────────────────────

/// 评测提取提示词：喂一篇风格鲜明的文章 → 提取 spec，输出 spec 精简度指标。
pub async fn eval_learn_prompt(
    client: &Client,
    base_url: &str,
    api_key: &str,
    model: &str,
    article: &str,
) -> Result<(), AppError> {
    let prompt = std::fs::read_to_string("prompts/learn_style.md")
        .map_err(|e| AppError::Intent(format!("读取提取提示词失败: {}", e)))?;
    let spec = call_llm(client, base_url, api_key, model, &prompt, article).await?;

    // 具体指标
    let spec_len = spec.chars().count();
    let sections = [
        "## 定位",
        "## 标题",
        "## 开头",
        "## 结构与论证",
        "## 段落与句式",
        "## 语气与人称",
        "## 用词",
        "## 结尾",
        "## 排版",
        "## 禁止",
    ];
    let covered = sections.iter().filter(|s| spec.contains(**s)).count();
    let total = sections.len();
    let avg_section_len = if covered == 0 {
        0.0
    } else {
        spec_len as f64 / covered as f64
    };

    println!("\n===== eval 2 learn 提取提示词（提取端）=====");
    println!("[指标] spec 字符数: {} (目标 ≤600)", spec_len);
    println!("[指标] 10 段覆盖率: {}/{}", covered, total);
    println!("[指标] 平均每段字数: {:.1}", avg_section_len);
    println!("--- spec ---\n{}", spec);

    Ok(())
}

// ── 手动评测入口（#[ignore] 测试）──────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const IDEA: &str = "文章主题：普通人为什么很难靠打工实现财富跃迁\n基本内容：工资有天花板，资产能复利。多数人停在第一桶金之前。";
    const ARTICLE: &str = "很多人把自律当成意志力的比赛，错了。自律的本质是什么？是你根本不需要去忍。想想看，天天靠意志力撑着的人，最后怎么样了？都崩了。真正的高手从来不硬扛。他们改变环境，而不是改变自己。你在沙发上刷手机，怪自己没自控力？废话。把手机放另一个房间，问题就没了。自律不是品格问题，是设计问题。";

    fn env() -> (String, String, String, Client) {
        dotenvy::dotenv().ok();
        let key = std::env::var("API_KEY").expect("API_KEY 未设置");
        let base = std::env::var("BASE_URL")
            .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
        let model = std::env::var("MODEL").unwrap_or_else(|_| "gpt-4o".to_string());
        (base, key, model, Client::new())
    }

    #[test]
    #[ignore = "真实 LLM 调用，需 API_KEY + 网络"]
    fn eval_style_prompt_suite() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (base, key, model, client) = env();
            let style = std::fs::read_to_string("styles/qingbian.md").expect("读取风格失败");
            eval_style_prompt(&client, &base, &key, &model, &style, IDEA)
                .await
                .expect("eval 1 失败");
        });
    }

    #[test]
    #[ignore = "真实 LLM 调用，需 API_KEY + 网络"]
    fn eval_learn_prompt_suite() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (base, key, model, client) = env();
            eval_learn_prompt(&client, &base, &key, &model, ARTICLE)
                .await
                .expect("eval 2 失败");
        });
    }
}
