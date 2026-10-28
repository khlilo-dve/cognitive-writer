//! Two-stage research learner: per-article cards, then audited incremental Writer specs.
use crate::{
    error::AppError,
    eval_store::write_json,
    io::write_file_atomic,
    research_dataset::{Dataset, sha256, valid_id},
    research_store::{ResearchStore, Role},
    research_validation::validated_as,
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
const SETS: [(&str, [&str; 3]); 2] = [
    ("wxb", ["wxb-02", "wxb-03", "wxb-08"]),
    ("wzq", ["wzq-02", "wzq-04", "wzq-09"]),
];

#[derive(Clone, Serialize)]
pub(crate) struct Source {
    pub article: String,
    pub source_id: String,
    pub sha256: String,
    pub text: String,
}

#[derive(Clone, Serialize)]
struct Set {
    author_code: String,
    sources: Vec<Source>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Card {
    pub id: String,
    pub operation: String,
    pub when: String,
    pub quote: String,
    pub caveat: String,
    pub misuse: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Local {
    pub cards: Vec<Card>,
    pub uncertain: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Rule {
    id: String,
    instruction: String,
    when: String,
    scope: String,
    from_cards: Vec<String>,
    misuse: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Change {
    prior_rule: String,
    action: String,
    reason: String,
    new_rule_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Merge {
    pub spec: String,
    pub rules: Vec<Rule>,
    pub changes: Vec<Change>,
    uncertain: Vec<String>,
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
                author_code: (*code).into(),
                sources,
            })
        })
        .collect()
}

fn parse_local(raw: &str, source: &Source) -> Result<Local, AppError> {
    let local: Local = serde_json::from_str(raw.trim())
        .map_err(|e| AppError::Parse(format!("逐篇观察 JSON 无效: {e}")))?;
    if local.cards.is_empty() {
        return Err(AppError::Parse("逐篇观察没有可核对卡片".into()));
    }
    let mut ids = HashSet::new();
    for card in &local.cards {
        let quote = card.quote.trim();
        if !valid_id(&card.id)
            || !card.id.starts_with('c')
            || !ids.insert(&card.id)
            || card.operation.trim().is_empty()
            || card.when.trim().is_empty()
            || card.caveat.trim().is_empty()
            || card.misuse.trim().is_empty()
            || !(4..=120).contains(&quote.chars().count())
            || quote != card.quote
            || !source.text.contains(quote)
        {
            return Err(AppError::Parse(format!(
                "{} 卡片 {} 字段或原文引文无效",
                source.article, card.id
            )));
        }
    }
    Ok(local)
}

pub(crate) fn source_leak(spec: &str, sources: &[Source], locals: &HashMap<String, Local>) -> bool {
    for local in locals.values() {
        for card in &local.cards {
            if card.quote.chars().count() >= 6 && spec.contains(&card.quote) {
                return true;
            }
        }
    }
    let chars = spec.chars().collect::<Vec<_>>();
    chars.windows(16).any(|window| {
        let piece = window.iter().collect::<String>();
        sources.iter().any(|source| source.text.contains(&piece))
    })
}

pub(crate) fn parse_merge(
    raw: &str,
    sources: &[Source],
    locals: &HashMap<String, Local>,
    previous: Option<&Merge>,
) -> Result<Merge, AppError> {
    let merge: Merge = serde_json::from_str(raw.trim())
        .map_err(|e| AppError::Parse(format!("增量风格 JSON 无效: {e}")))?;
    validate_execution_spec(&merge.spec)?;
    if merge.rules.is_empty()
        || merge.spec.contains("王小波")
        || merge.spec.contains("汪曾祺")
        || source_leak(&merge.spec, sources, locals)
    {
        return Err(AppError::Parse(
            "Writer 风格文件缺规则、含作者标签或复制原文片段".into(),
        ));
    }
    let mut cards = HashMap::new();
    for (article, local) in locals {
        for card in &local.cards {
            cards.insert(format!("{article}:{}", card.id), article.as_str());
        }
    }
    let mut ids = HashSet::new();
    for rule in &merge.rules {
        if !valid_id(&rule.id)
            || !ids.insert(rule.id.as_str())
            || rule.instruction.trim().is_empty()
            || rule.when.trim().is_empty()
            || rule.misuse.trim().is_empty()
            || rule.from_cards.is_empty()
            || !["local", "repeated", "conditional_variant"].contains(&rule.scope.as_str())
        {
            return Err(AppError::Parse("增量规则字段、编号或类型无效".into()));
        }
        let mut referenced = HashSet::new();
        for key in &rule.from_cards {
            let article = cards
                .get(key)
                .ok_or_else(|| AppError::Parse("规则引用未知逐篇卡片".into()))?;
            if !referenced.insert((key.as_str(), *article)) {
                return Err(AppError::Parse("规则重复引用同一卡片".into()));
            }
        }
        let article_count = referenced
            .iter()
            .map(|(_, article)| *article)
            .collect::<HashSet<_>>()
            .len();
        if (rule.scope == "repeated" && article_count < 2)
            || (rule.scope == "local" && article_count != 1)
        {
            return Err(AppError::Parse("规则支持范围与卡片文章数不符".into()));
        }
    }
    let prior = previous
        .map(|v| {
            v.rules
                .iter()
                .map(|r| r.id.as_str())
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default();
    let mut changed = HashSet::new();
    for change in &merge.changes {
        if !prior.contains(change.prior_rule.as_str())
            || !changed.insert(change.prior_rule.as_str())
            || change.reason.trim().is_empty()
            || !["retained", "refined", "retired"].contains(&change.action.as_str())
            || (change.action == "retired" && !change.new_rule_ids.is_empty())
            || (change.action != "retired" && change.new_rule_ids.is_empty())
            || change
                .new_rule_ids
                .iter()
                .any(|id| !ids.contains(id.as_str()))
        {
            return Err(AppError::Parse(
                "旧规则变更记录缺失、重复或目标不存在".into(),
            ));
        }
    }
    if changed != prior {
        return Err(AppError::Parse("增量结果静默遗漏旧规则".into()));
    }
    Ok(merge)
}

pub(crate) async fn run(run_id: &str) -> Result<(), AppError> {
    let manifest = Path::new(ROOT).join("datasets/authors-v1/manifest.json");
    let sets = sets(&manifest)?;
    let local_prompt = std::fs::read_to_string(format!("{ROOT}/prompts/voice-local-cards-v2.md"))?;
    let merge_prompt = std::fs::read_to_string(format!("{ROOT}/prompts/voice-merge-v2.md"))?;
    let method = std::fs::read(format!("{ROOT}/method-revision-19.md"))?;
    let store = Arc::new(ResearchStore::open(Path::new(ROOT), run_id, false)?);
    store.limit_run(6_000_000).await?;
    store.freeze(&json!({"suite":"voice_learning_incremental_v2","sets":sets,
        "local_prompt":local_prompt,"merge_prompt":merge_prompt,"method_sha256":sha256(&method),
        "local_model":Role::Structured.model(),"merge_model":Role::Analysis.model(),"run_cap_micro_cny":6_000_000}))?;
    std::fs::create_dir_all(store.root.join("cards"))?;
    std::fs::create_dir_all(store.root.join("profiles"))?;

    let mut local_tasks = tokio::task::JoinSet::<Result<Value, AppError>>::new();
    for set in &sets {
        for source in &set.sources {
            let store = Arc::clone(&store);
            let prompt = local_prompt.clone();
            let source = source.clone();
            let name = source.source_id.clone();
            local_tasks.spawn(async move {
            let input = json!({"article":source.article,"text":source.text}).to_string();
            match validated_as(&store, &name, Role::Structured, &prompt, &input,
                |raw| parse_local(raw, &source)).await {
                Ok(local) => {
                    write_json(&store.root.join(format!("cards/{name}.json")),
                        &json!({"source_id":name,"article":source.article,"sha256":source.sha256,"result":local}))?;
                    Ok(json!({"id":name,"status":"complete","cards":local.cards.len()}))
                },
                Err(error) => Ok(json!({"id":name,"status":"failed","error":error.to_string()})),
            }
        });
        }
    }
    let mut local_rows = Vec::new();
    while let Some(row) = local_tasks.join_next().await {
        local_rows.push(row.map_err(|_| AppError::Parse("逐篇任务异常终止".into()))??);
        local_rows.sort_by_key(|row| row["id"].as_str().unwrap_or("").to_string());
        write_json(
            &store.root.join("local-summary.json"),
            &json!({"rows":local_rows,"usage":store.usage().await}),
        )?;
    }
    if local_rows.iter().any(|row| row["status"] != "complete") {
        write_json(
            &store.root.join("summary.json"),
            &json!({"status":"partial","stage":"local_cards",
            "rows":local_rows,"usage":store.usage().await}),
        )?;
        return Err(AppError::Parse("逐篇卡片有失败，停止依赖综合".into()));
    }

    let mut rows = Vec::new();
    for set in &sets {
        let mut locals = HashMap::new();
        let mut previous: Option<Merge> = None;
        for (index, source) in set.sources.iter().enumerate() {
            let saved: Value = serde_json::from_slice(&std::fs::read(
                store.root.join(format!("cards/{}.json", source.source_id)),
            )?)
            .map_err(|e| AppError::Parse(e.to_string()))?;
            let local: Local = serde_json::from_value(saved["result"].clone())
                .map_err(|e| AppError::Parse(e.to_string()))?;
            locals.insert(source.article.clone(), local);
            let subset = &set.sources[..=index];
            let name = format!("{}-k{}", set.author_code, index + 1);
            let input = json!({"cards":locals,"previous":previous}).to_string();
            match validated_as(
                &store,
                &name,
                Role::Analysis,
                &merge_prompt,
                &input,
                |raw| parse_merge(raw, subset, &locals, previous.as_ref()),
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
    let complete = rows.len() == 6 && rows.iter().all(|row| row["status"] == "complete");
    write_json(
        &store.root.join("summary.json"),
        &json!({"status":if complete{"complete"}else{"partial"},
        "stage":"incremental_merge","rows":rows,"local_rows":local_rows,"usage":store.usage().await}),
    )?;
    if complete {
        Ok(())
    } else {
        Err(AppError::Parse("增量学习不完整，保留有效前缀与失败".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn source(text: &str) -> Source {
        Source {
            article: "a01".into(),
            source_id: "one".into(),
            sha256: sha256(text.as_bytes()),
            text: text.into(),
        }
    }
    #[test]
    fn rejects_raw_source_leak_and_silent_rule_loss() {
        let s = source("先把情形交代清楚，然后突然写下一个自己的判断。这个判断不要大声嚷嚷。");
        let mut local = HashMap::new();
        local.insert(
            "a01".into(),
            Local {
                cards: vec![Card {
                    id: "c1".into(),
                    operation: "先叙后评".into(),
                    when: "有材料".into(),
                    quote: "先把情形交代清楚".into(),
                    caveat: "单篇".into(),
                    misuse: "别编故事".into(),
                }],
                uncertain: vec![],
            },
        );
        let mut v = json!({"spec":"先叙述已有事实，再给有分寸的判断。","rules":[{"id":"r1",
            "instruction":"先叙后评","when":"有事实时","scope":"local","from_cards":["a01:c1"],
            "misuse":"不要编事实"}],"changes":[],"uncertain":[]});
        let first = parse_merge(&v.to_string(), &[s.clone()], &local, None).unwrap();
        v["spec"] = json!("先把情形交代清楚，然后突然写下一个自己的判断。");
        assert!(parse_merge(&v.to_string(), &[s.clone()], &local, None).is_err());
        v["spec"] = json!("先叙述已有事实，再给有分寸的判断。");
        assert!(parse_merge(&v.to_string(), &[s], &local, Some(&first)).is_err());
    }
}
