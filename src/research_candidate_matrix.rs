//! The full P2 matrix is gated by a completed and reviewed, version-matched pilot.
use crate::{
    error::AppError,
    eval_store::write_json,
    research_candidates::compare,
    research_compare::{finish_tasks, view_coverage},
    research_dataset::{sha256, valid_id},
    research_store::{ResearchStore, Role},
    style_compare::{self, View},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
const ROOT: &str = "research/style-aggregation";

#[derive(Clone, Serialize, Deserialize)]
struct Source {
    id: String,
    author: String,
    view: View,
}
#[derive(Clone, Serialize)]
struct Pair {
    id: String,
    kind: String,
    left: Source,
    right: Source,
}

fn read(path: impl AsRef<Path>) -> Result<Value, AppError> {
    serde_json::from_slice(&std::fs::read(path)?).map_err(|e| AppError::Parse(e.to_string()))
}

pub(crate) async fn run(run: &str, views: &str, pilot: &str) -> Result<(), AppError> {
    if !valid_id(views) || !valid_id(pilot) {
        return Err(AppError::Parse("来源run无效".into()));
    }
    let root = Path::new(ROOT);
    let prior = root.join("runs").join(pilot);
    let inputs = read(prior.join("inputs.json"))?;
    let review_path = prior.join("semantic-review.json");
    let review = read(&review_path)?;
    let summary = read(prior.join("summary.json"))?;
    let controls = read(prior.join("controls.json"))?;
    let prompt = std::fs::read_to_string(root.join("prompts/compare-candidates-v3.md"))?;
    if inputs["suite"] != "candidate_pilot_v3"
        || inputs["source"] != views
        || inputs["prompt"] != prompt
        || summary["status"] != "complete"
        || controls["passed"] != true
        || review["decision"] != "adopt_for_exploratory_matrix"
        || review["inputs_sha256"] != sha256(&std::fs::read(prior.join("inputs.json"))?)
    {
        return Err(AppError::Parse(
            "小试未完整通过并复核，或来源/参数不一致".into(),
        ));
    }
    let coverage = view_coverage(views)?;
    let mut sources = BTreeMap::new();
    for id in coverage["complete_ids"].as_array().unwrap() {
        let id = id.as_str().unwrap();
        let v = read(root.join(format!("runs/{views}/views/{id}.json")))?;
        let spec = v["item"]["spec"]
            .as_str()
            .ok_or_else(|| AppError::Parse("视图缺Spec".into()))?;
        sources.insert(
            id.to_owned(),
            Source {
                id: id.into(),
                author: v["item"]["author"].as_str().unwrap().into(),
                view: style_compare::parse_view(&v["view"].to_string(), spec)?,
            },
        );
    }
    let main: Vec<_> = sources
        .values()
        .filter(|s| !s.id.contains("-r"))
        .cloned()
        .collect();
    if main.len() != 20 {
        return Err(AppError::Parse("首次覆盖不足20".into()));
    }
    for s in &main {
        if serde_json::to_value(&s.view).map_err(|e| AppError::Parse(e.to_string()))?
            != inputs["views"][&s.id]
        {
            return Err(AppError::Parse("正式视图与已验证小试不一致".into()));
        }
    }
    let hashes = review["pair_sha256"]
        .as_object()
        .ok_or_else(|| AppError::Parse("缺少已复核样本哈希".into()))?;
    if hashes.len() != 6 {
        return Err(AppError::Parse("未复核全部6对".into()));
    }
    for (name, hash) in hashes {
        if !valid_id(name)
            || *hash != sha256(&std::fs::read(prior.join(format!("pairs/{name}.json")))?)
        {
            return Err(AppError::Parse("复核样本身份变化".into()));
        }
    }
    // Preserve the preregistered pilot orientation and reuse its six exact requests.
    let selected = inputs["selection"]
        .as_array()
        .ok_or_else(|| AppError::Parse("小试选样缺失".into()))?;
    let mut jobs = Vec::new();
    for i in 0..20 {
        for j in i + 1..20 {
            let pilot_pair = selected
                .iter()
                .enumerate()
                .find(|(_, p)| p[0] == main[i].id && p[1] == main[j].id);
            let (id, a, b) = if let Some((n, _)) = pilot_pair {
                (format!("pilot-{n:02}"), main[i].clone(), main[j].clone())
            } else if (i + j) % 2 == 0 {
                (
                    format!("pair-{i:02}-{j:02}"),
                    main[i].clone(),
                    main[j].clone(),
                )
            } else {
                (
                    format!("pair-{i:02}-{j:02}"),
                    main[j].clone(),
                    main[i].clone(),
                )
            };
            jobs.push(Pair {
                id,
                kind: "main".into(),
                left: a,
                right: b,
            });
        }
    }
    for (n, s) in sources.values().filter(|s| s.id.contains("-r")).enumerate() {
        let base = s.id.rsplit_once("-r").unwrap().0;
        jobs.push(Pair {
            id: format!("repeat-{n:02}"),
            kind: "repeat_noise".into(),
            left: sources[base].clone(),
            right: s.clone(),
        });
    }
    let swaps: Vec<_> = jobs
        .iter()
        .take(10)
        .enumerate()
        .map(|(n, p)| Pair {
            id: format!("swap-{n:02}"),
            kind: "order_check".into(),
            left: p.right.clone(),
            right: p.left.clone(),
        })
        .collect();
    jobs.extend(swaps);
    let store = Arc::new(ResearchStore::open(root, run, false)?);
    store.limit_run(35_000_000).await?;
    store.freeze(&json!({"suite":"candidate_matrix_v3","source":views,"pilot":pilot,"prompt":prompt,"jobs":jobs,"coverage":coverage,
        "review_sha256":sha256(&std::fs::read(review_path)?),"model":"mimo-v2.6-pro","thinking":"disabled","max_completion_tokens":16384,"batch_size":24,
        "run_cap_micro_cny":35000000,"metric":"candidate_maximum_overlap_v2","pilot_orientation_preserved":true}))?;
    store.import_completed(pilot).await?;
    write_json(&store.root.join("controls.json"), &controls)?;
    std::fs::create_dir_all(store.root.join("pairs"))?;
    let chains = Arc::new(tokio::sync::Semaphore::new(3));
    let failed = Arc::new(AtomicUsize::new(0));
    let mut tasks = tokio::task::JoinSet::new();
    for p in jobs {
        let store = Arc::clone(&store);
        let prompt = prompt.clone();
        let chains = Arc::clone(&chains);
        let failed = Arc::clone(&failed);
        tasks.spawn(async move {
            let _permit = chains
                .acquire()
                .await
                .map_err(|_| AppError::Parse("比较调度器关闭".into()))?;
            if failed.load(Ordering::SeqCst) >= 3 {
                return Ok(json!({"id":p.id,"kind":p.kind,"status":"deferred"}));
            }
            match compare(
                &store,
                &p.id,
                &prompt,
                Role::Structured,
                &p.left.view,
                &p.right.view,
            )
            .await
            {
                Ok(mut result) => {
                    result["left"] = json!(p.left.id);
                    result["right"] = json!(p.right.id);
                    result["left_author"] = json!(p.left.author);
                    result["right_author"] = json!(p.right.author);
                    result["within_author"] = json!(p.left.author == p.right.author);
                    result["kind"] = json!(p.kind);
                    write_json(&store.root.join(format!("pairs/{}.json", p.id)), &result)?;
                    Ok::<_, AppError>(json!({"id":p.id,"kind":p.kind,"status":"complete"}))
                }
                Err(e) => {
                    failed.fetch_add(1, Ordering::SeqCst);
                    Ok(json!({"id":p.id,"kind":p.kind,"status":"failed","error":e.to_string()}))
                }
            }
        });
    }
    finish_tasks(&store, &mut tasks, "candidate-matrix").await
}
