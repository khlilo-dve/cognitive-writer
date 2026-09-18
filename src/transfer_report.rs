//! Task-stratified reporting, deliberately without an overall adoption score.
use crate::error::AppError;
use crate::eval_transfer_audit::{ControlResult, Row};
use crate::transfer_quality::DIMENSIONS;
use serde_json::{Value, json};
const METHODS: [&str; 3] = ["single_stage", "two_stage", "direct_free"];
const GROUPS: [&str; 3] = ["historical", "strict", "creative"];

pub(crate) fn aggregate(rows: &[Row]) -> Result<Value, AppError> {
    if rows.len() != 45 {
        return Err(AppError::Parse(
            "应有45篇完整结果，不能用缺失项计算均分".into(),
        ));
    }
    let mut groups = serde_json::Map::new();
    for group in GROUPS {
        let mut methods = serde_json::Map::new();
        for method in METHODS {
            let cases: Vec<_> = rows
                .iter()
                .filter(|r| r.group == group && r.variant == method)
                .collect();
            let ids: std::collections::BTreeSet<_> = cases.iter().map(|r| r.sample).collect();
            if cases.len() != 5 || ids != (1..=5).collect() {
                return Err(AppError::Parse("每组每方法必须恰有五个独立样文".into()));
            }
            let means: serde_json::Map<_, _> = DIMENSIONS
                .iter()
                .enumerate()
                .map(|(i, k)| {
                    (
                        k.to_string(),
                        json!(
                            cases
                                .iter()
                                .map(|r| r.judgment.scores.values()[i])
                                .sum::<f64>()
                                / 5.0
                        ),
                    )
                })
                .collect();
            let mut comparisons = serde_json::Map::new();
            if method != "single_stage" {
                for (d, key) in DIMENSIONS.iter().enumerate() {
                    let mut deltas = Vec::new();
                    for r in &cases {
                        let old = rows
                            .iter()
                            .find(|o| {
                                o.group == group
                                    && o.variant == "single_stage"
                                    && o.sample == r.sample
                            })
                            .ok_or_else(|| AppError::Parse("缺少配对旧方法".into()))?;
                        deltas
                            .push(r.judgment.scores.values()[d] - old.judgment.scores.values()[d]);
                    }
                    let wins = deltas.iter().filter(|v| **v > 0.05).count();
                    let losses = deltas.iter().filter(|v| **v < -0.05).count();
                    comparisons.insert(key.to_string(),json!({"deltas_by_sample":deltas,"wins":wins,"ties":5-wins-losses,"losses":losses}));
                }
            }
            methods.insert(method.into(),json!({"means":means,"length_passes":cases.iter().filter(|r|r.length["within_range"]==true).count(),
                "n":5,"paired_against_legacy":comparisons}));
        }
        groups.insert(group.into(), Value::Object(methods));
    }
    Ok(Value::Object(groups))
}

pub(crate) fn control_drift(controls: &[ControlResult]) -> Value {
    let mut result = serde_json::Map::new();
    for prefix in ["extra", "leak"] {
        let a = controls.iter().find(|c| c.id == format!("{prefix}_strict"));
        let b = controls
            .iter()
            .find(|c| c.id == format!("{prefix}_creative"));
        if let (Some(a), Some(b)) = (a, b) {
            let x = a.judgment.scores.values();
            let y = b.judgment.scores.values();
            let delta: serde_json::Map<_, _> = DIMENSIONS
                .iter()
                .enumerate()
                .map(|(i, k)| (k.to_string(), json!(y[i] - x[i])))
                .collect();
            result.insert(
                prefix.into(),
                json!({"creative_minus_strict":delta,
                "non_fact_drift_observed":([0,1,3,4].iter().any(|i|(y[*i]-x[*i]).abs()>0.05))}),
            );
        }
    }
    Value::Object(result)
}

pub(crate) fn report(
    rows: &[Row],
    controls: &[ControlResult],
    model: &str,
    aggregates: &Value,
) -> Result<String, AppError> {
    let mut md = format!(
        "# 风格迁移评测修正：Transfer v2\n\n模型：{model}。提取提示词、Spec和生产生成包装保持不变；15篇历史文章重评，另有同一新题材的严格/创作两类任务各15篇。没有重新学习、多文章合并或新增用户人评。方法标签与旧分数不输入裁判。\n\n## 独立维度\n\n风格保真 / 原文独立性 / 事实合规 / 自然度 / 主题完成。事实问题不再计入原文污染或主题完成，长度单独由程序计数；不输出合成总分或自动采用结论。\n\n| 组别 | 方法 | 风格 | 原文独立 | 事实 | 自然度 | 主题完成 | 字数达标 |\n|---|---|---:|---:|---:|---:|---:|---:|\n"
    );
    for group in GROUPS {
        for method in METHODS {
            let data = &aggregates[group][method];
            let scores = &data["means"];
            md.push_str(&format!(
                "| {group} | {method} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} | {}/5 |\n",
                scores[DIMENSIONS[0]].as_f64().unwrap(),
                scores[DIMENSIONS[1]].as_f64().unwrap(),
                scores[DIMENSIONS[2]].as_f64().unwrap(),
                scores[DIMENSIONS[3]].as_f64().unwrap(),
                scores[DIMENSIONS[4]].as_f64().unwrap(),
                data["length_passes"]
            ));
        }
    }
    md.push_str("\n## 裁判机制控制\n\n以下控制不纳入方法均分。四个控制中，同一新增场景在严格任务应扣事实分、明确创作任务不应扣；同一源文内容搬运在两种任务下都应识别。短文故意未达600字，长度不得影响上述分项。\n\n| 控制 | 通过 | 五维分数 |\n|---|---|---|\n");
    for p in controls {
        md.push_str(&format!(
            "| {} | {} | {:?} |\n",
            p.id,
            p.passed,
            p.judgment.scores.values()
        ));
    }
    if controls.iter().any(|p| !p.passed) {
        md.push_str("\n**至少一个裁判控制未通过：本轮评分可靠性受限，不得据此直接判定方法优劣。失败控制及模型原响应完整保留。**\n");
    }
    md.push_str(&format!("\n控制通过仅代表预设方向正确，不等于评分稳定或人工校准。同文只改变事实权限时的分差（creative减strict）如下；事实分预期变化，其他维度的变化作为敏感性诊断保留，不据此调整裁判：\n```json\n{}\n```\n",serde_json::to_string_pretty(&control_drift(controls)).map_err(|e|AppError::Parse(e.to_string()))?));
    md.push_str("\n## 逐篇证据\n\n| ID | 组别 | 样文 | 方法 | 字数 | 五维分数 | 确定污染/事实问题 |\n|---|---|---:|---|---:|---|---|\n");
    for r in rows {
        md.push_str(&format!(
            "| [{}]({}) | {} | {} | {} | {} | {:?} | {}/{}（[诊断]({}_result.json)） |\n",
            r.id,
            r.body_artifact,
            r.group,
            r.sample,
            r.variant,
            r.length["characters"],
            r.judgment.scores.values(),
            r.judgment.source_leakage.len(),
            r.judgment.fact_violations.len(),
            r.id
        ));
    }
    md.push_str("\n## 解释限制\n\n历史文章按原任务权限重评；其原字数口径没有说明，当前非空白字符统计只作统一诊断，不倒推原评分。strict/creative是同一修理日素材包的两种权限，不能当作两个独立题材。生成与裁判同模型，每项一次采样，任务/体裁仍有限，没有显著性或泛化结论。事实严格场景与创作场景分别解释，不平均成单一胜者。原文独立分高也不证明完全无语义污染；必须保留引用证据、uncertainties和控制失败。旧综合分量尺已废弃，不能与新分数相减声称提升。默认方法本轮不自动切换。\n");
    Ok(md)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn incomplete_or_duplicate_groups_cannot_produce_successful_averages() {
        assert!(aggregate(&[]).is_err());
        let mut rows = Vec::new();
        for group in GROUPS {
            for variant in METHODS {
                for sample in 1..=5 {
                    rows.push(serde_json::from_value(json!({"id":format!("{group}{variant}{sample}"),"group":group,"variant":variant,"sample":sample,
                "body_artifact":"fixture.md","length":{"within_range":true},"judge_calls":1,"generation_calls":0,"previous_transfer":null,
                "judgment":{"scores":{"style_fidelity":4,"source_independence":5,"factual_compliance":3,"naturalness":4,"task_completion":5},
                "reasons":{},"source_leakage":[],"fact_violations":[],"task_issues":[],"uncertainties":[]}})).unwrap());
                }
            }
        }
        let result = aggregate(&rows).unwrap();
        assert_eq!(
            result["strict"]["two_stage"]["means"]["factual_compliance"],
            3.0
        );
        assert_eq!(
            result["strict"]["two_stage"]["means"]["source_independence"],
            5.0
        );
        assert!(result["strict"]["two_stage"].get("overall_score").is_none());
        rows[0].sample = 2;
        assert!(aggregate(&rows).is_err());
    }
}
