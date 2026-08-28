//! 两套 eval：风格提示词（生成端）+ learn 提取提示词（提取端）。
//!
//! 手动运行（需 API_KEY + 网络）：
//!   cargo test --lib -- --ignored eval --nocapture

use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::generate::{generate_outline, render_fulltext};
use crate::llm::{call_llm, with_retry};

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
        (self.introduction + self.argumentation + self.rhythm + self.persona + self.vocabulary + self.conclusion) / 6.0
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

    let user_input = format!(
        "### 1. 输入主题与素材\n{}\n\n### 2. 输入的风格规范 (Style Spec)\n{}\n\n### 3. 待评测文章\n{}",
        idea, style_spec, article_content
    );

    let raw_resp: String = with_retry(3, "Judge LLM", || {
        call_llm(client, base_url, api_key, model, &judge_prompt, &user_input)
    }).await?;

    // 解析 JSON（容忍 markdown code block 包裹）
    let json_str = if let Some(start) = raw_resp.find('{') {
        if let Some(end) = raw_resp.rfind('}') {
            &raw_resp[start..=end]
        } else {
            &raw_resp
        }
    } else {
        &raw_resp
    };

    let result: JudgeResult = serde_json::from_str(json_str).map_err(|e| {
        AppError::Parse(format!("Judge 响应 JSON 解析失败: {}\n原始返回:\n{}", e, raw_resp))
    })?;

    Ok(result)
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
        (self.coverage + self.actionability + self.conciseness + self.distinctiveness + self.cleanliness) / 5.0
    }
}

/// LLM-Judge B 的完整评审结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearnJudgeResult {
    pub scores: LearnJudgeScores,
    pub overall_score: f64,
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
    }).await?;

    let json_str = if let Some(start) = raw_resp.find('{') {
        if let Some(end) = raw_resp.rfind('}') {
            &raw_resp[start..=end]
        } else {
            &raw_resp
        }
    } else {
        &raw_resp
    };

    let result: LearnJudgeResult = serde_json::from_str(json_str).map_err(|e| {
        AppError::Parse(format!("Learn Judge 响应 JSON 解析失败: {}\n原始返回:\n{}", e, raw_resp))
    })?;

    Ok(result)
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

/// 跑整批测试集，生成每篇的大纲+正文，存到 target_dir 作为基准或候选文章。
pub async fn eval_style_batch(
    client: &Client,
    base_url: &str,
    api_key: &str,
    model: &str,
    style: &str,
    target_dir: &str,
) -> Result<(), AppError> {
    std::fs::create_dir_all(target_dir)
        .map_err(|e| AppError::Intent(format!("创建目录失败: {}", e)))?;

    for (i, idea) in TEST_SET.iter().enumerate() {
        let n = i + 1;
        let outline = generate_outline(client, base_url, api_key, model, style, idea).await?;
        let fulltext = render_fulltext(client, base_url, api_key, model, style, &outline, idea).await?;

        let quoted_idea: String = idea.split('\n').map(|l| format!("> {}", l)).collect::<Vec<_>>().join("\n");
        let content = format!(
            "# 主题 {}：{}\n\n{}\n\n## 大纲\n\n{}\n\n## 正文\n\n{}",
            n,
            idea.lines().next().unwrap_or("").trim_start_matches("文章主题："),
            quoted_idea,
            outline,
            fulltext,
        );
        let path = format!("{}/topic_{:02}.md", target_dir, n);
        std::fs::write(&path, content)
            .map_err(|e| AppError::Intent(format!("写文件失败: {}", e)))?;
        println!(
            "[{}] 主题 {} 已保存 → {} (正文 {} 字符)",
            target_dir,
            n,
            path,
            fulltext.chars().count()
        );
    }

    Ok(())
}

/// 跑整批样文测试集，提取 10 段式 Spec 并存到 target_dir。
pub async fn eval_learn_batch(
    client: &Client,
    base_url: &str,
    api_key: &str,
    model: &str,
    target_dir: &str,
) -> Result<(), AppError> {
    std::fs::create_dir_all(target_dir)
        .map_err(|e| AppError::Intent(format!("创建目录失败: {}", e)))?;

    let prompt = std::fs::read_to_string("prompts/learn_style.md")
        .map_err(|e| AppError::Intent(format!("读取提取提示词失败: {}", e)))?;

    for n in 1..=5 {
        let sample_path = format!("eval/samples/sample_{:02}.md", n);
        let sample_text = std::fs::read_to_string(&sample_path)
            .map_err(|e| AppError::Intent(format!("读取样文失败: {}", e)))?;

        let spec: String = with_retry(3, "Learn Spec LLM", || {
            call_llm(client, base_url, api_key, model, &prompt, &sample_text)
        }).await?;
        let out_path = format!("{}/spec_{:02}.md", target_dir, n);
        std::fs::write(&out_path, &spec)
            .map_err(|e| AppError::Intent(format!("写 Spec 失败: {}", e)))?;

        println!(
            "[{}] 样文 {} 已提取 → {} ({} 字符)",
            target_dir,
            n,
            out_path,
            spec.chars().count()
        );
    }

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
            let style = std::fs::read_to_string("styles/蔡叔.md").expect("读取风格失败");
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

    #[test]
    #[ignore = "真实 LLM 调用，需 API_KEY + 网络，跑整批冻结测试集生成 baseline"]
    fn eval_style_batch_suite() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (base, key, model, client) = env();
            let style = std::fs::read_to_string("styles/蔡叔.md").expect("读取风格失败");
            eval_style_batch(&client, &base, &key, &model, &style, "eval/baseline")
                .await
                .expect("批量生成 baseline 失败");
        });
    }

    #[test]
    #[ignore = "真实 LLM 调用，需 API_KEY + 网络，用 LLM-Judge 评测已落盘的 baseline 文章"]
    fn eval_judge_baseline_suite() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (base, key, model, client) = env();
            let style = std::fs::read_to_string("styles/蔡叔.md").expect("读取风格失败");

            println!("\n===== 开始运行 LLM-Judge Baseline 校准评测 =====");
            for (i, idea) in TEST_SET.iter().enumerate() {
                let n = i + 1;
                let path = format!("eval/baseline/topic_{:02}.md", n);
                let content = std::fs::read_to_string(&path)
                    .unwrap_or_else(|_| panic!("读取 baseline 文件失败: {}", path));

                println!("\n>>> 评测 主题 {} (来源: {}) ...", n, path);
                let result = judge_article(&client, &base, &key, &model, &style, idea, &content)
                    .await
                    .expect("Judge 评测失败");

                println!("得分: 破题={:.1}, 论证={:.1}, 节奏={:.1}, 人称={:.1}, 用词={:.1}, 结尾={:.1} | 均分={:.2}",
                    result.scores.introduction,
                    result.scores.argumentation,
                    result.scores.rhythm,
                    result.scores.persona,
                    result.scores.vocabulary,
                    result.scores.conclusion,
                    result.overall_score
                );
                println!("[风格贴合分析] {}", result.style_fidelity_analysis);
                println!("[活人感/质量分析] {}", result.humanness_quality_analysis);
                if !result.negative_hits.is_empty() {
                    println!("[命中禁令] {:?}", result.negative_hits);
                }
                println!("[改进建议] {}", result.improvement_suggestions);
            }
        });
    }

    #[test]
    #[ignore = "真实 LLM 调用，需 API_KEY + 网络，生成新候选文章并与 Baseline 进行自动对比与胜率分析"]
    fn eval_style_compare_suite() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (base, key, model, client) = env();
            let style = std::fs::read_to_string("styles/蔡叔.md").expect("读取风格失败");

            println!("\n===== 开始批量生成新版本候选文章 (eval/candidate/) =====");
            eval_style_batch(&client, &base, &key, &model, &style, "eval/candidate")
                .await
                .expect("生成候选文章失败");

            println!("\n===== 开始运行 LLM-Judge 对比评测 (Candidate vs Baseline) =====");
            let mut wins = 0;
            let mut ties = 0;
            let mut losses = 0;
            let mut total_delta = 0.0;

            for (i, idea) in TEST_SET.iter().enumerate() {
                let n = i + 1;
                let base_path = format!("eval/baseline/topic_{:02}.md", n);
                let cand_path = format!("eval/candidate/topic_{:02}.md", n);

                let base_content = std::fs::read_to_string(&base_path).expect("读取 baseline 失败");
                let cand_content = std::fs::read_to_string(&cand_path).expect("读取 candidate 失败");

                let base_judge = judge_article(&client, &base, &key, &model, &style, idea, &base_content)
                    .await
                    .expect("评测 baseline 失败");
                let cand_judge = judge_article(&client, &base, &key, &model, &style, idea, &cand_content)
                    .await
                    .expect("评测 candidate 失败");

                let delta = cand_judge.scores.mean() - base_judge.scores.mean();
                total_delta += delta;

                let verdict = if delta > 0.05 {
                    wins += 1;
                    "WIN 🟢"
                } else if delta < -0.05 {
                    losses += 1;
                    "LOSS 🔴"
                } else {
                    ties += 1;
                    "TIE ⚪"
                };

                println!("\n┌─────────────────────────────────────────────────────────────┐");
                println!("│ 主题 {:02} 对比: {:<40} [{}] │", n, idea.lines().next().unwrap_or("").trim_start_matches("文章主题："), verdict);
                println!("├──────────────┬────────────┬────────────┬────────────────────┤");
                println!("│ 评测维度     │ Baseline   │ Candidate  │ 差值 (Delta)       │");
                println!("├──────────────┼────────────┼────────────┼────────────────────┤");
                println!("│ 破题与切入   │ {:<10.1} │ {:<10.1} │ {:+<18.1} │", base_judge.scores.introduction, cand_judge.scores.introduction, cand_judge.scores.introduction - base_judge.scores.introduction);
                println!("│ 论证与洞察   │ {:<10.1} │ {:<10.1} │ {:+<18.1} │", base_judge.scores.argumentation, cand_judge.scores.argumentation, cand_judge.scores.argumentation - base_judge.scores.argumentation);
                println!("│ 节奏与句式   │ {:<10.1} │ {:<10.1} │ {:+<18.1} │", base_judge.scores.rhythm, cand_judge.scores.rhythm, cand_judge.scores.rhythm - base_judge.scores.rhythm);
                println!("│ 人称与视角   │ {:<10.1} │ {:<10.1} │ {:+<18.1} │", base_judge.scores.persona, cand_judge.scores.persona, cand_judge.scores.persona - base_judge.scores.persona);
                println!("│ 用词与去AI味 │ {:<10.1} │ {:<10.1} │ {:+<18.1} │", base_judge.scores.vocabulary, cand_judge.scores.vocabulary, cand_judge.scores.vocabulary - base_judge.scores.vocabulary);
                println!("│ 结尾收束     │ {:<10.1} │ {:<10.1} │ {:+<18.1} │", base_judge.scores.conclusion, cand_judge.scores.conclusion, cand_judge.scores.conclusion - base_judge.scores.conclusion);
                println!("├──────────────┼────────────┼────────────┼────────────────────┤");
                println!("│ 综合均分     │ {:<10.2} │ {:<10.2} │ {:+<18.2} │", base_judge.scores.mean(), cand_judge.scores.mean(), delta);
                println!("└──────────────┴────────────┴────────────┴────────────────────┘");
                println!("[Candidate 分析] 风格贴合: {}", cand_judge.style_fidelity_analysis);
                println!("[Candidate 分析] 活人质感: {}", cand_judge.humanness_quality_analysis);
                if !cand_judge.negative_hits.is_empty() {
                    println!("[Candidate 禁令] {:?}", cand_judge.negative_hits);
                }
            }

            let avg_delta = total_delta / TEST_SET.len() as f64;
            let win_rate = (wins as f64 / TEST_SET.len() as f64) * 100.0;
            println!("\n=================== 对比与胜率总结 ===================");
            println!("测试主题总数: {}", TEST_SET.len());
            println!("胜场 (Win):   {} ({:.1}%)", wins, win_rate);
            println!("平局 (Tie):   {}", ties);
            println!("败场 (Loss):  {}", losses);
            println!("平均分差 (Avg Δ): {:+.2}", avg_delta);
            println!("======================================================");
        });
    }

    #[test]
    #[ignore = "真实 LLM 调用，需 API_KEY + 网络，批量提取 sample 样文的 Baseline Spec"]
    fn eval_learn_batch_suite() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (base, key, model, client) = env();
            println!("\n===== 开始批量提取样文 Baseline Spec (eval/baseline_specs/) =====");
            eval_learn_batch(&client, &base, &key, &model, "eval/baseline_specs")
                .await
                .expect("批量提取 Baseline Spec 失败");
        });
    }

    #[test]
    #[ignore = "真实 LLM 调用，需 API_KEY + 网络，使用 LLM-Judge B (Rubric B) 评测提取出的 Baseline Spec"]
    fn eval_judge_learn_baseline_suite() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (base, key, model, client) = env();
            println!("\n===== 开始运行 LLM-Judge B (Rubric B) Spec 评测 =====");
            let mut total_score = 0.0;

            for n in 1..=5 {
                let sample_path = format!("eval/samples/sample_{:02}.md", n);
                let spec_path = format!("eval/baseline_specs/spec_{:02}.md", n);

                let sample_text = std::fs::read_to_string(&sample_path)
                    .unwrap_or_else(|_| panic!("读取样文失败: {}", sample_path));
                let spec_text = std::fs::read_to_string(&spec_path)
                    .unwrap_or_else(|_| panic!("读取 Spec 失败: {}", spec_path));

                println!("\n>>> 评测 Spec {:02} (来源样文: {}) ...", n, sample_path);
                let result = judge_learn_spec(&client, &base, &key, &model, &sample_text, &spec_text)
                    .await
                    .expect("Judge B 评测失败");

                let mean = result.scores.mean();
                total_score += mean;
                println!("得分: 结构覆盖={:.1}, 可执行性={:.1}, 精简度={:.1}, 特征捕捉={:.1}, 去样文污染={:.1} | 均分={:.2}",
                    result.scores.coverage,
                    result.scores.actionability,
                    result.scores.conciseness,
                    result.scores.distinctiveness,
                    result.scores.cleanliness,
                    mean
                );
                println!("[结构分析] {}", result.structure_analysis);
                println!("[特征分析] {}", result.fidelity_analysis);
                if !result.negative_hits.is_empty() {
                    println!("[命中问题] {:?}", result.negative_hits);
                }
                println!("[改进建议] {}", result.improvement_suggestions);
            }

            println!("\n================ 5 篇 Spec 评测总结 ================");
            println!("平均质量分: {:.2} / 5.0", total_score / 5.0);
            println!("====================================================");
        });
    }

    #[test]
    #[ignore = "真实 LLM 调用，需 API_KEY + 网络，对 5 个 Baseline Spec 分别生成 5 个主题文章并运行 Eval 1 评测"]
    fn eval_all_specs_on_eval1_suite() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (base, key, model, client) = env();
            println!("\n===== 开始对 5 个 Baseline Spec 进行全量 Eval 1 (生成 + Rubric A 评测) =====");

            for spec_idx in 1..=5 {
                let spec_path = format!("eval/baseline_specs/spec_{:02}.md", spec_idx);
                let spec_content = match std::fs::read_to_string(&spec_path) {
                    Ok(c) => c,
                    Err(e) => {
                        println!("跳过 Spec {:02}: 读取失败 ({})", spec_idx, e);
                        continue;
                    }
                };

                let target_dir = format!("eval/spec_runs/spec_{:02}", spec_idx);
                println!("\n-------------------------------------------------------------");
                println!(">>> 正在生成 Spec {:02} 的 5 篇主题文章 (目标目录: {}) ...", spec_idx, target_dir);
                eval_style_batch(&client, &base, &key, &model, &spec_content, &target_dir)
                    .await
                    .expect("生成文章批次失败");

                println!(">>> 正在评测 Spec {:02} 生成的文章 (Rubric A) ...", spec_idx);
                let mut total_score = 0.0;

                for (i, idea) in TEST_SET.iter().enumerate() {
                    let n = i + 1;
                    let article_path = format!("{}/topic_{:02}.md", target_dir, n);
                    let article_content = std::fs::read_to_string(&article_path).expect("读取生成文章失败");

                    let result = judge_article(&client, &base, &key, &model, &spec_content, idea, &article_content)
                        .await
                        .expect("LLM-Judge A 评测文章失败");

                    let mean = result.scores.mean();
                    total_score += mean;

                    println!(
                        "  [Spec {:02} | Topic {:02}] 均分: {:.2} (破题={:.1}, 论证={:.1}, 节奏={:.1}, 人称={:.1}, 用词={:.1}, 结尾={:.1})",
                        spec_idx, n, mean,
                        result.scores.introduction,
                        result.scores.argumentation,
                        result.scores.rhythm,
                        result.scores.persona,
                        result.scores.vocabulary,
                        result.scores.conclusion,
                    );
                }

                let avg = total_score / TEST_SET.len() as f64;
                println!(">>> Spec {:02} 综合 Eval 1 平均得分: {:.2} / 5.00", spec_idx, avg);
            }
            println!("\n================ 全量 Spec Eval 1 评测完成 ================");
        });
    }
}
