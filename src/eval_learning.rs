//! Matched current-model comparison: frozen single-stage prompt versus evidence + compilation.
use crate::error::AppError;
use crate::eval::{LearnJudgeResult, parse_learn_judgment, score_learn_judgment};
use crate::eval_store::{Store, write_json};
use crate::generate::{outline_system_prompt, render_prompt, style_system_prompt};
use crate::style_learning::{LearningPrompts, extract_with};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const TOPIC: &str = "写一篇600–900字的中文文章，题材是社区图书馆如何让不同读者都方便借书。素材：图书馆增设自助借书机；一位读者很快完成借阅，一位不熟悉机器的老年读者停在屏幕前；工作人员走过去协助；人工服务窗口仍保留。可讨论效率与可达性的关系，也可用这些场景组织叙事。允许按所给场景采用观察者第一人称，不添加人物履历、具体统计、外部引用或未提供的事件。不复述参考样文的素材和观点。";

struct Inputs {
    old: String,
    direct: Option<String>,
    new: LearningPrompts,
    judge: String,
    transfer: String,
    samples: Vec<String>,
}
#[derive(Debug, Serialize, Deserialize)]
struct Transfer {
    style_fidelity: f64,
    task_fulfillment: f64,
    cleanliness: f64,
    naturalness: f64,
    reasons: Vec<String>,
}
impl Transfer {
    fn mean(&self) -> f64 {
        (self.style_fidelity + self.task_fulfillment + self.cleanliness + self.naturalness) / 4.0
    }
}
#[derive(Serialize)]
struct Row {
    case: usize,
    variant: String,
    spec: String,
    body: String,
    extraction_calls: usize,
    extraction_seconds: f64,
    warnings: Vec<String>,
    judgment: LearnJudgeResult,
    checks: Value,
    transfer: Transfer,
}
fn text_valid(s: &str) -> Result<(), AppError> {
    if s.trim().is_empty() {
        Err(AppError::Parse("模型输出为空".into()))
    } else {
        Ok(())
    }
}
fn parse_transfer(raw: &str) -> Result<Transfer, AppError> {
    let clean = raw
        .trim()
        .strip_prefix("```json")
        .and_then(|s| s.trim().strip_suffix("```"))
        .unwrap_or(raw)
        .trim();
    let result: Transfer = serde_json::from_str(clean)
        .map_err(|e| AppError::Parse(format!("迁移评分 JSON 无效: {e}")))?;
    if [
        result.style_fidelity,
        result.task_fulfillment,
        result.cleanliness,
        result.naturalness,
    ]
    .iter()
    .any(|v| !v.is_finite() || !(1.0..=5.0).contains(v))
        || result.reasons.is_empty()
    {
        return Err(AppError::Parse("迁移评分必须在1–5且有理由".into()));
    }
    Ok(result)
}

async fn variant(store: &Store, input: &Inputs, case: usize, new: bool) -> Result<Row, AppError> {
    let variant = if new {
        if input.direct.is_some() {
            "direct_free"
        } else {
            "two_stage"
        }
    } else {
        "single_stage"
    };
    let name = format!("s{case:02}_{variant}");
    let source = &input.samples[case - 1];
    let start = std::time::Instant::now();
    let (spec, calls, warnings) = if new {
        let call = |stage, system: String, user: String| {
            let task = format!("{name}_{stage}");
            async move { store.call(&task, &system, &user, text_valid).await }
        };
        let learned = if let Some(prompt) = &input.direct {
            crate::style_learning::extract_direct_with(source, prompt, call).await?
        } else {
            extract_with(source, &input.new, call).await?
        };
        write_json(&store.root.join(format!("{name}_learning.json")), &learned)?;
        (learned.spec, learned.calls, learned.warnings)
    } else {
        (
            store
                .call(&format!("{name}_extract"), &input.old, source, text_valid)
                .await?,
            1,
            vec![],
        )
    };
    // Persist extraction latency once. Cache-only reruns must not replace it with milliseconds.
    let timing_path = store.root.join(format!("{name}_timing.json"));
    let seconds = match std::fs::read_to_string(&timing_path) {
        Ok(raw) => serde_json::from_str::<f64>(&raw).map_err(|e| AppError::Parse(e.to_string()))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let secs = start.elapsed().as_secs_f64();
            write_json(&timing_path, &secs)?;
            secs
        }
        Err(e) => return Err(e.into()),
    };
    std::fs::write(store.root.join(format!("{name}_spec.md")), &spec)?;
    let user = format!("原始样文：\n{source}\n\n待评风格指令：\n{spec}");
    let raw = store
        .call(&format!("{name}_judge"), &input.judge, &user, |s| {
            parse_learn_judgment(s).map(|_| ())
        })
        .await?;
    let judgment = score_learn_judgment(&raw, &spec)?;
    let outline = store
        .call(
            &format!("{name}_outline"),
            &outline_system_prompt(&spec),
            TOPIC,
            text_valid,
        )
        .await?;
    let body = store
        .call(
            &format!("{name}_body"),
            &style_system_prompt(&spec),
            &render_prompt(&outline, TOPIC),
            text_valid,
        )
        .await?;
    std::fs::write(store.root.join(format!("{name}_body.md")), &body)?;
    let raw = store
        .call(
            &format!("{name}_transfer"),
            &input.transfer,
            &format!("参考样文：\n{source}\n\n新任务：\n{TOPIC}\n\n待评正文：\n{body}"),
            |s| parse_transfer(s).map(|_| ()),
        )
        .await?;
    Ok(Row {
        case,
        variant: variant.into(),
        checks: crate::eval_runs::spec_checks(source, &spec),
        spec: format!("{name}_spec.md"),
        body: format!("{name}_body.md"),
        extraction_calls: calls,
        extraction_seconds: seconds,
        warnings,
        judgment,
        transfer: parse_transfer(&raw)?,
    })
}
fn semantic(row: &Row) -> f64 {
    let s = &row.judgment.scores;
    (s.coverage + s.actionability + s.distinctiveness + s.cleanliness) / 4.0
}
fn decision(rows: &[Row]) -> Value {
    let avg = |new: bool, f: fn(&Row) -> f64| {
        let group: Vec<_> = rows
            .iter()
            .filter(|r| (r.variant != "single_stage") == new)
            .collect();
        group.iter().map(|r| f(r)).sum::<f64>() / group.len() as f64
    };
    let old_sem = avg(false, semantic);
    let new_sem = avg(true, semantic);
    let old_clean = avg(false, |r| r.judgment.scores.cleanliness);
    let new_clean = avg(true, |r| r.judgment.scores.cleanliness);
    let old_transfer = avg(false, |r| r.transfer.mean());
    let new_transfer = avg(true, |r| r.transfer.mean());
    let severe = rows
        .iter()
        .filter(|r| r.variant != "single_stage")
        .any(|new| {
            let old = rows
                .iter()
                .find(|r| r.case == new.case && r.variant == "single_stage")
                .unwrap();
            (new.transfer.cleanliness <= 2.0 && new.transfer.cleanliness < old.transfer.cleanliness)
                || (new.judgment.scores.cleanliness <= 2.0
                    && new.judgment.scores.cleanliness < old.judgment.scores.cleanliness)
        });
    json!({"adopt_candidate":new_sem >= old_sem && new_clean >= old_clean && new_transfer >= old_transfer && !severe,
        "semantic_mean":{"single_stage":old_sem,"candidate":new_sem},
        "spec_cleanliness":{"single_stage":old_clean,"candidate":new_clean},
        "transfer_mean":{"single_stage":old_transfer,"candidate":new_transfer},
        "transfer_style_fidelity":{"single_stage":avg(false, |r|r.transfer.style_fidelity),"candidate":avg(true, |r|r.transfer.style_fidelity)},
        "transfer_task_fulfillment":{"single_stage":avg(false, |r|r.transfer.task_fulfillment),"candidate":avg(true, |r|r.transfer.task_fulfillment)},
        "transfer_cleanliness":{"single_stage":avg(false, |r|r.transfer.cleanliness),"candidate":avg(true, |r|r.transfer.cleanliness)},
        "new_severe_contamination":severe})
}
fn report(rows: &[Row], model: &str, verdict: &Value) -> String {
    let mut md = format!(
        "# 单篇风格学习对照\n\n模型：{model}；Rubric B v2。候选按本轮提示词在当前模型生成；未变化的旧请求可从同模型前轮精确复用，样文/主题/生成包装/Judge一致。每项一次采样，五种风格各迁移到同一个图书馆主题；不足以证明多主题泛化。模型兼任生成与评审，分数无人评背书。\n\n| 样本 | 方法 | 字数 | Spec均分 | 非长度均分 | 去污染 | 换题均分 | 提取请求数 | 提取耗时秒 |\n|---|---|---:|---:|---:|---:|---:|---:|---:|\n"
    );
    for r in rows {
        md.push_str(&format!(
            "| {} | [{}]({}) | {} | {:.2} | {:.2} | {:.1} | [{:.2}]({}) | {} | {:.1} |\n",
            r.case,
            r.variant,
            r.spec,
            r.judgment.characters_without_whitespace.unwrap(),
            r.judgment.overall_score,
            semantic(r),
            r.judgment.scores.cleanliness,
            r.transfer.mean(),
            r.body,
            r.extraction_calls,
            r.extraction_seconds
        ));
    }
    md.push_str(&format!("\n预先约定的采用条件计算：\n```json\n{}\n```\n\n提取耗时含并发排队与运输重试；中断后续跑可能仅反映完成该链的运行片段，不作为严格性能基准。请求数指提取逻辑请求，运输重试另查日志。字数分档未变，coverage语义已变，不与旧人评算误差。阶段JSON为请求响应检查点，完整成功以summary.status为准；语义是否可靠仍需查看证据和诊断。\n", serde_json::to_string_pretty(verdict).unwrap()));
    md
}

pub async fn run() -> Result<(), AppError> {
    let read = |path: &str| std::fs::read_to_string(path).map_err(AppError::from);
    let input = Inputs {
        old: read("eval/fixtures/learn_style_single_v1.md")?,
        direct: match std::env::var("EVAL_LEARNING_METHOD").as_deref() {
            Ok("direct_free") => Some(read("prompts/learn_style_direct.md")?),
            Ok("two_stage") | Err(_) => None,
            _ => {
                return Err(AppError::EnvVar(
                    "EVAL_LEARNING_METHOD须为direct_free或two_stage".into(),
                ));
            }
        },
        new: LearningPrompts::load()?,
        judge: read("prompts/eval_judge_learn.md")?,
        transfer: read("prompts/eval_judge_transfer.md")?,
        samples: (1..=5)
            .map(|i| read(&format!("eval/samples/sample_{i:02}.md")))
            .collect::<Result<_, _>>()?,
    };
    let store = Store::from_env()?;
    let mut frozen = json!({"schema":1,"suite":"single_article_learning_v2","model":store.model,
        "old_prompt":input.old,"new_prompts":input.new,"judge":input.judge,"transfer_judge":input.transfer,
        "samples":input.samples,"topic":TOPIC,"outline_template":outline_system_prompt("{style}"),
        "body_template":style_system_prompt("{style}"),"render_template":render_prompt("{outline}","{idea}"),
        "length_policy":"non-whitespace Unicode scalars; 400-600=5; 300-399/601-700=4; 200-299/701-850=3; 100-199/851-1000=2; otherwise=1"});
    if let Some(prompt) = &input.direct {
        frozen["direct_prompt"] = json!(prompt);
    }
    store.freeze_inputs(&frozen)?;
    let case = |i| {
        let input = &input;
        let store = &store;
        async move {
            // Alternating order reduces systematic order effects without revealing variants to judges.
            if i % 2 == 0 {
                let n = variant(store, input, i, true).await?;
                let o = variant(store, input, i, false).await?;
                Ok::<_, AppError>(vec![o, n])
            } else {
                let o = variant(store, input, i, false).await?;
                let n = variant(store, input, i, true).await?;
                Ok(vec![o, n])
            }
        }
    };
    let (a, b, c, d, e) = tokio::join!(case(1), case(2), case(3), case(4), case(5));
    match [a, b, c, d, e].into_iter().collect::<Result<Vec<_>, _>>() {
        Ok(groups) => {
            let rows: Vec<_> = groups.into_iter().flatten().collect();
            let verdict = decision(&rows);
            write_json(
                &store.root.join("summary.json"),
                &json!({"status":"complete","at":chrono::Utc::now().to_rfc3339(),
                "model":store.model,"rubric":"B-v2","human_reviewed":false,"decision":verdict,"rows":rows}),
            )?;
            std::fs::write(
                store.root.join("summary.md"),
                report(&rows, &store.model, &verdict),
            )?;
            println!("[complete] {} {}", store.root.display(), verdict);
            Ok(())
        }
        Err(e) => {
            write_json(
                &store.root.join("failure-learning.json"),
                &json!({"status":"failed","at":chrono::Utc::now().to_rfc3339(),"error":e.to_string()}),
            )?;
            Err(e)
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transfer_rejects_out_of_range_and_missing_diagnostics() {
        let raw = json!({"style_fidelity":5,"task_fulfillment":4,"cleanliness":5,"naturalness":4,"reasons":["有依据"]});
        assert_eq!(parse_transfer(&raw.to_string()).unwrap().mean(), 4.5);
        let mut bad = raw.clone();
        bad["cleanliness"] = json!(6);
        assert!(parse_transfer(&bad.to_string()).is_err());
        bad = raw;
        bad["reasons"] = json!([]);
        assert!(parse_transfer(&bad.to_string()).is_err());
    }
    #[test]
    fn adoption_requires_no_semantic_or_transfer_regression_or_new_severe_pollution() {
        fn row(new: bool, score: f64) -> Row {
            let raw = json!({"scores":{"coverage":score,"actionability":score,"conciseness":5,
                "distinctiveness":score,"cleanliness":score},"overall_score":score,
                "structure_analysis":"diagnostic","fidelity_analysis":"diagnostic",
                "negative_hits":[],"improvement_suggestions":"diagnostic"})
            .to_string();
            Row {
                case: 1,
                variant: if new { "two_stage" } else { "single_stage" }.into(),
                spec: String::new(),
                body: String::new(),
                extraction_calls: 2,
                extraction_seconds: 1.0,
                warnings: vec![],
                judgment: parse_learn_judgment(&raw).unwrap(),
                checks: json!({}),
                transfer: Transfer {
                    style_fidelity: score,
                    task_fulfillment: score,
                    cleanliness: score,
                    naturalness: score,
                    reasons: vec!["reason".into()],
                },
            }
        }
        assert_eq!(
            decision(&[row(false, 4.0), row(true, 5.0)])["adopt_candidate"],
            true
        );
        assert_eq!(
            decision(&[row(false, 4.0), row(true, 3.0)])["adopt_candidate"],
            false
        );
        let mut candidate = row(true, 5.0);
        candidate.transfer.cleanliness = 2.0;
        assert_eq!(
            decision(&[row(false, 4.0), candidate])["adopt_candidate"],
            false
        );
    }

    #[test]
    #[ignore = "当前模型单篇新旧对照；真实API；EVAL_RUN_ID支持续跑"]
    fn eval_single_article_learning_suite() {
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(run())
            .unwrap();
    }
}
