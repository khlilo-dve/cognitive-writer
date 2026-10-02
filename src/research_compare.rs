use crate::{
    error::AppError,
    eval_store::write_json,
    research_dataset::{sha256, valid_id},
    research_store::{ResearchStore, Role},
    style_compare::{self, Comparison, Feature, View},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{path::Path, sync::Arc};

const ROOT: &str = "research/style-aggregation";
#[derive(Clone, Serialize, Deserialize)]
struct Item {
    id: String,
    author: String,
    replicate: u64,
    spec: String,
    source_run: String,
}
#[derive(Clone, Serialize, Deserialize)]
struct Mapped {
    item: Item,
    view: View,
}

fn load_items(run: &str) -> Result<Vec<Item>, AppError> {
    if !valid_id(run) {
        return Err(AppError::Parse("来源run名称无效".into()));
    }
    let root = Path::new(ROOT).join("runs").join(run);
    let summary: Value = serde_json::from_slice(&std::fs::read(root.join("summary.json"))?)
        .map_err(|e| AppError::Parse(e.to_string()))?;
    if summary["status"] != "complete" {
        return Err(AppError::Parse("来源提取尚未完整完成".into()));
    }
    let rows = summary["rows"]
        .as_array()
        .ok_or_else(|| AppError::Parse("来源缺少rows".into()))?;
    let mut items = Vec::new();
    for row in rows {
        let id = row["artifact_id"]
            .as_str()
            .or(row["article_id"].as_str())
            .ok_or_else(|| AppError::Parse("来源id缺失".into()))?;
        if !valid_id(id) {
            return Err(AppError::Parse("来源artifact名称无效".into()));
        }
        let spec = std::fs::read_to_string(root.join(format!("single/{id}.md")))?;
        let saved: Value =
            serde_json::from_slice(&std::fs::read(root.join(format!("single/{id}.json")))?)
                .map_err(|e| AppError::Parse(e.to_string()))?;
        if saved["result"]["spec"].as_str() != Some(&spec)
            || row["characters"].as_u64()
                != Some(crate::spec_quality::character_count(&spec) as u64)
        {
            return Err(AppError::Parse("Style阅读副本与冻结结果不一致".into()));
        }
        items.push(Item {
            id: id.into(),
            author: row["author_id"]
                .as_str()
                .ok_or_else(|| AppError::Parse("作者元数据缺失".into()))?
                .into(),
            replicate: row["replicate"].as_u64().unwrap_or(1),
            spec,
            source_run: run.into(),
        });
    }
    Ok(items)
}

pub(crate) async fn validated<T, F>(
    store: &ResearchStore,
    name: &str,
    prompt: &str,
    input: &str,
    parse: F,
) -> Result<T, AppError>
where
    F: Fn(&str) -> Result<T, AppError>,
{
    let mut user = input.to_string();
    for attempt in 0..2 {
        let raw = store
            .call(
                &format!("{name}-{attempt}"),
                Role::Structured,
                prompt,
                &user,
                true,
            )
            .await?;
        match parse(&raw) {
            Ok(v) => return Ok(v),
            Err(e) if attempt == 0 => {
                user = format!(
                    "原始输入：{input}\n上次响应：{raw}\n结构/证据诊断：{e}\n只修复格式或证据定位，返回完整JSON，不改变判定标准。"
                );
            }
            Err(e) => return Err(e),
        }
    }
    Err(AppError::Parse("结构修复耗尽".into()))
}

pub(crate) async fn views(run: &str, base: &str, repeats: &str) -> Result<(), AppError> {
    let mut items = load_items(base)?;
    items.extend(load_items(repeats)?);
    if items.len() != 28 || items.iter().filter(|i| i.replicate == 1).count() != 20 {
        return Err(AppError::Parse("比较视图要求20首次+8重复".into()));
    }
    let mut ids = std::collections::HashSet::new();
    if items.iter().any(|i| !ids.insert(&i.id)) {
        return Err(AppError::Parse("重复artifact_id".into()));
    }
    let prompt = std::fs::read_to_string(format!("{ROOT}/prompts/comparison-view-v2.md"))?;
    let store = Arc::new(ResearchStore::open(Path::new(ROOT), run, false)?);
    store.freeze(&json!({"suite":"comparison_view_v2_structured","items":items,"prompt":prompt,"length_policy":"detailed-v1","analysis_thinking":"disabled","method_revision":1}))?;
    std::fs::create_dir_all(store.root.join("views"))?;
    let mut tasks = tokio::task::JoinSet::new();
    for item in items {
        let store = Arc::clone(&store);
        let prompt = prompt.clone();
        tasks.spawn(async move{
            let result=validated(&store,&format!("view-{}",item.id),&prompt,&style_compare::indexed_input(&item.spec),|s|style_compare::parse_indexed_view(s,&item.spec)).await;
            match result {
                Ok(view)=>{let output=Mapped{item:item.clone(),view};write_json(&store.root.join(format!("views/{}.json",item.id)),&output)?;Ok::<_,AppError>(json!({"id":item.id,"status":"complete","features":output.view.features.len()}))},
                Err(e)=>Ok(json!({"id":item.id,"status":"failed","error":e.to_string()})),
            }
        });
    }
    finish_tasks(&store, &mut tasks, "views").await
}

pub(crate) async fn finish_tasks(
    store: &ResearchStore,
    tasks: &mut tokio::task::JoinSet<Result<Value, AppError>>,
    stage: &str,
) -> Result<(), AppError> {
    let expected = tasks.len();
    let mut rows = Vec::new();
    while let Some(row) = tasks.join_next().await {
        rows.push(row.map_err(|_| AppError::Parse("研究任务异常终止".into()))??);
        rows.sort_by_key(|r| r["id"].as_str().unwrap_or("").to_string());
        write_json(
            &store.root.join("summary.json"),
            &json!({"status":"running","stage":stage,"expected":expected,"rows":rows,"usage":store.usage().await}),
        )?;
    }
    let failed = rows.iter().any(|r| r["status"] != "complete");
    write_json(
        &store.root.join("summary.json"),
        &json!({"status":if failed{"partial"}else{"complete"},"stage":stage,"expected":expected,"rows":rows,"usage":store.usage().await}),
    )?;
    if failed {
        Err(AppError::Parse("部分任务失败；保留独立完成链".into()))
    } else {
        Ok(())
    }
}

pub(crate) async fn compare(
    store: &ResearchStore,
    name: &str,
    prompt: &str,
    a: &View,
    b: &View,
) -> Result<Comparison, AppError> {
    let input = json!({"A":a,"B":b}).to_string();
    validated(store, name, prompt, &input, |s| {
        style_compare::parse_comparison(s, a, b)
    })
    .await
}

async fn controls(store: &ResearchStore, prompt: &str) -> Result<(), AppError> {
    let make = |rule: &str| View {
        features: vec![Feature {
            id: "f1".into(),
            dimension: "voice".into(),
            rule: rule.into(),
            condition: "全文叙述".into(),
            quote: rule.into(),
        }],
        uncertainties: vec![],
    };
    let a = make("全文保持冷静克制的语气，避免夸张情绪。");
    let cases = [
        ("self", a.clone(), 1, 0),
        ("synonym", make("叙述始终平静节制，不用情绪夸饰。"), 1, 0),
        (
            "opposite",
            make("全文采用热烈夸张的情绪表达，避免冷静克制。"),
            0,
            1,
        ),
        ("generic", make("全文表达自然清楚。"), 0, 0),
    ];
    let mut rows = Vec::new();
    for (name, b, m, c) in cases {
        let result = compare(store, &format!("control-{name}"), prompt, &a, &b).await?;
        let passed = result.matches.len() == m && result.conflicts.len() == c;
        rows.push(json!({"name":name,"passed":passed,"comparison":result}));
    }
    let passed = rows.iter().all(|r| r["passed"] == true);
    write_json(
        &store.root.join("controls.json"),
        &json!({"passed":passed,"rows":rows}),
    )?;
    if passed {
        Ok(())
    } else {
        Err(AppError::Parse(
            "语义比较控制未通过，不运行主矩阵；保留负面结果".into(),
        ))
    }
}

pub(crate) async fn pairs(run: &str, views_run: &str) -> Result<(), AppError> {
    if !valid_id(views_run) {
        return Err(AppError::Parse("来源run无效".into()));
    }
    let source = Path::new(ROOT).join("runs").join(views_run);
    let summary: Value = serde_json::from_slice(&std::fs::read(source.join("summary.json"))?)
        .map_err(|e| AppError::Parse(e.to_string()))?;
    if summary["status"] != "complete" {
        return Err(AppError::Parse("视图尚未完成".into()));
    }
    let mut mapped = Vec::<Mapped>::new();
    for entry in std::fs::read_dir(source.join("views"))? {
        let p = entry?.path();
        if p.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let m: Mapped = serde_json::from_slice(&std::fs::read(p)?)
            .map_err(|e| AppError::Parse(e.to_string()))?;
        style_compare::parse_view(
            &serde_json::to_string(&m.view).map_err(|e| AppError::Parse(e.to_string()))?,
            &m.item.spec,
        )?;
        mapped.push(m);
    }
    mapped.sort_by_key(|m| m.item.id.clone());
    let main: Vec<_> = mapped
        .iter()
        .filter(|m| m.item.replicate == 1)
        .cloned()
        .collect();
    if main.len() != 20 || mapped.len() != 28 {
        return Err(AppError::Parse("矩阵输入要求20首次和8重复视图".into()));
    }
    let prompt = std::fs::read_to_string(format!("{ROOT}/prompts/compare-pair-v1.md"))?;
    let store = Arc::new(ResearchStore::open(Path::new(ROOT), run, false)?);
    store.freeze(&json!({"suite":"style_pairs_v2_structured","source":views_run,"views_sha256":sha256(&serde_json::to_vec(&mapped).map_err(|e|AppError::Parse(e.to_string()))?),"prompt":prompt,"metric":"one_to_one_jaccard_per_axis_v1","analysis_thinking":"disabled","method_revision":1}))?;
    if let Err(e) = controls(&store, &prompt).await {
        write_json(
            &store.root.join("summary.json"),
            &json!({"status":"evaluation_invalid","error":e.to_string(),"usage":store.usage().await}),
        )?;
        return Err(e);
    }
    std::fs::create_dir_all(store.root.join("pairs"))?;
    let mut jobs = Vec::new();
    for i in 0..20 {
        for j in i + 1..20 {
            let (a, b) = if (i + j) % 2 == 0 {
                (main[i].clone(), main[j].clone())
            } else {
                (main[j].clone(), main[i].clone())
            };
            jobs.push((format!("pair-{i:02}-{j:02}"), "main".to_string(), a, b));
        }
    }
    for (n, m) in mapped.iter().filter(|m| m.item.replicate != 1).enumerate() {
        let base_id = m
            .item
            .id
            .rsplit_once("-r")
            .ok_or_else(|| AppError::Parse("重复样本id无效".into()))?
            .0;
        let a = main
            .iter()
            .find(|a| a.item.id == base_id)
            .ok_or_else(|| AppError::Parse("重复样本缺原样本".into()))?
            .clone();
        jobs.push((
            format!("repeat-{n:02}"),
            "repeat_noise".into(),
            a,
            m.clone(),
        ));
    }
    let swaps: Vec<_> = jobs
        .iter()
        .take(10)
        .enumerate()
        .map(|(i, (_, _, a, b))| {
            (
                format!("swap-{i:02}"),
                "order_check".into(),
                b.clone(),
                a.clone(),
            )
        })
        .collect();
    jobs.extend(swaps);
    let mut tasks = tokio::task::JoinSet::new();
    for (id, kind, a, b) in jobs {
        let store = Arc::clone(&store);
        let prompt = prompt.clone();
        tasks.spawn(async move{
            match compare(&store,&id,&prompt,&a.view,&b.view).await {
                Ok(c)=>{let metrics=style_compare::metrics(&a.view,&b.view,&c);
                    write_json(&store.root.join(format!("pairs/{id}.json")),&json!({"id":id,"kind":kind,"left":a.item.id,"right":b.item.id,"left_author":a.item.author,"right_author":b.item.author,"within_author":a.item.author==b.item.author,"comparison":c,"metrics":metrics}))?;
                    Ok::<_,AppError>(json!({"id":id,"kind":kind,"status":"complete"}))},
                Err(e)=>Ok(json!({"id":id,"kind":kind,"status":"failed","error":e.to_string()})),
            }
        });
    }
    finish_tasks(&store, &mut tasks, "pairs").await
}
