//! Candidate-level semantics; combinatorial constraints are enforced without an LLM.
use crate::{
    error::AppError,
    style_compare::{self, Comparison, Feature, Link, View},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Serialize)]
pub(crate) struct Candidate {
    pub candidate_id: String,
    pub left: Feature,
    pub right: Feature,
}
#[derive(Clone, Deserialize, Serialize, PartialEq, Debug)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Label {
    Equivalent,
    Opposite,
    Distinct,
    Uncertain,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Judgment {
    pub candidate_id: String,
    pub label: Label,
    pub reason: String,
}

pub(crate) fn candidates(a: &View, b: &View) -> Vec<Candidate> {
    let mut out = Vec::new();
    for left in &a.features {
        for right in &b.features {
            if left.dimension == right.dimension {
                out.push(Candidate {
                    candidate_id: format!("c{:04}", out.len()),
                    left: left.clone(),
                    right: right.clone(),
                });
            }
        }
    }
    out
}

pub(crate) fn parse(raw: &str, expected: &[Candidate]) -> Result<Vec<Judgment>, AppError> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Response {
        judgments: Vec<Judgment>,
    }
    let value: Response =
        serde_json::from_str(raw).map_err(|e| AppError::Parse(format!("候选JSON: {e}")))?;
    let ids: BTreeSet<_> = expected.iter().map(|c| c.candidate_id.as_str()).collect();
    let mut seen = BTreeSet::new();
    for j in &value.judgments {
        if !ids.contains(j.candidate_id.as_str())
            || !seen.insert(j.candidate_id.as_str())
            || j.reason.trim().is_empty()
        {
            return Err(AppError::Parse(format!(
                "候选{}非法、重复/矛盾或缺理由",
                j.candidate_id
            )));
        }
    }
    if seen != ids {
        return Err(AppError::Parse(format!(
            "候选遗漏: {:?}",
            ids.difference(&seen).collect::<Vec<_>>()
        )));
    }
    Ok(value.judgments)
}

fn augment(
    left: &str,
    edges: &BTreeMap<String, BTreeSet<String>>,
    assigned: &mut BTreeMap<String, String>,
    seen: &mut BTreeSet<String>,
) -> bool {
    for right in &edges[left] {
        if !seen.insert(right.clone()) {
            continue;
        }
        let previous = assigned.get(right).cloned();
        if previous.is_none() || augment(previous.as_ref().unwrap(), edges, assigned, seen) {
            assigned.insert(right.clone(), left.to_owned());
            return true;
        }
    }
    false
}

pub(crate) fn maximum_matching(pairs: &[(String, String)]) -> Vec<(String, String)> {
    let mut edges = BTreeMap::<String, BTreeSet<String>>::new();
    for (a, b) in pairs {
        edges.entry(a.clone()).or_default().insert(b.clone());
    }
    let mut assigned = BTreeMap::new();
    for left in edges.keys() {
        augment(left, &edges, &mut assigned, &mut BTreeSet::new());
    }
    let mut result: Vec<_> = assigned.into_iter().map(|(b, a)| (a, b)).collect();
    result.sort();
    result
}

pub(crate) fn summarize(
    a: &View,
    b: &View,
    all: &[Candidate],
    js: &[Judgment],
) -> Result<Value, AppError> {
    // Validate again after joining batches: no missing or conflicting candidate can disappear.
    parse(&json!({"judgments":js}).to_string(), all)?;
    let by_id: BTreeMap<_, _> = js.iter().map(|j| (j.candidate_id.as_str(), j)).collect();
    let equivalent: Vec<_> = all
        .iter()
        .filter(|c| by_id[c.candidate_id.as_str()].label == Label::Equivalent)
        .collect();
    let edges: Vec<_> = equivalent
        .iter()
        .map(|c| (c.left.id.clone(), c.right.id.clone()))
        .collect();
    let selected = maximum_matching(&edges);
    let matches: Vec<_> = selected
        .iter()
        .map(|(l, r)| {
            let c = equivalent
                .iter()
                .find(|c| c.left.id == *l && c.right.id == *r)
                .unwrap();
            Link {
                left: l.clone(),
                right: r.clone(),
                reason: by_id[c.candidate_id.as_str()].reason.clone(),
            }
        })
        .collect();
    let mut metrics = style_compare::metrics(
        a,
        b,
        &Comparison {
            matches,
            conflicts: vec![],
            uncertain_left: vec![],
            uncertain_right: vec![],
            notes: vec![],
        },
    );
    for axis in &mut metrics {
        for key in ["conflicts", "uncertain_left", "uncertain_right"] {
            axis.as_object_mut().unwrap().remove(key);
        }
    }
    let mut axes = Vec::new();
    for dim in style_compare::DIMENSIONS {
        let conflicts: Vec<_> = all
            .iter()
            .filter(|c| {
                c.left.dimension == dim && by_id[c.candidate_id.as_str()].label == Label::Opposite
            })
            .collect();
        axes.push(json!({"dimension":dim,"opposite_edges":conflicts.len(),
            "opposite_left_features":conflicts.iter().map(|c|&c.left.id).collect::<BTreeSet<_>>().len(),
            "opposite_right_features":conflicts.iter().map(|c|&c.right.id).collect::<BTreeSet<_>>().len(),
            "uncertain_edges":all.iter().filter(|c|c.left.dimension==dim && by_id[c.candidate_id.as_str()].label==Label::Uncertain).count()}));
    }
    Ok(
        json!({"metric_version":"candidate_maximum_overlap_v2","selected_equivalent_edges":selected,
        "equivalent_candidate_count":equivalent.len(),"overlap_only":metrics,"candidate_conflicts_and_uncertainty":axes,
        "note":"候选冲突和未知独立报告；未选边保留在judgments；不混用旧版冲突计数"}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matching_reassigns_and_cardinality_survives_transpose() {
        let edges = vec![
            ("a1".into(), "b1".into()),
            ("a1".into(), "b2".into()),
            ("a2".into(), "b1".into()),
        ];
        assert_eq!(maximum_matching(&edges).len(), 2);
        assert_eq!(
            maximum_matching(
                &edges
                    .iter()
                    .map(|(a, b)| (b.clone(), a.clone()))
                    .collect::<Vec<_>>()
            )
            .len(),
            2
        );
        assert!(maximum_matching(&[]).is_empty());
        assert_eq!(
            maximum_matching(&vec![edges[0].clone(), edges[0].clone(), edges[1].clone()]).len(),
            1
        );
    }
    #[test]
    fn missing_duplicate_and_contradictory_candidates_fail() {
        let f = Feature {
            id: "f".into(),
            dimension: "voice".into(),
            rule: "r".into(),
            condition: "all".into(),
            quote: "r".into(),
        };
        let c = vec![Candidate {
            candidate_id: "c0000".into(),
            left: f.clone(),
            right: f,
        }];
        let j = json!({"candidate_id":"c0000","label":"distinct","reason":"different"});
        assert!(parse(&json!({"judgments":[]}).to_string(), &c).is_err());
        assert!(parse(&json!({"judgments":[j,j]}).to_string(), &c).is_err());
        let mut opposite = j.clone();
        opposite["label"] = json!("opposite");
        assert!(parse(&json!({"judgments":[j,opposite]}).to_string(), &c).is_err());
        assert_eq!(
            parse(&json!({"judgments":[j]}).to_string(), &c).unwrap()[0].label,
            Label::Distinct
        );
    }
}
