//! 两套 eval：风格提示词（生成端）+ learn 提取提示词（提取端）。
//!
//! 手动运行（需 API_KEY + 网络）：
//!   cargo test --lib eval_automated_suite -- --ignored --nocapture
//! See eval/AGENTS.md for individual suites and checkpoint rules.

use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::generate::{generate_outline, render_fulltext};
use crate::llm::{call_llm, with_retry};

/// Extract only the body of a legacy batch archive. Plain articles remain intact.
pub fn article_body(content: &str) -> Result<&str, AppError> {
    let content = content.trim();
    let body = if content.starts_with("# 主题 ") && content.lines().any(|line| line == "## 大纲")
    {
        content
            .split_once("\n## 正文\n")
            .or_else(|| content.split_once("\n## 正文\r\n"))
            .map(|(_, body)| body.trim())
            .ok_or_else(|| AppError::Parse("评测归档缺少正文分隔符".into()))?
    } else {
        content
    };
    if body.is_empty() {
        return Err(AppError::Parse("待评测正文为空".into()));
    }
    Ok(body)
}

pub(crate) fn article_judge_input(
    style: &str,
    idea: &str,
    article: &str,
) -> Result<String, AppError> {
    Ok(format!(
        "### 1. 输入主题与素材\n{idea}\n\n### 2. 输入的风格规范 (Style Spec)\n{style}\n\n### 3. 待评测文章\n{}",
        article_body(article)?
    ))
}

fn parse_response<T: serde::de::DeserializeOwned>(raw: &str) -> Result<T, AppError> {
    let json = raw
        .find('{')
        .zip(raw.rfind('}'))
        .filter(|(s, e)| s <= e)
        .map(|(s, e)| &raw[s..=e])
        .unwrap_or(raw);
    serde_json::from_str(json).map_err(|e| AppError::Parse(format!("Judge JSON 无效: {e}")))
}

fn validate_scores(scores: &[f64]) -> Result<(), AppError> {
    if scores
        .iter()
        .any(|s| !s.is_finite() || !(1.0..=5.0).contains(s))
    {
        return Err(AppError::Parse("Judge 分数必须在 1–5 之间".into()));
    }
    Ok(())
}

pub(crate) fn parse_article_judgment(raw: &str) -> Result<JudgeResult, AppError> {
    let mut r: JudgeResult = parse_response(raw)?;
    validate_scores(&[
        r.scores.introduction,
        r.scores.argumentation,
        r.scores.rhythm,
        r.scores.persona,
        r.scores.vocabulary,
        r.scores.conclusion,
    ])?;
    r.overall_score = r.scores.mean();
    Ok(r)
}

pub(crate) fn parse_learn_judgment(raw: &str) -> Result<LearnJudgeResult, AppError> {
    let mut r: LearnJudgeResult = parse_response(raw)?;
    validate_scores(&[
        r.scores.coverage,
        r.scores.actionability,
        r.scores.conciseness,
        r.scores.distinctiveness,
        r.scores.cleanliness,
    ])?;
    r.overall_score = r.scores.mean();
    Ok(r)
}

/// Preserve the model estimate, then apply the shared deterministic length policy.
pub(crate) fn score_learn_judgment(raw: &str, spec: &str) -> Result<LearnJudgeResult, AppError> {
    let mut result = parse_learn_judgment(raw)?;
    let count = crate::spec_quality::character_count(spec);
    result.model_conciseness = Some(result.scores.conciseness);
    result.model_score_mean = Some(result.overall_score);
    result.characters_without_whitespace = Some(count);
    result.scores.conciseness = crate::spec_quality::conciseness_score(count);
    result.overall_score = result.scores.mean();
    Ok(result)
}

// ── eval 1：风格提示词（生成端）───────────────────────────────────────

/// LLM-Judge 对单篇文章的 6 维度评分
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JudgeScores {
    pub introduction: f64,
    pub argumentation: f64,
    pub rhythm: f64,
    pub persona: f64,
    pub vocabulary: f64,
    pub conclusion: f64,
}

impl JudgeScores {
    pub fn mean(&self) -> f64 {
        (self.introduction
            + self.argumentation
            + self.rhythm
            + self.persona
            + self.vocabulary
            + self.conclusion)
            / 6.0
    }
}

/// LLM-Judge 的完整评审结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JudgeResult {
    pub scores: JudgeScores,
    pub overall_score: f64,
    pub style_fidelity_analysis: String,
    pub humanness_quality_analysis: String,
    pub negative_hits: Vec<String>,
    #[serde(deserialize_with = "deserialize_string_or_vec")]
    pub improvement_suggestions: String,
}

fn deserialize_string_or_vec<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrVec {
        String(String),
        Vec(Vec<String>),
    }

    match StringOrVec::deserialize(deserializer)? {
        StringOrVec::String(s) => Ok(s),
        StringOrVec::Vec(v) => Ok(v.join("\n")),
    }
}

/// 调用 LLM-Judge 评测一篇文章（以 Style Spec 风格匹配度为核心准绳）
pub async fn judge_article(
    client: &Client,
    base_url: &str,
    api_key: &str,
    model: &str,
    style_spec: &str,
    idea: &str,
    article_content: &str,
) -> Result<JudgeResult, AppError> {
    let judge_prompt = std::fs::read_to_string("prompts/eval_judge_style.md")
        .map_err(|e| AppError::Intent(format!("读取 Judge 提示词失败: {}", e)))?;

    let user_input = article_judge_input(style_spec, idea, article_content)?;

    let raw_resp: String = with_retry(3, "Judge LLM", || {
        call_llm(client, base_url, api_key, model, &judge_prompt, &user_input)
    })
    .await?;

    parse_article_judgment(&raw_resp)
}

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
    let forbidden = [
        "众所周知",
        "总而言之",
        "值得注意的是",
        "毋庸置疑",
        "综上所述",
    ];
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

/// LLM-Judge B 对提取出的风格 Spec 的 5 维度评分
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LearnJudgeScores {
    pub coverage: f64,
    pub actionability: f64,
    pub conciseness: f64,
    pub distinctiveness: f64,
    pub cleanliness: f64,
}

impl LearnJudgeScores {
    pub fn mean(&self) -> f64 {
        (self.coverage
            + self.actionability
            + self.conciseness
            + self.distinctiveness
            + self.cleanliness)
            / 5.0
    }
}

/// LLM-Judge B 的完整评审结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearnJudgeResult {
    pub scores: LearnJudgeScores,
    pub overall_score: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_conciseness: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_score_mean: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub characters_without_whitespace: Option<usize>,
    pub structure_analysis: String,
    pub fidelity_analysis: String,
    pub negative_hits: Vec<String>,
    #[serde(deserialize_with = "deserialize_string_or_vec")]
    pub improvement_suggestions: String,
}

/// 调用 LLM-Judge B 评测提取出的 Spec 质量（Rubric B 5 维度）
pub async fn judge_learn_spec(
    client: &Client,
    base_url: &str,
    api_key: &str,
    model: &str,
    sample_article: &str,
    extracted_spec: &str,
) -> Result<LearnJudgeResult, AppError> {
    let judge_prompt = std::fs::read_to_string("prompts/eval_judge_learn.md")
        .map_err(|e| AppError::Intent(format!("读取 Learn Judge 提示词失败: {}", e)))?;

    let user_input = format!(
        "### 1. 输入原始样文\n{}\n\n### 2. 提取出的风格规范 (Spec)\n{}",
        sample_article, extracted_spec
    );

    let raw_resp: String = with_retry(3, "Learn Judge LLM", || {
        call_llm(client, base_url, api_key, model, &judge_prompt, &user_input)
    })
    .await?;

    score_learn_judgment(&raw_resp, extracted_spec)
}

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
    let spec_len = crate::spec_quality::character_count(&spec);
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
    println!("[指标] spec 非空白字符数: {} (目标 400–600)", spec_len);
    println!("[指标] 10 段覆盖率: {}/{}", covered, total);
    println!("[指标] 平均每段字数: {:.1}", avg_section_len);
    println!("--- spec ---\n{}", spec);

    Ok(())
}

// ── 冻结测试集 + 批量生成与提取 ────────────────────────────────────────

/// 冻结测试集：5 个跨领域主题（来源：Obsidian 2-notes）。
/// 改 prompt 时**不动这里**——测试集冻结，变量只有被评测的 prompt。
pub const TEST_SET: [&str; 5] = [
    "文章主题：复利的真相——早期靠本金，后期靠复利\n基本内容：复利公式三个变量（本金、收益率、期数）各有认知谬误；「每天进步1%」是用次方算加法的事；大部分收益来自本金而非利滚利；巴菲特百万赌局证明长期稳定高收益罕见。",
    "文章主题：幸存者偏差——你只看到活下来的，看不到死掉的\n基本内容：看到的成功样本是残酷淘汰后的幸存者；一万名基金经理里会有313人靠运气连续5年盈利；失败者也有和成功者同样的特质，真正区别只有运气；我们在签约那只打出伊利亚特的猴子。",
    "文章主题：刚需是自学的结果，不是自学的原因\n基本内容：你不会的东西对你感觉上就不是刚需；人脑用已有信息拼出自洽图像，把缺的技能排除在必要性之外；一旦学会就发现离不开它。",
    "文章主题：AI是效率倍增器，不是躺平工具\n基本内容：AI压扁中间层脑力工作、放大头部生产力；「AI会做所以我不学」等于把市场定价权交给别人的模型；正确姿势是让AI成为倍增器，拉高单位小时产出。",
    "文章主题：从消费者转向生产者\n基本内容：消费是被动接收价值，生产是主动创造价值；生产者思维带来复利（作品、资产、能力），消费者思维只消耗；离生产越近的人赚钱越多。",
];

pub use crate::eval_batch::{eval_learn_batch, eval_style_batch};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_archive_scores_only_final_body() {
        let archive = "# 主题 1：包装标题\n\n> 包装素材\n\n## 大纲\n\n包装大纲\n\n## 正文\n\n# 真标题\n正文\n## 正文\n文章自己的小节";
        let input = article_judge_input("规则", "主题", archive).unwrap();
        assert!(input.ends_with("# 真标题\n正文\n## 正文\n文章自己的小节"));
        assert!(!input.contains("包装"));
        let windows_archive = archive.replace('\n', "\r\n");
        assert!(
            !article_judge_input("规则", "主题", &windows_archive)
                .unwrap()
                .contains("包装")
        );
        assert!(article_body("# 主题 1：x\n## 大纲\nx").is_err());
        assert!(article_body(" ").is_err());
        assert_eq!(
            article_body("# 普通文章\n## 正文\nx").unwrap(),
            "# 普通文章\n## 正文\nx"
        );
    }

    #[test]
    fn scores_reject_out_of_range_and_recompute_mean() {
        let mut v = serde_json::json!({"scores": {"coverage":5,"actionability":4,"conciseness":3,
            "distinctiveness":2,"cleanliness":1}, "overall_score":5,
            "structure_analysis":"s", "fidelity_analysis":"f", "negative_hits":[],
            "improvement_suggestions":["a","b"]});
        let r = parse_learn_judgment(&format!("```json\n{v}\n```")).unwrap();
        assert_eq!(r.overall_score, 3.0);
        assert_eq!(r.improvement_suggestions, "a\nb");
        v["scores"]["coverage"] = serde_json::json!(6);
        assert!(parse_learn_judgment(&v.to_string()).is_err());
        assert!(parse_learn_judgment("not json").is_err());
    }

    macro_rules! suite {
        ($name:ident, $mode:literal) => {
            #[test]
            #[ignore = "真实模型评测；需 API_KEY 和网络；设置 EVAL_RUN_ID 续跑"]
            fn $name() {
                tokio::runtime::Runtime::new()
                    .unwrap()
                    .block_on(crate::eval_runs::run($mode))
                    .unwrap();
            }
        };
    }
    suite!(eval_automated_suite, "all");
    suite!(eval_judge_baseline_suite, "baseline_a");
    suite!(eval_judge_learn_baseline_suite, "baseline_b");
    suite!(eval_style_compare_suite, "compare_a");
    suite!(eval_learn_compare_suite, "compare_b");
    suite!(eval_all_specs_on_eval1_suite, "cross");
}
