//! Evidence-anchored representation views and deterministic per-axis overlap.
use crate::error::AppError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

pub(crate) const DIMENSIONS: [&str; 9] = [
    "language",
    "voice",
    "rhythm",
    "information",
    "narration",
    "constraints",
    "title",
    "opening",
    "ending",
];
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Feature {
    pub id: String,
    pub dimension: String,
    pub rule: String,
    pub condition: String,
    pub quote: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct View {
    pub features: Vec<Feature>,
    pub uncertainties: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Link {
    pub left: String,
    pub right: String,
    pub reason: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Comparison {
    pub matches: Vec<Link>,
    pub conflicts: Vec<Link>,
    pub uncertain_left: Vec<String>,
    pub uncertain_right: Vec<String>,
    pub notes: Vec<String>,
}

pub(crate) fn indexed_input(spec: &str) -> String {
    json!({"lines":spec.split('\n').enumerate().map(|(i,text)|json!({"line":i+1,"text":text})).collect::<Vec<_>>()}).to_string()
}

pub(crate) fn parse_indexed_view(raw: &str, spec: &str) -> Result<View, AppError> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct IndexedFeature {
        id: String,
        dimension: String,
        rule: String,
        condition: String,
        line_start: usize,
        line_end: usize,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct IndexedView {
        features: Vec<IndexedFeature>,
        uncertainties: Vec<String>,
    }
    let indexed: IndexedView =
        serde_json::from_str(raw).map_err(|e| AppError::Parse(format!("行引用视图JSON: {e}")))?;
    let lines: Vec<_> = spec.split('\n').collect();
    let mut features = Vec::new();
    for f in indexed.features {
        if f.line_start == 0
            || f.line_end < f.line_start
            || f.line_end > lines.len()
            || f.line_end - f.line_start > 2
        {
            return Err(AppError::Parse("引用行号不存在、倒序或范围超过3行".into()));
        }
        let quote = lines[f.line_start - 1..f.line_end]
            .join("\n")
            .trim()
            .to_owned();
        features.push(Feature {
            id: f.id,
            dimension: f.dimension,
            rule: f.rule,
            condition: f.condition,
            quote,
        });
    }
    let view = View {
        features,
        uncertainties: indexed.uncertainties,
    };
    parse_view(
        &serde_json::to_string(&view).map_err(|e| AppError::Parse(e.to_string()))?,
        spec,
    )
}

pub(crate) fn parse_view(raw: &str, spec: &str) -> Result<View, AppError> {
    let v: View =
        serde_json::from_str(raw).map_err(|e| AppError::Parse(format!("比较视图JSON: {e}")))?;
    if v.features.is_empty() || v.features.len() > 40 {
        return Err(AppError::Parse("特征数须为1–40，不填充缺失维度".into()));
    }
    let mut ids = HashSet::new();
    for f in &v.features {
        if f.id.is_empty()
            || !ids.insert(&f.id)
            || !DIMENSIONS.contains(&f.dimension.as_str())
            || f.rule.trim().is_empty()
            || f.condition.trim().is_empty()
            || f.quote.chars().count() < 6
            || !spec.contains(&f.quote)
        {
            return Err(AppError::Parse(format!(
                "特征{}重复/维度不合法/缺内容/quote无法逐字定位",
                f.id
            )));
        }
    }
    Ok(v)
}

pub(crate) fn parse_comparison(raw: &str, a: &View, b: &View) -> Result<Comparison, AppError> {
    let mut value: Value =
        serde_json::from_str(raw).map_err(|e| AppError::Parse(format!("比较JSON: {e}")))?;
    // Lossless compatibility: the model sometimes includes an ID's dimension and reason.
    // Preserve those explanations in notes; do not invent or remove a relation.
    let mut explanations = Vec::new();
    for (key, side, view) in [("uncertain_left", "A", a), ("uncertain_right", "B", b)] {
        let list = value[key]
            .as_array_mut()
            .ok_or_else(|| AppError::Parse("未知项须为数组".into()))?;
        for item in list {
            if let Some(object) = item.as_object() {
                if object.len() < 2
                    || !object.keys().all(|k| {
                        [
                            "id",
                            "dimension",
                            "reason",
                            "left_candidate",
                            "right_candidate",
                        ]
                        .contains(&k.as_str())
                    })
                {
                    return Err(AppError::Parse(
                        "未知项对象需id/reason，另可含dimension或左右候选ID".into(),
                    ));
                }
                let get = |key: &str| {
                    object
                        .get(key)
                        .and_then(Value::as_str)
                        .filter(|v| !v.trim().is_empty())
                        .ok_or_else(|| AppError::Parse("未知项对象字段须为非空字符串".into()))
                };
                let id = get("id")?;
                get("reason")?;
                if !view.features.iter().any(|f| {
                    f.id == id
                        && (!object.contains_key("dimension")
                            || object["dimension"].as_str() == Some(f.dimension.as_str()))
                }) {
                    return Err(AppError::Parse("未知项对象的ID或维度与输入不符".into()));
                }
                for (key, candidates) in [("left_candidate", a), ("right_candidate", b)] {
                    if object.contains_key(key)
                        && !candidates
                            .features
                            .iter()
                            .any(|f| object[key].as_str() == Some(f.id.as_str()))
                    {
                        return Err(AppError::Parse(format!("未知项{id}的{key}不在输入中")));
                    }
                }
                explanations.push(format!("{side} unknown: {}", Value::Object(object.clone())));
                *item = json!(id);
            }
        }
    }
    if !explanations.is_empty() {
        let notes = value["notes"]
            .as_array_mut()
            .ok_or_else(|| AppError::Parse("notes须为数组".into()))?;
        notes.extend(explanations.into_iter().map(Value::String));
    }
    let c: Comparison =
        serde_json::from_value(value).map_err(|e| AppError::Parse(format!("比较JSON: {e}")))?;
    let left: HashMap<_, _> = a.features.iter().map(|f| (f.id.as_str(), f)).collect();
    let right: HashMap<_, _> = b.features.iter().map(|f| (f.id.as_str(), f)).collect();
    let mut used_a = HashSet::new();
    let mut used_b = HashSet::new();
    let mut errors = Vec::new();
    for p in c.matches.iter().chain(&c.conflicts) {
        let (Some(fa), Some(fb)) = (left.get(p.left.as_str()), right.get(p.right.as_str())) else {
            errors.push(format!("关系A.{}—B.{}引用了不存在的ID", p.left, p.right));
            continue;
        };
        if fa.dimension != fb.dimension {
            errors.push(format!(
                "关系A.{}({})—B.{}({})跨维度；输入维度不可改写",
                p.left, fa.dimension, p.right, fb.dimension
            ));
        }
        if !used_a.insert(p.left.as_str()) {
            errors.push(format!(
                "A.{}在关系中重复使用；每个ID只能参与一个关系",
                p.left
            ));
        }
        if !used_b.insert(p.right.as_str()) {
            errors.push(format!(
                "B.{}在关系中重复使用；每个ID只能参与一个关系",
                p.right
            ));
        }
        if p.reason.trim().is_empty() {
            errors.push(format!("关系A.{}—B.{}缺少依据", p.left, p.right));
        }
    }
    for id in &c.uncertain_left {
        if !left.contains_key(id.as_str()) || !used_a.insert(id.as_str()) {
            errors.push(format!("A侧未知项{id}不存在或已被使用"));
        }
    }
    for id in &c.uncertain_right {
        if !right.contains_key(id.as_str()) || !used_b.insert(id.as_str()) {
            errors.push(format!("B侧未知项{id}不存在或已被使用"));
        }
    }
    if !errors.is_empty() {
        return Err(AppError::Parse(format!(
            "比较结构违规：{}",
            errors.join("；")
        )));
    }
    Ok(c)
}

pub(crate) fn metrics(a: &View, b: &View, c: &Comparison) -> Vec<Value> {
    DIMENSIONS.iter().map(|d|{
        let aa:HashSet<_>=a.features.iter().filter(|f|f.dimension==*d).map(|f|f.id.as_str()).collect();
        let bb:HashSet<_>=b.features.iter().filter(|f|f.dimension==*d).map(|f|f.id.as_str()).collect();
        let n=c.matches.iter().filter(|p|aa.contains(p.left.as_str())).count();
        let conflicts=c.conflicts.iter().filter(|p|aa.contains(p.left.as_str())).count();
        let denominator=aa.len()+bb.len()-n;
        json!({"dimension":d,"left_count":aa.len(),"right_count":bb.len(),"matched":n,"conflicts":conflicts,
            "overlap":if denominator==0 {None} else {Some(n as f64/denominator as f64)},
            "coverage":if aa.is_empty()&&bb.is_empty(){"neither"}else if aa.is_empty()||bb.is_empty(){"one_sided"}else{"both"},
            "uncertain_left":c.uncertain_left.iter().filter(|id|aa.contains(id.as_str())).count(),
            "uncertain_right":c.uncertain_right.iter().filter(|id|bb.contains(id.as_str())).count()})
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rich_uncertainty_keeps_ids_and_reasons_without_changing_matches() {
        let a = view();
        let raw = r#"{"matches":[],"conflicts":[],"uncertain_left":[{"id":"f1","dimension":"voice","reason":"无法确定是否同义"}],"uncertain_right":["f1"],"notes":[]}"#;
        let parsed = parse_comparison(raw, &a, &a).unwrap();
        assert_eq!(parsed.uncertain_left, vec!["f1"]);
        assert!(parsed.matches.is_empty());
        assert!(parsed.conflicts.is_empty());
        assert!(parsed.notes[0].contains("无法确定是否同义"));
        assert!(parse_comparison(
            &raw.replace("\"dimension\":\"voice\"", "\"dimension\":\"rhythm\""),
            &a,
            &a
        )
        .is_err());
        let optional = r#"{"matches":[],"conflicts":[],"uncertain_left":[{"id":"f1","right_candidate":"f1","reason":"仅作未决候选"}],"uncertain_right":[],"notes":[]}"#;
        let parsed = parse_comparison(optional, &a, &a).unwrap();
        assert!(parsed.matches.is_empty());
        assert_eq!(parsed.uncertain_left, vec!["f1"]);
        assert!(parsed.notes[0].contains("right_candidate"));
        assert!(parse_comparison(&optional.replace("\"id\":\"f1\",", ""), &a, &a).is_err());
    }
    #[test]
    fn one_repair_receives_all_structural_violations() {
        let a = view();
        let mut b = view();
        b.features[0].dimension = "rhythm".into();
        let raw = r#"{"matches":[{"left":"f1","right":"f1","reason":"假定等价"},{"left":"f1","right":"f1","reason":"重复"}],"conflicts":[],"uncertain_left":[],"uncertain_right":[],"notes":[]}"#;
        let error = parse_comparison(raw, &a, &b).err().unwrap().to_string();
        assert!(error.contains("跨维度"));
        assert!(error.contains("A.f1在关系中重复"));
        assert!(error.contains("B.f1在关系中重复"));
    }
    #[test]
    fn indexed_quotes_preserve_exact_words_and_reject_invalid_ranges() {
        let spec = "# 规则\n\n谈主张时像当面商量，不冒充客观公论。\n";
        let raw = r#"{"features":[{"id":"f1","dimension":"voice","rule":"像当面商量","condition":"谈主张时","line_start":3,"line_end":3}],"uncertainties":[]}"#;
        let v = parse_indexed_view(raw, spec).unwrap();
        assert_eq!(v.features[0].quote, "谈主张时像当面商量，不冒充客观公论。");
        assert!(
            parse_indexed_view(&raw.replace("\"line_start\":3", "\"line_start\":0"), spec).is_err()
        );
        assert!(
            parse_indexed_view(&raw.replace("\"line_end\":3", "\"line_end\":9"), spec).is_err()
        );
    }
    fn view() -> View {
        View {
            features: vec![Feature {
                id: "f1".into(),
                dimension: "voice".into(),
                rule: "保持距离".into(),
                condition: "未限定".into(),
                quote: "保持平静的叙述距离".into(),
            }],
            uncertainties: vec![],
        }
    }
    #[test]
    fn unknown_dimensions_are_not_perfect_matches_and_conflicts_stay_separate() {
        let a = view();
        let c = Comparison {
            matches: vec![],
            conflicts: vec![Link {
                left: "f1".into(),
                right: "f1".into(),
                reason: "相同条件反向".into(),
            }],
            uncertain_left: vec![],
            uncertain_right: vec![],
            notes: vec![],
        };
        let c = parse_comparison(&serde_json::to_string(&c).unwrap(), &a, &a).unwrap();
        let m = metrics(&a, &a, &c);
        assert!(m.iter().find(|r| r["dimension"] == "title").unwrap()["overlap"].is_null());
        let v = m.iter().find(|r| r["dimension"] == "voice").unwrap();
        assert_eq!(v["conflicts"], 1);
        assert_eq!(v["overlap"], 0.0);
    }
    #[test]
    fn fabricated_quotes_and_many_to_one_matches_are_rejected() {
        let a = view();
        assert!(parse_view(&serde_json::to_string(&a).unwrap(), "不同原文").is_err());
        parse_view(&serde_json::to_string(&a).unwrap(), "保持平静的叙述距离").unwrap();
        let c = Comparison {
            matches: vec![
                Link {
                    left: "f1".into(),
                    right: "f1".into(),
                    reason: "同义".into(),
                },
                Link {
                    left: "f1".into(),
                    right: "f1".into(),
                    reason: "重复".into(),
                },
            ],
            conflicts: vec![],
            uncertain_left: vec![],
            uncertain_right: vec![],
            notes: vec![],
        };
        assert!(parse_comparison(&serde_json::to_string(&c).unwrap(), &a, &a).is_err());
    }
}
