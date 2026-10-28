//! Live research adapter for the reusable 1–10 article text learner.
use crate::{
    error::AppError,
    eval_store::write_json,
    io::write_file_atomic,
    research_compare::finish_tasks,
    research_dataset::{Dataset, sha256},
    research_store::{ResearchStore, Role},
    voice_learning::{VoicePrompts, learn_voice_with},
};
use serde::Serialize;
use serde_json::{Value, json};
use std::{path::Path, sync::Arc};

const ROOT: &str = "research/style-aggregation";

#[derive(Serialize)]
struct Job {
    id: String,
    source_ids: Vec<String>,
    texts: Vec<String>,
}

fn jobs() -> Result<Vec<Job>, AppError> {
    let manifest = Path::new(ROOT).join("datasets/authors-v1/manifest.json");
    let dataset: Dataset = serde_json::from_slice(&std::fs::read(&manifest)?)
        .map_err(|e| AppError::Parse(e.to_string()))?;
    let mut jobs = Vec::new();
    for (code, author, seed, triple) in [
        (
            "wxb",
            "wang_xiaobo",
            "wxb-02",
            ["wxb-02", "wxb-03", "wxb-08"],
        ),
        (
            "wzq",
            "wang_zengqi",
            "wzq-02",
            ["wzq-02", "wzq-04", "wzq-09"],
        ),
    ] {
        let all = dataset
            .articles
            .iter()
            .filter(|a| a.author_id == author && a.split == "discovery")
            .collect::<Vec<_>>();
        if all.len() != 10 {
            return Err(AppError::Parse("作者发现集必须十篇".into()));
        }
        for count in [1, 3, 10] {
            let selected = all
                .iter()
                .filter(|a| {
                    count == 10
                        || (count == 1 && a.article_id == seed)
                        || (count == 3 && triple.contains(&a.article_id.as_str()))
                })
                .copied()
                .collect::<Vec<_>>();
            if selected.len() != count {
                return Err(AppError::Parse("核心验证样本数量错误".into()));
            }
            let texts = selected
                .iter()
                .map(|a| dataset.read_article(&manifest, a))
                .collect::<Result<Vec<_>, _>>()?;
            jobs.push(Job {
                id: format!("{code}-k{count}"),
                source_ids: selected.iter().map(|a| a.article_id.clone()).collect(),
                texts,
            });
        }
    }
    Ok(jobs)
}

pub(crate) async fn run(run_id: &str) -> Result<(), AppError> {
    let jobs = jobs()?;
    let prompts = VoicePrompts::load()?;
    let store = Arc::new(ResearchStore::open(Path::new(ROOT), run_id, false)?);
    store.limit_run(4_000_000).await?;
    store.freeze(
        &json!({"suite":"voice_core_v1_live","jobs":jobs,"prompts":prompts,
        "model":Role::Structured.model(),"run_cap_micro_cny":4_000_000,
        "method_sha256":sha256(&std::fs::read(format!("{ROOT}/method-revision-25.md"))?)}),
    )?;
    std::fs::create_dir_all(store.root.join("profiles"))?;
    std::fs::create_dir_all(store.root.join("analyses"))?;
    let mut tasks = tokio::task::JoinSet::<Result<Value, AppError>>::new();
    for job in jobs {
        let store = Arc::clone(&store);
        let prompts = prompts.clone();
        tasks.spawn(async move {
            let id=job.id.clone();
            let result=learn_voice_with(&job.texts,&prompts,|stage,system,user|{
                let store=Arc::clone(&store);let name=format!("{id}-{stage}");
                async move {store.call(&name,Role::Structured,&system,&user,false).await}
            }).await;
            match result {
                Ok(draft)=>{
                    write_json(&store.root.join(format!("profiles/{id}.json")),
                        &json!({"id":id,"source_ids":job.source_ids,"result":draft}))?;
                    write_file_atomic(&store.root.join(format!("profiles/{id}.md")),&draft.spec)?;
                    write_file_atomic(&store.root.join(format!("analyses/{id}.md")),&draft.analysis)?;
                    Ok(json!({"id":id,"status":"complete","sources":draft.sources.len(),
                        "calls":draft.calls,"warnings":draft.warnings,"fingerprint":draft.fingerprint}))
                }
                Err(error)=>Ok(json!({"id":id,"status":"failed","error":error.to_string()})),
            }
        });
    }
    finish_tasks(&store, &mut tasks, "voice_core_live").await
}
