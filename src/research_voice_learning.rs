//! Exploratory 1→2→3 article learning with a separate Writer spec and evidence ledger.
use crate::{
    error::AppError,
    eval_store::write_json,
    io::write_file_atomic,
    research_compare::finish_tasks,
    research_dataset::{Dataset, sha256},
    research_store::{ResearchStore, Role},
    research_validation::validated_as,
    spec_quality::validate_execution_spec,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::HashSet, path::Path, sync::Arc};

const ROOT: &str = "research/style-aggregation";
const SOURCE_SETS: [(&str, [&str; 3]); 2] = [
    ("wxb", ["wxb-02", "wxb-03", "wxb-08"]),
    ("wzq", ["wzq-02", "wzq-04", "wzq-09"]),
];

#[derive(Clone, Serialize)]
struct Source {
    article: String,
    source_id: String,
    sha256: String,
    text: String,
}

#[derive(Clone, Serialize)]
struct Job {
    id: String,
    sources: Vec<Source>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Evidence {
    article: String,
    quote: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Claim {
    rule: String,
    condition: String,
    scope: String,
    evidence: Vec<Evidence>,
    variation: String,
    misuse: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Learned {
    spec: String,
    claims: Vec<Claim>,
    uncertain: Vec<String>,
}

fn validate(raw: &str, sources: &[Source]) -> Result<Learned, AppError> {
    let learned: Learned = serde_json::from_str(raw.trim())
        .map_err(|e| AppError::Parse(format!("风格学习 JSON 无效: {e}")))?;
    validate_execution_spec(&learned.spec)?;
    if learned.claims.is_empty()
        || learned.spec.contains("王小波")
        || learned.spec.contains("汪曾祺")
    {
        return Err(AppError::Parse("缺少机制或 Writer 文件出现作者标签".into()));
    }
    for (index, claim) in learned.claims.iter().enumerate() {
        if claim.rule.trim().is_empty()
            || claim.condition.trim().is_empty()
            || claim.misuse.trim().is_empty()
            || claim.evidence.is_empty()
            || !["local", "repeated", "conditional_variant"].contains(&claim.scope.as_str())
        {
            return Err(AppError::Parse(format!(
                "机制 {} 缺少写法、条件、误用或依据",
                index + 1
            )));
        }
        let mut seen = HashSet::new();
        for evidence in &claim.evidence {
            let source = sources
                .iter()
                .find(|s| s.article == evidence.article)
                .ok_or_else(|| AppError::Parse(format!("机制 {} 引用未知文章", index + 1)))?;
            let quote = evidence.quote.trim();
            if !(4..=120).contains(&quote.chars().count())
                || quote != evidence.quote
                || !source.text.contains(quote)
                || !seen.insert((evidence.article.as_str(), quote))
            {
                return Err(AppError::Parse(format!(
                    "机制 {} 的引文无法逐字定位",
                    index + 1
                )));
            }
        }
        let distinct = claim
            .evidence
            .iter()
            .map(|e| e.article.as_str())
            .collect::<HashSet<_>>();
        if claim.scope == "repeated" && distinct.len() < 2 {
            return Err(AppError::Parse(format!(
                "机制 {} 把单篇依据称为反复出现",
                index + 1
            )));
        }
        if claim.scope == "local" && distinct.len() != 1 {
            return Err(AppError::Parse(format!(
                "机制 {} 的局部标签与跨篇引文冲突",
                index + 1
            )));
        }
    }
    Ok(learned)
}

fn jobs(manifest: &Path) -> Result<Vec<Job>, AppError> {
    let dataset = Dataset::load(manifest)?;
    let mut jobs = Vec::new();
    for (author, ids) in SOURCE_SETS {
        let mut sources = Vec::new();
        for (index, id) in ids.iter().enumerate() {
            let article = dataset
                .articles
                .iter()
                .find(|a| a.article_id == *id && a.split == "discovery")
                .ok_or_else(|| AppError::Parse(format!("缺少冻结发现集文章 {id}")))?;
            let text = dataset.read_article(manifest, article)?;
            sources.push(Source {
                article: format!("a{:02}", index + 1),
                source_id: (*id).into(),
                sha256: sha256(text.as_bytes()),
                text,
            });
            jobs.push(Job {
                id: format!("{author}-k{}", index + 1),
                sources: sources.clone(),
            });
        }
    }
    Ok(jobs)
}

pub(crate) async fn run(run_id: &str) -> Result<(), AppError> {
    let manifest = Path::new(ROOT).join("datasets/authors-v1/manifest.json");
    let prompt = std::fs::read_to_string(format!("{ROOT}/prompts/voice-learning-v1.md"))?;
    let method = std::fs::read(format!("{ROOT}/method-revision-18.md"))?;
    let jobs = jobs(&manifest)?;
    let store = Arc::new(ResearchStore::open(Path::new(ROOT), run_id, false)?);
    store.limit_run(4_000_000).await?;
    store.freeze(&json!({"suite":"voice_learning_v1_pilot","jobs":jobs,"prompt":prompt,
        "method_sha256":sha256(&method),"model":Role::Structured.model(),"run_cap_micro_cny":4_000_000}))?;
    std::fs::create_dir_all(store.root.join("profiles"))?;
    let mut tasks = tokio::task::JoinSet::<Result<Value, AppError>>::new();
    for job in jobs {
        let store = Arc::clone(&store);
        let prompt = prompt.clone();
        tasks.spawn(async move {
            let input = json!({"articles":job.sources.iter().map(|s|
                json!({"article":s.article,"text":s.text})).collect::<Vec<_>>()})
            .to_string();
            let name = job.id.clone();
            let result = validated_as(&store, &name, Role::Structured, &prompt, &input, |raw| {
                validate(raw, &job.sources)
            })
            .await;
            match result {
                Ok(learned) => {
                    write_json(
                        &store.root.join(format!("profiles/{name}.json")),
                        &json!({"id":name,"sources":job.sources.iter().map(|s|
                            json!({"article":s.article,"source_id":s.source_id,"sha256":s.sha256}))
                            .collect::<Vec<_>>(),"result":learned}),
                    )?;
                    write_file_atomic(
                        &store.root.join(format!("profiles/{name}.md")),
                        &learned.spec,
                    )?;
                    Ok(
                        json!({"id":name,"status":"complete","claims":learned.claims.len(),
                        "sources":job.sources.len()}),
                    )
                }
                Err(error) => Ok(json!({"id":name,"status":"failed","error":error.to_string()})),
            }
        });
    }
    finish_tasks(&store, &mut tasks, "voice_learning_pilot").await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(article: &str, text: &str) -> Source {
        Source {
            article: article.into(),
            source_id: article.into(),
            sha256: sha256(text.as_bytes()),
            text: text.into(),
        }
    }

    #[test]
    fn rejects_fabricated_quote_and_fake_cross_article_support() {
        let sources = [
            source("a01", "先把已知事说清，再用短句落下判断。"),
            source("a02", "先交代人和物，随后评点一句。"),
        ];
        let mut v = json!({"spec":"先叙述，再让判断接上。","claims":[{"rule":"先叙后评",
            "condition":"已有可叙材料时","scope":"repeated","evidence":[
                {"article":"a01","quote":"先把已知事说清"},
                {"article":"a02","quote":"先交代人和物"}],
            "variation":"","misuse":"别凭空补故事"}],"uncertain":[]});
        assert!(validate(&v.to_string(), &sources).is_ok());
        v["claims"][0]["evidence"][1]["quote"] = json!("不存在的原文");
        assert!(validate(&v.to_string(), &sources).is_err());
        v["claims"][0]["evidence"].as_array_mut().unwrap().pop();
        assert!(validate(&v.to_string(), &sources).is_err());
    }
}
