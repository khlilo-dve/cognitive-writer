//! Three reviewed pairs, a revised extractor, and a judge checked against human scores.
use serde::Serialize;
use serde_json::{Value, json};

use crate::error::AppError;
use crate::eval::{LearnJudgeResult, parse_learn_judgment, score_learn_judgment};
use crate::eval_runs::spec_checks;
use crate::eval_store::{Store, write_json};

const HISTORY: &str = "eval/runs/20260905-auto-v1";
const DIMENSIONS: [&str; 5] = [
    "coverage",
    "actionability",
    "conciseness",
    "distinctiveness",
    "cleanliness",
];
const NON_LENGTH: [usize; 4] = [0, 1, 3, 4];

struct Case {
    name: String,
    sample: String,
    baseline: String,
    candidate: String,
    human: [[f64; 5]; 2],
    historical: [LearnJudgeResult; 2],
}

#[derive(Serialize)]
struct Row {
    case: String,
    variant: String,
    artifact: String,
    judgment: LearnJudgeResult,
    mechanical_checks: Value,
    human_raw_scores: Option<[f64; 5]>,
    human_mean_with_deterministic_length: Option<f64>,
    historical_judgment: Option<LearnJudgeResult>,
    non_length_signed_delta: Option<[f64; 4]>,
    historical_non_length_signed_delta: Option<[f64; 4]>,
}

fn read(path: &str) -> Result<String, AppError> {
    std::fs::read_to_string(path).map_err(|e| AppError::FileRead(format!("{path}: {e}")))
}

fn read_json(path: &str) -> Result<Value, AppError> {
    serde_json::from_str(&read(path)?).map_err(|e| AppError::Parse(format!("{path}: {e}")))
}

fn human_scores(value: &Value) -> Result<[f64; 5], AppError> {
    let scores: [f64; 5] = serde_json::from_value(value.clone())
        .map_err(|_| AppError::Parse("人工评分必须恰好包含五个数值".into()))?;
    if scores
        .iter()
        .any(|s| !s.is_finite() || !(1.0..=5.0).contains(s))
    {
        return Err(AppError::Parse("人工评分须在1–5之间".into()));
    }
    Ok(scores)
}

fn scores(r: &LearnJudgeResult) -> [f64; 5] {
    [
        r.scores.coverage,
        r.scores.actionability,
        r.scores.conciseness,
        r.scores.distinctiveness,
        r.scores.cleanliness,
    ]
}

fn delta(model: [f64; 5], human: [f64; 5]) -> [f64; 4] {
    NON_LENGTH.map(|i| model[i] - human[i])
}

fn load_cases(human: &Value) -> Result<Vec<Case>, AppError> {
    if human["dimensions"] != json!(DIMENSIONS) {
        return Err(AppError::Parse("人工评分维度顺序不符".into()));
    }
    (1..=3)
        .map(|n| {
            let name = format!("spec_{n:02}");
            let history = |prefix: &str| -> Result<LearnJudgeResult, AppError> {
                let suffix = if prefix == "candidate_b" {
                    "_judge"
                } else {
                    ""
                };
                let checkpoint = read_json(&format!("{HISTORY}/{prefix}_{n:02}{suffix}.json"))?;
                parse_learn_judgment(
                    checkpoint["response"]
                        .as_str()
                        .ok_or_else(|| AppError::Parse("历史检查点缺少response".into()))?,
                )
            };
            Ok(Case {
                sample: read(&format!("eval/samples/sample_{n:02}.md"))?,
                baseline: read(&format!("eval/baseline_specs/spec_{n:02}.md"))?,
                candidate: read(&format!("{HISTORY}/candidate_b_{n:02}.md"))?,
                human: [
                    human_scores(&human["raw_scores"][&name]["baseline"])?,
                    human_scores(&human["raw_scores"][&name]["candidate"])?,
                ],
                historical: [history("baseline_b")?, history("candidate_b")?],
                name,
            })
        })
        .collect()
}

async fn judge(
    store: &Store,
    prompt: &str,
    case: &Case,
    variant: &str,
    spec: &str,
) -> Result<Row, AppError> {
    let name = format!("{}_{variant}", case.name);
    // Human scores and version labels are deliberately absent from the model request.
    let user = format!(
        "### 1. 输入原始样文\n{}\n\n### 2. 提取出的风格规范 (Spec)\n{spec}",
        case.sample
    );
    let raw = store
        .call(&format!("{name}_judge"), prompt, &user, |s| {
            parse_learn_judgment(s).map(|_| ())
        })
        .await?;
    let judgment = score_learn_judgment(&raw, spec)?;
    let artifact = format!("{name}.md");
    std::fs::write(store.root.join(&artifact), spec)?;
    let index = match variant {
        "baseline" => Some(0),
        "old_candidate" => Some(1),
        _ => None,
    };
    let human_raw_scores = index.map(|i| case.human[i]);
    let historical_judgment = index.map(|i| case.historical[i].clone());
    let non_length_signed_delta = human_raw_scores.map(|h| delta(scores(&judgment), h));
    let historical_non_length_signed_delta =
        index.map(|i| delta(scores(&case.historical[i]), case.human[i]));
    let human_mean_with_deterministic_length = human_raw_scores.map(|mut h| {
        h[2] = judgment.scores.conciseness;
        h.iter().sum::<f64>() / 5.0
    });
    Ok(Row {
        case: case.name.clone(),
        variant: variant.into(),
        artifact,
        judgment,
        mechanical_checks: spec_checks(&case.sample, spec),
        human_raw_scores,
        historical_judgment,
        non_length_signed_delta,
        historical_non_length_signed_delta,
        human_mean_with_deterministic_length,
    })
}

async fn run_case(
    store: &Store,
    judge_prompt: &str,
    learn_prompt: &str,
    case: &Case,
) -> Result<Vec<Row>, AppError> {
    let baseline = judge(store, judge_prompt, case, "baseline", &case.baseline).await?;
    let candidate = judge(store, judge_prompt, case, "old_candidate", &case.candidate).await?;
    let spec = store
        .call(
            &format!("{}_new_candidate", case.name),
            learn_prompt,
            &case.sample,
            |s| {
                if s.trim().is_empty() {
                    Err(AppError::Parse("候选Spec为空".into()))
                } else {
                    Ok(())
                }
            },
        )
        .await?;
    let revised = judge(store, judge_prompt, case, "new_candidate", &spec).await?;
    Ok(vec![baseline, candidate, revised])
}

fn deviations(rows: &[Row], historical: bool) -> [f64; 4] {
    let arrays: Vec<_> = rows
        .iter()
        .filter_map(|r| {
            if historical {
                r.historical_non_length_signed_delta
            } else {
                r.non_length_signed_delta
            }
        })
        .collect();
    std::array::from_fn(|d| arrays.iter().map(|a| a[d].abs()).sum::<f64>() / arrays.len() as f64)
}

fn report(rows: &[Row], model: &str) -> String {
    let mut md = format!(
        "# 三组人工校准与提取提示词实验\n\n模型：{model}。仅 Spec 01–03；原始人评和历史产物未改写。新候选尚无人评。人工数据用于修正规范和离线计算偏差，没有输入本次 Judge 请求。\n\n五维顺序：结构覆盖 / 可执行性 / 精简度 / 特征捕捉 / 去污染。精简度按非空白 Unicode 字符数分档，含标题、标点、Markdown；模型原始分数单独保存。\n\n| 样本 | 版本 | 字数 | 五维分数 | 新均分 | 模型原始精简度 |\n|---|---|---:|---|---:|---:|\n"
    );
    for r in rows {
        md.push_str(&format!(
            "| {} | [{}]({}) | {} | {:?} | {:.2} | {:.1} |\n",
            r.case,
            r.variant,
            r.artifact,
            r.judgment.characters_without_whitespace.unwrap_or(0),
            scores(&r.judgment),
            r.judgment.overall_score,
            r.judgment.model_conciseness.unwrap_or(0.0)
        ));
    }
    md.push_str("\n## 与人评的偏差\n\n以下排除精简度，以免长度口径变化掩盖 Judge 误差。向量顺序：结构覆盖 / 可执行性 / 特征捕捉 / 去污染；正值表示模型高估。原始人评中 Spec 02 候选为用户接受的历史模型分数，独立性有限。\n\n| 样本 | 版本 | 历史模型减人评 | 新模型减人评 |\n|---|---|---|---|\n");
    for r in rows.iter().filter(|r| r.human_raw_scores.is_some()) {
        md.push_str(&format!(
            "| {} | {} | {:?} | {:?} |\n",
            r.case,
            r.variant,
            r.historical_non_length_signed_delta.unwrap(),
            r.non_length_signed_delta.unwrap()
        ));
    }
    let before = deviations(rows, true);
    let after = deviations(rows, false);
    md.push_str(&format!(
        "\n六个已评产物、四维平均绝对误差：历史 {:.3} → 新 Judge {:.3}。逐维：{:?} → {:?}。\n",
        before.iter().sum::<f64>() / 4.0,
        after.iter().sum::<f64>() / 4.0,
        before,
        after
    ));
    md.push_str("\n## 提取修正效果\n\n旧候选与新候选使用同一版 Judge 对照；Δ 为新减旧。\n\n| 样本 | 五维 Δ | 均分 Δ |\n|---|---|---:|\n");
    for group in rows.chunks_exact(3) {
        let old = scores(&group[1].judgment);
        let new = scores(&group[2].judgment);
        let delta: [f64; 5] = std::array::from_fn(|i| new[i] - old[i]);
        md.push_str(&format!(
            "| {} | {:?} | {:+.2} |\n",
            group[0].case,
            delta,
            group[2].judgment.overall_score - group[1].judgment.overall_score
        ));
    }
    md.push_str("\n## 解释限制\n\n三组参与了提示词修正，不是留出集；每项一次采样，不能证明泛化提升或统计显著性。历史生成元数据不完整，旧新候选差异不能完全归因于提示词。Judge 仍可能漏掉换词污染或过度惩罚语域特征；机械重合检查只提供线索，零重合不代表干净。新候选评分是模型判断，不冒充人工验收。原始人评的精简度不用于上述误差计算，其余四维保留原值；另外记录按新长度政策换算的人评均分以便同口径查看。\n");
    md
}

/// Explicit online entry; uses only the three authorized calibration pairs.
pub async fn run() -> Result<(), AppError> {
    let human = read_json("eval/human_calibration.json")?;
    let cases = load_cases(&human)?;
    let learn = read("prompts/learn_style.md")?;
    let judge = read("prompts/eval_judge_learn.md")?;
    let store = Store::from_env()?;
    store.freeze_inputs(&json!({"schema":2, "suite":"three_case_calibration",
        "model":store.model, "learn_prompt":learn, "judge_prompt":judge,
        "human_calibration":human, "historical_run":HISTORY,
        "length_policy":"non-whitespace Unicode scalars; 400–600=5; 300–399/601–700=4; 200–299/701–850=3; 100–199/851–1000=2; otherwise=1",
        "cases":cases.iter().map(|c| json!({"name":c.name,"sample":c.sample,
            "baseline":c.baseline,"old_candidate":c.candidate,"historical_judgments":c.historical})).collect::<Vec<_>>()}))?;
    std::fs::write(store.root.join("candidate_learn_prompt.md"), &learn)?;
    std::fs::write(store.root.join("judge_prompt.md"), &judge)?;
    let (a, b, c) = tokio::join!(
        run_case(&store, &judge, &learn, &cases[0]),
        run_case(&store, &judge, &learn, &cases[1]),
        run_case(&store, &judge, &learn, &cases[2])
    );
    let result = [a, b, c].into_iter().collect::<Result<Vec<_>, _>>();
    match result {
        Ok(groups) => {
            let rows: Vec<_> = groups.into_iter().flatten().collect();
            write_json(
                &store.root.join("summary.json"),
                &json!({
                "status":"complete", "completed_at":chrono::Utc::now().to_rfc3339(),
                "model":store.model, "human_calibration_cases":3, "new_candidates_human_reviewed":false,
                "expected_checkpoints":12, "expected_judgments":9, "expected_new_specs":3,
                "non_length_dimensions":["coverage","actionability","distinctiveness","cleanliness"],
                "historical_mae_by_dimension":deviations(&rows,true),
                "calibrated_judge_mae_by_dimension":deviations(&rows,false), "rows":rows}),
            )?;
            std::fs::write(store.root.join("summary.md"), report(&rows, &store.model))?;
            println!(
                "[complete] {} (9 judgments, 3 new Specs)",
                store.root.display()
            );
            Ok(())
        }
        Err(error) => {
            write_json(
                &store.root.join("failure-calibration.json"),
                &json!({"status":"failed",
                "at":chrono::Utc::now().to_rfc3339(), "error":error.to_string(),
                "resume":"用相同 EVAL_RUN_ID 运行 eval_calibration_suite"}),
            )?;
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_values_preserve_halves_and_length_does_not_affect_semantic_delta() {
        let human = human_scores(&json!([5, 4.5, 1, 4.5, 5])).unwrap();
        assert_eq!(human[1], 4.5);
        assert_eq!(
            delta([5.0, 4.0, 5.0, 5.0, 5.0], human),
            [0.0, -0.5, 0.5, 0.0]
        );
        assert!(human_scores(&json!([5, 4, 3, 2])).is_err());
        assert!(human_scores(&json!([5, 4, 3, 2, 0])).is_err());
    }

    #[test]
    fn deterministic_grade_preserves_model_estimate_and_recomputes_mean() {
        let raw = json!({"scores":{"coverage":5,"actionability":4,"conciseness":1,
            "distinctiveness":4,"cleanliness":5},"overall_score":99,
            "structure_analysis":"s","fidelity_analysis":"f","negative_hits":[],
            "improvement_suggestions":"i"})
        .to_string();
        let result = score_learn_judgment(&raw, &"字".repeat(400)).unwrap();
        assert_eq!(result.model_conciseness, Some(1.0));
        assert_eq!(result.model_score_mean, Some(3.8));
        assert_eq!(result.scores.conciseness, 5.0);
        assert_eq!(result.overall_score, 4.6);
        assert_eq!(parse_learn_judgment(&raw).unwrap().scores.conciseness, 1.0);
    }

    #[test]
    #[ignore = "三组真实模型校准；需API_KEY和网络；EVAL_RUN_ID断点续跑"]
    fn eval_calibration_suite() {
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(run())
            .unwrap();
    }
}
