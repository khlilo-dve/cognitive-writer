use crate::{
    error::AppError,
    eval_store::write_json,
    research_compare::{finish_tasks, validated},
    research_dataset::{sha256, valid_id, Dataset},
    research_generation::{self, Generated},
    research_judgment,
    research_profile::ROOT,
    research_store::ResearchStore,
    transfer_quality::{self, Judgment},
};
use serde_json::{json, Value};
use std::{collections::BTreeMap, path::Path, sync::Arc};

async fn grade(
    store: &ResearchStore,
    name: &str,
    prompt: &str,
    source: &str,
    task: &str,
    body: &str,
) -> Result<Judgment, AppError> {
    let input = transfer_quality::judge_input(source, task, "strict", body);
    validated(store, name, prompt, &input, |s| {
        transfer_quality::parse_judgment(s, source, body)
    })
    .await
}

async fn controls(store: &ResearchStore, transfer: &str, author: &str) -> Result<Value, AppError> {
    let source="我给规矩写了一张请假条。它说批准，转头把自己的名字填进了出勤表。规矩没有腿，走得却比人快。";
    let reference_b = "雨落在屋檐上。院里的叶子湿了。傍晚，窗子透出一点灯光。";
    let task="严格素材：活动室设工具柜，居民登记借还。涵盖工具柜和登记借还，不得增添数据、人物对话或其他事件。";
    let base = "活动室设了工具柜，居民登记后借用，归还时也需要登记。";
    let mut rows = Vec::new();
    for (name, body) in [
        ("clean", base.to_owned()),
        (
            "fabrication",
            format!("{base}统计显示，已有九百人借用，节约了三万元。"),
        ),
        (
            "leak",
            format!("{base}我给规矩写了一张请假条。它说批准，转头把自己的名字填进了出勤表。"),
        ),
    ] {
        let j = grade(
            store,
            &format!("control-{name}"),
            transfer,
            source,
            task,
            &body,
        )
        .await?;
        let passed = match name {
            "clean" => {
                j.scores.factual_compliance == 5.0
                    && j.scores.source_independence == 5.0
                    && j.scores.style_fidelity <= 3.0
            }
            "fabrication" => j.scores.factual_compliance <= 4.0 && !j.fact_violations.is_empty(),
            _ => j.scores.source_independence <= 4.0 && !j.source_leakage.is_empty(),
        };
        rows.push(json!({"name":name,"passed":passed,"judgment":j}));
    }
    let raw_input = json!({"A":source,"B":reference_b,"article":source}).to_string();
    let j = validated(store, "control-author-self", author, &raw_input, |s| {
        research_judgment::parse(s, source, reference_b, source)
    })
    .await?;
    rows.push(json!({"name":"author-self","passed":j.choice=="A"&&j.fidelity_a>j.fidelity_b,"judgment":j}));
    let passed = rows.iter().all(|r| r["passed"] == true);
    let value = json!({"passed":passed,"rows":rows});
    write_json(&store.root.join("controls.json"), &value)?;
    if !passed {
        return Err(AppError::Parse("生成评分控制未通过，不启动正式评分".into()));
    }
    Ok(value)
}

pub(crate) async fn run(
    run: &str,
    generation: &str,
    phase: &str,
    reference_run: Option<&str>,
) -> Result<(), AppError> {
    if !["reference", "confirmation"].contains(&phase) {
        return Err(AppError::Parse("评估阶段必须reference/confirmation".into()));
    }
    let generated = research_generation::load(generation)?;
    let generation_sha =
        sha256(&serde_json::to_vec(&generated).map_err(|e| AppError::Parse(e.to_string()))?);
    let transfer = std::fs::read_to_string("prompts/eval_judge_transfer_v2.md")?;
    let author = std::fs::read_to_string(format!("{ROOT}/prompts/author-match-v1.md"))?;
    let mut inherited = Value::Null;
    if phase == "confirmation" {
        let previous = reference_run
            .filter(|s| valid_id(s))
            .ok_or_else(|| AppError::Parse("确认评估需要已完成reference run".into()))?;
        let dir = Path::new(ROOT).join("runs").join(previous);
        let inputs: Value = serde_json::from_slice(&std::fs::read(dir.join("inputs.json"))?)
            .map_err(|e| AppError::Parse(e.to_string()))?;
        let summary: Value = serde_json::from_slice(&std::fs::read(dir.join("summary.json"))?)
            .map_err(|e| AppError::Parse(e.to_string()))?;
        inherited = serde_json::from_slice(&std::fs::read(dir.join("controls.json"))?)
            .map_err(|e| AppError::Parse(e.to_string()))?;
        if summary["status"] != "complete"
            || inherited["passed"] != true
            || inputs["generation_sha256"] != generation_sha
            || inputs["transfer_prompt"] != transfer
            || inputs["author_prompt"] != author
        {
            return Err(AppError::Parse(
                "开发未完成/控制未过/生成或评分方法变化，拒绝解封确认材料".into(),
            ));
        }
    }
    let manifest = Path::new(ROOT).join("datasets/authors-v1/manifest.json");
    let ds = Dataset::load(&manifest)?;
    let mut sources = BTreeMap::<String, String>::new();
    let mut references = BTreeMap::new();
    for a in ds.selected(false) {
        sources
            .entry(a.author_id.clone())
            .or_default()
            .push_str(&format!(
                "\n\n[参考文章]\n{}",
                ds.read_article(&manifest, a)?
            ));
    }
    for a in ds.articles.iter().filter(|a| a.split == phase) {
        if references
            .insert(a.author_id.clone(), ds.read_article(&manifest, a)?)
            .is_some()
        {
            return Err(AppError::Parse("每作者每阶段只能有一篇独立参考".into()));
        }
    }
    if sources.len() != 2 || references.len() != 2 {
        return Err(AppError::Parse("作者与独立参考覆盖不完整".into()));
    }
    let jobs: Vec<_> = generated
        .into_iter()
        .filter(|g| g.job.phase == phase)
        .collect();
    if jobs.len() != if phase == "reference" { 20 } else { 40 } {
        return Err(AppError::Parse("评估阶段篇数与预登记不符".into()));
    }
    let store = Arc::new(ResearchStore::open(Path::new(ROOT), run, false)?);
    store.freeze(&json!({"suite":"style_generation_eval_v2_structured","analysis_thinking":"disabled","method_revision":1,"phase":phase,"generation_run":generation,"generation_sha256":generation_sha,"transfer_prompt":transfer,"author_prompt":author,"source_context":"same ten discovery sources per author for every method; no Spec shown","sources":sources,"references":references,"jobs":jobs,"reference_run":reference_run}))?;
    if phase == "reference" {
        if let Err(e) = controls(&store, &transfer, &author).await {
            write_json(
                &store.root.join("summary.json"),
                &json!({"status":"evaluation_invalid","error":e.to_string(),"usage":store.usage().await}),
            )?;
            return Err(e);
        }
    } else {
        write_json(&store.root.join("controls.json"), &inherited)?;
    }
    std::fs::create_dir_all(store.root.join("judgments"))?;
    let authors: Vec<_> = references.keys().cloned().collect();
    let mut tasks = tokio::task::JoinSet::new();
    let mut positions = BTreeMap::new();
    for g in jobs {
        let target_index = authors
            .iter()
            .position(|a| *a == g.job.author)
            .ok_or_else(|| AppError::Parse("待评文章作者不在参考集合".into()))?;
        let left_is_target = next_target_position(&mut positions, &g.job.author, &g.job.method);
        let first = &authors[if left_is_target {
            target_index
        } else {
            1 - target_index
        }];
        let second = &authors[if left_is_target {
            1 - target_index
        } else {
            target_index
        }];
        let a = references[first].clone();
        let b = references[second].clone();
        let target = if g.job.author == *first { "A" } else { "B" }.to_owned();
        let source = sources[&g.job.author].clone();
        let store = Arc::clone(&store);
        let transfer = transfer.clone();
        let author = author.clone();
        tasks.spawn(async move{
            let result=evaluate(&store,&g,&source,&a,&b,&target,&transfer,&author).await;
            match result {Ok(row)=>{write_json(&store.root.join(format!("judgments/{}.json",g.job.id)),&row)?;Ok::<_,AppError>(json!({"id":g.job.id,"status":"complete","author":g.job.author,"method":g.job.method,"topic":g.job.topic_id}))},Err(e)=>Ok(json!({"id":g.job.id,"status":"failed","error":e.to_string()}))}
        });
    }
    finish_tasks(&store, &mut tasks, "generation_evaluation").await
}

fn next_target_position(
    counts: &mut BTreeMap<(String, String), usize>,
    author: &str,
    method: &str,
) -> bool {
    let n = counts
        .entry((author.to_owned(), method.to_owned()))
        .or_default();
    let left = *n % 2 == 0;
    *n += 1;
    left
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn neutral_author_assignment_cannot_force_all_targets_onto_the_same_side() {
        let mut counts = BTreeMap::new();
        let observed: Vec<_> = ["a", "b", "a", "b"]
            .iter()
            .map(|a| next_target_position(&mut counts, a, "neutral"))
            .collect();
        assert_eq!(observed, vec![true, true, false, false]);
        assert!(next_target_position(&mut counts, "a", "single"));
        assert!(!next_target_position(&mut counts, "a", "single"));
    }
}

async fn evaluate(
    store: &ResearchStore,
    g: &Generated,
    source: &str,
    a: &str,
    b: &str,
    target: &str,
    transfer: &str,
    author: &str,
) -> Result<Value, AppError> {
    let t = grade(
        store,
        &format!("{}-transfer", g.job.id),
        transfer,
        source,
        &g.job.task,
        &g.body,
    )
    .await?;
    let input = json!({"A":a,"B":b,"article":g.body}).to_string();
    let j = validated(
        store,
        &format!("{}-author", g.job.id),
        author,
        &input,
        |s| research_judgment::parse(s, a, b, &g.body),
    )
    .await?;
    Ok(
        json!({"id":g.job.id,"author":g.job.author,"method":g.job.method,"style_id":g.job.style_id,"topic":g.job.topic_id,"phase":g.job.phase,"replicate":g.job.replicate,"length":g.length,"transfer":t,"author_match":j,"target_label":target,"target_fidelity":if target=="A"{j.fidelity_a}else{j.fidelity_b},"other_fidelity":if target=="A"{j.fidelity_b}else{j.fidelity_a},"body_sha256":sha256(g.body.as_bytes())}),
    )
}
