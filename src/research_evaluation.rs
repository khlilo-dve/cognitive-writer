use crate::{
    error::AppError,
    eval_store::write_json,
    research_compare::{finish_tasks, validated},
    research_dataset::{Dataset, sha256, valid_id},
    research_generation::{self, Generated},
    research_judgment,
    research_profile::ROOT,
    research_receipt::{self, Checkpoint},
    research_store::ResearchStore,
    transfer_quality::{self, Judgment},
};
use serde_json::{Value, json};
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
    let source = "我给规矩写了一张请假条。它说批准，转头把自己的名字填进了出勤表。规矩没有腿，走得却比人快。";
    let reference_b = "雨落在屋檐上。院里的叶子湿了。傍晚，窗子透出一点灯光。";
    let task = "严格素材：活动室设工具柜，居民登记借还。涵盖工具柜和登记借还，不得增添数据、人物对话或其他事件。";
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
    run_with_context(run, generation, phase, reference_run, false, false).await
}

pub(crate) async fn run_compact(
    run: &str,
    generation: &str,
    phase: &str,
    reference_run: Option<&str>,
) -> Result<(), AppError> {
    run_with_context(run, generation, phase, reference_run, true, false).await
}

pub(crate) async fn run_compact_reuse(
    run: &str,
    generation: &str,
    phase: &str,
    reference_run: Option<&str>,
) -> Result<(), AppError> {
    run_with_context(run, generation, phase, reference_run, true, true).await
}

fn reused_controls(
    generation_sha: &str,
    transfer: &str,
    author: &str,
) -> Result<(Value, Value), AppError> {
    let source = Path::new(ROOT).join("runs/pragmatic-reference-01");
    let inputs: Value = serde_json::from_slice(&std::fs::read(source.join("inputs.json"))?)
        .map_err(|e| AppError::Parse(e.to_string()))?;
    let control_bytes = std::fs::read(source.join("controls.json"))?;
    let controls: Value =
        serde_json::from_slice(&control_bytes).map_err(|e| AppError::Parse(e.to_string()))?;
    if inputs["generation_sha256"] != generation_sha
        || inputs["transfer_prompt"] != transfer
        || inputs["author_prompt"] != author
        || controls["passed"] != true
    {
        return Err(AppError::Parse(
            "旧控制快照生成身份、Prompt或通过状态不一致".into(),
        ));
    }
    let rows = controls["rows"]
        .as_array()
        .ok_or_else(|| AppError::Parse("旧控制记录缺失".into()))?;
    if rows.len() != 4
        || ["clean", "fabrication", "leak", "author-self"]
            .iter()
            .any(|name| {
                !rows
                    .iter()
                    .any(|r| r["name"] == *name && r["passed"] == true)
            })
    {
        return Err(AppError::Parse("旧控制未完整通过".into()));
    }
    for name in ["clean", "fabrication", "leak", "author-self"] {
        let prefix = format!("control-{name}");
        let path = [1, 0]
            .into_iter()
            .map(|attempt| source.join(format!("requests/{prefix}-{attempt}.json")))
            .find(|p| p.exists())
            .ok_or_else(|| AppError::Parse("旧控制缺少响应检查点".into()))?;
        let cp: Checkpoint = serde_json::from_slice(&std::fs::read(path)?)
            .map_err(|e| AppError::Parse(e.to_string()))?;
        research_receipt::verify(&cp)?;
        if cp.receipt.model != "mimo-v2.6-pro"
            || cp.receipt.finish_reason != "stop"
            || cp.receipt.content.trim().is_empty()
        {
            return Err(AppError::Parse("旧控制响应模型/完成状态无效".into()));
        }
    }
    Ok((
        controls,
        json!({"source_run":"pragmatic-reference-01","controls_sha256":sha256(&control_bytes),
        "caveat":"same controls passed once and failed on a later rerun; exploratory judge only"}),
    ))
}

async fn run_with_context(
    run: &str,
    generation: &str,
    phase: &str,
    reference_run: Option<&str>,
    compact: bool,
    reuse: bool,
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
    let mut prior_inputs = Value::Null;
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
            || inputs["suite"]
                != if reuse {
                    "style_generation_eval_v2_compact_reuse"
                } else if compact {
                    "style_generation_eval_v2_compact"
                } else {
                    "style_generation_eval_v2_structured"
                }
            || inputs["generation_sha256"] != generation_sha
            || inputs["transfer_prompt"] != transfer
            || inputs["author_prompt"] != author
        {
            return Err(AppError::Parse(
                "开发未完成/控制未过/生成或评分方法变化，拒绝解封确认材料".into(),
            ));
        }
        prior_inputs = inputs;
    }
    let manifest = Path::new(ROOT).join("datasets/authors-v1/manifest.json");
    let ds = Dataset::load(&manifest)?;
    let mut sources = BTreeMap::<String, String>::new();
    let mut references = BTreeMap::new();
    let mut selection = Value::Null;
    let mut selected_ids = BTreeMap::<String, Vec<String>>::new();
    if compact {
        let sampling: Value =
            serde_json::from_slice(&std::fs::read(format!("{ROOT}/datasets/sampling-v1.json"))?)
                .map_err(|e| AppError::Parse(e.to_string()))?;
        if sampling["manifest_sha256"] != sha256(&std::fs::read(&manifest)?) {
            return Err(AppError::Parse("评分的预定单篇基线与资料身份变化".into()));
        }
        for (author, data) in sampling["authors"]
            .as_object()
            .ok_or_else(|| AppError::Parse("缺少冻结作者清单".into()))?
        {
            let baselines = data["generation_baselines"]
                .as_object()
                .ok_or_else(|| AppError::Parse("缺少预定三篇参照".into()))?;
            if baselines.len() != 3 {
                return Err(AppError::Parse("每位作者需要三个题材参照".into()));
            }
            let mut ids = Vec::new();
            for topic in ["daily", "reading", "society"] {
                let id = baselines[topic]
                    .as_str()
                    .ok_or_else(|| AppError::Parse("基线文章ID无效".into()))?;
                if !ds
                    .selected(false)
                    .iter()
                    .any(|a| a.article_id == id && a.author_id == *author && a.topic == topic)
                {
                    return Err(AppError::Parse("评分参考不是冻结作者/主题discovery".into()));
                }
                ids.push(id.to_owned());
            }
            selected_ids.insert(author.clone(), ids);
        }
        if selected_ids.len() != 2 {
            return Err(AppError::Parse("评分需要两位作者参照".into()));
        }
        selection = json!(selected_ids);
        if phase == "confirmation" && prior_inputs["source_selection"] != selection {
            return Err(AppError::Parse("确认评分参考清单与开发阶段不同".into()));
        }
    }
    for a in ds.selected(false) {
        if compact
            && !selected_ids
                .get(&a.author_id)
                .is_some_and(|ids| ids.contains(&a.article_id))
        {
            continue;
        }
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
    if compact {
        store
            .limit_run(if phase == "reference" {
                15_000_000
            } else {
                30_000_000
            })
            .await?;
    }
    let (control_snapshot, control_source) = if reuse {
        if phase == "reference" {
            let (controls, source) = reused_controls(&generation_sha, &transfer, &author)?;
            (Some(controls), source)
        } else {
            (None, prior_inputs["control_source"].clone())
        }
    } else {
        (None, Value::Null)
    };
    let mut frozen = json!({"suite":"style_generation_eval_v2_structured","analysis_thinking":"disabled","method_revision":1,"phase":phase,"generation_run":generation,"generation_sha256":generation_sha,"transfer_prompt":transfer,"author_prompt":author,"source_context":"same ten discovery sources per author for every method; no Spec shown","sources":sources,"references":references,"jobs":jobs,"reference_run":reference_run});
    if compact {
        frozen["suite"] = json!(if reuse {
            "style_generation_eval_v2_compact_reuse"
        } else {
            "style_generation_eval_v2_compact"
        });
        frozen["method_revision"] = json!(if reuse { 15 } else { 14 });
        frozen["source_context"] = json!(
            "same three preregistered discovery baseline articles per author for every method; no Spec shown"
        );
        frozen["source_selection"] = selection;
        frozen["run_cap_micro_cny"] = json!(if phase == "reference" {
            15_000_000
        } else {
            30_000_000
        });
        frozen["protocol_sha256"] = json!(sha256(&std::fs::read(format!(
            "{ROOT}/method-revision-{}.md",
            if reuse { 15 } else { 14 }
        ))?));
        if reuse {
            frozen["control_source"] = control_source;
        }
    }
    store.freeze(&frozen)?;
    if phase == "reference" {
        if let Some(controls) = control_snapshot {
            write_json(&store.root.join("controls.json"), &controls)?;
        } else if let Err(e) = controls(&store, &transfer, &author).await {
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
