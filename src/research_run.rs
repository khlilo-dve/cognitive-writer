use crate::{
    error::AppError,
    eval_store::write_json,
    research_dataset::{Dataset, sha256},
    research_store::{ResearchStore, Role},
    style_learning::extract_direct_with,
};
use serde_json::{Value, json};
use std::{path::Path, sync::Arc};

const ROOT: &str = "research/style-aggregation";

pub async fn run(args: &[&str]) -> Result<(), AppError> {
    match args {
        ["view-coverage",run]=>{println!("{}",crate::research_compare::view_coverage(run)?);Ok(())},
        ["profiles",run,views,pairs]=>crate::research_profile::run(run,views,pairs).await,
        ["convergence",run,profiles,p2]=>crate::research_convergence::run(run,profiles,p2).await,
        ["generate",run,profiles,single]=>crate::research_generation::run(run,profiles,single).await,
        ["evaluate",run,generation,"reference"]=>crate::research_evaluation::run(run,generation,"reference",None).await,
        ["evaluate",run,generation,"confirmation",reference]=>crate::research_evaluation::run(run,generation,"confirmation",Some(reference)).await,
        ["views",run,base,repeats]=>crate::research_compare::views(run,base,repeats).await,
        ["pairs",run,views]=>crate::research_compare::pairs(run,views).await,
        ["validate", path] => {
            let ds = Dataset::load(Path::new(path))?;
            println!(
                "{}: {} verified articles, {} discovery",
                ds.dataset_id,
                ds.articles.len(),
                ds.selected(false).len()
            );
            Ok(())
        }
        ["extract", path, run_id, mode] if ["pilot", "full", "repeat"].contains(mode) => {
            extract(Path::new(path), run_id, mode, None).await
        }
        ["extract",path,run_id,"full",source_run] => {
            extract(Path::new(path),run_id,"full",Some(source_run)).await
        }
        _ => Err(AppError::Parse(
            "用法: research validate <manifest> | extract <manifest> <run> pilot|full|repeat [source] | views <run> <single> <repeat> | pairs <run> <views> | profiles <run> <views> <pairs> | convergence <run> <profiles> <pairs> | generate <run> <profiles> <single> | evaluate <run> <generation> reference|confirmation [reference_run]"
                .into(),
        )),
    }
}

async fn extract(
    path: &Path,
    run_id: &str,
    mode: &str,
    reuse: Option<&str>,
) -> Result<(), AppError> {
    let pilot = mode == "pilot";
    let ds = Dataset::load(path)?;
    let selected = ds.selected(mode != "full");
    let mut counts = std::collections::BTreeMap::new();
    for a in &selected {
        *counts.entry(&a.author_id).or_insert(0usize) += 1;
    }
    let expected = if mode != "full" { 2 } else { 10 };
    if counts.len() != 2 || counts.values().any(|n| *n != expected) {
        return Err(AppError::Parse(format!(
            "该阶段要求两位作者各{expected}篇discovery"
        )));
    }
    let prompt = std::fs::read_to_string("prompts/learn_style_direct.md")?;
    let store = Arc::new(ResearchStore::open(Path::new(ROOT), run_id, pilot)?);
    let mut frozen = json!({"schema":1,"suite":"style_research_single_v2_detailed","dataset":ds,"pilot":pilot,
        "prompt":prompt,"prompt_sha256":sha256(prompt.as_bytes()),"model":"mimo-v2.6-pro",
        "max_completion_tokens":16384,"thinking":"enabled","protocol":"v2_detailed",
        "writer_model":Role::Writer.model(),"length_policy":crate::spec_quality::DETAILED_POLICY,"source_snapshot":path.to_string_lossy()});
    // Keep the original pilot input identity intact for offline replay.
    if mode != "pilot" {
        frozen["mode"] = json!(mode);
        frozen["reuse_run"] = json!(reuse);
    }
    store.freeze(&frozen)?;
    if let Some(source) = reuse {
        store.import_completed(source).await?;
    }
    std::fs::create_dir_all(store.root.join("single"))?;
    let mut rows = Vec::<Value>::new();
    let mut jobs = tokio::task::JoinSet::new();
    for a in selected {
        let source = ds.read_article(path, a)?;
        let replicates = if mode == "repeat" {
            vec![2, 3]
        } else {
            vec![1]
        };
        for replicate in replicates {
            let a = a.clone();
            let source = source.clone();
            let prompt = prompt.clone();
            let store = Arc::clone(&store);
            jobs.spawn(async move {
                let id=if replicate==1 {a.article_id.clone()} else {format!("{}-r{replicate}",a.article_id)};
                let result=extract_direct_with(&source,&prompt,|stage,system,user|{
                    let store=Arc::clone(&store); let name=format!("{id}-{stage}");
                    async move {store.call(&name,Role::Analysis,&system,&user,true).await}
                }).await;
                let learned=match result {
                    Ok(v)=>v,
                    Err(e)=>return Ok::<Value,AppError>(json!({"article_id":a.article_id,"artifact_id":id,"replicate":replicate,"status":"failed","error":e.to_string()})),
                };
                write_json(&store.root.join(format!("single/{id}.json")),&json!({"article_id":a.article_id,"artifact_id":id,
                    "replicate":replicate,"source_sha256":a.sha256,"model":"mimo-v2.6-pro","result":learned}))?;
                crate::io::write_file_atomic(&store.root.join(format!("single/{id}.md")),&learned.spec)?;
                Ok(json!({"article_id":a.article_id,"artifact_id":id,"author_id":a.author_id,"replicate":replicate,"status":"complete",
                    "characters":crate::spec_quality::character_count(&learned.spec),"warnings":learned.warnings,"calls":learned.calls}))
            });
        }
    }
    write_json(
        &store.root.join("state.json"),
        &json!({"stage":"single","status":"running","mode":mode,"expected_jobs":jobs.len(),"rows":rows}),
    )?;
    while let Some(result) = jobs.join_next().await {
        rows.push(result.map_err(|_| AppError::Parse("研究提取任务异常终止".into()))??);
        rows.sort_by_key(|v| v["artifact_id"].as_str().unwrap_or("").to_owned());
        save_summary(&store, &rows, "running").await?;
    }
    let failed = rows.iter().any(|r| r["status"] != "complete");
    save_summary(&store, &rows, if failed { "partial" } else { "complete" }).await?;
    if failed {
        Err(AppError::Parse(
            "部分独立提取失败，已保存全部完成链与错误".into(),
        ))
    } else {
        Ok(())
    }
}

async fn save_summary(store: &ResearchStore, rows: &[Value], status: &str) -> Result<(), AppError> {
    let usage = store.usage().await;
    let summary = json!({"status":status,"stage":"single","rows":rows,"usage":usage,"completed_at":chrono::Utc::now().to_rfc3339()});
    write_json(&store.root.join("summary.json"), &summary)?;
    write_json(&store.root.join("state.json"), &summary)?;
    let mut md = format!(
        "# 单篇独立提取\n\n状态：{status}。本阶段只验证提取与恢复，不证明聚合有效。\n\n|文章|状态|非空白字符|\n|---|---|---:|\n"
    );
    for r in rows {
        md.push_str(&format!(
            "|{}|{}|{}|\n",
            r["article_id"].as_str().unwrap_or(""),
            r["status"].as_str().unwrap_or(""),
            r.get("characters")
                .map(Value::to_string)
                .unwrap_or("NA".into())
        ));
    }
    md.push_str(&format!(
        "\n本run占用费用（含不确定预留）：{}元；全研究累计占用：{}元。\n",
        usage["run_occupied_cny"], usage["occupied_cny"]
    ));
    crate::io::write_file_atomic(&store.root.join("summary.md"), &md)
}
