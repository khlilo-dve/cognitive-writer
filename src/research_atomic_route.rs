//! A matched Flash feasibility check on frozen atomic views; never overwrites Pro outcomes.
use crate::{
    error::AppError,
    eval_store::write_json,
    research_candidates::{compare_configured, control_passed},
    research_compare::finish_tasks,
    research_dataset::{sha256, valid_id},
    research_store::{ResearchStore, Role},
    style_compare::{self, View},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path, sync::Arc};
const ROOT: &str = "research/style-aggregation";
fn read(path: impl AsRef<Path>) -> Result<Value, AppError> {
    serde_json::from_slice(&std::fs::read(path)?).map_err(|e| AppError::Parse(e.to_string()))
}

pub(crate) async fn run(run: &str, source: &str) -> Result<(), AppError> {
    if !valid_id(source) {
        return Err(AppError::Parse("原子来源run无效".into()));
    }
    let root = Path::new(ROOT);
    let prior = root.join("runs").join(source);
    let inputs = read(prior.join("inputs.json"))?;
    let reviewed = inputs["suite"] == "atomic_view_reviewed_v1";
    let status = read(prior.join("summary.json"))?;
    if !(reviewed && status["status"] == "views_reviewed"
        || inputs["suite"] == "atomic_view_pilot_v2" && status["status"] == "complete")
        || read(prior.join("view-summary.json"))?["status"] != "complete"
    {
        return Err(AppError::Parse(
            "来源不是完整Pro对照或独立校核视图，不启动Flash试验".into(),
        ));
    }
    let prompt = std::fs::read_to_string(root.join("prompts/compare-candidates-v4.md"))?;
    if inputs["compare_prompt"] != prompt {
        return Err(AppError::Parse("比较Prompt已变，不能称匹配对照".into()));
    }
    let mut views = BTreeMap::<String, View>::new();
    let review = if reviewed {
        read(prior.join("semantic-review.json"))?
    } else {
        Value::Null
    };
    if reviewed
        && (review["decision"] != "approved_for_feasibility_only"
            || review["inputs_sha256"] != sha256(&std::fs::read(prior.join("inputs.json"))?))
    {
        return Err(AppError::Parse("校核输入身份或用途不匹配".into()));
    }
    for (id, spec) in inputs["sources"]
        .as_object()
        .ok_or_else(|| AppError::Parse("原子来源缺失".into()))?
    {
        let v = read(prior.join(format!("views/{id}.json")))?;
        if reviewed
            && review["view_sha256"][id]
                != sha256(&std::fs::read(prior.join(format!("views/{id}.json")))?)
        {
            return Err(AppError::Parse("校核后的视图被改动".into()));
        }
        if v["spec"] != *spec {
            return Err(AppError::Parse("原子Spec身份变化".into()));
        }
        views.insert(
            id.clone(),
            style_compare::parse_view(&v["view"].to_string(), spec.as_str().unwrap())?,
        );
    }
    let frozen = read(root.join("runs/p2-candidates-pilot-03/inputs.json"))?;
    for (i, s) in inputs["selection"]
        .as_array()
        .ok_or_else(|| AppError::Parse("来源选样缺失".into()))?
        .iter()
        .enumerate()
    {
        let left = s[0]
            .as_str()
            .ok_or_else(|| AppError::Parse("左侧ID缺失".into()))?;
        let right = s[1]
            .as_str()
            .ok_or_else(|| AppError::Parse("右侧ID缺失".into()))?;
        let expected = serde_json::to_value(crate::style_candidates::candidates(
            &views[left],
            &views[right],
        ))
        .map_err(|e| AppError::Parse(e.to_string()))?;
        if !reviewed
            && read(prior.join(format!("pairs/atomic-{i:02}.json")))?["candidates"] != expected
        {
            return Err(AppError::Parse(
                "原子视图已不同于Pro对照，不能进行配对模型实验".into(),
            ));
        }
    }
    let controls: Vec<(String, View, View)> = serde_json::from_value(frozen["controls"].clone())
        .map_err(|e| AppError::Parse(e.to_string()))?;
    if controls.len() != 13 || frozen["prompt"] != prompt {
        return Err(AppError::Parse("缺少固定13控制或Prompt不一致".into()));
    }
    let store = Arc::new(ResearchStore::open(root, run, false)?);
    store.limit_run(1_000_000).await?;
    store.freeze(&json!({"suite":"atomic_flash_route_v1","source":source,"views":views,"selection":inputs["selection"],"controls":controls,"prompt":prompt,
        "model":"mimo-v2.6-flash","thinking":"disabled","max_completion_tokens":16384,"batch_size":8,"rule_only":true,"run_cap_micro_cny":1000000,
        "design":if reviewed{"standalone_feasibility_not_pro_flash_quality_comparison"}else{"matched_pro_flash_development_comparison"},"source_review":review}))?;
    let mut checks = Vec::new();
    for (id, a, b) in controls {
        let row = match compare_configured(
            &store,
            &format!("control-{id}"),
            &prompt,
            Role::FlashAnalysis,
            &a,
            &b,
            8,
            true,
        )
        .await
        {
            Ok(result) => json!({"id":id,"passed":control_passed(&id,&result)?,"result":result}),
            Err(e) => json!({"id":id,"passed":false,"error":e.to_string()}),
        };
        let passed = row["passed"] == true;
        checks.push(row);
        write_json(
            &store.root.join("controls.json"),
            &json!({"passed":checks.len()==13&&checks.iter().all(|r|r["passed"]==true),"rows":checks}),
        )?;
        write_json(
            &store.root.join("summary.json"),
            &json!({"status":if passed{"running"}else{"evaluation_invalid"},"rows":[],"controls":checks,"usage":store.usage().await}),
        )?;
        println!("atomic Flash control {id}: {passed}");
        if !passed {
            return Err(AppError::Parse(format!(
                "Flash控制{id}未通过，真实样本不启动"
            )));
        }
    }
    std::fs::create_dir_all(store.root.join("pairs"))?;
    let chains = Arc::new(tokio::sync::Semaphore::new(3));
    let mut jobs = tokio::task::JoinSet::new();
    for (i, s) in inputs["selection"].as_array().unwrap().iter().enumerate() {
        let left = s[0].as_str().unwrap().to_string();
        let right = s[1].as_str().unwrap().to_string();
        let kind = s[2].as_str().unwrap().to_string();
        let a = views[&left].clone();
        let b = views[&right].clone();
        let store = Arc::clone(&store);
        let prompt = prompt.clone();
        let chains = Arc::clone(&chains);
        jobs.spawn(async move {
            let _permit = chains
                .acquire()
                .await
                .map_err(|_| AppError::Parse("Flash小试调度关闭".into()))?;
            let id = format!("atomic-{i:02}");
            match compare_configured(&store, &id, &prompt, Role::FlashAnalysis, &a, &b, 8, true)
                .await
            {
                Ok(mut result) => {
                    result["left"] = json!(left);
                    result["right"] = json!(right);
                    result["kind"] = json!(kind);
                    write_json(&store.root.join(format!("pairs/{id}.json")), &result)?;
                    Ok::<_, AppError>(json!({"id":id,"kind":kind,"status":"complete"}))
                }
                Err(e) => Ok(json!({"id":id,"kind":kind,"status":"failed","error":e.to_string()})),
            }
        });
    }
    finish_tasks(&store, &mut jobs, "atomic_flash_semantic_review_required").await
}
