//! Predeclared practical release gates; no overall score, no tuning the judge.
use crate::error::AppError;
use crate::eval::LearnJudgeResult;
use crate::eval_transfer_audit::Row;
use crate::transfer_quality::DIMENSIONS;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Spec {
    pub sample: usize,
    pub variant: String,
    pub text: String,
    pub extraction_calls: usize,
    pub judgment: LearnJudgeResult,
}
pub(crate) fn evaluate(specs: &[Spec], rows: &[Row]) -> Result<Value, AppError> {
    let expected: std::collections::BTreeSet<_> = (1..=5)
        .flat_map(|i| ["single_stage", "direct_free"].map(|m| (i, m.to_string())))
        .collect();
    let actual = specs
        .iter()
        .map(|s| (s.sample, s.variant.clone()))
        .collect();
    if specs.len() != 10 || expected != actual || rows.len() != 20 {
        return Err(AppError::Parse(
            "发布评测需要两方法各五份Spec及严格/创作各十篇文章".into(),
        ));
    }
    let mut failures = Vec::new();
    let candidates: Vec<_> = specs
        .iter()
        .filter(|s| s.variant == "direct_free")
        .collect();
    for s in &candidates {
        let n = crate::spec_quality::character_count(&s.text);
        if crate::spec_quality::validate_execution_spec(&s.text).is_err()
            || !(400..=600).contains(&n)
        {
            failures.push(format!("Spec{}格式或400–600字目标未达标（{n}）", s.sample));
        }
        if s.judgment.scores.cleanliness < 4.0 {
            failures.push(format!("Spec{}去污染低于4", s.sample));
        }
    }
    let action = candidates
        .iter()
        .map(|s| s.judgment.scores.actionability)
        .sum::<f64>()
        / 5.0;
    let distinctive = candidates
        .iter()
        .map(|s| s.judgment.scores.distinctiveness)
        .sum::<f64>()
        / 5.0;
    if action < 4.0 || distinctive < 4.0 {
        failures.push("Spec可执行性或特征捕捉均分低于4".into());
    }
    let mut by_mode = serde_json::Map::new();
    for mode in ["strict", "creative"] {
        let mut methods = serde_json::Map::new();
        for method in ["single_stage", "direct_free"] {
            let mut cases: Vec<_> = rows
                .iter()
                .filter(|r| r.group == mode && r.variant == method)
                .collect();
            cases.sort_by_key(|r| r.sample);
            if cases.len() != 5
                || cases.iter().map(|r| r.sample).collect::<Vec<_>>() != (1..=5).collect::<Vec<_>>()
            {
                return Err(AppError::Parse("迁移结果重复或缺失配对样文".into()));
            }
            let means: [f64; 5] = std::array::from_fn(|i| {
                cases
                    .iter()
                    .map(|r| r.judgment.scores.values()[i])
                    .sum::<f64>()
                    / 5.0
            });
            let length_passes = cases
                .iter()
                .filter(|r| r.length["within_range"] == true)
                .count();
            if method == "direct_free" {
                let minima = [4.0, 4.8, if mode == "strict" { 4.4 } else { 4.8 }, 3.8, 4.8];
                for i in 0..5 {
                    if means[i] + 1e-9 < minima[i] {
                        failures.push(format!(
                            "{mode} {} {:.2} < {}",
                            DIMENSIONS[i], means[i], minima[i]
                        ));
                    }
                }
                for r in &cases {
                    let v = r.judgment.scores.values();
                    if [0, 1, 2, 4].iter().any(|i| v[*i] <= 2.0) {
                        failures.push(format!("{mode}样文{}存在≤2的关键维度", r.sample));
                    }
                }
                if length_passes < 4 {
                    failures.push(format!("{mode}字数仅{length_passes}/5达标"));
                }
            }
            let means: serde_json::Map<_, _> = DIMENSIONS
                .iter()
                .zip(means)
                .map(|(k, v)| (k.to_string(), json!(v)))
                .collect();
            methods.insert(
                method.into(),
                json!({"means":means,"length_passes":length_passes,"n":5}),
            );
        }
        let mut deltas = serde_json::Map::new();
        for key in DIMENSIONS {
            deltas.insert(
                key.into(),
                json!(
                    methods["direct_free"]["means"][key].as_f64().unwrap()
                        - methods["single_stage"]["means"][key].as_f64().unwrap()
                ),
            );
        }
        methods.insert("candidate_minus_legacy".into(), Value::Object(deltas));
        let mut paired = serde_json::Map::new();
        for (dimension, key) in DIMENSIONS.iter().enumerate() {
            let mut counts = [0; 3];
            for sample in 1..=5 {
                let score = |method: &str| {
                    rows.iter()
                        .find(|r| r.group == mode && r.variant == method && r.sample == sample)
                        .unwrap()
                        .judgment
                        .scores
                        .values()[dimension]
                };
                let delta = score("direct_free") - score("single_stage");
                counts[if delta > 0.0 {
                    0
                } else if delta == 0.0 {
                    1
                } else {
                    2
                }] += 1;
            }
            paired.insert(
                key.to_string(),
                json!({"wins":counts[0],"ties":counts[1],"losses":counts[2]}),
            );
        }
        methods.insert("paired_counts".into(), Value::Object(paired));
        by_mode.insert(mode.into(), Value::Object(methods));
    }
    Ok(
        json!({"criteria":"free-release-v1","passed":failures.is_empty(),"failures":failures,
        "spec_means":{"actionability":action,"distinctiveness":distinctive},"by_mode":by_mode,
        "interpretation":"工程期望门槛，不是整体分数或统计显著优越性"}),
    )
}

pub(crate) fn report(
    specs: &[Spec],
    rows: &[Row],
    gate: &Value,
    phase: &str,
    model: &str,
) -> String {
    let mut md = format!(
        "# 自由格式学习发布评测\n\n阶段：{phase}；模型：{model}。固定Rubric B v2和Transfer v2，不合成总分。\n\n门槛通过：{}\n\n| 模式 | 方法 | 风格 | 原文独立 | 事实 | 自然度 | 素材完成 | 长度达标 |\n|---|---|---:|---:|---:|---:|---:|---:|\n",
        gate["passed"]
    );
    for mode in ["strict", "creative"] {
        for method in ["single_stage", "direct_free"] {
            let d = &gate["by_mode"][mode][method];
            let m = &d["means"];
            md.push_str(&format!(
                "| {mode} | {method} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} | {}/5 |\n",
                m[DIMENSIONS[0]].as_f64().unwrap(),
                m[DIMENSIONS[1]].as_f64().unwrap(),
                m[DIMENSIONS[2]].as_f64().unwrap(),
                m[DIMENSIONS[3]].as_f64().unwrap(),
                m[DIMENSIONS[4]].as_f64().unwrap(),
                d["length_passes"]
            ));
        }
    }
    md.push_str("\n## 分项配对胜/平/负\n\n| 模式 | 维度 | 新版胜/平/负 |\n|---|---|---|\n");
    for mode in ["strict", "creative"] {
        for dimension in DIMENSIONS {
            let c = &gate["by_mode"][mode]["paired_counts"][dimension];
            md.push_str(&format!(
                "| {mode} | {dimension} | {}/{}/{} |\n",
                c["wins"], c["ties"], c["losses"]
            ));
        }
    }
    md.push_str("\n## 未达标项\n\n");
    for f in gate["failures"].as_array().unwrap() {
        md.push_str(&format!("- {}\n", f.as_str().unwrap()));
    }
    md.push_str("\n## Spec\n\n| 样文 | 方法 | 字数 | 可执行性 | 特征 | 去污染 |\n|---|---|---:|---:|---:|---:|\n");
    for s in specs {
        md.push_str(&format!(
            "| {} | [{}](spec_{}_{:02}.md) | {} | {} | {} | {} |\n",
            s.sample,
            s.variant,
            s.variant,
            s.sample,
            crate::spec_quality::character_count(&s.text),
            s.judgment.scores.actionability,
            s.judgment.scores.distinctiveness,
            s.judgment.scores.cleanliness
        ));
    }
    md.push_str("\n## 正文与诊断\n\n");
    for r in rows {
        md.push_str(&format!(
            "- {} 样文{} {}：[正文]({}) / [评分]({}_result.json)\n",
            r.group, r.sample, r.variant, r.body_artifact, r.id
        ));
    }
    md.push_str("\n开发样文为已使用的五篇单篇文章；确认阶段只换题材，不是作者留出。每项一次生成、同模型裁判，有已知评分波动。门槛只表示本阶段实用预期，不宣称所有维度都胜过旧版或统计显著。本次未修改裁判、题材事实权限或生成包装来提高候选成绩。\n");
    md
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixtures() -> (Vec<Spec>, Vec<Row>) {
        let mut specs = Vec::new();
        let mut rows = Vec::new();
        for sample in 1..=5 {
            for variant in ["single_stage", "direct_free"] {
                let judgment=serde_json::from_value(json!({"scores":{"coverage":5,"actionability":4,"conciseness":5,"distinctiveness":4,"cleanliness":5},"overall_score":4.6,"structure_analysis":"s","fidelity_analysis":"f","negative_hits":[],"improvement_suggestions":"i"})).unwrap();
                specs.push(Spec {
                    sample,
                    variant: variant.into(),
                    text: "规则".repeat(250),
                    extraction_calls: 1,
                    judgment,
                });
                for group in ["strict", "creative"] {
                    rows.push(serde_json::from_value(json!({"id":format!("{sample}{variant}{group}"),"group":group,"sample":sample,"variant":variant,
                "body_artifact":"fixture","length":{"within_range":true},"judge_calls":1,"generation_calls":2,"previous_transfer":null,
                "judgment":{"scores":{"style_fidelity":4,"source_independence":5,"factual_compliance":5,"naturalness":4,"task_completion":5},"reasons":{},"source_leakage":[],"fact_violations":[],"task_issues":[],"uncertainties":[]}})).unwrap());
                }
            }
        }
        (specs, rows)
    }
    #[test]
    fn gate_rejects_missing_cases_severe_failures_and_length_regressions() {
        let (mut specs, mut rows) = fixtures();
        let gate = evaluate(&specs, &rows).unwrap();
        assert_eq!(gate["passed"], true);
        assert_eq!(
            gate["by_mode"]["strict"]["paired_counts"]["style_fidelity"],
            json!({"wins":0,"ties":5,"losses":0})
        );
        rows[2].judgment.scores.factual_compliance = 2.0;
        assert_eq!(evaluate(&specs, &rows).unwrap()["passed"], false);
        rows[2].judgment.scores.factual_compliance = 5.0;
        rows[2].length["within_range"] = json!(false);
        rows[6].length["within_range"] = json!(false);
        assert_eq!(evaluate(&specs, &rows).unwrap()["passed"], false);
        specs[1].text = "短".into();
        assert_eq!(evaluate(&specs, &rows).unwrap()["passed"], false);
        rows.pop();
        assert!(evaluate(&specs, &rows).is_err());
    }
}
