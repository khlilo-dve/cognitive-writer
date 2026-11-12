//! Bounded P7 real study: two current ten-article jobs, grounded fact controls and two Writer tasks.
use crate::{
    error::AppError,
    eval_store::write_json,
    learning_chat::ChatRef,
    learning_chat_store as chat,
    learning_flow::{Call, WriterInput},
    learning_job::{self, JobPrompts},
    learning_transport::{self, LearningClient, LearningResponse},
    learning_workspace::LearningWorkspace,
    learning_workspace_store as io,
    research_receipt::{Checkpoint, verify},
    research_store::{ResearchStore, Role},
    voice_learning::sha256,
    writer_facts,
};
use serde_json::{Value, json};
use std::{path::Path, sync::Arc};
const ROOT: &str = "research/style-aggregation";
fn error(s: &str) -> AppError {
    AppError::LearningWorkspace(s.into())
}
async fn call(store: Arc<ResearchStore>, c: Call) -> Result<String, AppError> {
    let expected = crate::learning_flow_writer::request(&c)?;
    let name = format!(
        "p7-{}-{}",
        &sha256(c.root.to_string_lossy().as_bytes())[..12],
        c.stage
    );
    learning_transport::call_with(
        &c.root,
        &c.stage,
        expected.clone(),
        false,
        || async { Ok(()) },
        || async {
            let identity = format!(
                "{}/{}",
                store
                    .root
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or(""),
                name
            );
            if !store.root.join(format!("requests/{name}.json")).exists()
                && store.usage().await["entries"]
                    .as_object()
                    .ok_or_else(|| error("费用账本无效"))?
                    .keys()
                    .any(|k| k == &identity || k.starts_with(&format!("{identity}/")))
            {
                return Err(error("该请求已有未知费用，未隐式重发"));
            }
            let role = if c.profile.model == "mimo-v2.6-pro" {
                Role::Structured
            } else {
                Role::FlashAnalysis
            };
            store
                .call(
                    &name,
                    role,
                    &c.system,
                    &c.user,
                    expected["body"].get("response_format").is_some(),
                )
                .await?;
            let cp: Checkpoint = io::read(&store.root.join(format!("requests/{name}.json")))?;
            verify(&cp)?;
            if cp.request != expected {
                return Err(error("P7研究与产品请求身份不符"));
            }
            Ok(LearningResponse {
                content: cp.receipt.content,
                model: cp.receipt.model,
                finish_reason: cp.receipt.finish_reason,
                usage: cp.receipt.usage,
            })
        },
    )
    .await
}
async fn learn(store: Arc<ResearchStore>, input: Value, code: &str) -> Result<Value, AppError> {
    let phase: Value = if store.root.join("phase-budget.json").exists() {
        io::read(&store.root.join("phase-budget.json"))?
    } else {
        json!({})
    };
    let reused = phase["reuse_learning_run"].as_str();
    let learning_root = if let Some(id) = reused {
        if !crate::research_dataset::valid_id(id)
            || id
                == store
                    .root
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
            || !phase["prior_runs"]
                .as_array()
                .is_some_and(|v| v.iter().any(|v| v == id))
        {
            return Err(error("复用学习来源必须是扣费前序run"));
        }
        Path::new(ROOT).join("runs").join(id)
    } else {
        store.root.clone()
    };
    let ws = LearningWorkspace::open(&learning_root.join("workspaces").join(code))?;
    let texts: Vec<String> =
        serde_json::from_value(input["texts"].clone()).map_err(|e| error(&e.to_string()))?;
    let link = learning_root.join(format!("links/{code}.json"));
    let job = if link.exists() {
        let v: Value = io::read(&link)?;
        learning_job::load(
            &ws,
            v["job_id"].as_str().ok_or_else(|| error("任务链接无效"))?,
        )?
    } else {
        let profile = LearningClient::from_env()?.profile();
        let job = learning_job::prepare(
            &ws,
            texts.clone(),
            format!("P7当前方法10篇真实质量小试：{code}，候选仍待语义审阅"),
            None,
            profile,
            JobPrompts::load()?,
        )?;
        io::publish_json(&link, &json!({"job_id":job.id}))?;
        job
    };
    let root = learning_job::directory(&ws, &job.id)?;
    let candidate = if reused.is_some() {
        let expected = crate::voice_learning::sources(&texts)?
            .into_iter()
            .map(|s| s.text)
            .collect::<Vec<_>>();
        let state = learning_job::state(&ws, &job.id)?;
        if state.status != "complete"
            || job.articles != expected
            || job.profile != LearningClient::from_env()?.profile()
            || job.learning_method != crate::voice_learning::METHOD
            || job.revision_method != crate::voice_revision::METHOD
            || serde_json::to_value(&job.prompts).ok()
                != serde_json::to_value(JobPrompts::load()?).ok()
        {
            return Err(error("复用任务未完成或来源/方法/配置已变化"));
        }
        ws.preview(
            state
                .candidate_id
                .as_deref()
                .ok_or_else(|| error("复用候选缺失"))?,
        )?
    } else {
        learning_job::run_with(&ws, &job.id, |stage, system, user| {
            call(
                Arc::clone(&store),
                Call {
                    root: root.clone(),
                    profile: job.profile.clone(),
                    stage,
                    system,
                    user,
                },
            )
        })
        .await?
    };
    std::fs::create_dir_all(store.root.join("styles"))?;
    write_json(
        &store.root.join(format!("styles/{code}.json")),
        &json!({"job_id":job.id,"candidate_id":candidate.id,"source_ids":input["source_ids"],"reused_from":reused,"articles":job.articles,"draft":candidate.input.draft,"review":candidate.input.review}),
    )?;
    crate::io::write_file_atomic(
        &store.root.join(format!("styles/{code}.md")),
        &candidate.input.draft.spec,
    )?;
    Ok(
        json!({"id":format!("{code}-k10"),"status":"complete","job_id":job.id,"candidate_id":candidate.id,"article_count":candidate.input.articles.len()}),
    )
}
fn resume_root(store: &ResearchStore) -> Result<std::path::PathBuf, AppError> {
    let path = store.root.join("phase-budget.json");
    if !path.exists() {
        return Ok(store.root.clone());
    }
    let phase: Value = io::read(&path)?;
    let Some(id) = phase["resume_writer_run"].as_str() else {
        return Ok(store.root.clone());
    };
    if !crate::research_dataset::valid_id(id)
        || id
            == store
                .root
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
        || !phase["prior_runs"]
            .as_array()
            .is_some_and(|a| a.iter().any(|v| v == id))
    {
        return Err(error("恢复Writer来源必须是扣费前序run"));
    }
    let root = Path::new(ROOT).join("runs").join(id);
    let prior: Value = io::read(&root.join("inputs.json"))?;
    if prior["fact_prompts"]
        != serde_json::to_value(writer_facts::Prompts::load()?)
            .map_err(|e| error(&e.to_string()))?
    {
        return Err(error("恢复Writer事实提示已变化"));
    }
    Ok(root)
}
async fn writer(
    store: Arc<ResearchStore>,
    code: &str,
    style: &str,
    request: &str,
) -> Result<Value, AppError> {
    let resume_root = resume_root(&store)?;
    let link = resume_root.join(format!("writer-links/{code}.json"));
    let r: ChatRef = if link.exists() {
        io::read(&link)?
    } else {
        if resume_root != store.root {
            return Err(error("待恢复Writer链接缺失"));
        }
        let mut profile = LearningClient::from_env()?.profile();
        profile.model = "mimo-v2.6-flash".into();
        let r = chat::create_with(
            &store.root.join("writers").join(code),
            profile,
            "P7固定写作，不做意图解释".into(),
            "P7固定范围，不做材料解释".into(),
        )?;
        io::publish_json(&link, &r)?;
        r
    };
    let input = WriterInput {
        method: Some(writer_facts::METHOD.into()),
        label: code.into(),
        spec: style.into(),
        request: request.into(),
    };
    let result =
        crate::learning_flow_writer::run_with(&r, &input, &mut |c| call(Arc::clone(&store), c))
            .await?;
    let root = crate::learning_flow_writer::directory(&r, &input)?;
    std::fs::create_dir_all(store.root.join("articles"))?;
    write_json(
        &store.root.join(format!("articles/{code}.json")),
        &json!({"input":input,"outline":result.outline,"body":result.fulltext,"writer_root":root,"fact_checked":result.fact_checked}),
    )?;
    crate::io::write_file_atomic(
        &store.root.join(format!("articles/{code}.md")),
        &result.fulltext,
    )?;
    Ok(json!({"id":code,"status":"complete","writer_root":root,"fact_checked":result.fact_checked}))
}
pub(crate) async fn run(run: &str) -> Result<(), AppError> {
    let store = Arc::new(ResearchStore::open(Path::new(ROOT), run, false)?);
    let phase: Value = if store.root.join("phase-budget.json").exists() {
        io::read(&store.root.join("phase-budget.json"))?
    } else {
        json!({"prior_runs":[]})
    };
    let usage = store.usage().await;
    let mut prior_used = 0u64;
    let mut seen = std::collections::HashSet::new();
    for prior in phase["prior_runs"]
        .as_array()
        .ok_or_else(|| error("阶段预算来源无效"))?
    {
        let id = prior
            .as_str()
            .filter(|s| crate::research_dataset::valid_id(s) && *s != run)
            .ok_or_else(|| error("旧run身份无效"))?;
        if !seen.insert(id) {
            return Err(error("旧run重复"));
        }
        for e in usage["entries"]
            .as_object()
            .ok_or_else(|| error("账本无效"))?
            .values()
            .filter(|e| e["run_id"] == id)
        {
            prior_used = prior_used.saturating_add(
                e["actual_micro"]
                    .as_u64()
                    .or(e["reserved_micro"].as_u64())
                    .unwrap_or(u64::MAX),
            );
        }
    }
    let cap = 5_000_000u64.saturating_sub(prior_used);
    store.limit_run(cap).await?;
    let bytes = std::fs::read(format!("{ROOT}/runs/voice-learning-core-01/inputs.json"))?;
    let source: Value = serde_json::from_slice(&bytes).map_err(|e| error(&e.to_string()))?;
    let mut groups = vec![];
    for code in ["wxb", "wzq"] {
        let group = source["jobs"]
            .as_array()
            .and_then(|a| a.iter().find(|j| j["id"] == format!("{code}-k10")))
            .ok_or_else(|| error("缺冻结十篇来源"))?;
        groups.push(group.clone());
    }
    let prior: Value = io::read(&Path::new(ROOT).join("runs/learning-flow-real-01/writer.json"))?;
    let positive = json!({"request":"写一段图书馆夜读建议。已知图书馆十点闭馆，我希望留半小时收拾与回宿舍。可以提出建议和按时间算术推导个人安排，不补设施或经历。","draft":"图书馆十点闭馆。我希望留半小时收拾与回宿舍，所以把九点半作为自己停读的时间。这是我的安排。你也可以给收尾留出时间，不必把读到最后一刻当作目标。"});
    let tasks = json!({"wxb":"给大学生写一篇旧书交换活动说明。只用这些事实：活动周六14点到16点；每人最多带3本旧书；交换由双方当面自愿决定；不收钱。可以明确提出建议和条件推理，不编造亲历或未提供的安排。约600字，材料不足时按需要写。",
        "wzq":"写一篇整理阳台便签的短随笔。只用这些事实：我整理了12张旧便签，留下4张，把8张收进盒子；我有点舍不得，也觉得清爽些。保留这些已给感受，可以写明确的感想，不补现场、他人或过去经历，不把未给原因写成事实。约600字，按材料需要展开。"});
    let negative_ws =
        LearningWorkspace::open(&Path::new(ROOT).join("runs/learning-flow-real-01/workspace"))?;
    let original_version = negative_ws.version("P5夜读", 1)?;
    if serde_json::to_value(original_version.reference()).ok()
        != Some(prior["selected_version"].clone())
    {
        return Err(error("旧负例来源版本身份不符"));
    }
    let negative_spec = original_version.candidate.input.draft.spec;
    let prompts = writer_facts::Prompts::load()?;
    let frozen = json!({"suite":"p7-bounded-quality-v1","groups":groups,"source_sha256":sha256(&bytes),"learning_method":crate::voice_learning::METHOD,"revision_method":crate::voice_revision::METHOD,"learning_prompts":JobPrompts::load()?,"writer_method":writer_facts::METHOD,"fact_prompts":prompts,"positive_control":positive,"negative_control":prior,"negative_spec":negative_spec,"code_sha256":{"writer_facts":sha256(&std::fs::read("src/writer_facts.rs")?),"flow_writer":sha256(&std::fs::read("src/learning_flow_writer.rs")?),"generate":sha256(&std::fs::read("src/generate.rs")?)},"tasks":tasks,"phase":phase,"run_cap_micro_cny":cap});
    store.freeze(&frozen)?;
    let mut rows = vec![];
    // Two independent author jobs; the shared ledger serializes reservations.
    let (a, b) = tokio::join!(
        learn(Arc::clone(&store), groups[0].clone(), "wxb"),
        learn(Arc::clone(&store), groups[1].clone(), "wzq")
    );
    for (id, result) in [("wxb-k10", a), ("wzq-k10", b)] {
        rows.push(match result {
            Ok(v) => v,
            Err(e) => json!({"id":id,"status":"failed","error":e.to_string()}),
        });
    }
    let profile = LearningClient::from_env()?.profile();
    let resumed = resume_root(&store)?;
    std::fs::create_dir_all(store.root.join("controls"))?;
    let p = writer_facts::review_with(
        &resumed.join("controls/positive"),
        &profile,
        &prompts.review,
        "writer-fact-review",
        positive["request"].as_str().unwrap(),
        positive["draft"].as_str().unwrap(),
        &mut |c| call(Arc::clone(&store), c),
    )
    .await;
    rows.push(match p{Ok(v)=>{write_json(&store.root.join("controls/positive.json"),&v)?;json!({"id":"positive-control","status":if v.clear(){"complete"}else{"failed"},"model_found_no_issue":v.clear()})},Err(e)=>json!({"id":"positive-control","status":"failed","error":e.to_string()})});
    let input = WriterInput {
        method: Some(writer_facts::METHOD.into()),
        label: "P5旧负例".into(),
        spec: negative_spec,
        request: prior["request"].as_str().unwrap().into(),
    };
    let negative_root = resumed.join("controls/negative");
    let n = writer_facts::check_with(
        &negative_root,
        &profile,
        &input,
        prior["fulltext"].as_str().unwrap(),
        &prompts,
        &mut |c| call(Arc::clone(&store), c),
    )
    .await;
    let first: Option<writer_facts::Review> =
        io::read(&negative_root.join("facts-first.json")).ok();
    let found = first.as_ref().is_some_and(|r| !r.clear());
    if let Ok(ref body) = n {
        crate::io::write_file_atomic(&store.root.join("controls/negative-repaired.md"), body)?;
    }
    rows.push(json!({"id":"negative-control","status":if found&&n.is_ok(){"complete"}else{"failed"},"initial_issues_detected":found,"error":n.err().map(|e|e.to_string())}));
    for code in ["wxb", "wzq"] {
        let style_path = store.root.join(format!("styles/{code}.md"));
        let result = if style_path.exists() {
            writer(
                Arc::clone(&store),
                code,
                &std::fs::read_to_string(style_path)?,
                tasks[code].as_str().unwrap(),
            )
            .await
        } else {
            Err(error("十篇候选未完成，未用旧候选代替"))
        };
        rows.push(match result {
            Ok(v) => v,
            Err(e) => json!({"id":code,"status":"failed","error":e.to_string()}),
        });
    }
    let mut receipts = 0;
    if store.root.join("requests").exists() {
        for entry in std::fs::read_dir(store.root.join("requests"))? {
            let p = entry?.path();
            if p.extension().is_some_and(|s| s == "json") {
                verify(&io::read::<Checkpoint>(&p)?)?;
                receipts += 1;
            }
        }
    }
    let mut usage = store.usage().await;
    if let Some(e) = usage["entries"].as_object_mut() {
        e.retain(|_, v| v["run_id"] == run);
    }
    let passed = rows.iter().all(|r| r["status"] == "complete");
    write_json(
        &store.root.join("summary.json"),
        &json!({"status":if passed{"complete"}else{"partial"},"rows":rows,"verified_receipts":receipts,"usage":usage,"scope":"bounded P7 model study, pending implementer source/claim and draft review; not human feedback or release certification","finished_at":chrono::Utc::now().to_rfc3339()}),
    )?;
    if passed {
        Ok(())
    } else {
        Err(error("P7部分任务或控制未通过，原回执及失败已保留"))
    }
}
