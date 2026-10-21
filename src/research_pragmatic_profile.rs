//! Exploratory multi-article Style candidates, separate from the strict evidence gate.
use crate::{
    error::AppError,
    eval_store::write_json,
    research_compare::{finish_tasks, validated},
    research_dataset::sha256,
    research_profile::{self, Artifact, Plan, ROOT},
    research_store::ResearchStore,
    style_compare::View,
};
use serde_json::{Value, json};
use std::{collections::HashSet, path::Path, sync::Arc};

fn required<'a>(v: &'a Value, field: &str) -> Result<&'a str, AppError> {
    v[field]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AppError::Parse(format!("聚合候选缺少{field}")))
}

fn validate(
    raw: &str,
    plan: &Plan,
    sources: &[(String, String, String)],
) -> Result<(Artifact, Value), AppError> {
    let doc: Value = serde_json::from_str(raw).map_err(|e| AppError::Parse(e.to_string()))?;
    let rules = doc["rules"]
        .as_array()
        .ok_or_else(|| AppError::Parse("聚合缺少rules".into()))?;
    if !(4..=6).contains(&rules.len()) {
        return Err(AppError::Parse("聚合需要4–6条主要表达机制".into()));
    }
    let mut names = HashSet::new();
    let mut md = String::from(
        "# 写作声音执行说明\n\n仅根据多篇文章中的表达倾向使用以下规则；任务素材和事实边界优先。\n\n",
    );
    for rule in rules {
        let mechanism = required(rule, "mechanism")?;
        let when = required(rule, "when")?;
        let instruction = required(rule, "instruction")?;
        if !names.insert(mechanism)
            || mechanism.chars().count() > 100
            || when.chars().count() > 100
            || !(100..=900).contains(&instruction.chars().count())
        {
            return Err(AppError::Parse("聚合机制重复、过长或说明过短".into()));
        }
        let evidence = rule["evidence"]
            .as_array()
            .ok_or_else(|| AppError::Parse("缺少逐篇原文依据".into()))?;
        if !(2..=4).contains(&evidence.len()) {
            return Err(AppError::Parse("每条规则需要2–4篇原文依据".into()));
        }
        let mut articles = HashSet::new();
        let mut topics = HashSet::new();
        for e in evidence {
            let alias = required(e, "article")?;
            let quote = required(e, "quote")?;
            let (id, topic, text) = sources
                .iter()
                .enumerate()
                .find_map(|(i, s)| (alias == format!("a{:02}", i + 1)).then_some(s))
                .ok_or_else(|| AppError::Parse("引用非当前作者文章".into()))?;
            if !articles.insert(id)
                || !(8..=120).contains(&quote.chars().count())
                || !text.contains(quote)
            {
                return Err(AppError::Parse(format!(
                    "{alias}引用重复、过长或不能逐字定位"
                )));
            }
            topics.insert(topic);
        }
        if topics.len() < 2 {
            return Err(AppError::Parse("每条规则至少覆盖两个题材".into()));
        }
        md.push_str(&format!(
            "## {mechanism}\n\n适用时机：{when}。\n\n{instruction}\n\n"
        ));
    }
    let source_sha256 =
        sha256(&serde_json::to_vec(sources).map_err(|e| AppError::Parse(e.to_string()))?);
    let length = crate::spec_quality::character_count(&md);
    let mut warnings = vec!["探索性多篇候选：引文逐字校验，不代表语义和作者稳定性通过".into()];
    if length < 1500 {
        warnings.push("执行说明低于1500字符完整度参考，保留不足".into());
    }
    Ok((
        Artifact {
            plan: plan.clone(),
            profile: None,
            view: View {
                features: vec![],
                uncertainties: vec!["未执行正式语义比较".into()],
            },
            spec: md,
            source_sha256,
            warnings,
        },
        doc,
    ))
}

pub(crate) async fn run(run: &str) -> Result<(), AppError> {
    let source_map = research_profile::load_sources("p2-views-02")?;
    let sampling: Value =
        serde_json::from_slice(&std::fs::read(format!("{ROOT}/datasets/sampling-v1.json"))?)
            .map_err(|e| AppError::Parse(e.to_string()))?;
    let manifest = std::fs::read(format!("{ROOT}/datasets/authors-v1/manifest.json"))?;
    if sampling["manifest_sha256"] != sha256(&manifest) {
        return Err(AppError::Parse("冻结抽样与manifest不符".into()));
    }
    let mut plans = research_profile::make_plans(&sampling, &source_map)?;
    plans.retain(|p| p.mode == "primary" && p.articles.len() == 10);
    if plans.len() != 2 {
        return Err(AppError::Parse("需要两位作者全量主子集".into()));
    }
    let prompt = std::fs::read_to_string(format!("{ROOT}/prompts/pragmatic-profile-v1.md"))?;
    let store = Arc::new(ResearchStore::open(Path::new(ROOT), run, false)?);
    store.limit_run(4_000_000).await?;
    store.freeze(&json!({"suite":"pragmatic_profile_v1","p2_semantic_status":"measurement_invalid",
        "plans":plans,"sources":source_map,"sampling":sampling,"prompt":prompt,"model":"mimo-v2.6-pro",
        "run_cap_micro_cny":4000000,"protocol_sha256":sha256(&std::fs::read(format!("{ROOT}/method-revision-11.md"))?)}))?;
    std::fs::create_dir_all(store.root.join("profiles"))?;
    std::fs::create_dir_all(store.root.join("claims"))?;
    write_json(&store.root.join("plans.json"), &plans)?;
    let mut jobs = tokio::task::JoinSet::new();
    for plan in plans {
        let sources: Vec<(String, String, String)> = plan
            .articles
            .iter()
            .map(|id| {
                let s = &source_map[id].1;
                (id.clone(), s.topic.clone(), s.text.clone())
            })
            .collect();
        let input:Vec<_>=sources.iter().enumerate().map(|(i,(_,topic,text))|
            json!({"article":format!("a{:02}",i+1),"topic":topic,"text":text})).collect();
        let store = Arc::clone(&store);
        let prompt = prompt.clone();
        jobs.spawn(async move {
            let id=plan.id.clone();
            let result=validated(&store,&format!("{id}-synthesis"),&prompt,&json!({"articles":input}).to_string(),
                |raw|validate(raw,&plan,&sources)).await;
            match result {
                Ok((artifact,claims))=>{
                    write_json(&store.root.join(format!("profiles/{id}.json")),&artifact)?;
                    crate::io::write_file_atomic(&store.root.join(format!("profiles/{id}.md")),&artifact.spec)?;
                    write_json(&store.root.join(format!("claims/{id}.json")),&claims)?;
                    Ok::<_,AppError>(json!({"id":id,"author":artifact.plan.author,"status":"complete","rules":claims["rules"].as_array().unwrap().len(),"characters":crate::spec_quality::character_count(&artifact.spec),"warnings":artifact.warnings}))
                }
                Err(e)=>Ok(json!({"id":id,"author":plan.author,"status":"failed","error":e.to_string()})),
            }
        });
    }
    finish_tasks(&store, &mut jobs, "profiles").await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_identity_quote_and_topic_checks_reject_fabricated_support() {
        let plan = Plan {
            id: "p".into(),
            author: "author".into(),
            articles: vec!["x".into(), "y".into()],
            mode: "primary".into(),
            references: vec![],
        };
        let sources = vec![
            (
                "x".into(),
                "daily".into(),
                "慢慢说出一件小事，随后给出平实判断。".into(),
            ),
            (
                "y".into(),
                "reading".into(),
                "先交代读到的情形，再让短句收住。".into(),
            ),
        ];
        let rule = json!({"mechanism":"先叙后评的表达倾向","when":"叙述后判断","instruction":"先用可感的事实或动作建立场景，再用较短的判断收束。不要沿用原文中的事件、人物和观点，也不必在每段强行重复。面对素材不具备相应动作的任务，可以保留叙述与判断的关系，换成任务确有的信息。避免把简短当作省略必写事实，判断应贴在已交代的内容后。这样的安排是有限样本里观察到的倾向，使用时需服从具体任务，而不是固定的模板。",
            "evidence":[{"article":"a01","quote":"慢慢说出一件小事"},{"article":"a02","quote":"先交代读到的情形"}]});
        let doc = json!({"rules":[rule.clone(),rule.clone(),rule.clone(),rule.clone()]});
        assert!(validate(&doc.to_string(), &plan, &sources).is_err()); // repeated mechanisms
        let mut unique = doc;
        for (i, r) in unique["rules"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .enumerate()
        {
            r["mechanism"] = json!(format!("机制{i}描述"));
        }
        assert!(validate(&unique.to_string(), &plan, &sources).is_ok());
        unique["rules"][0]["evidence"][1]["quote"] = json!("原文没有的虚构引文");
        assert!(validate(&unique.to_string(), &plan, &sources).is_err());
    }
}
