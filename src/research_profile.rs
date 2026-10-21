use crate::{
    error::AppError,
    eval_store::write_json,
    research_compare::{finish_tasks, validated},
    research_dataset::{Dataset, sha256, valid_id},
    research_store::ResearchStore,
    style_compare::{self, View},
    style_profile::{self, Profile, Source},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path, sync::Arc};

pub(crate) const ROOT: &str = "research/style-aggregation";
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Plan {
    pub id: String,
    pub author: String,
    pub articles: Vec<String>,
    pub mode: String,
    pub references: Vec<Value>,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Artifact {
    pub plan: Plan,
    pub profile: Option<Profile>,
    pub view: View,
    pub spec: String,
    pub source_sha256: String,
    pub warnings: Vec<String>,
}

pub(crate) fn load_sources(views: &str) -> Result<BTreeMap<String, (String, Source, String)>, AppError> {
    if !valid_id(views) {
        return Err(AppError::Parse("views run无效".into()));
    }
    let dir = Path::new(ROOT).join("runs").join(views);
    crate::research_compare::view_coverage(views)?;
    let manifest = Path::new(ROOT).join("datasets/authors-v1/manifest.json");
    let ds = Dataset::load(&manifest)?;
    let mut result = BTreeMap::new();
    for a in ds.selected(false) {
        let raw: Value = serde_json::from_slice(&std::fs::read(
            dir.join(format!("views/{}.json", a.article_id)),
        )?)
        .map_err(|e| AppError::Parse(e.to_string()))?;
        let spec = raw["item"]["spec"]
            .as_str()
            .ok_or_else(|| AppError::Parse("视图缺少Style".into()))?
            .to_owned();
        if raw["item"]["id"] != a.article_id
            || raw["item"]["replicate"] != 1
            || raw["item"]["author"] != a.author_id
        {
            return Err(AppError::Parse("视图样本身份变化".into()));
        }
        let view = style_compare::parse_view(&raw["view"].to_string(), &spec)?;
        result.insert(
            a.article_id.clone(),
            (
                a.author_id.clone(),
                Source {
                    id: String::new(),
                    article_id: a.article_id.clone(),
                    topic: a.topic.clone(),
                    text: ds.read_article(&manifest, a)?,
                    view,
                },
                spec,
            ),
        );
    }
    if result.len() != 20 {
        return Err(AppError::Parse("聚合要求20份首次视图".into()));
    }
    Ok(result)
}

pub(crate) fn make_plans(
    sampling: &Value,
    sources: &BTreeMap<String, (String, Source, String)>,
) -> Result<Vec<Plan>, AppError> {
    let mut plans = BTreeMap::<String, Plan>::new();
    let authors = sampling["authors"]
        .as_object()
        .ok_or_else(|| AppError::Parse("抽样authors缺失".into()))?;
    for (author, data) in authors {
        let orders = data["orders"]
            .as_array()
            .ok_or_else(|| AppError::Parse("抽样orders缺失".into()))?;
        if orders.len() != 5 {
            return Err(AppError::Parse("需要5个冻结顺序".into()));
        }
        for order in orders {
            for k in [1, 2, 3, 5, 7, 10] {
                let mut ids: Vec<String> =
                    serde_json::from_value(order["subsets"][k.to_string()].clone())
                        .map_err(|e| AppError::Parse(e.to_string()))?;
                ids.sort();
                let mut unique = ids.clone();
                unique.dedup();
                if ids.len() != k
                    || unique.len() != k
                    || ids
                        .iter()
                        .any(|id| sources.get(id).is_none_or(|s| s.0 != *author))
                {
                    return Err(AppError::Parse("抽样包含重复/跨作者/留出文章".into()));
                }
                let id = format!("profile-{}", &sha256(ids.join("|").as_bytes())[..16]);
                let entry = plans.entry(id.clone()).or_insert(Plan {
                    id,
                    author: author.clone(),
                    articles: ids.clone(),
                    mode: "primary".into(),
                    references: vec![],
                });
                if entry.articles != ids {
                    return Err(AppError::Parse("子集身份冲突".into()));
                }
                entry.references.push(json!({"seed":order["seed"],"k":k}));
            }
        }
    }
    let full: Vec<_> = plans
        .values()
        .filter(|p| p.articles.len() == 10)
        .cloned()
        .collect();
    if full.len() != 2 {
        return Err(AppError::Parse("需要两位作者全量子集".into()));
    }
    for p in full {
        for mode in ["repeat", "reverse", "shuffle"] {
            let id = format!("{}-{mode}", p.id);
            plans.insert(
                id.clone(),
                Plan {
                    id,
                    mode: mode.into(),
                    references: vec![],
                    ..p.clone()
                },
            );
        }
    }
    Ok(plans.into_values().collect())
}

pub(crate) async fn run(run: &str, views: &str, comparison_run: &str) -> Result<(), AppError> {
    if !valid_id(comparison_run) {
        return Err(AppError::Parse("P2比较run无效".into()));
    }
    let previous = Path::new(ROOT).join("runs").join(comparison_run);
    let read_gate = |name: &str| -> Result<Value, AppError> {
        serde_json::from_slice(&std::fs::read(previous.join(name))?)
            .map_err(|e| AppError::Parse(e.to_string()))
    };
    let gate_inputs = read_gate("inputs.json")?;
    if !["style_pairs_v2_structured", "candidate_matrix_v3"]
        .contains(&gate_inputs["suite"].as_str().unwrap_or(""))
        || read_gate("summary.json")?["status"] != "complete"
        || read_gate("controls.json")?["passed"] != true
        || gate_inputs["source"] != views
    {
        return Err(AppError::Parse(
            "P2比较尚未完成、控制未过或视图来源不一致".into(),
        ));
    }
    execute(run, views, Some(comparison_run), 1).await
}

/// Independent source-grounded feasibility experiment; does not certify P2.
pub(crate) async fn pilot(run: &str, views: &str, version: u8) -> Result<(), AppError> {
    execute(run, views, None, version).await
}

async fn execute(
    run: &str,
    views: &str,
    comparison_run: Option<&str>,
    version: u8,
) -> Result<(), AppError> {
    let source_map = load_sources(views)?;
    let sampling: Value =
        serde_json::from_slice(&std::fs::read(format!("{ROOT}/datasets/sampling-v1.json"))?)
            .map_err(|e| AppError::Parse(e.to_string()))?;
    let manifest = std::fs::read(format!("{ROOT}/datasets/authors-v1/manifest.json"))?;
    if sampling["manifest_sha256"] != sha256(&manifest) {
        return Err(AppError::Parse("冻结抽样与manifest不一致".into()));
    }
    let mut plans = make_plans(&sampling, &source_map)?;
    if comparison_run.is_none() {
        plans.retain(|p| p.mode == "primary" && p.articles.len() == 10);
        if plans.len() != 2 {
            return Err(AppError::Parse("独立小试要求两份全量主Profile".into()));
        }
    }
    let prompt_version = if version == 2 { 3 } else { 2 };
    let aggregate = std::fs::read_to_string(format!(
        "{ROOT}/prompts/aggregate-profile-v{prompt_version}.md"
    ))?;
    let compile = std::fs::read_to_string(format!("{ROOT}/prompts/compile-profile-v1.md"))?;
    let store = Arc::new(ResearchStore::open(Path::new(ROOT), run, false)?);
    let mut frozen = json!({"suite":"evidence_profile_v2_structured","analysis_thinking":"disabled","method_revision":1,"sources":source_map,"sampling":sampling,"plans":plans,"aggregate_prompt":aggregate,"compile_prompt":compile,"rule_cap":12,"classification":"protocol_v2_detailed","comparison_gate":comparison_run});
    if comparison_run.is_none() {
        store.limit_run(4_000_000).await?;
        frozen["suite"] = json!(format!("independent_profile_pilot_v{version}"));
        frozen["method_revision"] = json!(6 + version);
        if version == 2 {
            frozen["rule_cap"] = json!(6);
        }
        frozen["p2_status"] = json!("measurement_invalid_no_author_similarity_conclusion");
        frozen["run_cap_micro_cny"] = json!(4_000_000);
        frozen["protocol_sha256"] = json!(sha256(&std::fs::read(format!(
            "{ROOT}/method-revision-{:02}.md",
            6 + version
        ))?));
    }
    store.freeze(&frozen)?;
    std::fs::create_dir_all(store.root.join("profiles"))?;
    write_json(&store.root.join("plans.json"), &plans)?;
    let mut jobs = tokio::task::JoinSet::new();
    for plan in plans {
        let mut sources: Vec<_> = plan
            .articles
            .iter()
            .enumerate()
            .map(|(i, id)| {
                let mut s = source_map[id].1.clone();
                s.id = format!("a{:02}", i + 1);
                s
            })
            .collect();
        let single_spec = source_map[&plan.articles[0]].2.clone();
        if plan.mode == "reverse" {
            sources.reverse();
        }
        if plan.mode == "shuffle" {
            sources.sort_by_key(|s| sha256(format!("71:{}", s.article_id).as_bytes()));
        }
        let store = Arc::clone(&store);
        let aggregate = aggregate.clone();
        let compile = compile.clone();
        jobs.spawn(async move{
            let id=plan.id.clone();
            let result=build(&store,plan,sources,single_spec,&aggregate,&compile,if version == 2 { 6 } else { 12 }).await;
            match result {
                Ok(a)=>{write_json(&store.root.join(format!("profiles/{id}.json")),&a)?;
                    crate::io::write_file_atomic(&store.root.join(format!("profiles/{id}.md")),&a.spec)?;
                    Ok::<_,AppError>(json!({"id":id,"status":"complete","author":a.plan.author,"k":a.plan.articles.len(),"mode":a.plan.mode,"rules":a.view.features.len(),"characters":crate::spec_quality::character_count(&a.spec),"warnings":a.warnings}))},
                Err(e)=>Ok(json!({"id":id,"status":"failed","error":e.to_string()})),
            }
        });
    }
    finish_tasks(&store, &mut jobs, "profiles").await
}

async fn build(
    store: &ResearchStore,
    plan: Plan,
    sources: Vec<Source>,
    single_spec: String,
    aggregate: &str,
    compile: &str,
    rule_cap: usize,
) -> Result<Artifact, AppError> {
    let source_sha256 =
        sha256(&serde_json::to_vec(&sources).map_err(|e| AppError::Parse(e.to_string()))?);
    if sources.len() == 1 {
        return Ok(Artifact {
            plan,
            profile: None,
            view: sources[0].view.clone(),
            spec: single_spec,
            source_sha256,
            warnings: vec!["k=1沿用原单篇表示，不计算作者稳定支持率".into()],
        });
    }
    let anonymous: Vec<_> = sources
        .iter()
        .map(|s| json!({"article":s.id,"lines":s.text.split('\n').enumerate().map(|(i,text)|json!({"line":i+1,"text":text})).collect::<Vec<_>>(),"features":s.view.features}))
        .collect();
    let profile = validated(
        store,
        &format!("{}-merge", plan.id),
        aggregate,
        &json!({"articles":anonymous}).to_string(),
        |raw| {
            let profile = style_profile::parse_indexed(raw, &sources)?;
            if profile.rules.len() > rule_cap {
                return Err(AppError::Parse(format!("当前聚合最多{rule_cap}条机制")));
            }
            Ok(profile)
        },
    )
    .await?;
    // Save verified evidence even if the later compilation fails.
    write_json(
        &store
            .root
            .join(format!("profiles/{}-evidence.json", plan.id)),
        &profile,
    )?;
    let executable = profile.executable();
    let mut warnings = Vec::new();
    let spec = if executable.is_empty() {
        warnings.push("没有满足稳定/弱稳定门槛的可执行规则，未编造补充".into());
        String::new()
    } else {
        let rules:Vec<_>=executable.iter().map(|r|json!({"feature_id":r.candidate.id,"rule":r.candidate.rule,"condition":r.candidate.condition,"classification":r.classification})).collect();
        validated(
            store,
            &format!("{}-compile", plan.id),
            compile,
            &json!({"rules":rules}).to_string(),
            |raw| style_profile::compile(raw, &profile),
        )
        .await?
    };
    if !spec.is_empty() && crate::spec_quality::character_count(&spec) < 1500 {
        warnings.push("执行说明低于1500字完整度参考；保留不足，不通过重复抽样凑字".into());
    }
    let view = profile.view();
    Ok(Artifact {
        plan,
        profile: Some(profile),
        view,
        spec,
        source_sha256,
        warnings,
    })
}

pub(crate) fn load(run: &str) -> Result<Vec<Artifact>, AppError> {
    if !valid_id(run) {
        return Err(AppError::Parse("Profile run无效".into()));
    }
    let root = Path::new(ROOT).join("runs").join(run);
    let summary: Value = serde_json::from_slice(&std::fs::read(root.join("summary.json"))?)
        .map_err(|e| AppError::Parse(e.to_string()))?;
    if summary["status"] != "complete" {
        return Err(AppError::Parse("Profile阶段尚未完整完成".into()));
    }
    let plans: Vec<Plan> = serde_json::from_slice(&std::fs::read(root.join("plans.json"))?)
        .map_err(|e| AppError::Parse(e.to_string()))?;
    let mut artifacts = Vec::new();
    for p in plans {
        let a: Artifact = serde_json::from_slice(&std::fs::read(
            root.join(format!("profiles/{}.json", p.id)),
        )?)
        .map_err(|e| AppError::Parse(e.to_string()))?;
        if a.plan.id != p.id
            || a.plan.articles != p.articles
            || a.spec != std::fs::read_to_string(root.join(format!("profiles/{}.md", p.id)))?
        {
            return Err(AppError::Parse("Profile产物身份或Markdown变化".into()));
        }
        artifacts.push(a);
    }
    Ok(artifacts)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn subset_plans_are_independent_deduplicated_and_reject_holdout_ids() {
        let mut sources = BTreeMap::new();
        let mut authors = serde_json::Map::new();
        for author in ["alpha", "beta"] {
            let ids: Vec<_> = (0..10).map(|i| format!("{author}-{i:02}")).collect();
            for id in &ids {
                sources.insert(
                    id.clone(),
                    (
                        author.to_owned(),
                        Source {
                            id: String::new(),
                            article_id: id.clone(),
                            topic: "daily".into(),
                            text: "样本文本".into(),
                            view: View {
                                features: vec![],
                                uncertainties: vec![],
                            },
                        },
                        String::new(),
                    ),
                );
            }
            let orders: Vec<_> = [17, 29, 43, 71, 101]
                .iter()
                .map(|seed| {
                    let subsets: serde_json::Map<_, _> = [1, 2, 3, 5, 7, 10]
                        .iter()
                        .map(|k| (k.to_string(), json!(ids[..*k])))
                        .collect();
                    json!({"seed":seed,"subsets":subsets})
                })
                .collect();
            authors.insert(author.into(), json!({"orders":orders}));
        }
        let mut sampling = json!({"authors":authors});
        let plans = make_plans(&sampling, &sources).unwrap();
        assert_eq!(plans.len(), 18); // Six unique subsets per author, plus six robustness checks.
        assert_eq!(plans.iter().filter(|p| p.mode != "primary").count(), 6);
        assert!(
            plans
                .iter()
                .filter(|p| p.mode == "primary")
                .all(|p| p.references.len() == 5)
        );
        assert!(
            plans
                .iter()
                .filter(|p| p.articles.len() == 1)
                .all(|p| p.articles[0].ends_with("00"))
        );
        sampling["authors"]["alpha"]["orders"][0]["subsets"]["1"] = json!(["confirmation-article"]);
        assert!(make_plans(&sampling, &sources).is_err());
    }
}
