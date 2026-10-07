//! Small, separately budgeted Flash routing check. Never mixes its judgments into Pro runs.
use crate::{
    error::AppError,
    eval_store::write_json,
    research_compare::view_coverage,
    research_dataset::{sha256, valid_id},
    research_profile::ROOT,
    research_store::{ResearchStore, Role},
    research_validation::validated_as,
    style_compare::{self, Comparison, Feature, View},
};
use serde_json::{json, Value};
use std::{collections::BTreeMap, path::Path};

fn one(rule: &str) -> View {
    View {
        features: vec![Feature {
            id: "f1".into(),
            dimension: "voice".into(),
            rule: rule.into(),
            condition: "全文叙述".into(),
            quote: rule.into(),
        }],
        uncertainties: vec![],
    }
}

async fn compare(
    store: &ResearchStore,
    id: &str,
    prompt: &str,
    a: &View,
    b: &View,
) -> Result<Comparison, AppError> {
    validated_as(
        store,
        id,
        Role::FlashAnalysis,
        prompt,
        &json!({"A":a,"B":b}).to_string(),
        |s| style_compare::parse_comparison(s, a, b),
    )
    .await
}

fn mixed() -> (View, View) {
    let rules = [
        ("language", "使用日常词语"),
        ("language", "使用专业术语并紧接解释"),
        ("voice", "第一人称承认判断有限"),
        ("voice", "与读者平等商量"),
        ("rhythm", "长句之后以短句停顿"),
        ("rhythm", "并列句保持相近长度"),
        ("information", "先给具体事实再给判断"),
        ("information", "转折前先承认局部成立"),
        ("narration", "叙述后接一句简短评点"),
        ("narration", "议论前先交代可感知细节"),
        ("constraints", "只在事实允许时使用第一人称"),
        ("constraints", "少量使用比喻且不改变事实"),
    ];
    let a = View {
        features: rules
            .iter()
            .enumerate()
            .map(|(i, (d, r))| Feature {
                id: format!("a{:02}", i + 1),
                dimension: (*d).into(),
                rule: (*r).into(),
                condition: "适用时".into(),
                quote: (*r).into(),
            })
            .collect(),
        uncertainties: vec![],
    };
    let mut b = a.clone();
    for (i, f) in b.features.iter_mut().enumerate() {
        f.id = format!("b{:02}", 12 - i);
    }
    b.features.reverse();
    (a, b)
}

async fn controls(store: &ResearchStore, prompt: &str) -> Result<Vec<Value>, AppError> {
    let a = one("全文保持冷静克制的语气，避免夸张情绪。");
    let mut cases = vec![
        ("self", a.clone(), a.clone(), 1, 0),
        (
            "synonym",
            a.clone(),
            one("叙述始终平静节制，不用情绪夸饰。"),
            1,
            0,
        ),
        (
            "opposite",
            a.clone(),
            one("全文采用热烈夸张的情绪表达，避免冷静克制。"),
            0,
            1,
        ),
        ("generic", a, one("全文表达自然清楚。"), 0, 0),
    ];
    let (a, b) = mixed();
    cases.push(("mixed", a, b, 12, 0));
    let mut rows = Vec::new();
    for (id, a, b, m, c) in cases {
        match compare(store, &format!("control-{id}"), prompt, &a, &b).await {
            Ok(j) => {
                let exact = id != "mixed"
                    || j.matches.iter().all(|p| {
                        let left = a.features.iter().find(|f| f.id == p.left).unwrap();
                        let right = b.features.iter().find(|f| f.id == p.right).unwrap();
                        left.rule == right.rule
                    });
                rows.push(json!({"name":id,"passed":exact&&j.matches.len()==m&&j.conflicts.len()==c,"comparison":j}));
            }
            Err(e) => rows.push(json!({"name":id,"passed":false,"error":e.to_string()})),
        }
        write_json(
            &store.root.join("controls.json"),
            &json!({"passed":rows.len()==5&&rows.iter().all(|r|r["passed"]==true),"rows":rows}),
        )?;
    }
    Ok(rows)
}

pub(crate) async fn run(run: &str, views: &str) -> Result<(), AppError> {
    if !valid_id(views) {
        return Err(AppError::Parse("视图来源无效".into()));
    }
    let coverage = view_coverage(views)?;
    let source = Path::new(ROOT).join("runs").join(views);
    let selection = [
        ("wxb-01", "wxb-02"),
        ("wxb-03", "wxb-04"),
        ("wzq-01", "wzq-02"),
        ("wzq-03", "wzq-04"),
        ("wxb-01", "wzq-01"),
        ("wxb-02", "wzq-02"),
    ];
    let mut all = BTreeMap::<String, View>::new();
    for id in coverage["complete_ids"].as_array().unwrap() {
        let id = id.as_str().unwrap();
        let raw: Value =
            serde_json::from_slice(&std::fs::read(source.join(format!("views/{id}.json")))?)
                .map_err(|e| AppError::Parse(e.to_string()))?;
        if raw["item"]["replicate"] != 1 {
            continue;
        }
        let spec = raw["item"]["spec"]
            .as_str()
            .ok_or_else(|| AppError::Parse("视图缺少Spec".into()))?;
        all.insert(
            id.into(),
            style_compare::parse_view(&raw["view"].to_string(), spec)?,
        );
    }
    if all.len() != 20 {
        return Err(AppError::Parse("路由试验需要20份首次来源".into()));
    }
    let ids: Vec<_> = all.keys().cloned().collect();
    let prompt = std::fs::read_to_string(format!("{ROOT}/prompts/compare-pair-v1.md"))?;
    let store = ResearchStore::open(Path::new(ROOT), run, false)?;
    store.limit_run(1_000_000).await?;
    store.freeze(&json!({"suite":"flash_routing_feasibility_v1","source":views,"views":all,"selection":selection,
        "prompt":prompt,"model":Role::FlashAnalysis.model(),"thinking":"disabled","max_completion_tokens":16384,"run_cap_micro_cny":1000000,
        "authorization":"user allowed appropriate Flash routing on 2026-10-04","scope":"engineering feasibility, not a model-quality ranking"}))?;
    let controls = controls(&store, &prompt).await?;
    if controls.iter().any(|c| c["passed"] != true) {
        write_json(
            &store.root.join("summary.json"),
            &json!({"status":"evaluation_invalid","rows":[],"controls":controls,"error":"Flash控制未全部通过，不进行真实样本匹配","usage":store.usage().await}),
        )?;
        return Err(AppError::Parse("Flash路由控制失败".into()));
    }
    std::fs::create_dir_all(store.root.join("pairs"))?;
    let mut rows = Vec::new();
    for (left, right) in selection {
        let i = ids.iter().position(|v| v == left).unwrap();
        let j = ids.iter().position(|v| v == right).unwrap();
        let id = format!("pair-{i:02}-{j:02}");
        let (left, right) = if (i + j) % 2 == 0 {
            (left, right)
        } else {
            (right, left)
        };
        let a = &all[left];
        let b = &all[right];
        match compare(&store, &id, &prompt, a, b).await {
            Ok(c) => {
                write_json(
                    &store.root.join(format!("pairs/{id}.json")),
                    &json!({"id":id,"left":left,"right":right,"comparison":c,"metrics":style_compare::metrics(a,b,&c),"views_sha256":sha256(&serde_json::to_vec(&(a,b)).map_err(|e|AppError::Parse(e.to_string()))?)}),
                )?;
                rows.push(json!({"id":id,"left":left,"right":right,"status":"complete"}));
            }
            Err(e) => rows.push(
                json!({"id":id,"left":left,"right":right,"status":"failed","error":e.to_string()}),
            ),
        }
        write_json(
            &store.root.join("summary.json"),
            &json!({"status":"running","rows":rows,"controls":controls,"usage":store.usage().await}),
        )?;
    }
    let complete = rows.iter().all(|r| r["status"] == "complete");
    write_json(
        &store.root.join("summary.json"),
        &json!({"status":if complete{"complete"}else{"partial"},"rows":rows,"controls":controls,"decision":"requires evidence review before any full-route adoption","usage":store.usage().await}),
    )?;
    if complete {
        Ok(())
    } else {
        Err(AppError::Parse(
            "Flash真实样本有未通过项；保留结果，不自动采用".into(),
        ))
    }
}
