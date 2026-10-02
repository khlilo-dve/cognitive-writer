use crate::{
    error::AppError,
    eval_store::write_json,
    research_compare::{compare, finish_tasks},
    research_dataset::{sha256, valid_id},
    research_profile::{self, Artifact, ROOT},
    research_store::ResearchStore,
    style_compare,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::BTreeMap, path::Path, sync::Arc};

#[derive(Clone, Serialize, Deserialize)]
struct Pair {
    id: String,
    left: String,
    right: String,
    references: Vec<Value>,
}

fn pairs(artifacts: &[Artifact]) -> Result<Vec<Pair>, AppError> {
    let mut index = BTreeMap::new();
    let mut pairs = BTreeMap::<(String, String), Pair>::new();
    for a in artifacts.iter().filter(|a| a.plan.mode == "primary") {
        for r in &a.plan.references {
            let seed = r["seed"]
                .as_u64()
                .ok_or_else(|| AppError::Parse("缺少seed".into()))?;
            let k = r["k"]
                .as_u64()
                .ok_or_else(|| AppError::Parse("缺少k".into()))?;
            if index
                .insert((a.plan.author.clone(), seed, k), a.plan.id.clone())
                .is_some()
            {
                return Err(AppError::Parse("重复子集映射".into()));
            }
        }
    }
    let mut authors: Vec<_> = artifacts.iter().map(|a| a.plan.author.clone()).collect();
    authors.sort();
    authors.dedup();
    if authors.len() != 2 {
        return Err(AppError::Parse("收敛要求两位作者".into()));
    }
    let mut add = |left: String, right: String, reference: Value| {
        let key = if left < right {
            (left.clone(), right.clone())
        } else {
            (right.clone(), left.clone())
        };
        let id = format!(
            "converge-{}",
            &sha256(format!("{}|{}", key.0, key.1).as_bytes())[..16]
        );
        pairs
            .entry(key)
            .or_insert(Pair {
                id,
                left,
                right,
                references: vec![],
            })
            .references
            .push(reference);
    };
    let get = |author: &str, seed: u64, k: u64| {
        index
            .get(&(author.to_owned(), seed, k))
            .cloned()
            .ok_or_else(|| AppError::Parse("冻结子集覆盖不完整".into()))
    };
    let sizes = [1, 2, 3, 5, 7, 10];
    for author in &authors {
        for seed in [17, 29, 43, 71, 101] {
            for step in sizes.windows(2) {
                add(
                    get(author, seed, step[0])?,
                    get(author, seed, step[1])?,
                    json!({"kind":"adjacent","author":author,"seed":seed,"from_k":step[0],"to_k":step[1]}),
                );
            }
            for k in [1, 2, 3, 5, 7] {
                add(
                    get(author, seed, k)?,
                    get(author, seed, 10)?,
                    json!({"kind":"full_reference","author":author,"seed":seed,"from_k":k,"to_k":10}),
                );
            }
        }
        for a in artifacts
            .iter()
            .filter(|a| a.plan.author == *author && a.plan.mode != "primary")
        {
            add(
                get(author, 17, 10)?,
                a.plan.id.clone(),
                json!({"kind":"aggregation_noise","author":author,"mode":a.plan.mode,"k":10}),
            );
        }
    }
    for seed in [17, 29, 43, 71, 101] {
        for k in sizes {
            add(
                get(&authors[0], seed, k)?,
                get(&authors[1], seed, k)?,
                json!({"kind":"between_authors","seed":seed,"k":k}),
            );
        }
    }
    Ok(pairs.into_values().collect())
}

pub(crate) async fn run(run: &str, profiles: &str, p2_run: &str) -> Result<(), AppError> {
    if !valid_id(p2_run) {
        return Err(AppError::Parse("P2 run无效".into()));
    }
    let root = Path::new(ROOT).join("runs").join(p2_run);
    let controls: Value = serde_json::from_slice(&std::fs::read(root.join("controls.json"))?)
        .map_err(|e| AppError::Parse(e.to_string()))?;
    let summary: Value = serde_json::from_slice(&std::fs::read(root.join("summary.json"))?)
        .map_err(|e| AppError::Parse(e.to_string()))?;
    let p2_inputs: Value = serde_json::from_slice(&std::fs::read(root.join("inputs.json"))?)
        .map_err(|e| AppError::Parse(e.to_string()))?;
    let prompt = std::fs::read_to_string(format!("{ROOT}/prompts/compare-pair-v1.md"))?;
    if controls["passed"] != true
        || summary["status"] != "complete"
        || p2_inputs["prompt"] != prompt
    {
        return Err(AppError::Parse(
            "P2比较未完成、控制未过或比较Prompt改变".into(),
        ));
    }
    let artifacts = research_profile::load(profiles)?;
    let plan = pairs(&artifacts)?;
    let by_id: BTreeMap<_, _> = artifacts.iter().map(|a| (a.plan.id.clone(), a)).collect();
    let store = Arc::new(ResearchStore::open(Path::new(ROOT), run, false)?);
    store.freeze(&json!({"suite":"profile_convergence_v2_structured","analysis_thinking":"disabled","method_revision":1,"profile_run":profiles,"artifacts":artifacts,"pairs":plan,"prompt":prompt,"p2_control_run":p2_run,"controls":controls}))?;
    std::fs::create_dir_all(store.root.join("pairs"))?;
    write_json(&store.root.join("plans.json"), &plan)?;
    let mut tasks = tokio::task::JoinSet::new();
    for pair in plan {
        let a = by_id[&pair.left].view.clone();
        let b = by_id[&pair.right].view.clone();
        let store = Arc::clone(&store);
        let prompt = prompt.clone();
        tasks.spawn(async move {
            // Empty profiles represent missing evidence; do not ask a judge to invent matches.
            let result=if a.features.is_empty()||b.features.is_empty(){Ok(style_compare::Comparison{matches:vec![],conflicts:vec![],uncertain_left:vec![],uncertain_right:vec![],notes:vec!["至少一侧Profile为空，程序保留覆盖状态".into()]})}
                else {compare(&store,&pair.id,&prompt,&a,&b).await};
            match result {
                Ok(c)=>{write_json(&store.root.join(format!("pairs/{}.json",pair.id)),&json!({"pair":pair,"comparison":c,"metrics":style_compare::metrics(&a,&b,&c)}))?;
                    Ok::<_,AppError>(json!({"id":pair.id,"status":"complete"}))},
                Err(e)=>Ok(json!({"id":pair.id,"status":"failed","error":e.to_string()})),
            }
        });
    }
    finish_tasks(&store, &mut tasks, "convergence").await
}
