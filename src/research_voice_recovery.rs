//! Offline re-execution against verified, identical cached requests; no provider fallback.
use crate::{
    error::AppError,
    eval_store::write_json,
    io::write_file_atomic,
    research_dataset::{sha256, valid_id},
    research_receipt::{Checkpoint, verify},
    voice_revision::{RevisionPrompts, revise_voice_with},
};
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::{Arc, Mutex},
};

const ROOT: &str = "research/style-aggregation";

pub(crate) async fn run(run_id: &str) -> Result<(), AppError> {
    if !valid_id(run_id) || run_id == "voice-learning-scope-01" {
        return Err(AppError::Parse("恢复run身份无效".into()));
    }
    let common = Path::new(ROOT);
    let source = common.join("runs/voice-learning-scope-01");
    let source_bytes = std::fs::read(source.join("inputs.json"))?;
    let plan: Value =
        serde_json::from_slice(&source_bytes).map_err(|e| AppError::Parse(e.to_string()))?;
    let prompts: RevisionPrompts = serde_json::from_value(plan["prompts"].clone())
        .map_err(|e| AppError::Parse(e.to_string()))?;
    let jobs = plan["jobs"]
        .as_array()
        .filter(|j| j.len() == 6)
        .ok_or_else(|| AppError::Parse("恢复输入须原六组".into()))?;
    let root = common.join("runs").join(run_id);
    for folder in ["profiles", "reviews", "requests"] {
        std::fs::create_dir_all(root.join(folder))?;
    }
    let frozen = json!({"suite":"voice_scope_recovery_v2","jobs":jobs,"prompts":plan["prompts"],
        "source_run":"voice-learning-scope-01","source_inputs_sha256":sha256(&source_bytes),
        "method_sha256":sha256(&std::fs::read(common.join("method-revision-27.md"))?),"new_requests":0});
    let frozen_path = root.join("inputs.json");
    if frozen_path.exists() {
        let old: Value = serde_json::from_slice(&std::fs::read(&frozen_path)?)
            .map_err(|e| AppError::Parse(e.to_string()))?;
        if old != frozen {
            return Err(AppError::Parse("恢复输入变更，请新建run".into()));
        }
    } else {
        write_json(&frozen_path, &frozen)?;
    }
    let ledger = common.join("usage-ledger.json");
    let ledger_before = sha256(&std::fs::read(&ledger)?);
    let reused = Arc::new(Mutex::new(Vec::<Value>::new()));
    let mut rows = Vec::new();
    for job in jobs {
        let id = job["id"]
            .as_str()
            .filter(|i| valid_id(i))
            .ok_or_else(|| AppError::Parse("恢复id无效".into()))?;
        let texts: Vec<String> = serde_json::from_value(job["texts"].clone())
            .map_err(|e| AppError::Parse(e.to_string()))?;
        let draft = serde_json::from_value(job["draft"].clone())
            .map_err(|e| AppError::Parse(e.to_string()))?;
        let revision = revise_voice_with(&texts, &draft, &prompts, |stage, system, user| {
            let path = source.join(format!("requests/{id}-{stage}.json"));
            let target = root.join(format!("requests/{id}-{stage}.json"));
            let reused = Arc::clone(&reused);
            async move {
                let bytes = std::fs::read(&path)?;
                let cached: Checkpoint =
                    serde_json::from_slice(&bytes).map_err(|e| AppError::Parse(e.to_string()))?;
                verify(&cached)?;
                if cached.request["body"]["messages"]
                    != json!([
                    {"role":"system","content":system},{"role":"user","content":user}])
                    || cached.receipt.model != "mimo-v2.6-pro"
                    || cached.receipt.finish_reason != "stop"
                {
                    return Err(AppError::Parse(
                        "缓存请求/模型身份不一致，无网络回退".into(),
                    ));
                }
                write_json(&target, &cached)?;
                reused
                    .lock()
                    .map_err(|_| AppError::Parse("缓存留证锁失败".into()))?
                    .push(json!({"source":path,"source_file_sha256":sha256(&bytes)}));
                Ok(cached.receipt.content)
            }
        })
        .await?;
        write_json(
            &root.join(format!("profiles/{id}.json")),
            &json!({"id":id,
            "source_ids":job["source_ids"],"parent_run":"voice-learning-core-01",
            "response_source":"voice-learning-scope-01","parent_spec_sha256":revision.parent_spec_sha256,
            "review":revision.review,"revision_calls":revision.revision_calls,"result":revision.result}),
        )?;
        write_file_atomic(
            &root.join(format!("profiles/{id}.md")),
            &revision.result.spec,
        )?;
        write_file_atomic(&root.join(format!("reviews/{id}.md")), &revision.review)?;
        rows.push(json!({"id":id,"status":"complete","sources":texts.len(),"revision_calls":revision.revision_calls}));
    }
    let ledger_after = sha256(&std::fs::read(&ledger)?);
    if ledger_after != ledger_before {
        return Err(AppError::Parse("离线恢复期间账本变化".into()));
    }
    write_json(
        &root.join("summary.json"),
        &json!({"status":"complete","stage":"voice_scope_recovery",
        "rows":rows,"new_requests":0,"usage":{"run_occupied_cny":0.0}}),
    )?;
    let reused = reused
        .lock()
        .map_err(|_| AppError::Parse("缓存留证锁失败".into()))?;
    write_json(
        &root.join("recovery-audit.json"),
        &json!({"status":"passed","new_requests":0,
        "ledger_before_sha256":ledger_before,"ledger_after_sha256":ledger_after,"reused":*reused}),
    )?;
    println!(
        "Recovered six revisions using {} verified cached responses; zero new requests",
        reused.len()
    );
    Ok(())
}
