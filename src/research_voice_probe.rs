//! Same-condition before/after/control Writer diagnostics, with immutable raw drafts.
use crate::{
    error::AppError,
    eval_store::write_json,
    generate::style_system_prompt,
    io::write_file_atomic,
    research_compare::finish_tasks,
    research_dataset::sha256,
    research_store::{ResearchStore, Role},
};
use serde_json::{Value, json};
use std::{path::Path, sync::Arc};

const ROOT: &str = "research/style-aggregation";

pub(crate) async fn run(run_id: &str) -> Result<(), AppError> {
    let before = Path::new(ROOT).join("runs/voice-learning-core-01");
    let after = Path::new(ROOT).join("runs/voice-learning-scope-recovered-01");
    let summary: Value = serde_json::from_slice(&std::fs::read(after.join("summary.json"))?)
        .map_err(|e| AppError::Parse(e.to_string()))?;
    if summary["status"] != "complete" || summary["rows"].as_array().is_none_or(|r| r.len() != 6) {
        return Err(AppError::Parse("范围修订六组尚未完整，未启动用稿".into()));
    }
    let task_bytes = std::fs::read(format!("{ROOT}/datasets/voice-probe-tasks-v1.json"))?;
    let suite: Value =
        serde_json::from_slice(&task_bytes).map_err(|e| AppError::Parse(e.to_string()))?;
    let tasks = suite["tasks"]
        .as_array()
        .filter(|t| t.len() == 2)
        .ok_or_else(|| AppError::Parse("须保留冻结两题".into()))?;
    let policy = std::fs::read_to_string(format!("{ROOT}/prompts/writer-closed-facts-v1.md"))?;
    let mut variants = Vec::new();
    for id in ["wxb-k1", "wxb-k10", "wzq-k1", "wzq-k10"] {
        for (condition, root) in [("before", &before), ("after", &after)] {
            let spec = std::fs::read_to_string(root.join(format!("profiles/{id}.md")))?;
            let saved: Value =
                serde_json::from_slice(&std::fs::read(root.join(format!("profiles/{id}.json")))?)
                    .map_err(|e| AppError::Parse(e.to_string()))?;
            if saved["result"]["spec"].as_str() != Some(&spec) {
                return Err(AppError::Parse("Writer候选阅读副本不一致".into()));
            }
            variants.push(json!({"style_id":id,"condition":condition,"spec":spec,
                "source_run":root.file_name().and_then(|s|s.to_str())}));
        }
    }
    variants.push(json!({"style_id":"neutral","condition":"control","spec":"","source_run":null}));
    let mut jobs = Vec::new();
    for variant in variants {
        let spec = variant["spec"]
            .as_str()
            .ok_or_else(|| AppError::Parse("缺spec".into()))?;
        let system = format!("{}\n\n本次事实约束：\n{policy}", style_system_prompt(spec));
        for task in tasks {
            let topic = task["id"]
                .as_str()
                .ok_or_else(|| AppError::Parse("缺题目id".into()))?;
            let user = format!(
                "{}\n素材：{}\n要求：{}",
                suite["instruction"].as_str().unwrap_or(""),
                task["facts"].as_str().unwrap_or(""),
                task["requirements"].as_str().unwrap_or("")
            );
            jobs.push(
                json!({"id":format!("{}-{}-{topic}",variant["style_id"].as_str().unwrap_or(""),
                variant["condition"].as_str().unwrap_or("")),"style":variant,"topic":topic,
                "system":system,"user":user}),
            );
        }
    }
    let plan_path = Path::new(ROOT)
        .join("runs")
        .join(run_id)
        .join("interpretation-plan.md");
    let plan = std::fs::read(&plan_path)?;
    let store = Arc::new(ResearchStore::open(Path::new(ROOT), run_id, false)?);
    store.limit_run(1_000_000).await?;
    store.freeze(&json!({"suite":"voice_scope_probe_v1","jobs":jobs,
        "tasks_sha256":sha256(&task_bytes),"interpretation_plan_sha256":sha256(&plan),
        "model":Role::Writer.model(),"run_cap_micro_cny":1_000_000}))?;
    std::fs::create_dir_all(store.root.join("articles"))?;
    let mut tasks = tokio::task::JoinSet::<Result<Value, AppError>>::new();
    for job in jobs {
        let store = Arc::clone(&store);
        tasks.spawn(async move {
            let id = job["id"]
                .as_str()
                .ok_or_else(|| AppError::Parse("缺稿件id".into()))?;
            let result = store
                .call(
                    id,
                    Role::Writer,
                    job["system"].as_str().unwrap_or(""),
                    job["user"].as_str().unwrap_or(""),
                    false,
                )
                .await;
            match result {
                Ok(body) => {
                    write_json(
                        &store.root.join(format!("articles/{id}.json")),
                        &json!({"job":job,"body":body}),
                    )?;
                    write_file_atomic(&store.root.join(format!("articles/{id}.md")), &body)?;
                    Ok(json!({"id":id,"status":"complete"}))
                }
                Err(error) => Ok(json!({"id":id,"status":"failed","error":error.to_string()})),
            }
        });
    }
    finish_tasks(&store, &mut tasks, "voice_scope_probe").await
}
