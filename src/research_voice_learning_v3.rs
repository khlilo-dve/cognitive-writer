//! Narrow per-article observations followed by the audited incremental compiler.
use crate::{
    error::AppError,
    eval_store::write_json,
    io::write_file_atomic,
    research_dataset::{Dataset, sha256, valid_id},
    research_store::{ResearchStore, Role},
    research_validation::validated_as,
    research_voice_learning_v2::{Card, Local, Merge, Source, parse_merge},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::Arc,
};

const ROOT: &str = "research/style-aggregation";
const SETS: [(&str, [&str; 2]); 2] = [("wxb", ["wxb-02", "wxb-03"]), ("wzq", ["wzq-02", "wzq-04"])];

#[derive(Clone, Serialize)]
struct Set {
    code: String,
    sources: Vec<Source>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Observation {
    id: String,
    operation: String,
    quote: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LocalV3 {
    observations: Vec<Observation>,
    uncertain: Vec<String>,
}

fn parse_local(raw: &str, source: &Source) -> Result<LocalV3, AppError> {
    let local: LocalV3 = serde_json::from_str(raw.trim())
        .map_err(|e| AppError::Parse(format!("精简逐篇 JSON 无效: {e}")))?;
    if local.observations.is_empty() || local.observations.len() > 8 {
        return Err(AppError::Parse("逐篇观察为空或超过八条初选上限".into()));
    }
    let mut ids = HashSet::new();
    for observation in &local.observations {
        if !valid_id(&observation.id)
            || !observation.id.starts_with('c')
            || !ids.insert(&observation.id)
            || observation.operation.trim().is_empty()
            || !(6..=45).contains(&observation.quote.chars().count())
            || observation.quote.trim() != observation.quote
            || !source.text.contains(&observation.quote)
        {
            return Err(AppError::Parse(format!(
                "{} 观察 {} 的原文短引文无效",
                source.article, observation.id
            )));
        }
    }
    Ok(local)
}

fn into_cards(local: LocalV3) -> Local {
    Local {
        cards: local
            .observations
            .into_iter()
            .map(|observation| Card {
                id: observation.id,
                operation: observation.operation,
                quote: observation.quote,
                when: "待跨篇综合判断".into(),
                caveat: "单篇观察，不能直接升级为作者定律".into(),
                misuse: "待跨篇综合判断".into(),
            })
            .collect(),
        uncertain: local.uncertain,
    }
}

fn sets(manifest: &Path) -> Result<Vec<Set>, AppError> {
    let dataset = Dataset::load(manifest)?;
    SETS.iter()
        .map(|(code, ids)| {
            let sources = ids
                .iter()
                .enumerate()
                .map(|(index, id)| {
                    let article = dataset
                        .articles
                        .iter()
                        .find(|a| a.article_id == *id && a.split == "discovery")
                        .ok_or_else(|| AppError::Parse(format!("缺少发现集文章 {id}")))?;
                    let text = dataset.read_article(manifest, article)?;
                    Ok(Source {
                        article: format!("a{:02}", index + 1),
                        source_id: (*id).into(),
                        sha256: sha256(text.as_bytes()),
                        text,
                    })
                })
                .collect::<Result<Vec<_>, AppError>>()?;
            Ok(Set {
                code: (*code).into(),
                sources,
            })
        })
        .collect()
}

pub(crate) async fn run(run_id: &str) -> Result<(), AppError> {
    let manifest = Path::new(ROOT).join("datasets/authors-v1/manifest.json");
    let sets = sets(&manifest)?;
    let local_prompt = std::fs::read_to_string(format!("{ROOT}/prompts/voice-local-v3.md"))?;
    let merge_prompt = std::fs::read_to_string(format!("{ROOT}/prompts/voice-merge-v3.md"))?;
    let method = std::fs::read(format!("{ROOT}/method-revision-20.md"))?;
    let store = Arc::new(ResearchStore::open(Path::new(ROOT), run_id, false)?);
    store.limit_run(4_000_000).await?;
    store.freeze(&json!({"suite":"voice_learning_vertical_v3","sets":sets,
        "local_prompt":local_prompt,"merge_prompt":merge_prompt,"method_sha256":sha256(&method),
        "local_model":Role::Structured.model(),"merge_model":Role::Analysis.model(),"run_cap_micro_cny":4_000_000}))?;
    std::fs::create_dir_all(store.root.join("cards"))?;
    std::fs::create_dir_all(store.root.join("profiles"))?;
    let mut tasks = tokio::task::JoinSet::<Result<Value, AppError>>::new();
    for set in &sets {
        for source in &set.sources {
            let store = Arc::clone(&store);
            let prompt = local_prompt.clone();
            let source = source.clone();
            tasks.spawn(async move {
            let name = source.source_id.clone();
            let input = json!({"article":source.article,"text":source.text}).to_string();
            match validated_as(&store, &name, Role::Structured, &prompt, &input,
                |raw| parse_local(raw, &source)).await {
                Ok(local) => {
                    write_json(&store.root.join(format!("cards/{name}.json")),
                        &json!({"source_id":name,"article":source.article,"sha256":source.sha256,"result":local}))?;
                    Ok(json!({"id":name,"status":"complete","observations":local.observations.len()}))
                }
                Err(error) => Ok(json!({"id":name,"status":"failed","error":error.to_string()})),
            }
        });
        }
    }
    let mut local_rows = Vec::new();
    while let Some(row) = tasks.join_next().await {
        local_rows.push(row.map_err(|_| AppError::Parse("逐篇任务异常终止".into()))??);
        local_rows.sort_by_key(|v| v["id"].as_str().unwrap_or("").to_string());
        write_json(
            &store.root.join("local-summary.json"),
            &json!({"rows":local_rows,"usage":store.usage().await}),
        )?;
    }
    if local_rows.iter().any(|row| row["status"] != "complete") {
        write_json(
            &store.root.join("summary.json"),
            &json!({"status":"partial","stage":"local_observation",
            "local_rows":local_rows,"rows":[],"usage":store.usage().await}),
        )?;
        return Err(AppError::Parse("精简逐篇观察有失败，未启动综合".into()));
    }
    let mut rows = Vec::new();
    for set in &sets {
        let mut cards: HashMap<String, Local> = HashMap::new();
        let mut previous: Option<Merge> = None;
        for (index, source) in set.sources.iter().enumerate() {
            let saved: Value = serde_json::from_slice(&std::fs::read(
                store.root.join(format!("cards/{}.json", source.source_id)),
            )?)
            .map_err(|e| AppError::Parse(e.to_string()))?;
            let local: LocalV3 = serde_json::from_value(saved["result"].clone())
                .map_err(|e| AppError::Parse(e.to_string()))?;
            cards.insert(source.article.clone(), into_cards(local));
            let subset = &set.sources[..=index];
            let name = format!("{}-k{}", set.code, index + 1);
            let input = json!({"cards":cards,"previous":previous}).to_string();
            match validated_as(
                &store,
                &name,
                Role::Analysis,
                &merge_prompt,
                &input,
                |raw| parse_merge(raw, subset, &cards, previous.as_ref()),
            )
            .await
            {
                Ok(merge) => {
                    write_json(
                        &store.root.join(format!("profiles/{name}.json")),
                        &json!({"id":name,"source_ids":subset.iter().map(|s| &s.source_id).collect::<Vec<_>>(),
                            "result":merge}),
                    )?;
                    write_file_atomic(
                        &store.root.join(format!("profiles/{name}.md")),
                        &merge.spec,
                    )?;
                    rows.push(
                        json!({"id":name,"status":"complete","rules":merge.rules.len(),
                        "changes":merge.changes.len()}),
                    );
                    previous = Some(merge);
                }
                Err(error) => {
                    rows.push(json!({"id":name,"status":"failed","error":error.to_string()}));
                    break;
                }
            }
            write_json(
                &store.root.join("summary.json"),
                &json!({"status":"running","stage":"incremental_merge",
                "rows":rows,"local_rows":local_rows,"usage":store.usage().await}),
            )?;
        }
    }
    let complete = rows.len() == 4 && rows.iter().all(|row| row["status"] == "complete");
    write_json(
        &store.root.join("summary.json"),
        &json!({"status":if complete{"complete"}else{"partial"},
        "stage":"incremental_merge","rows":rows,"local_rows":local_rows,"usage":store.usage().await}),
    )?;
    if complete {
        Ok(())
    } else {
        Err(AppError::Parse("两篇竖切未完整，保留有效前缀与失败".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn short_observation_requires_exact_contiguous_quote() {
        let s = Source {
            article: "a01".into(),
            source_id: "test".into(),
            sha256: String::new(),
            text: "先写手边的事，再轻轻说出判断。".into(),
        };
        let valid = json!({"observations":[{"id":"c1","operation":"先叙后评",
            "quote":"先写手边的事"}],"uncertain":[]});
        assert!(parse_local(&valid.to_string(), &s).is_ok());
        let invalid = json!({"observations":[{"id":"c1","operation":"先叙后评",
            "quote":"先写手边的事，轻轻说出判断"}],"uncertain":[]});
        assert!(parse_local(&invalid.to_string(), &s).is_err());
    }
}
