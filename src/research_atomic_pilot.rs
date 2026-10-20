//! Test atomic representation units without modifying any saved Style or production setting.
use crate::{
    error::AppError,
    eval_store::write_json,
    research_candidates::compare_configured,
    research_compare::{finish_tasks, validated},
    research_store::{ResearchStore, Role},
    style_compare::{self, View},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path, sync::Arc};
const ROOT: &str = "research/style-aggregation";

fn parse(raw: &str, spec: &str, version: u8) -> Result<View, AppError> {
    let view = style_compare::parse_indexed_view(raw, spec)?;
    if view.features.len() > 20
        || view.features.iter().any(|f| {
            crate::spec_quality::character_count(&f.rule) > 60
                || crate::spec_quality::character_count(&f.condition) > 60
        })
    {
        return Err(AppError::Parse(
            "原子视图超过20条或单条rule/condition超过60字符；须拆清机制，不可补充新规则".into(),
        ));
    }
    if version == 2 {
        for f in &view.features {
            if ["结尾", "篇末", "收尾"].iter().any(|s| f.rule.contains(s))
                && f.dimension != "ending"
            {
                return Err(AppError::Parse(format!(
                    "{}含篇末专用机制，应拆成单条并归ending；不得放rhythm/voice等一般维度",
                    f.id
                )));
            }
            if ["先给判断", "先给结论", "先说判断", "先说结论"]
                .iter()
                .any(|s| f.rule.contains(s))
                && f.dimension == "rhythm"
            {
                return Err(AppError::Parse(format!(
                    "{}是判断/解释次序，属于information，不是句长停顿rhythm",
                    f.id
                )));
            }
        }
    }
    Ok(view)
}

pub(crate) async fn run(run: &str, version: u8) -> Result<(), AppError> {
    if ![1, 2].contains(&version) {
        return Err(AppError::Parse("原子视图版本无效".into()));
    }
    let root = Path::new(ROOT);
    let ids = [
        "wxb-01",
        "wxb-02",
        "wzq-01",
        "wzq-02",
        "wxb-01-r2",
        "wzq-01-r2",
    ];
    let selection = [
        ("wxb-01", "wxb-02", "within"),
        ("wzq-01", "wzq-02", "within"),
        ("wxb-01", "wzq-01", "between"),
        ("wxb-02", "wzq-02", "between"),
        ("wxb-01", "wxb-01-r2", "repeat"),
        ("wzq-01", "wzq-01-r2", "repeat"),
    ];
    let mut sources = BTreeMap::<String, String>::new();
    for id in ids {
        let v: Value = serde_json::from_slice(&std::fs::read(
            root.join(format!("runs/p2-views-02/views/{id}.json")),
        )?)
        .map_err(|e| AppError::Parse(e.to_string()))?;
        if v["item"]["id"] != id {
            return Err(AppError::Parse("原Style身份不一致".into()));
        }
        sources.insert(
            id.into(),
            v["item"]["spec"]
                .as_str()
                .ok_or_else(|| AppError::Parse("原Style缺失".into()))?
                .into(),
        );
    }
    let view_prompt =
        std::fs::read_to_string(root.join(format!("prompts/atomic-view-v{version}.md")))?;
    let compare_prompt = std::fs::read_to_string(root.join("prompts/compare-candidates-v4.md"))?;
    let control_root = root.join("runs/p2-candidates-pilot-03");
    let controls: Value =
        serde_json::from_slice(&std::fs::read(control_root.join("controls.json"))?)
            .map_err(|e| AppError::Parse(e.to_string()))?;
    let old: Value = serde_json::from_slice(&std::fs::read(control_root.join("inputs.json"))?)
        .map_err(|e| AppError::Parse(e.to_string()))?;
    if controls["passed"] != true
        || controls["rows"].as_array().map(Vec::len) != Some(13)
        || old["prompt"] != compare_prompt
        || old["thinking"] != "enabled"
        || old["batch_size"] != 8
        || old["max_completion_tokens"] != 32768
    {
        return Err(AppError::Parse("比较控制或配置不匹配；未发送新请求".into()));
    }
    let store = Arc::new(ResearchStore::open(root, run, false)?);
    store.limit_run(3_000_000).await?;
    store.freeze(&json!({"suite":format!("atomic_view_pilot_v{version}"),"sources":sources,"selection":selection,"view_prompt":view_prompt,"compare_prompt":compare_prompt,
        "control_source":"p2-candidates-pilot-03","controls":controls,"view_model":"mimo-v2.6-pro","view_thinking":"disabled","compare_model":"mimo-v2.6-pro","compare_thinking":"enabled","batch_size":8,"max_compare_tokens":32768,"run_cap_micro_cny":3000000,"method_revision":if version==1{5}else{6}}))?;
    write_json(&store.root.join("controls.json"), &controls)?;
    std::fs::create_dir_all(store.root.join("views"))?;
    std::fs::create_dir_all(store.root.join("pairs"))?;
    let mut views = BTreeMap::new();
    let mut rows = Vec::<Value>::new();
    for (id, spec) in &sources {
        match validated(
            &store,
            &format!("view-{id}"),
            &view_prompt,
            &style_compare::indexed_input(spec),
            |raw| parse(raw, spec, version),
        )
        .await
        {
            Ok(view) => {
                write_json(
                    &store.root.join(format!("views/{id}.json")),
                    &json!({"id":id,"view":view,"spec":spec}),
                )?;
                rows.push(json!({"id":id,"status":"complete","features":view.features.len()}));
                views.insert(id.clone(), view);
            }
            Err(e) => {
                rows.push(json!({"id":id,"status":"failed","error":e.to_string()}));
                write_json(
                    &store.root.join("summary.json"),
                    &json!({"status":"partial","stage":"atomic_views","rows":rows,"usage":store.usage().await}),
                )?;
                return Err(e);
            }
        }
        write_json(
            &store.root.join("summary.json"),
            &json!({"status":"running","stage":"atomic_views","rows":rows,"usage":store.usage().await}),
        )?;
    }
    write_json(
        &store.root.join("view-summary.json"),
        &json!({"status":"complete","rows":rows}),
    )?;
    write_json(
        &store.root.join("summary.json"),
        &json!({"status":"running","stage":"atomic_pairs","expected":6,"rows":[],"views_complete":6,"usage":store.usage().await}),
    )?;
    let chains = Arc::new(tokio::sync::Semaphore::new(3));
    let failed = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut tasks = tokio::task::JoinSet::new();
    for (i, (left, right, kind)) in selection.into_iter().enumerate() {
        let store = Arc::clone(&store);
        let prompt = compare_prompt.clone();
        let chains = Arc::clone(&chains);
        let failed = Arc::clone(&failed);
        let a = views[left].clone();
        let b = views[right].clone();
        tasks.spawn(async move {
            let id = format!("atomic-{i:02}");
            let _permit = chains
                .acquire()
                .await
                .map_err(|_| AppError::Parse("原子小试调度关闭".into()))?;
            if failed.load(std::sync::atomic::Ordering::SeqCst) > 0 {
                return Ok(json!({"id":id,"status":"deferred"}));
            }
            match compare_configured(
                &store,
                &id,
                &prompt,
                Role::CandidateAnalysis,
                &a,
                &b,
                8,
                true,
            )
            .await
            {
                Ok(mut result) => {
                    result["left"] = json!(left);
                    result["right"] = json!(right);
                    result["kind"] = json!(kind);
                    write_json(&store.root.join(format!("pairs/{id}.json")), &result)?;
                    Ok::<_, AppError>(json!({"id":id,"kind":kind,"status":"complete"}))
                }
                Err(e) => {
                    failed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Ok(json!({"id":id,"status":"failed","error":e.to_string()}))
                }
            }
        });
    }
    finish_tasks(&store, &mut tasks, "atomic_pilot_review_required").await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ending_and_judgment_order_cannot_drift_into_rhythm() {
        for rule in ["结尾用短句收束。", "先给判断，再补解释。"] {
            let raw = json!({"features":[{"id":"f1","dimension":"rhythm","rule":rule,"condition":"未限定","line_start":1,"line_end":1}],"uncertainties":[]});
            assert!(parse(&raw.to_string(), rule, 1).is_ok());
            assert!(parse(&raw.to_string(), rule, 2).is_err());
            let mut fixed = raw;
            fixed["features"][0]["dimension"] = json!(if rule.contains("结尾") {
                "ending"
            } else {
                "information"
            });
            assert!(parse(&fixed.to_string(), rule, 2).is_ok());
        }
    }
}
