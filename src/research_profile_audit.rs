//! Independent per-article audit of frozen aggregation hypotheses.
use crate::{
    error::AppError,
    eval_store::write_json,
    research_compare::{finish_tasks, validated},
    research_dataset::sha256,
    research_profile::{self, Plan, ROOT},
    research_receipt::{self, Checkpoint},
    research_store::ResearchStore,
    style_compare::{Feature, View},
    style_profile::{self, Profile, Source},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path, sync::Arc};

fn decode(raw: &str, candidates: &[Value], source: &Source) -> Result<Profile, AppError> {
    let value: Value = serde_json::from_str(raw).map_err(|e| AppError::Parse(e.to_string()))?;
    let evidence = value["evidence"]
        .as_array()
        .ok_or_else(|| AppError::Parse("缺少evidence".into()))?;
    if evidence.len() != candidates.len() {
        return Err(AppError::Parse("候选覆盖数量不符".into()));
    }
    let mut rules = Vec::new();
    for candidate in candidates {
        let found: Vec<_> = evidence
            .iter()
            .filter(|e| e["rule_id"] == candidate["id"])
            .collect();
        if found.len() != 1 {
            return Err(AppError::Parse(format!(
                "候选{}缺失或重复",
                candidate["id"]
            )));
        }
        let mut e = found[0].clone();
        let obj = e
            .as_object_mut()
            .ok_or_else(|| AppError::Parse("证据非对象".into()))?;
        obj.remove("rule_id");
        obj.insert("article".into(), json!(source.id));
        let mut rule = candidate.clone();
        rule["evidence"] = json!([e]);
        rules.push(rule);
    }
    style_profile::parse_indexed(
        &json!({"rules":rules,"omissions":[]}).to_string(),
        std::slice::from_ref(source),
    )
}

async fn judge(
    store: &ResearchStore,
    id: &str,
    prompt: &str,
    candidates: &[Value],
    source: &Source,
) -> Result<Profile, AppError> {
    let input = json!({"lines":source.text.split('\n').enumerate().map(|(i,t)|json!({"line":i+1,"text":t})).collect::<Vec<_>>(),
        "features":source.view.features,"candidates":candidates});
    validated(store, id, prompt, &input.to_string(), |raw| {
        decode(raw, candidates, source)
    })
    .await
}

fn controls() -> Vec<Value> {
    vec![
        json!({"id":"positive","text":"我记不清当时是星期几，只记得天很冷。","rule":"以第一人称明说记不清具体时间","condition":"提及记忆中的时间时","expected":"support"}),
        json!({"id":"subject","text":"莎士比亚也许解释不清自己的意思。","rule":"作者以第一人称承认自己的无知","condition":"谈论自己的认识时","expected":"unknown"}),
        json!({"id":"order","text":"这真是荒唐。我推开门，走进空荡荡的屋子。","rule":"先叙述动作，随后以短句作评价","condition":"这一段的叙述与评价排列","expected":"counter"}),
        json!({"id":"negation","text":"这样的办法真是胡闹！","rule":"表达不满时不用感叹号","condition":"表达不满时","expected":"counter"}),
    ]
}

pub(crate) async fn run(run: &str) -> Result<(), AppError> {
    let base = Path::new(ROOT).join("runs/p3-independent-pilot-02");
    let input: Value = serde_json::from_slice(&std::fs::read(base.join("inputs.json"))?)
        .map_err(|e| AppError::Parse(e.to_string()))?;
    let plans: Vec<Plan> = serde_json::from_value(input["plans"].clone())
        .map_err(|e| AppError::Parse(e.to_string()))?;
    let source_map = research_profile::load_sources("p2-views-02")?;
    if serde_json::to_value(&source_map).map_err(|e| AppError::Parse(e.to_string()))?
        != input["sources"]
    {
        return Err(AppError::Parse("逐篇核验来源与冻结小试不一致".into()));
    }
    let mut by_author = BTreeMap::<String, Vec<Value>>::new();
    let mut provenance = Vec::new();
    for plan in &plans {
        let bytes = std::fs::read(base.join(format!("requests/{}-merge-0.json", plan.id)))?;
        let cp: Checkpoint =
            serde_json::from_slice(&bytes).map_err(|e| AppError::Parse(e.to_string()))?;
        research_receipt::verify(&cp)?;
        let raw: Value = serde_json::from_str(&cp.receipt.content)
            .map_err(|e| AppError::Parse(e.to_string()))?;
        let candidates: Vec<Value> = raw["rules"].as_array().ok_or_else(|| AppError::Parse("候选rules缺失".into()))?
            .iter().map(|r| json!({"id":r["id"],"dimension":r["dimension"],"rule":r["rule"],"condition":r["condition"]})).collect();
        if candidates.is_empty() || candidates.len() > 6 {
            return Err(AppError::Parse("冻结候选数异常".into()));
        }
        by_author.insert(plan.author.clone(), candidates);
        provenance.push(json!({"author":plan.author,"request_sha256":sha256(&bytes)}));
    }
    if by_author.len() != 2 {
        return Err(AppError::Parse("需要两位作者的冻结候选".into()));
    }
    let prompt = std::fs::read_to_string(format!("{ROOT}/prompts/profile-evidence-audit-v1.md"))?;
    let control_cases = controls();
    let store = Arc::new(ResearchStore::open(Path::new(ROOT), run, false)?);
    store.limit_run(4_000_000).await?;
    store.freeze(&json!({"suite":"profile_evidence_audit_v1","sources":source_map,"candidates":by_author,"provenance":provenance,
        "prompt":prompt,"controls":control_cases,"run_cap_micro_cny":4000000,"protocol_sha256":sha256(&std::fs::read(format!("{ROOT}/method-revision-09.md"))?)}))?;
    let mut control_rows = Vec::new();
    for c in control_cases {
        let candidate =
            json!({"id":"p01","dimension":"voice","rule":c["rule"],"condition":c["condition"]});
        let source = Source {
            id: "a01".into(),
            article_id: "synthetic".into(),
            topic: "control".into(),
            text: c["text"].as_str().unwrap().into(),
            view: View {
                features: vec![Feature {
                    id: "f01".into(),
                    dimension: "voice".into(),
                    rule: c["rule"].as_str().unwrap().into(),
                    condition: c["condition"].as_str().unwrap().into(),
                    quote: c["text"].as_str().unwrap().into(),
                }],
                uncertainties: vec![],
            },
        };
        let result = judge(
            &store,
            &format!("control-{}", c["id"].as_str().unwrap()),
            &prompt,
            &[candidate],
            &source,
        )
        .await?;
        let passed =
            result.rules[0].candidate.evidence[0].status == c["expected"].as_str().unwrap();
        control_rows
            .push(json!({"id":c["id"],"expected":c["expected"],"passed":passed,"result":result}));
        write_json(
            &store.root.join("controls.json"),
            &json!({"rows":control_rows,"passed":passed && control_rows.len()==4}),
        )?;
        if !passed {
            write_json(
                &store.root.join("summary.json"),
                &json!({"status":"evaluation_invalid","stage":"profile_evidence_audit","rows":[],"controls":control_rows,"usage":store.usage().await}),
            )?;
            return Err(AppError::Parse("逐篇证据控制失败；不执行真实文章".into()));
        }
    }
    std::fs::create_dir_all(store.root.join("articles"))?;
    let mut jobs = tokio::task::JoinSet::new();
    for (article_id, (author, mut source, _)) in source_map {
        source.id = article_id.clone();
        let candidates = by_author[&author].clone();
        let store = Arc::clone(&store);
        let prompt = prompt.clone();
        jobs.spawn(async move {
            match judge(&store, &format!("audit-{article_id}"), &prompt, &candidates, &source).await {
                Ok(profile) => {
                    write_json(&store.root.join(format!("articles/{article_id}.json")), &profile)?;
                    Ok::<_,AppError>(json!({"id":article_id,"author":author,"status":"complete"}))
                }
                Err(e) => Ok(json!({"id":article_id,"author":author,"status":"failed","error":e.to_string()})),
            }
        });
    }
    finish_tasks(&store, &mut jobs, "profile_evidence_audit").await?;
    let source_map = research_profile::load_sources("p2-views-02")?;
    let mut aggregates = Vec::new();
    for plan in plans {
        let mut rules = by_author[&plan.author].clone();
        for r in &mut rules {
            r["evidence"] = json!([]);
        }
        let mut sources = Vec::new();
        for id in &plan.articles {
            let article: Profile = serde_json::from_slice(&std::fs::read(
                store.root.join(format!("articles/{id}.json")),
            )?)
            .map_err(|e| AppError::Parse(e.to_string()))?;
            for (r, a) in rules.iter_mut().zip(article.rules) {
                r["evidence"].as_array_mut().unwrap().push(
                    serde_json::to_value(a.candidate.evidence[0].clone())
                        .map_err(|e| AppError::Parse(e.to_string()))?,
                );
            }
            let mut source = source_map[id].1.clone();
            source.id = id.clone();
            sources.push(source);
        }
        let profile = style_profile::parse(&json!({"rules":rules,"omissions":["固定候选的逐篇核验，不是完整作者风格；未编译或采用"]}).to_string(), &sources)?;
        aggregates.push(json!({"author":plan.author,"profile":profile}));
    }
    write_json(&store.root.join("aggregates.json"), &aggregates)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_missing_or_duplicate_rule_votes_without_rewriting_hypotheses() {
        let source = Source {
            id: "a01".into(),
            article_id: "a01".into(),
            topic: "test".into(),
            text: "这是原文的真实句子。".into(),
            view: View {
                features: vec![],
                uncertainties: vec![],
            },
        };
        let candidates =
            vec![json!({"id":"p01","dimension":"voice","rule":"作者自嘲","condition":"谈自己"})];
        assert!(decode(r#"{"evidence":[]}"#, &candidates, &source).is_err());
        let raw = r#"{"evidence":[{"rule_id":"p01","status":"unknown","feature_ids":[],"line_start":0,"line_end":0,"reason":"主体不同"}]}"#;
        let result = decode(raw, &candidates, &source).unwrap();
        assert_eq!(result.rules[0].candidate.rule, "作者自嘲");
        assert_eq!(result.rules[0].unknown_articles, vec!["a01"]);
    }
}
