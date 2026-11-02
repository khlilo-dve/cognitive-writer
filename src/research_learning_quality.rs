//! Small new-topic Writer probes from immutable source-learning jobs.
use crate::{
    error::AppError,
    eval_store::write_json,
    generate::style_system_prompt,
    io::write_file_atomic,
    learning_job,
    learning_workspace::LearningWorkspace,
    research_compare::finish_tasks,
    research_dataset::{sha256, valid_id},
    research_store::{ResearchStore, Role},
};
use serde_json::{Value, json};
use std::{path::Path, sync::Arc};
const ROOT: &str = "research/style-aggregation";

pub(crate) async fn run(run_id: &str, source_id: &str) -> Result<(), AppError> {
    run_suite(run_id, source_id, false).await
}
pub(crate) async fn boundary(run_id: &str, source_id: &str) -> Result<(), AppError> {
    run_suite(run_id, source_id, true).await
}
async fn run_suite(run_id: &str, source_id: &str, boundary: bool) -> Result<(), AppError> {
    if !valid_id(source_id) {
        return Err(AppError::Parse("来源run无效".into()));
    }
    let source = Path::new(ROOT).join("runs").join(source_id);
    let summary: Value = serde_json::from_slice(&std::fs::read(source.join("summary.json"))?)
        .map_err(|e| AppError::Parse(e.to_string()))?;
    if summary["status"] != "complete" || summary["rows"].as_array().is_none_or(|r| r.len() != 2) {
        return Err(AppError::Parse("四任务来源尚未完整".into()));
    }
    let task_file = if boundary {
        "learning-boundary-tasks-v1.json"
    } else {
        "learning-quality-tasks-v1.json"
    };
    let bytes = std::fs::read(format!("{ROOT}/datasets/{task_file}"))?;
    let suite: Value =
        serde_json::from_slice(&bytes).map_err(|e| AppError::Parse(e.to_string()))?;
    let topics = suite["tasks"]
        .as_array()
        .filter(|r| r.len() == 2)
        .ok_or_else(|| AppError::Parse("固定两题缺失".into()))?;
    let policy = std::fs::read_to_string(format!("{ROOT}/prompts/writer-closed-facts-v1.md"))?;
    let mut variants = Vec::new();
    for code in ["wxb", "wzq"] {
        let ws = LearningWorkspace::open(&source.join("workspaces").join(code))?;
        for count in [1, 2] {
            let link: Value = serde_json::from_slice(&std::fs::read(
                source.join(format!("links/{code}-k{count}.json")),
            )?)
            .map_err(|e| AppError::Parse(e.to_string()))?;
            let id = link["job_id"]
                .as_str()
                .ok_or_else(|| AppError::Parse("任务链接缺失".into()))?;
            let job = learning_job::load(&ws, id)?;
            let state = learning_job::state(&ws, id)?;
            if state.status != "complete"
                || job.articles.len() != count
                || job.profile.model != "mimo-v2.6-pro"
            {
                return Err(AppError::Parse("来源任务身份不符".into()));
            }
            let candidate = ws.preview(
                state
                    .candidate_id
                    .as_deref()
                    .ok_or_else(|| AppError::Parse("缺来源候选".into()))?,
            )?;
            variants.push(json!({"id":format!("{code}-k{count}"),"source_run":source_id,"job_id":id,"candidate_id":candidate.id,"fingerprint":candidate.input.draft.fingerprint,"spec":candidate.input.draft.spec}));
        }
    }
    if !boundary {
        variants.push(json!({"id":"neutral","source_run":null,"spec":""}));
    }
    let mut jobs = Vec::new();
    for variant in variants {
        let spec = variant["spec"]
            .as_str()
            .ok_or_else(|| AppError::Parse("缺spec".into()))?;
        for topic in topics {
            let topic_id = topic["id"]
                .as_str()
                .filter(|s| valid_id(s))
                .ok_or_else(|| AppError::Parse("题目ID无效".into()))?;
            if boundary {
                let expected = if variant["id"].as_str().unwrap_or("").starts_with("wxb-") {
                    "abstract_choices"
                } else {
                    "given_feeling"
                };
                if topic_id != expected {
                    continue;
                }
            }
            let user = format!(
                "{}\n素材：{}\n要求：{}",
                suite["instruction"].as_str().unwrap_or(""),
                topic["facts"].as_str().unwrap_or(""),
                topic["requirements"].as_str().unwrap_or("")
            );
            jobs.push(json!({"id":format!("{}-{topic_id}",variant["id"].as_str().unwrap_or("")),"style":variant,"topic":topic_id,"system":format!("{}\n\n本次事实约束：\n{policy}",style_system_prompt(spec)),"user":user}));
        }
    }
    let store = Arc::new(ResearchStore::open(Path::new(ROOT), run_id, false)?);
    let cap = if boundary { 500_000 } else { 1_000_000 };
    store.limit_run(cap).await?;
    let method_file = if boundary {
        "method-revision-29.md"
    } else {
        "method-revision-28.md"
    };
    let method = std::fs::read(format!("{ROOT}/{method_file}"))?;
    store.freeze(&json!({"suite":if boundary {"learning_boundary_probe_v1"}else{"learning_quality_probe_v1"},"jobs":jobs,"tasks_sha256":sha256(&bytes),"method_sha256":sha256(&method),"model":Role::Writer.model(),"run_cap_micro_cny":cap}))?;
    std::fs::create_dir_all(store.root.join("articles"))?;
    let mut tasks = tokio::task::JoinSet::new();
    for job in jobs {
        let store = Arc::clone(&store);
        tasks.spawn(async move {
            let id = job["id"]
                .as_str()
                .ok_or_else(|| AppError::Parse("缺稿件ID".into()))?;
            match store
                .call(
                    id,
                    Role::Writer,
                    job["system"].as_str().unwrap_or(""),
                    job["user"].as_str().unwrap_or(""),
                    false,
                )
                .await
            {
                Ok(body) => {
                    write_json(
                        &store.root.join(format!("articles/{id}.json")),
                        &json!({"job":job,"body":body}),
                    )?;
                    write_file_atomic(&store.root.join(format!("articles/{id}.md")), &body)?;
                    Ok::<_, AppError>(json!({"id":id,"status":"complete"}))
                }
                Err(error) => Ok(json!({"id":id,"status":"failed","error":error.to_string()})),
            }
        });
    }
    finish_tasks(
        &store,
        &mut tasks,
        if boundary {
            "learning_boundary_writer_probe"
        } else {
            "learning_quality_writer_probe"
        },
    )
    .await
}
