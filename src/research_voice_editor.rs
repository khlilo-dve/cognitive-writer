//! Editorial pass that separates the evidence ledger from the Writer-facing voice.
use crate::{
    error::AppError,
    eval_store::write_json,
    io::write_file_atomic,
    research_compare::finish_tasks,
    research_dataset::{Dataset, sha256},
    research_store::{ResearchStore, Role},
    research_validation::validated_as,
    research_voice_learning_v2::{Card, Local, Source, source_leak},
    spec_quality::validate_execution_spec,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::Arc,
};

const ROOT: &str = "research/style-aggregation";
const SOURCE_RUN: &str = "voice-learning-vertical-01";
const SETS: [(&str, [&str; 2]); 2] = [("wxb", ["wxb-02", "wxb-03"]), ("wzq", ["wzq-02", "wzq-04"])];

#[derive(Clone, Serialize)]
struct Job {
    id: String,
    draft: Value,
    sources: Vec<Source>,
    cards: HashMap<String, Local>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Mechanism {
    name: String,
    rule_ids: Vec<String>,
    why: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct HeldBack {
    rule_id: String,
    reason: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Edited {
    spec: String,
    mechanisms: Vec<Mechanism>,
    held_back: Vec<HeldBack>,
    uncertain: Vec<String>,
}

fn parse_edited(raw: &str, job: &Job) -> Result<Edited, AppError> {
    let edited: Edited = serde_json::from_str(raw.trim())
        .map_err(|e| AppError::Parse(format!("声音编辑 JSON 无效: {e}")))?;
    validate_execution_spec(&edited.spec)?;
    let ungrounded_quota = [
        "一到两次",
        "一至两次",
        "至少一次",
        "一两次即可",
        "每段至少",
        "全篇出现一次",
    ];
    if edited.spec.contains("王小波")
        || edited.spec.contains("汪曾祺")
        || source_leak(&edited.spec, &job.sources, &job.cards)
        || ungrounded_quota
            .iter()
            .any(|phrase| edited.spec.contains(phrase))
    {
        return Err(AppError::Parse(
            "Writer 文件复制原文、出现作者标签或无依据的使用配额".into(),
        ));
    }
    let rules = job.draft["rules"]
        .as_array()
        .ok_or_else(|| AppError::Parse("来源草稿缺少规则".into()))?;
    let ids = rules
        .iter()
        .map(|r| {
            r["id"]
                .as_str()
                .ok_or_else(|| AppError::Parse("来源规则缺ID".into()))
        })
        .collect::<Result<HashSet<_>, _>>()?;
    if edited.mechanisms.is_empty() || (rules.len() >= 5 && edited.mechanisms.len() >= rules.len())
    {
        return Err(AppError::Parse(
            "编辑结果仍逐条机械展开，缺少上位机制".into(),
        ));
    }
    let mut used = HashSet::new();
    for mechanism in &edited.mechanisms {
        if mechanism.name.trim().is_empty()
            || mechanism.why.trim().is_empty()
            || mechanism.rule_ids.is_empty()
        {
            return Err(AppError::Parse("上位机制缺名称、理由或来源规则".into()));
        }
        for id in &mechanism.rule_ids {
            if !ids.contains(id.as_str()) || !used.insert(id.as_str()) {
                return Err(AppError::Parse("上位机制引用未知或重复规则".into()));
            }
        }
    }
    for held in &edited.held_back {
        if held.reason.trim().is_empty()
            || !ids.contains(held.rule_id.as_str())
            || !used.insert(held.rule_id.as_str())
        {
            return Err(AppError::Parse("暂缓规则缺理由、未知或重复".into()));
        }
    }
    if used != ids {
        return Err(AppError::Parse("编辑结果静默丢失输入规则".into()));
    }
    Ok(edited)
}

fn jobs() -> Result<Vec<Job>, AppError> {
    let source_root = Path::new(ROOT).join("runs").join(SOURCE_RUN);
    let summary: Value = serde_json::from_slice(&std::fs::read(source_root.join("summary.json"))?)
        .map_err(|e| AppError::Parse(e.to_string()))?;
    if summary["status"] != "complete" || summary["rows"].as_array().is_none_or(|r| r.len() != 4) {
        return Err(AppError::Parse(
            "两位作者四份增量草稿未完整，不启动编辑".into(),
        ));
    }
    let manifest = Path::new(ROOT).join("datasets/authors-v1/manifest.json");
    let dataset = Dataset::load(&manifest)?;
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
            let saved: Value = serde_json::from_slice(&std::fs::read(
                source_root.join(format!("cards/{id}.json")),
            )?)
            .map_err(|e| AppError::Parse(e.to_string()))?;
            if saved["sha256"].as_str() != Some(&sha256(text.as_bytes()))
                || saved["article"] != alias
            {
                return Err(AppError::Parse("逐篇卡片与冻结原文身份不符".into()));
            }
            let observations = saved["result"]["observations"]
                .as_array()
                .ok_or_else(|| AppError::Parse("卡片缺观察".into()))?;
            let local = Local {
                cards: observations
                    .iter()
                    .map(|o| Card {
                        id: o["id"].as_str().unwrap_or("").into(),
                        operation: o["operation"].as_str().unwrap_or("").into(),
                        quote: o["quote"].as_str().unwrap_or("").into(),
                        when: "待综合判断".into(),
                        caveat: "单篇观察".into(),
                        misuse: "待综合判断".into(),
                    })
                    .collect(),
                uncertain: vec![],
            };
            cards.insert(alias.clone(), local);
            sources.push(Source {
                article: alias,
                source_id: (*id).into(),
                sha256: sha256(text.as_bytes()),
                text,
            });
            let name = format!("{code}-k{}", index + 1);
            let saved: Value = serde_json::from_slice(&std::fs::read(
                source_root.join(format!("profiles/{name}.json")),
            )?)
            .map_err(|e| AppError::Parse(e.to_string()))?;
            let draft = saved["result"].clone();
            if draft["spec"].as_str()
                != Some(&std::fs::read_to_string(
                    source_root.join(format!("profiles/{name}.md")),
                )?)
            {
                return Err(AppError::Parse("来源风格阅读副本与审阅记录不一致".into()));
            }
            jobs.push(Job {
                id: name,
                draft,
                sources: sources.clone(),
                cards: cards.clone(),
            });
        }
    }
    Ok(jobs)
}

pub(crate) async fn run(run_id: &str) -> Result<(), AppError> {
    let jobs = jobs()?;
    let prompt = std::fs::read_to_string(format!("{ROOT}/prompts/voice-editor-v1.md"))?;
    let method = std::fs::read(format!("{ROOT}/method-revision-21.md"))?;
    let store = Arc::new(ResearchStore::open(Path::new(ROOT), run_id, false)?);
    store.limit_run(2_000_000).await?;
    store.freeze(
        &json!({"suite":"voice_learning_editor_v1","source_run":SOURCE_RUN,"jobs":jobs,
        "prompt":prompt,"method_sha256":sha256(&method),"model":Role::Structured.model(),
        "run_cap_micro_cny":2_000_000}),
    )?;
    std::fs::create_dir_all(store.root.join("profiles"))?;
    let mut tasks = tokio::task::JoinSet::<Result<Value, AppError>>::new();
    for job in jobs {
        let store = Arc::clone(&store);
        let prompt = prompt.clone();
        tasks.spawn(async move {
            let name = job.id.clone();
            let input = json!({"draft":job.draft["spec"],"rules":job.draft["rules"],
                "uncertain":job.draft["uncertain"]})
            .to_string();
            match validated_as(&store, &name, Role::Structured, &prompt, &input, |raw| {
                parse_edited(raw, &job)
            })
            .await
            {
                Ok(edited) => {
                    write_json(
                        &store.root.join(format!("profiles/{name}.json")),
                        &json!({"id":name,"source_run":SOURCE_RUN,"source_ids":job.sources.iter()
                            .map(|s| &s.source_id).collect::<Vec<_>>(),"result":edited}),
                    )?;
                    write_file_atomic(
                        &store.root.join(format!("profiles/{name}.md")),
                        &edited.spec,
                    )?;
                    Ok(
                        json!({"id":name,"status":"complete","mechanisms":edited.mechanisms.len(),
                        "held_back":edited.held_back.len()}),
                    )
                }
                Err(error) => Ok(json!({"id":name,"status":"failed","error":error.to_string()})),
            }
        });
    }
    finish_tasks(&store, &mut tasks, "voice_editor").await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_unexplained_drop_and_mechanical_quota() {
        let job = Job {
            id: "test".into(),
            draft: json!({"rules":[{"id":"r1"},{"id":"r2"}]}),
            sources: vec![],
            cards: HashMap::new(),
        };
        let valid = json!({"spec":"先从已有材料中找到可判断的关系，再按读者能跟上的次序解释。",
            "mechanisms":[{"name":"关系推进","rule_ids":["r1"],"why":"带动声音"}],
            "held_back":[{"rule_id":"r2","reason":"局部线索不足"}],"uncertain":[]});
        assert!(parse_edited(&valid.to_string(), &job).is_ok());
        let mut broken = valid.clone();
        broken["held_back"] = json!([]);
        assert!(parse_edited(&broken.to_string(), &job).is_err());
        broken = valid;
        broken["spec"] = json!("同一手法每篇至少一次，其余随意。");
        assert!(parse_edited(&broken.to_string(), &job).is_err());
    }
}
