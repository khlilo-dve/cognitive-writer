use crate::{
    error::AppError,
    eval_store::write_json,
    research_dataset::{Dataset, sha256},
    research_store::{ResearchStore, Role},
    style_learning::extract_direct_with,
};
use serde_json::{Value, json};
use std::path::Path;

const ROOT: &str = "research/style-aggregation";

pub async fn run(args: &[&str]) -> Result<(), AppError> {
    match args {
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
        ["extract", path, run_id, mode] if ["pilot", "full"].contains(mode) => {
            extract(Path::new(path), run_id, *mode == "pilot").await
        }
        _ => Err(AppError::Parse(
            "用法: research validate <manifest> | research extract <manifest> <run_id> pilot|full"
                .into(),
        )),
    }
}

async fn extract(path: &Path, run_id: &str, pilot: bool) -> Result<(), AppError> {
    let ds = Dataset::load(path)?;
    let selected = ds.selected(pilot);
    let mut counts = std::collections::BTreeMap::new();
    for a in &selected {
        *counts.entry(&a.author_id).or_insert(0usize) += 1;
    }
    let expected = if pilot { 2 } else { 10 };
    if counts.len() != 2 || counts.values().any(|n| *n != expected) {
        return Err(AppError::Parse(format!(
            "该阶段要求两位作者各{expected}篇discovery"
        )));
    }
    let prompt = std::fs::read_to_string("prompts/learn_style_direct.md")?;
    let store = ResearchStore::open(Path::new(ROOT), run_id, pilot)?;
    store.freeze(&json!({"schema":1,"suite":"style_research_single_v2_detailed","dataset":ds,"pilot":pilot,
        "prompt":prompt,"prompt_sha256":sha256(prompt.as_bytes()),"model":"mimo-v2.6-pro",
        "max_completion_tokens":16384,"thinking":"enabled","protocol":"v2_detailed",
        "writer_model":Role::Writer.model(),"length_policy":crate::spec_quality::DETAILED_POLICY,"source_snapshot":path.to_string_lossy()}))?;
    std::fs::create_dir_all(store.root.join("single"))?;
    let mut rows = Vec::<Value>::new();
    for a in selected {
        write_json(
            &store.root.join("state.json"),
            &json!({"stage":"single","status":"running","active":a.article_id,"rows":rows}),
        )?;
        let source = ds.read_article(path, a)?;
        let result = extract_direct_with(&source, &prompt, |stage, system, user| {
            let store = &store;
            let name = format!("{}-{stage}", a.article_id);
            async move {
                store
                    .call(&name, Role::Analysis, &system, &user, true)
                    .await
            }
        })
        .await;
        match result {
            Ok(learned) => {
                let id = &a.article_id;
                write_json(
                    &store.root.join(format!("single/{id}.json")),
                    &json!({"article_id":id,
                    "source_sha256":a.sha256,"model":"mimo-v2.6-pro","result":learned}),
                )?;
                crate::io::write_file_atomic(
                    &store.root.join(format!("single/{id}.md")),
                    &learned.spec,
                )?;
                rows.push(json!({"article_id":id,"author_id":a.author_id,"status":"complete",
                    "characters":crate::spec_quality::character_count(&learned.spec),"warnings":learned.warnings,"calls":learned.calls}));
            }
            Err(e) => {
                rows.push(
                    json!({"article_id":a.article_id,"status":"failed","error":e.to_string()}),
                );
                // A malformed/transport response is retained, never replaced by repeated sampling.
                save_summary(&store, &rows, "partial").await?;
                return Err(e);
            }
        }
    }
    save_summary(&store, &rows, "complete").await
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
