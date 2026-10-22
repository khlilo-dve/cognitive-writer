use crate::{
    error::AppError,
    eval_store::write_json,
    generate::{outline_system_prompt, render_prompt, style_system_prompt},
    research_compare::finish_tasks,
    research_dataset::{Dataset, sha256, valid_id},
    research_profile::{self, ROOT},
    research_store::{ResearchStore, Role},
    transfer_quality::length_check,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::{BTreeMap, BTreeSet}, path::Path, sync::Arc};

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Job {
    pub id: String,
    pub author: String,
    pub method: String,
    pub style_id: String,
    pub source_articles: Vec<String>,
    pub topic_id: String,
    pub phase: String,
    pub replicate: usize,
    pub spec: String,
    pub task: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Generated {
    pub job: Job,
    pub outline: String,
    pub body: String,
    pub length: Value,
}

pub(crate) async fn run(run: &str, profiles: &str, single: &str) -> Result<(), AppError> {
    if !valid_id(single) {
        return Err(AppError::Parse("首次Style run无效".into()));
    }
    let artifacts = research_profile::load(profiles)?;
    let profile_root = Path::new(ROOT).join("runs").join(profiles);
    let profile_inputs: Value =
        serde_json::from_slice(&std::fs::read(profile_root.join("inputs.json"))?)
            .map_err(|e| AppError::Parse(e.to_string()))?;
    let exploratory = ["pragmatic_profile_v1", "pragmatic_profile_salvage_v1"]
        .contains(&profile_inputs["suite"].as_str().unwrap_or(""));
    let mut profile_review = Value::Null;
    if exploratory {
        profile_review =
            serde_json::from_slice(&std::fs::read(profile_root.join("semantic-review.json"))?)
                .map_err(|e| AppError::Parse(e.to_string()))?;
        let mut identities = Vec::new();
        for artifact in &artifacts {
            let path = profile_root.join(format!("profiles/{}.json", artifact.plan.id));
            identities.push(format!(
                "{}:{}",
                artifact.plan.id,
                sha256(&std::fs::read(path)?)
            ));
        }
        let hash = sha256(identities.join("|").as_bytes());
        if profile_review["decision"] != "exploratory_only"
            || profile_review["artifacts_sha256"] != hash
        {
            return Err(AppError::Parse(
                "探索性Profile尚未逐条复核或输入身份变化".into(),
            ));
        }
    }
    let manifest = Path::new(ROOT).join("datasets/authors-v1/manifest.json");
    let ds = Dataset::load(&manifest)?;
    let sampling: Value =
        serde_json::from_slice(&std::fs::read(format!("{ROOT}/datasets/sampling-v1.json"))?)
            .map_err(|e| AppError::Parse(e.to_string()))?;
    if sampling["manifest_sha256"] != sha256(&std::fs::read(&manifest)?) {
        return Err(AppError::Parse("生成抽样manifest身份变化".into()));
    }
    let tasks: Value = serde_json::from_slice(&std::fs::read(format!(
        "{ROOT}/datasets/generation-tasks-v1.json"
    ))?)
    .map_err(|e| AppError::Parse(e.to_string()))?;
    let topics = tasks["tasks"]
        .as_array()
        .ok_or_else(|| AppError::Parse("生成任务缺失".into()))?;
    if topics.len() != 3 {
        return Err(AppError::Parse("要求3个固定主题".into()));
    }
    let mut styles = BTreeMap::new();
    for full in artifacts
        .iter()
        .filter(|a| a.plan.mode == "primary" && a.plan.articles.len() == 10)
    {
        if full.spec.trim().is_empty() {
            return Err(AppError::Parse(
                "全量Profile无可执行Style；不将空Style冒充聚合实验".into(),
            ));
        }
        let author = &full.plan.author;
        let mut entries = vec![(
            "aggregate".to_owned(),
            full.plan.id.clone(),
            full.spec.clone(),
            full.plan.articles.clone(),
        )];
        let baselines = sampling["authors"][author]["generation_baselines"]
            .as_object()
            .ok_or_else(|| AppError::Parse("缺少冻结生成基线".into()))?;
        if baselines.len() != 3 {
            return Err(AppError::Parse("每作者要求3份生成基线".into()));
        }
        for (topic, id) in baselines {
            let id = id
                .as_str()
                .ok_or_else(|| AppError::Parse("基线ID无效".into()))?;
            if !ds
                .selected(false)
                .iter()
                .any(|a| a.article_id == id && a.author_id == *author && a.topic == *topic)
            {
                return Err(AppError::Parse(
                    "生成基线不是预定作者/主题的discovery".into(),
                ));
            }
            let saved: Value = serde_json::from_slice(&std::fs::read(format!(
                "{ROOT}/runs/{single}/single/{id}.json"
            ))?)
            .map_err(|e| AppError::Parse(e.to_string()))?;
            let spec = std::fs::read_to_string(format!("{ROOT}/runs/{single}/single/{id}.md"))?;
            if saved["result"]["spec"] != spec {
                return Err(AppError::Parse("单篇基线与保存结果不同".into()));
            }
            entries.push(("single".into(), id.into(), spec, vec![id.into()]));
        }
        styles.insert(author.clone(), entries);
    }
    if styles.len() != 2 {
        return Err(AppError::Parse("需要两个全量作者Profile".into()));
    }
    let mut jobs = Vec::new();
    let task_text = |t: &Value| {
        format!(
            "{}\n{}\n主题：{}\n核心素材：{}\n必须覆盖：{}",
            tasks["length"].as_str().unwrap_or(""),
            tasks["permission"].as_str().unwrap_or(""),
            t["title"].as_str().unwrap_or(""),
            t["facts"].as_str().unwrap_or(""),
            t["requirements"].as_str().unwrap_or("")
        )
    };
    for (author, entries) in &styles {
        for (method, style_id, spec, source_articles) in entries {
            for t in topics {
                for replicate in 1..=2 {
                    jobs.push(Job {
                        id: format!("article-{:03}", jobs.len() + 1),
                        author: author.clone(),
                        method: method.clone(),
                        style_id: style_id.clone(),
                        source_articles: source_articles.clone(),
                        topic_id: field(t, "id")?,
                        phase: field(t, "phase")?,
                        replicate,
                        spec: spec.clone(),
                        task: task_text(t),
                    });
                }
            }
        }
    }
    let authors: Vec<_> = styles.keys().cloned().collect();
    for t in topics {
        for replicate in 1..=4 {
            jobs.push(Job {
                id: format!("article-{:03}", jobs.len() + 1),
                author: authors[(replicate - 1) % 2].clone(),
                method: "neutral".into(),
                style_id: "none".into(),
                source_articles: vec![],
                topic_id: field(t, "id")?,
                phase: field(t, "phase")?,
                replicate,
                spec: "未指定特定写作风格，按素材写自然连贯的中文，不模仿名家。".into(),
                task: task_text(t),
            });
        }
    }
    if jobs.len() != 60 {
        return Err(AppError::Parse("生成任务应为48主对照+12无风格".into()));
    }
    // Freeze before seeing any generated outputs or opening confirmation references.
    let store = Arc::new(ResearchStore::open(Path::new(ROOT), run, false)?);
    if exploratory {
        store.limit_run(15_000_000).await?;
    }
    let mut frozen = json!({"suite":"style_generation_v1","profile_run":profiles,"single_run":single,"tasks":tasks,"jobs":jobs,"outline_wrapper":outline_system_prompt("STYLE"),"body_wrapper":style_system_prompt("STYLE"),"render_wrapper":render_prompt("OUTLINE","TASK"),"writer":"mimo-v2.6-flash"});
    if exploratory {
        frozen["suite"] = json!("style_generation_v1_exploratory");
        frozen["exploratory_profile_review"] = profile_review;
        frozen["run_cap_micro_cny"] = json!(15_000_000);
    }
    store.freeze(&frozen)?;
    std::fs::create_dir_all(store.root.join("articles"))?;
    jobs.sort_by_key(|j| sha256(format!("17:{}", j.id).as_bytes()));
    let mut pending = tokio::task::JoinSet::new();
    for job in jobs {
        let store = Arc::clone(&store);
        pending.spawn(async move {
        let result=generate(&store,job.clone()).await;
        match result {Ok(a)=>{write_json(&store.root.join(format!("articles/{}.json",job.id)),&a)?;crate::io::write_file_atomic(&store.root.join(format!("articles/{}.md",job.id)),&a.body)?;Ok::<_,AppError>(json!({"id":job.id,"status":"complete","author":job.author,"method":job.method,"topic":job.topic_id,"phase":job.phase,"replicate":job.replicate,"length":a.length}))},Err(e)=>Ok(json!({"id":job.id,"status":"failed","error":e.to_string()}))}
    });
    }
    finish_tasks(&store, &mut pending, "generation").await
}

/// Run a small, preregistered transfer pilot using the same Flash writer path.
pub(crate) async fn run_frozen(run: &str, plan_path: &str) -> Result<(), AppError> {
    let frozen: Value = serde_json::from_slice(&std::fs::read(plan_path)?)
        .map_err(|e| AppError::Parse(e.to_string()))?;
    if frozen["suite"] != "detailed_style_transfer_pilot_v1"
        || frozen["writer"] != Role::Writer.model()
        || frozen["run_cap_micro_cny"] != 1_000_000
    {
        return Err(AppError::Parse("详细风格试写计划版本、模型或费用上限无效".into()));
    }
    let jobs: Vec<Job> = serde_json::from_value(frozen["jobs"].clone())
        .map_err(|e| AppError::Parse(e.to_string()))?;
    validate_frozen_jobs(&jobs)?;
    let store = Arc::new(ResearchStore::open(Path::new(ROOT), run, false)?);
    store.limit_run(1_000_000).await?;
    store.freeze(&frozen)?;
    std::fs::create_dir_all(store.root.join("articles"))?;
    let mut pending = tokio::task::JoinSet::new();
    for job in jobs {
        let store = Arc::clone(&store);
        pending.spawn(async move {
            let result = generate(&store, job.clone()).await;
            match result {
                Ok(article) => {
                    write_json(&store.root.join(format!("articles/{}.json", job.id)), &article)?;
                    crate::io::write_file_atomic(
                        &store.root.join(format!("articles/{}.md", job.id)),
                        &article.body,
                    )?;
                    Ok::<_, AppError>(json!({"id":job.id,"status":"complete","author":job.author,
                        "method":job.method,"topic":job.topic_id,"phase":job.phase}))
                }
                Err(error) => Ok(json!({"id":job.id,"status":"failed","error":error.to_string()})),
            }
        });
    }
    finish_tasks(&store, &mut pending, "detailed_style_transfer_pilot").await
}

fn validate_frozen_jobs(jobs: &[Job]) -> Result<(), AppError> {
    let mut identities = BTreeSet::new();
    let mut ids = BTreeSet::new();
    let mut tasks = BTreeMap::new();
    for job in jobs {
        let identity = (job.author.as_str(), job.topic_id.as_str(), job.method.as_str());
        if !valid_id(&job.id)
            || !ids.insert(job.id.as_str())
            || !["wang_xiaobo", "wang_zengqi"].contains(&job.author.as_str())
            || !["lost_cards", "seed_box"].contains(&job.topic_id.as_str())
            || !["aggregate", "single", "neutral"].contains(&job.method.as_str())
            || job.phase != "development"
            || job.replicate != 1
            || job.spec.trim().is_empty()
            || job.task.trim().is_empty()
            || job.task.contains("600–900")
            || !identities.insert(identity)
        {
            return Err(AppError::Parse("详细风格试写身份或任务无效".into()));
        }
        if let Some(prior) = tasks.insert(job.topic_id.as_str(), job.task.as_str()) {
            if prior != job.task {
                return Err(AppError::Parse("同题各方法的任务素材不一致".into()));
            }
        }
    }
    if jobs.len() != 12 || ids.len() != 12 || identities.len() != 12 || tasks.len() != 2 {
        return Err(AppError::Parse("详细风格试写须两作者×两题×三方法".into()));
    }
    Ok(())
}

fn field(v: &Value, key: &str) -> Result<String, AppError> {
    v[key]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| AppError::Parse(format!("缺少任务字段{key}")))
}
async fn generate(store: &ResearchStore, job: Job) -> Result<Generated, AppError> {
    let outline = store
        .call(
            &format!("{}-outline", job.id),
            Role::Writer,
            &outline_system_prompt(&job.spec),
            &job.task,
            false,
        )
        .await?;
    let body = store
        .call(
            &format!("{}-body", job.id),
            Role::Writer,
            &style_system_prompt(&job.spec),
            &render_prompt(&outline, &job.task),
            false,
        )
        .await?;
    let length = length_check(&body);
    Ok(Generated {
        job,
        outline,
        body,
        length,
    })
}

pub(crate) fn load(run: &str) -> Result<Vec<Generated>, AppError> {
    if !valid_id(run) {
        return Err(AppError::Parse("生成run无效".into()));
    }
    let root = Path::new(ROOT).join("runs").join(run);
    let summary: Value = serde_json::from_slice(&std::fs::read(root.join("summary.json"))?)
        .map_err(|e| AppError::Parse(e.to_string()))?;
    if summary["status"] != "complete" || summary["rows"].as_array().is_none_or(|r| r.len() != 60) {
        return Err(AppError::Parse(
            "60篇生成尚未完成，确认参考保持未解封".into(),
        ));
    }
    let mut result = Vec::new();
    for i in 1..=60 {
        let id = format!("article-{i:03}");
        let a: Generated =
            serde_json::from_slice(&std::fs::read(root.join(format!("articles/{id}.json")))?)
                .map_err(|e| AppError::Parse(e.to_string()))?;
        if a.job.id != id
            || a.body != std::fs::read_to_string(root.join(format!("articles/{id}.md")))?
        {
            return Err(AppError::Parse("生成产物身份变化".into()));
        }
        result.push(a);
    }
    Ok(result)
}

#[cfg(test)]
mod frozen_tests {
    use super::*;

    #[test]
    fn rejects_unmatched_task_in_paid_transfer_matrix() {
        let mut jobs = Vec::new();
        for author in ["wang_xiaobo", "wang_zengqi"] {
            for topic in ["lost_cards", "seed_box"] {
                for method in ["aggregate", "single", "neutral"] {
                    jobs.push(Job { id: format!("article-{:03}", jobs.len() + 1),
                        author: author.into(), method: method.into(), style_id: method.into(),
                        source_articles: vec![], topic_id: topic.into(), phase: "development".into(),
                        replicate: 1, spec: "写作说明".into(), task: format!("共同素材：{topic}") });
                }
            }
        }
        assert!(validate_frozen_jobs(&jobs).is_ok());
        jobs[1].task.push_str("另加事实");
        assert!(validate_frozen_jobs(&jobs).is_err());
    }
}
