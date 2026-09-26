//! Reproducible evaluation runs; historical baselines are always read-only.
use serde::Serialize;
use serde_json::{Value, json};

use crate::error::AppError;
use crate::eval::{
    TEST_SET, article_body, article_judge_input, parse_article_judgment, parse_learn_judgment,
    score_detailed_learn_judgment,
};
use crate::eval_store::{Store, write_json};
use crate::generate::{outline_system_prompt, render_prompt, style_system_prompt};

#[derive(Serialize)]
struct Row {
    suite: String,
    case: String,
    score: f64,
    baseline_score: Option<f64>,
    delta: Option<f64>,
    artifact: String,
    details: Value,
}

struct Inputs {
    style: String,
    judge_a: String,
    judge_b: String,
    learn: crate::style_learning::LearningPrompts,
    articles: Vec<String>,
    samples: Vec<String>,
    specs: Vec<String>,
}

fn read(path: &str) -> Result<String, AppError> {
    std::fs::read_to_string(path).map_err(|e| AppError::FileRead(format!("{path}: {e}")))
}

impl Inputs {
    fn load() -> Result<Self, AppError> {
        let read_set = |directory: &str, prefix: &str| -> Result<Vec<String>, AppError> {
            (1..=5)
                .map(|n| read(&format!("eval/{directory}/{prefix}_{n:02}.md")))
                .collect()
        };
        Ok(Self {
            style: read("styles/蔡叔.md")?,
            judge_a: read("prompts/eval_judge_style.md")?,
            judge_b: read("prompts/eval_judge_learn.md")?,
            learn: crate::style_learning::LearningPrompts::load()?,
            articles: read_set("baseline", "topic")?,
            samples: read_set("samples", "sample")?,
            specs: read_set("baseline_specs", "spec")?,
        })
    }
}

fn text_valid(text: &str) -> Result<(), AppError> {
    if text.trim().is_empty() {
        Err(AppError::Parse("模型返回空文本".into()))
    } else {
        Ok(())
    }
}

async fn judge_a(
    store: &Store,
    input: &Inputs,
    name: &str,
    style: &str,
    idea: &str,
    body: &str,
) -> Result<Value, AppError> {
    let user = article_judge_input(style, idea, body)?;
    let raw = store
        .call(name, &input.judge_a, &user, |s| {
            parse_article_judgment(s).map(|_| ())
        })
        .await?;
    serde_json::to_value(parse_article_judgment(&raw)?).map_err(|e| AppError::Parse(e.to_string()))
}

async fn judge_b(
    store: &Store,
    input: &Inputs,
    name: &str,
    sample: &str,
    spec: &str,
) -> Result<Value, AppError> {
    let user = format!("### 1. 输入原始样文\n{sample}\n\n### 2. 提取出的风格规范 (Spec)\n{spec}");
    let raw = store
        .call(name, &input.judge_b, &user, |s| {
            parse_learn_judgment(s).map(|_| ())
        })
        .await?;
    let mut result = serde_json::to_value(score_detailed_learn_judgment(&raw, spec)?)
        .map_err(|e| AppError::Parse(e.to_string()))?;
    result["mechanical_checks"] = spec_checks(sample, spec);
    Ok(result)
}

pub(crate) fn spec_checks(sample: &str, spec: &str) -> Value {
    let mut checks = legacy_spec_checks(sample, spec);
    let n = crate::spec_quality::character_count(spec);
    checks
        .as_object_mut()
        .unwrap()
        .remove("within_600_characters");
    checks["within_target_range"] = json!(n >= crate::spec_quality::DETAILED_MIN);
    checks["execution_text_valid"] =
        json!(crate::spec_quality::validate_execution_spec(spec).is_ok());
    checks["deterministic_conciseness"] = json!(crate::spec_quality::detailed_length_score(n));
    checks["length_policy"] = json!(crate::spec_quality::DETAILED_POLICY);
    checks["note"] = json!("长度只检查充分度下限；冗余与可执行性另评。12字重合仅为线索。");
    checks
}

pub(crate) fn legacy_spec_checks(sample: &str, spec: &str) -> Value {
    let compact = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
    let source = compact(sample);
    let chars: Vec<_> = compact(spec).chars().collect();
    let mut overlaps = Vec::new();
    for window in chars.windows(12) {
        let phrase: String = window.iter().collect();
        if source.contains(&phrase) && overlaps.len() < 5 {
            overlaps.push(phrase);
        }
    }
    json!({"characters_without_whitespace": chars.len(), "within_600_characters": chars.len() <= 600,
        "within_target_range": (400..=600).contains(&chars.len()),
        "deterministic_conciseness": crate::spec_quality::conciseness_score(chars.len()),
        "execution_text_valid": chars.len() <= 1000 && crate::spec_quality::validate_execution_spec(spec).is_ok(),
        "format_policy": "free-form-v1", "source_overlap_12_char_examples": overlaps,
        "note": "连续 12 字重合仅为复核线索，不能识别换词转述的语义污染；精简度由代码按长度分档，其余维度由模型判断。"})
}

fn row(
    suite: &str,
    case: String,
    details: Value,
    baseline: Option<f64>,
    artifact: String,
) -> Result<Row, AppError> {
    let score = details["overall_score"]
        .as_f64()
        .ok_or_else(|| AppError::Parse("缺少评测均分".into()))?;
    Ok(Row {
        suite: suite.into(),
        case,
        score,
        baseline_score: baseline,
        delta: baseline.map(|b| score - b),
        artifact,
        details,
    })
}

async fn baseline_a(store: &Store, input: &Inputs) -> Result<Vec<Row>, AppError> {
    let mut rows = Vec::new();
    for (i, idea) in TEST_SET.iter().enumerate() {
        let name = format!("baseline_a_{:02}", i + 1);
        let details = judge_a(store, input, &name, &input.style, idea, &input.articles[i]).await?;
        rows.push(row(
            "baseline_a",
            format!("topic_{:02}", i + 1),
            details,
            None,
            format!("{name}.json"),
        )?);
    }
    Ok(rows)
}

async fn baseline_b(store: &Store, input: &Inputs) -> Result<Vec<Row>, AppError> {
    let mut rows = Vec::new();
    for i in 0..5 {
        let name = format!("baseline_b_{:02}", i + 1);
        let details = judge_b(store, input, &name, &input.samples[i], &input.specs[i]).await?;
        rows.push(row(
            "baseline_b",
            format!("spec_{:02}", i + 1),
            details,
            None,
            format!("{name}.json"),
        )?);
    }
    Ok(rows)
}

async fn generate(store: &Store, name: &str, style: &str, idea: &str) -> Result<String, AppError> {
    let outline = store
        .call(
            &format!("{name}_outline"),
            &outline_system_prompt(style),
            idea,
            text_valid,
        )
        .await?;
    let body = store
        .call(
            &format!("{name}_body"),
            &style_system_prompt(style),
            &render_prompt(&outline, idea),
            text_valid,
        )
        .await?;
    std::fs::write(store.root.join(format!("{name}.md")), &body)?;
    Ok(body)
}

async fn compare_a(store: &Store, input: &Inputs, baselines: &[Row]) -> Result<Vec<Row>, AppError> {
    let mut rows = Vec::new();
    for (i, idea) in TEST_SET.iter().enumerate() {
        let name = format!("candidate_a_{:02}", i + 1);
        let body = generate(store, &name, &input.style, idea).await?;
        let details = judge_a(
            store,
            input,
            &format!("{name}_judge"),
            &input.style,
            idea,
            &body,
        )
        .await?;
        rows.push(row(
            "compare_a",
            format!("topic_{:02}", i + 1),
            details,
            Some(baselines[i].score),
            format!("{name}.md"),
        )?);
    }
    Ok(rows)
}

async fn compare_b(store: &Store, input: &Inputs, baselines: &[Row]) -> Result<Vec<Row>, AppError> {
    let candidate = input.learn.clone();
    write_json(&store.root.join("candidate_learn_prompts.json"), &candidate)?;
    let mut rows = Vec::new();
    for i in 0..5 {
        let name = format!("candidate_b_{:02}", i + 1);
        let learned = crate::style_learning::extract_with(
            &input.samples[i],
            &candidate,
            |stage, system, user| {
                let task = format!("{name}_{stage}");
                async move { store.call(&task, &system, &user, text_valid).await }
            },
        )
        .await?;
        write_json(&store.root.join(format!("{name}_learning.json")), &learned)?;
        let spec = learned.spec;
        std::fs::write(store.root.join(format!("{name}.md")), &spec)?;
        let details = judge_b(
            store,
            input,
            &format!("{name}_judge"),
            &input.samples[i],
            &spec,
        )
        .await?;
        rows.push(row(
            "compare_b",
            format!("spec_{:02}", i + 1),
            details,
            Some(baselines[i].score),
            format!("{name}.md"),
        )?);
    }
    Ok(rows)
}

async fn cross_spec(store: &Store, input: &Inputs, s: usize) -> Result<Vec<Row>, AppError> {
    let mut rows = Vec::new();
    let spec = &input.specs[s];
    for (t, idea) in TEST_SET.iter().enumerate() {
        let name = format!("cross_s{:02}_t{:02}", s + 1, t + 1);
        let body = generate(store, &name, spec, idea).await?;
        let details = judge_a(store, input, &format!("{name}_judge"), spec, idea, &body).await?;
        rows.push(row(
            "cross",
            name.clone(),
            details,
            None,
            format!("{name}.md"),
        )?);
    }
    Ok(rows)
}

async fn cross(store: &Store, input: &Inputs) -> Result<Vec<Row>, AppError> {
    let (a, b, c, d, e) = tokio::join!(
        cross_spec(store, input, 0),
        cross_spec(store, input, 1),
        cross_spec(store, input, 2),
        cross_spec(store, input, 3),
        cross_spec(store, input, 4)
    );
    Ok([a, b, c, d, e]
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .flatten()
        .collect())
}

fn report(rows: &[Row], model: &str) -> String {
    let mut md = format!(
        "# 自动评测结果\n\n模型：{model}\n\n模型评分尚未经人工校准。历史 Baseline 的生成模型和提示词元数据不全，分差不能单独归因于本轮提示词改动。\n\n| 组别 | 样本 | 得分 | 基线 | Δ | 产物 |\n|---|---|---:|---:|---:|---|\n"
    );
    for r in rows {
        let fmt = |v: Option<f64>| v.map(|s| format!("{s:.2}")).unwrap_or_else(|| "—".into());
        md.push_str(&format!(
            "| {} | {} | {:.2} | {} | {} | [查看]({}) |\n",
            r.suite,
            r.case,
            r.score,
            fmt(r.baseline_score),
            fmt(r.delta),
            r.artifact
        ));
    }
    md.push_str("\n## 分组统计\n\n");
    for suite in [
        "baseline_a",
        "baseline_b",
        "compare_a",
        "compare_b",
        "cross",
    ] {
        let group: Vec<_> = rows.iter().filter(|r| r.suite == suite).collect();
        if group.is_empty() {
            continue;
        }
        let mean = group.iter().map(|r| r.score).sum::<f64>() / group.len() as f64;
        md.push_str(&format!("- {suite}：{} 项，均分 {mean:.2}", group.len()));
        if group[0].delta.is_some() {
            let wins = group
                .iter()
                .filter(|r| r.delta.unwrap_or(0.0) > 0.05)
                .count();
            let losses = group
                .iter()
                .filter(|r| r.delta.unwrap_or(0.0) < -0.05)
                .count();
            md.push_str(&format!(
                "，胜/平/负 {wins}/{}/{losses}，胜率 {:.0}%（单次评测，阈值 ±0.05）",
                group.len() - wins - losses,
                wins as f64 * 100.0 / group.len() as f64
            ));
        }
        md.push('\n');
    }
    md.push_str("\n## 人工复核\n\n按 baseline_b / compare_b 的 JSON 中 mechanical_checks、negative_hits 与诊断核对原文和 Spec，逐篇标记漏抓、误抓、素材污染与是否接受候选。无需重新生成。\n");
    md
}

/// Online suites share checkpoints under EVAL_RUN_ID. Offline tests never call this entry.
pub async fn run(mode: &str) -> Result<(), AppError> {
    if ![
        "all",
        "baseline_a",
        "baseline_b",
        "compare_a",
        "compare_b",
        "cross",
    ]
    .contains(&mode)
    {
        return Err(AppError::Parse("未知评测模式".into()));
    }
    let input = Inputs::load()?;
    // Fail on missing/malformed frozen inputs before making any model request.
    for article in &input.articles {
        article_body(article)?;
    }
    let store = Store::from_env()?;
    store.freeze_inputs(&json!({"schema":1, "model":store.model, "topics":TEST_SET,
        "style":input.style, "judge_a":input.judge_a, "judge_b":input.judge_b,
        "length_policy":crate::spec_quality::DETAILED_POLICY, "learn":input.learn, "baseline_articles":input.articles,
        "samples":input.samples, "baseline_specs":input.specs,
        "outline_template":outline_system_prompt("{style}"),
        "body_template":style_system_prompt("{style}"),
        "render_template":render_prompt("{outline}", "{idea}")}))?;
    println!("[run] {} (mode={mode})", store.root.display());
    let group_a = async {
        let mut rows = Vec::new();
        if ["all", "baseline_a", "compare_a"].contains(&mode) {
            let baseline = baseline_a(&store, &input).await?;
            if ["all", "compare_a"].contains(&mode) {
                rows.extend(compare_a(&store, &input, &baseline).await?);
            }
            rows.extend(baseline);
        }
        Ok::<_, AppError>(rows)
    };
    let group_b = async {
        let mut rows = Vec::new();
        if ["all", "baseline_b", "compare_b"].contains(&mode) {
            let baseline = baseline_b(&store, &input).await?;
            if ["all", "compare_b"].contains(&mode) {
                rows.extend(compare_b(&store, &input, &baseline).await?);
            }
            rows.extend(baseline);
        }
        Ok::<_, AppError>(rows)
    };
    let group_cross = async {
        if ["all", "cross"].contains(&mode) {
            cross(&store, &input).await
        } else {
            Ok(Vec::new())
        }
    };
    let (a, b, c) = tokio::join!(group_a, group_b, group_cross);
    let result = [a, b, c]
        .into_iter()
        .collect::<Result<Vec<_>, AppError>>()
        .map(|groups| groups.into_iter().flatten().collect::<Vec<Row>>());

    match result {
        Ok(rows) => {
            let prefix = if mode == "all" {
                "summary".to_string()
            } else {
                format!("summary-{mode}")
            };
            write_json(
                &store.root.join(format!("{prefix}.json")),
                &json!({"status":"complete", "mode":mode,
                "model":store.model, "human_calibrated":false, "completed_at":chrono::Utc::now().to_rfc3339(), "rows":rows}),
            )?;
            std::fs::write(
                store.root.join(format!("{prefix}.md")),
                report(&rows, &store.model),
            )?;
            println!("[complete] {} ({})", store.root.display(), rows.len());
            Ok(())
        }
        Err(error) => {
            write_json(
                &store.root.join(format!("failure-{mode}.json")),
                &json!({"status":"failed", "mode":mode,
                "at":chrono::Utc::now().to_rfc3339(), "error":error.to_string(), "resume":"使用相同 EVAL_RUN_ID 重跑同一入口；完成检查点自动复用"}),
            )?;
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mechanical_checks_allow_free_format_and_expose_source_quotes() {
        let checks = spec_checks(
            "这是一整句仅属于样文的特定内容。",
            "## 标题\n这是一整句仅属于样文的特定内容。",
        );
        assert_eq!(checks["execution_text_valid"], true);
        let detailed = spec_checks("", &"字".repeat(6500));
        assert_eq!(detailed["execution_text_valid"], true);
        assert_eq!(detailed["within_target_range"], true);
        assert_eq!(detailed["deterministic_conciseness"], 5.0);
        assert!(
            !checks["source_overlap_12_char_examples"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            legacy_spec_checks("", &"字".repeat(601))["within_600_characters"],
            false
        );
    }

    #[test]
    fn summary_uses_actual_scores_and_marks_uncalibrated_results() {
        let rows = vec![
            row(
                "compare_a",
                "test".into(),
                json!({"overall_score":4.0}),
                Some(3.0),
                "test.md".into(),
            )
            .unwrap(),
        ];
        let text = report(&rows, "fixture");
        assert!(text.contains("胜/平/负 1/0/0"));
        assert!(text.contains("未经人工校准"));
        assert!(text.contains("| 4.00 | 3.00 | 1.00 |"));
    }
}
