//! Isolated, capped validation of the candidate comparator before any matrix adoption.
use crate::{
    error::AppError,
    eval_store::write_json,
    research_compare::view_coverage,
    research_dataset::valid_id,
    research_store::{ResearchStore, Role},
    research_validation::validated_as,
    style_candidates::{self as sc, Judgment, Label},
    style_compare::{self, Feature, View},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};
const ROOT: &str = "research/style-aggregation";

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

fn controls(revision: u8) -> Vec<(String, View, View)> {
    let a = one("全文保持冷静克制的语气，避免夸张情绪。");
    let mut cases = vec![
        ("self".into(), a.clone(), a.clone()),
        (
            "synonym".into(),
            a.clone(),
            one("叙述始终平静节制，不用情绪夸饰。"),
        ),
        (
            "opposite".into(),
            a.clone(),
            one("全文采用热烈夸张的情绪表达，避免冷静克制。"),
        ),
        ("generic".into(), a, one("全文表达自然清楚。")),
    ];
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
    cases.push(("mixed".into(), a, b));
    if revision >= 3 {
        for (id, a, b) in [
            (
                "compatible",
                "叙述中允许偶尔使用比喻",
                "叙述中多用可感知细节",
            ),
            ("partial", "以短句为主，关键处补充解释", "以短句为主"),
            (
                "opposite-ending",
                "结尾必须提出明确行动建议",
                "结尾禁止提出行动建议",
            ),
            (
                "synonym-facts",
                "讲述自身经历时不编造亲历",
                "叙述个人经历时不得虚构自己的见闻",
            ),
        ] {
            cases.push((id.into(), one(a), one(b)));
        }
        let mut a = one("以疑问开头");
        a.features[0].condition = "写争议话题时".into();
        let mut b = a.clone();
        b.features[0].condition = "写个人回忆时".into();
        cases.push(("different-condition".into(), a, b));
    }
    cases
}

pub(crate) async fn compare(
    store: &ResearchStore,
    id: &str,
    prompt: &str,
    role: Role,
    a: &View,
    b: &View,
) -> Result<Value, AppError> {
    compare_configured(store, id, prompt, role, a, b, 24, false).await
}

fn candidate_input(chunk: &[sc::Candidate], rule_only: bool) -> String {
    if rule_only {
        let feature = |f: &Feature| json!({"id":f.id,"dimension":f.dimension,"rule":f.rule,"condition":f.condition});
        json!({"candidates":chunk.iter().map(|c|json!({"candidate_id":c.candidate_id,"left":feature(&c.left),"right":feature(&c.right)})).collect::<Vec<_>>()}).to_string()
    } else {
        json!({"candidates":chunk}).to_string()
    }
}

pub(crate) async fn compare_configured(
    store: &ResearchStore,
    id: &str,
    prompt: &str,
    role: Role,
    a: &View,
    b: &View,
    batch_size: usize,
    rule_only: bool,
) -> Result<Value, AppError> {
    let candidates = sc::candidates(a, b);
    let mut judgments = Vec::<Judgment>::new();
    for (batch, chunk) in candidates.chunks(batch_size).enumerate() {
        let input = candidate_input(chunk, rule_only);
        let mut result = validated_as(
            store,
            &format!("{id}-batch-{batch:02}"),
            role,
            prompt,
            &input,
            |raw| sc::parse(raw, chunk),
        )
        .await?;
        judgments.append(&mut result);
    }
    let metrics = sc::summarize(a, b, &candidates, &judgments)?;
    Ok(json!({"id":id,"candidates":candidates,"judgments":judgments,"metrics":metrics}))
}

fn control_passed(id: &str, result: &Value) -> Result<bool, AppError> {
    let js: Vec<Judgment> = serde_json::from_value(result["judgments"].clone())
        .map_err(|e| AppError::Parse(e.to_string()))?;
    if id == "mixed" {
        let mut equivalent = 0;
        for c in result["candidates"].as_array().unwrap() {
            // Responses may reorder judgments; resolve by the stable candidate ID.
            let actual = js
                .iter()
                .find(|x| x.candidate_id == c["candidate_id"].as_str().unwrap())
                .unwrap();
            let same = c["left"]["rule"] == c["right"]["rule"];
            if same {
                equivalent += 1;
            }
            if (same && actual.label != Label::Equivalent)
                || (!same && actual.label != Label::Distinct)
            {
                return Ok(false);
            }
        }
        Ok(equivalent == 12)
    } else {
        let expected = match id {
            "self" | "synonym" | "synonym-facts" => Label::Equivalent,
            "opposite" | "opposite-ending" => Label::Opposite,
            _ => Label::Distinct,
        };
        Ok(js.len() == 1 && js[0].label == expected)
    }
}

pub(crate) async fn run(run: &str, views: &str, revision: u8) -> Result<(), AppError> {
    if ![2, 3, 4].contains(&revision) {
        return Err(AppError::Parse("候选方法版本无效".into()));
    }
    if !valid_id(views) {
        return Err(AppError::Parse("视图来源无效".into()));
    }
    let coverage = view_coverage(views)?;
    let mut all = BTreeMap::<String, View>::new();
    for id in coverage["complete_ids"].as_array().unwrap() {
        let id = id.as_str().unwrap();
        let raw: Value = serde_json::from_slice(&std::fs::read(
            Path::new(ROOT).join(format!("runs/{views}/views/{id}.json")),
        )?)
        .map_err(|e| AppError::Parse(e.to_string()))?;
        if raw["item"]["replicate"] != 1 {
            continue;
        }
        let spec = raw["item"]["spec"]
            .as_str()
            .ok_or_else(|| AppError::Parse("缺少Spec".into()))?;
        all.insert(
            id.into(),
            style_compare::parse_view(&raw["view"].to_string(), spec)?,
        );
    }
    if all.len() != 20 {
        return Err(AppError::Parse("首次视图覆盖不足".into()));
    }
    let selection = [
        ("wxb-01", "wxb-02"),
        ("wxb-03", "wxb-04"),
        ("wzq-01", "wzq-02"),
        ("wzq-03", "wzq-04"),
        ("wxb-01", "wzq-01"),
        ("wxb-02", "wzq-02"),
    ];
    let prompt =
        std::fs::read_to_string(format!("{ROOT}/prompts/compare-candidates-v{revision}.md"))?;
    let mut controls = controls(revision);
    if revision == 4 {
        let fixture: Value = serde_json::from_slice(&std::fs::read(format!(
            "{ROOT}/datasets/candidate-diagnostic-v1.json"
        ))?)
        .map_err(|e| AppError::Parse(e.to_string()))?;
        for c in fixture["cases"].as_array().unwrap().iter().take(3) {
            let parse = |k: &str| {
                serde_json::from_value::<Feature>(c[k].clone())
                    .map_err(|e| AppError::Parse(e.to_string()))
            };
            controls.push((
                c["id"].as_str().unwrap().into(),
                View {
                    features: vec![parse("left")?],
                    uncertainties: vec![],
                },
                View {
                    features: vec![parse("right")?],
                    uncertainties: vec![],
                },
            ));
        }
    }
    let role = if revision == 2 || revision == 4 {
        Role::CandidateAnalysis
    } else {
        Role::Structured
    };
    let batch_size = if revision == 4 { 8 } else { 24 };
    let store = std::sync::Arc::new(ResearchStore::open(Path::new(ROOT), run, false)?);
    store.limit_run(3_000_000).await?;
    store.freeze(&json!({"suite":format!("candidate_pilot_v{revision}"),"source":views,"views":all,"selection":selection,"controls":controls,"prompt":prompt,
        "model":"mimo-v2.6-pro","thinking":if revision!=3{"enabled"}else{"disabled"},"max_completion_tokens":if revision!=3{32768}else{16384},"batch_size":batch_size,"run_cap_micro_cny":3000000,"method_revision":revision}))?;
    let mut control_rows = Vec::new();
    let mut rows = Vec::<Value>::new();
    for (id, a, b) in &controls {
        let result = compare_configured(
            &store,
            &format!("control-{id}"),
            &prompt,
            role,
            a,
            b,
            batch_size,
            revision == 4,
        )
        .await;
        let row = match result {
            Ok(result) => {
                let passed = control_passed(id, &result)?;
                json!({"id":id,"passed":passed,"result":result})
            }
            Err(e) => json!({"id":id,"passed":false,"error":e.to_string()}),
        };
        let passed = row["passed"] == true;
        control_rows.push(row);
        write_json(
            &store.root.join("controls.json"),
            &json!({"passed":control_rows.len()==controls.len() && control_rows.iter().all(|r|r["passed"]==true),"rows":control_rows}),
        )?;
        write_json(
            &store.root.join("summary.json"),
            &json!({"status":if passed{"running"}else{"evaluation_invalid"},"rows":rows,"controls":control_rows,"usage":store.usage().await}),
        )?;
        println!("candidate control {id}: {passed}");
        if !passed {
            return Err(AppError::Parse(format!("候选控制{id}失败，停止真实样本")));
        }
    }
    std::fs::create_dir_all(store.root.join("pairs"))?;
    if revision == 4 {
        let mut tasks = tokio::task::JoinSet::new();
        let chains = std::sync::Arc::new(tokio::sync::Semaphore::new(3));
        let failed = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        for (i, (left, right)) in selection.iter().enumerate() {
            let left = left.to_string();
            let right = right.to_string();
            let a = all[&left].clone();
            let b = all[&right].clone();
            let store = std::sync::Arc::clone(&store);
            let prompt = prompt.clone();
            let chains = std::sync::Arc::clone(&chains);
            let failed = std::sync::Arc::clone(&failed);
            tasks.spawn(async move {
                let id = format!("pilot-{i:02}");
                let _permit = chains
                    .acquire()
                    .await
                    .map_err(|_| AppError::Parse("小试调度器关闭".into()))?;
                if failed.load(std::sync::atomic::Ordering::SeqCst) > 0 {
                    return Ok(json!({"id":id,"status":"deferred"}));
                }
                match compare_configured(&store, &id, &prompt, role, &a, &b, 8, true).await {
                    Ok(mut result) => {
                        result["left"] = json!(left);
                        result["right"] = json!(right);
                        write_json(&store.root.join(format!("pairs/{id}.json")), &result)?;
                        Ok::<_, AppError>(
                            json!({"id":id,"left":left,"right":right,"status":"complete"}),
                        )
                    }
                    Err(e) => {
                        failed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        Ok(json!({"id":id,"status":"failed","error":e.to_string()}))
                    }
                }
            });
        }
        return crate::research_compare::finish_tasks(
            &store,
            &mut tasks,
            "candidate-pilot-review-required",
        )
        .await;
    }
    for (i, (left, right)) in selection.iter().enumerate() {
        let id = format!("pilot-{i:02}");
        match compare(&store, &id, &prompt, role, &all[*left], &all[*right]).await {
            Ok(mut result) => {
                result["left"] = json!(left);
                result["right"] = json!(right);
                write_json(&store.root.join(format!("pairs/{id}.json")), &result)?;
                rows.push(json!({"id":id,"left":left,"right":right,"status":"complete"}));
            }
            Err(e) => {
                rows.push(json!({"id":id,"status":"failed","error":e.to_string()}));
                write_json(
                    &store.root.join("summary.json"),
                    &json!({"status":"partial","rows":rows,"controls":control_rows,"usage":store.usage().await}),
                )?;
                return Err(e);
            }
        }
        write_json(
            &store.root.join("summary.json"),
            &json!({"status":"running","rows":rows,"controls":control_rows,"usage":store.usage().await}),
        )?;
        println!("candidate pair {id}: complete");
    }
    write_json(
        &store.root.join("summary.json"),
        &json!({"status":"complete","rows":rows,"controls":control_rows,"decision":"requires_semantic_review_not_matrix_adoption","usage":store.usage().await}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rule_only_request_keeps_conditions_and_excludes_broader_evidence() {
        let mut a = one("短句结尾");
        a.features[0].quote = "UNRELATED BROADER EVIDENCE".into();
        let candidates = sc::candidates(&a, &a);
        let input = candidate_input(&candidates, true);
        assert!(!input.contains("UNRELATED"));
        assert!(!input.contains("quote"));
        let parsed: Value = serde_json::from_str(&input).unwrap();
        assert_eq!(parsed["candidates"][0]["left"]["condition"], "全文叙述");
        assert_eq!(candidates[0].left.quote, "UNRELATED BROADER EVIDENCE");
        assert!(candidate_input(&candidates, false).contains("UNRELATED"));
    }
}
