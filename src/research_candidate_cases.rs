//! Preregistered single-candidate diagnostic, isolated from matrix scores.
use crate::{
    error::AppError,
    eval_store::write_json,
    research_store::{ResearchStore, Role},
    research_validation::validated_as,
    style_candidates::{self as sc, Candidate, Label},
    style_compare::Feature,
};
use serde::Deserialize;
use serde_json::json;
use std::path::Path;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    id: String,
    left: Feature,
    right: Feature,
    expected: Label,
    reason: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Suite {
    schema: u8,
    description: String,
    cases: Vec<Case>,
}

pub(crate) async fn run(run: &str, file: &str) -> Result<(), AppError> {
    let raw = std::fs::read_to_string(file)?;
    let suite: Suite = serde_json::from_str(&raw).map_err(|e| AppError::Parse(e.to_string()))?;
    if suite.schema != 1
        || suite.description.trim().is_empty()
        || suite.cases.is_empty()
        || suite.cases.len() > 40
    {
        return Err(AppError::Parse("诊断套件数量或说明无效".into()));
    }
    let mut ids = std::collections::HashSet::new();
    for c in &suite.cases {
        if !crate::research_dataset::valid_id(&c.id)
            || !ids.insert(&c.id)
            || c.reason.trim().is_empty()
            || c.left.dimension != c.right.dimension
        {
            return Err(AppError::Parse("诊断样本ID或维度无效".into()));
        }
    }
    let root = Path::new("research/style-aggregation");
    let prompt = std::fs::read_to_string(root.join("prompts/compare-candidates-v3.md"))?;
    let store = ResearchStore::open(root, run, false)?;
    store.limit_run(1_000_000).await?;
    store.freeze(&json!({"suite":"single_candidate_diagnostic_v1","cases_file":file,"cases_json":raw,"prompt":prompt,
        "model":"mimo-v2.6-pro","thinking":"enabled","max_completion_tokens":32768,"run_cap_micro_cny":1000000}))?;
    let mut rows = Vec::new();
    for case in suite.cases {
        let candidate = vec![Candidate {
            candidate_id: "c0000".into(),
            left: case.left,
            right: case.right,
        }];
        let input = json!({"candidates":candidate}).to_string();
        let outcome = validated_as(
            &store,
            &case.id,
            Role::CandidateAnalysis,
            &prompt,
            &input,
            |s| sc::parse(s, &candidate),
        )
        .await;
        let row = match outcome {
            Ok(js) => {
                json!({"id":case.id,"status":"complete","passed":js[0].label==case.expected,"expected":case.expected,"expected_reason":case.reason,"judgment":js[0]})
            }
            Err(e) => json!({"id":case.id,"status":"failed","passed":false,"error":e.to_string()}),
        };
        println!("single candidate {}: {}", case.id, row["passed"]);
        rows.push(row);
        write_json(
            &store.root.join("summary.json"),
            &json!({"status":"running","rows":rows,"usage":store.usage().await}),
        )?;
    }
    let passed = rows.iter().all(|r| r["passed"] == true);
    write_json(
        &store.root.join("summary.json"),
        &json!({"status":if passed{"complete"}else{"evaluation_invalid"},"rows":rows,"usage":store.usage().await,
        "limitation":"实现者预期标签的开发诊断，不是独立人评；不是完整矩阵采用依据"}),
    )
}
