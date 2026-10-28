//! Exploratory direct prose learning; evidence cards are kept only for auditing leakage.
use crate::{
    error::AppError,
    eval_store::write_json,
    io::write_file_atomic,
    research_compare::finish_tasks,
    research_dataset::{Dataset, sha256},
    research_store::{ResearchStore, Role},
    research_voice_learning_v2::{Card, Local, Source, source_leak},
    spec_quality::validate_execution_spec,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::{collections::HashMap, path::Path, sync::Arc};

const ROOT: &str = "research/style-aggregation";
const SETS: [(&str, [&str; 2]); 2] = [("wxb", ["wxb-02", "wxb-03"]), ("wzq", ["wzq-02", "wzq-04"])];

#[derive(Clone, Serialize)]
pub(crate) struct Job {
    pub id: String,
    pub sources: Vec<Source>,
    pub cards: HashMap<String, Local>,
}

pub(crate) fn jobs() -> Result<Vec<Job>, AppError> {
    let manifest = Path::new(ROOT).join("datasets/authors-v1/manifest.json");
    let dataset = Dataset::load(&manifest)?;
    let card_root = Path::new(ROOT).join("runs/voice-learning-vertical-01/cards");
    let mut jobs = Vec::new();
    for (code, ids) in SETS {
        let mut sources = Vec::new();
        let mut cards = HashMap::new();
        for (index, id) in ids.iter().enumerate() {
            let article = dataset
                .articles
                .iter()
                .find(|a| a.article_id == *id && a.split == "discovery")
                .ok_or_else(|| AppError::Parse(format!("缺少发现集文章 {id}")))?;
            let text = dataset.read_article(&manifest, article)?;
            let alias = format!("a{:02}", index + 1);
            let saved: Value =
                serde_json::from_slice(&std::fs::read(card_root.join(format!("{id}.json")))?)
                    .map_err(|e| AppError::Parse(e.to_string()))?;
            if saved["sha256"].as_str() != Some(&sha256(text.as_bytes()))
                || saved["article"] != alias
            {
                return Err(AppError::Parse("审阅卡片与原文身份不符".into()));
            }
            let observations = saved["result"]["observations"]
                .as_array()
                .ok_or_else(|| AppError::Parse("审阅卡片缺观察".into()))?;
            cards.insert(
                alias.clone(),
                Local {
                    cards: observations
                        .iter()
                        .map(|o| Card {
                            id: o["id"].as_str().unwrap_or("").into(),
                            operation: o["operation"].as_str().unwrap_or("").into(),
                            quote: o["quote"].as_str().unwrap_or("").into(),
                            when: String::new(),
                            caveat: String::new(),
                            misuse: String::new(),
                        })
                        .collect(),
                    uncertain: vec![],
                },
            );
            sources.push(Source {
                article: alias,
                source_id: (*id).into(),
                sha256: sha256(text.as_bytes()),
                text,
            });
            jobs.push(Job {
                id: format!("{code}-k{}", index + 1),
                sources: sources.clone(),
                cards: cards.clone(),
            });
        }
    }
    Ok(jobs)
}

pub(crate) fn validate(spec: &str, job: &Job) -> Result<(), AppError> {
    validate_execution_spec(spec)?;
    let quota = [
        "一到两次",
        "一至两次",
        "至少一次",
        "一两次即可",
        "每段至少",
        "全篇出现一次",
        "一篇出现一次",
        "一篇用一次",
        "每篇必须",
    ];
    if spec.contains("王小波")
        || spec.contains("汪曾祺")
        || source_leak(spec, &job.sources, &job.cards)
        || quota.iter().any(|phrase| spec.contains(phrase))
        || (job.sources.len() == 1
            && ["一贯如此", "作者总是", "最稳定的", "全部文章"]
                .iter()
                .any(|phrase| spec.contains(phrase)))
    {
        return Err(AppError::Parse(
            "候选含原文内容、作者标签、无依据配额或单篇越界断言".into(),
        ));
    }
    Ok(())
}

pub(crate) async fn run(run_id: &str) -> Result<(), AppError> {
    let jobs = jobs()?;
    let prompt = std::fs::read_to_string(format!("{ROOT}/prompts/voice-prose-v1.md"))?;
    let method = std::fs::read(format!("{ROOT}/method-revision-22.md"))?;
    let store = Arc::new(ResearchStore::open(Path::new(ROOT), run_id, false)?);
    store.limit_run(2_000_000).await?;
    store.freeze(
        &json!({"suite":"voice_learning_prose_v1","jobs":jobs,"prompt":prompt,
        "method_sha256":sha256(&method),"model":Role::Structured.model(),
        "run_cap_micro_cny":2_000_000}),
    )?;
    std::fs::create_dir_all(store.root.join("profiles"))?;
    let mut tasks = tokio::task::JoinSet::<Result<Value, AppError>>::new();
    for job in jobs {
        let store = Arc::clone(&store);
        let prompt = prompt.clone();
        tasks.spawn(async move {
            let name = job.id.clone();
            let original = json!({"articles":job.sources.iter().map(|source|
                json!({"article":source.article,"text":source.text})).collect::<Vec<_>>()}).to_string();
            let mut input = original.clone();
            let mut result = None;
            for attempt in 0..2 {
                let response = match store.call(&format!("{name}-{attempt}"), Role::Structured,
                    &prompt, &input, false).await {
                    Ok(response) => response,
                    Err(error) => return Ok(json!({"id":name,"status":"failed","error":error.to_string()})),
                };
                let spec = response.trim();
                match validate(spec, &job) {
                    Ok(()) => { result = Some(spec.to_string()); break; }
                    Err(error) if attempt == 0 => {
                        input = format!("原始文章：{original}\n上次候选：{spec}\n程序诊断：{error}\n请重写完整Markdown，只修正无依据配额、原文内容和证据语气。")
                    }
                    Err(error) => {
                        return Ok(json!({"id":name,"status":"failed","error":error.to_string()}));
                    }
                }
            }
            let spec = result.ok_or_else(|| AppError::Parse("候选修复耗尽".into()))?;
            write_json(&store.root.join(format!("profiles/{name}.json")),
                &json!({"id":name,"source_ids":job.sources.iter().map(|s| &s.source_id)
                    .collect::<Vec<_>>(),"spec":spec,"status":"draft_pending_semantic_review"}))?;
            write_file_atomic(&store.root.join(format!("profiles/{name}.md")), &spec)?;
            Ok(json!({"id":name,"status":"complete","sources":job.sources.len()}))
        });
    }
    finish_tasks(&store, &mut tasks, "voice_prose_pilot").await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_source_copy_and_ungrounded_quota() {
        let source = Source {
            article: "a01".into(),
            source_id: "x".into(),
            sha256: String::new(),
            text: "先写眼前事物的变化，再让个人判断慢慢出现。".into(),
        };
        let job = Job {
            id: "test".into(),
            sources: vec![source],
            cards: HashMap::new(),
        };
        assert!(validate("先描述已有材料，再让判断自然接上。", &job).is_ok());
        assert!(validate("先写眼前事物的变化，再让个人判断慢慢出现。", &job).is_err());
        assert!(validate("一篇出现一次反问就够了。", &job).is_err());
    }
}
