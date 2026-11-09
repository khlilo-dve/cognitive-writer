//! P4 fixed conversation checks using real Flash and the shared research ledger.
use crate::{
    error::AppError,
    eval_store::write_json,
    learning_agent,
    learning_chat::{self, ChatRef, Event, ModelCall},
    learning_chat_store as chat_store,
    learning_transport::LearningClient,
    learning_workspace_store as io,
    research_receipt::{Checkpoint, verify},
    research_store::{ResearchStore, Role},
};
use serde_json::{Value, json};
use std::{path::Path, sync::Arc};
const ROOT: &str = "research/style-aggregation";
fn error(s: &str) -> AppError {
    AppError::LearningWorkspace(s.into())
}
fn usage_for(usage: Value, run: &str) -> Value {
    let mut u = usage;
    if let Some(entries) = u["entries"].as_object_mut() {
        entries.retain(|_, v| v["run_id"] == run);
    }
    u
}
pub(crate) async fn run(run: &str) -> Result<(), AppError> {
    let store = Arc::new(ResearchStore::open(Path::new(ROOT), run, false)?);
    let phase: Value = if store.root.join("phase-budget.json").exists() {
        io::read(&store.root.join("phase-budget.json"))?
    } else {
        json!({"prior_runs":[]})
    };
    let mut cap = 500000u64;
    let usage = store.usage().await;
    let mut seen = std::collections::HashSet::new();
    for prior in phase["prior_runs"]
        .as_array()
        .ok_or_else(|| error("阶段来源无效"))?
    {
        let prior = prior
            .as_str()
            .filter(|p| crate::research_dataset::valid_id(p) && *p != run)
            .ok_or_else(|| error("阶段来源身份无效"))?;
        if !seen.insert(prior) {
            return Err(error("阶段来源重复"));
        }
        for v in usage["entries"]
            .as_object()
            .ok_or_else(|| error("账本无效"))?
            .values()
            .filter(|v| v["run_id"] == prior)
        {
            cap = cap.saturating_sub(
                v["actual_micro"]
                    .as_u64()
                    .or(v["reserved_micro"].as_u64())
                    .unwrap_or(u64::MAX),
            );
        }
    }
    store.limit_run(cap).await?;
    let fixtures = store.root.join("fixtures");
    std::fs::create_dir_all(&fixtures)?;
    for (name, body) in [
        (
            "甲.md",
            "我把书签夹进昨天翻过的页码。今天读到同一处，纸角还微微翘着。",
        ),
        (
            "乙.md",
            "窗边的书架空出一格。我把借来的书放进去，桌面才有了写字的地方。",
        ),
        (
            "双文.md",
            "# 纸上候车\n\n我在车站把旧车票夹进书里，等下一个站名响起。\n\n# 给团队的项目备忘\n\n上线前先检查错误日志，再核对数据回写。每次失败保留回执，不把请求成功当作验收。",
        ),
    ] {
        let path = fixtures.join(name);
        if path.exists() {
            if std::fs::read_to_string(&path)? != body {
                return Err(error("冻结夹具变化"));
            }
        } else {
            io::publish_json(&path.with_extension("source.json"), &json!({"text":body}))?;
            std::fs::write(&path, body)?;
        }
    }
    let fixture = std::fs::canonicalize(&fixtures)?;
    let cases = vec![
        json!({"id":"clear","input":format!("学习这两篇 \"{}\" \"{}\"，保存为“笔记声音”，再写一篇图书馆文章，800字，给学生看",fixture.join("甲.md").display(),fixture.join("乙.md").display())}),
        json!({"id":"named","input":format!("学习这个材料 \"{}\"，命名为随笔研究；接着写一篇读书文章，700字，给学生看",fixture.join("甲.md").display())}),
        json!({"id":"reply","input":format!("学习 \"{}\"，只学一篇；这两篇是独立文章，不能合并",fixture.join("双文.md").display()),"reply":"我更想保留项目备忘那一篇"}),
        json!({"id":"use","input":"不要学习，用轻辩风格写一篇关于阅读的文章，600字，给学生看"}),
    ];
    let mut profile = LearningClient::from_env()?.profile();
    profile.model = "mimo-v2.6-flash".into();
    let prompt = std::fs::read_to_string("prompts/learning_chat.md")?;
    let agent_prompt = std::fs::read_to_string("prompts/learning_agent.md")?;
    store.freeze(&json!({"suite":"learning_chat_p4_v1","cases":cases,"profile":profile,"prompt":prompt,"agent_prompt":agent_prompt,"phase":phase,"run_cap_micro_cny":cap,"scope":"P4 orchestration with synthetic sources; no P5 learning or Writer generation"}))?;
    std::fs::create_dir_all(store.root.join("links"))?;
    let mut rows = Vec::new();
    for case in cases {
        let code = case["id"].as_str().ok_or_else(|| error("缺用例身份"))?;
        let original = case["input"].as_str().ok_or_else(|| error("缺原话"))?;
        let link = store.root.join(format!("links/{code}.json"));
        let r: ChatRef = if link.exists() {
            io::read(&link)?
        } else {
            let r = chat_store::create_with(
                &store.root.join("workspaces").join(code),
                profile.clone(),
                prompt.clone(),
                agent_prompt.clone(),
            )?;
            io::publish_json(&link, &r)?;
            learning_chat::submit(&r, original)?;
            r
        };
        let (mut outcome, mut failure) = match drive(&store, &r, code).await {
            Ok(v) => (Some(v), None),
            Err(e) => (None, Some(e.to_string())),
        };
        if code == "reply" {
            if let Some(id) = learning_chat::active(&r)? {
                let s = learning_agent::load(&chat_store::ws(&r)?, &id)?.state;
                if s.status == learning_agent::Status::WaitingInput && s.answers.is_empty() {
                    learning_chat::submit(&r, case["reply"].as_str().unwrap())?;
                    match drive(&store, &r, code).await {
                        Ok(v) => outcome = Some(v),
                        Err(e) => failure = Some(e.to_string()),
                    };
                }
            }
        }
        let active = learning_chat::active(&r)?;
        let mut details = json!({});
        let passed = if code == "use" {
            matches!(outcome,Some(Event::Use{ref style_name,original: ref text}) if style_name=="轻辩"&&text==original)
                && active.is_none()
        } else if let Some(id) = &active {
            let ws = chat_store::ws(&r)?;
            let s = learning_agent::load(&ws, id)?.state;
            let p = crate::learning_agent_store::plan(&ws, id)?;
            details = json!({"task_id":id,"domain_status":s.status,"questions":s.questions,"answers":s.answers,"selected":s.selected,"save_name":p.delegate.save_name,"followup_request":p.delegate.followup_request,"original_request":p.delegate.user_request});
            s.status == learning_agent::Status::ReadyForLearning
                && if code == "clear" {
                    learning_agent::ready_texts(&ws, id)?.len() == 2
                        && p.delegate.save_name.as_deref() == Some("笔记声音")
                        && p.delegate.followup_request.as_deref() == Some(original)
                        && s.decision_steps == 0
                        && s.questions.is_empty()
                } else if code == "named" {
                    p.delegate.save_name.as_deref() == Some("随笔研究")
                        && p.delegate.followup_request.as_deref() == Some(original)
                        && s.questions.is_empty()
                } else {
                    s.selected == vec!["s001-a002"]
                        && s.questions.len() == 1
                        && s.answers.len() == 1
                }
        } else {
            false
        };
        rows.push(json!({"id":code,"chat_id":r.id,"status":if passed&&failure.is_none(){"complete"}else{"failed"},"passed":passed&&failure.is_none(),"details":details,"error":failure,"event":outcome}));
        write_json(
            &store.root.join("summary.json"),
            &json!({"status":"running","rows":rows,"usage":usage_for(store.usage().await,run)}),
        )?;
    }
    let mut receipts = 0;
    for entry in std::fs::read_dir(store.root.join("requests"))? {
        let path = entry?.path();
        if path.extension().is_some_and(|e| e == "json") {
            let cp: Checkpoint = io::read(&path)?;
            verify(&cp)?;
            receipts += 1;
        }
    }
    let passed = rows.iter().all(|r| r["passed"] == true);
    write_json(
        &store.root.join("summary.json"),
        &json!({"status":if passed{"complete"}else{"partial"},"rows":rows,"verified_receipts":receipts,"usage":usage_for(store.usage().await,run),"scope":"P4 same engine with ResearchStore adapter; synthetic inputs and fixed user replies; not human feedback or style quality","finished_at":chrono::Utc::now().to_rfc3339()}),
    )?;
    if passed {
        Ok(())
    } else {
        Err(error("部分P4真实用例未达预期，状态及回执已保存"))
    }
}
async fn drive(store: &Arc<ResearchStore>, r: &ChatRef, code: &str) -> Result<Event, AppError> {
    let store = Arc::clone(store);
    let code = code.to_string();
    learning_chat::run_with(r, move |request: ModelCall| {
        let store = Arc::clone(&store);
        let code = code.clone();
        async move {
            let name = format!(
                "{code}-{}-{}-{}",
                if request.parent { "parent" } else { "domain" },
                &request.owner_id[..8],
                request.call.step
            );
            let path = store.root.join(format!("requests/{name}.json"));
            if !path.exists() {
                let usage = store.usage().await;
                let identity = format!(
                    "{}/{}",
                    store
                        .root
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or(""),
                    name
                );
                if usage["entries"]
                    .as_object()
                    .ok_or_else(|| error("账本无效"))?
                    .keys()
                    .any(|k| k == &identity || k.starts_with(&format!("{identity}/")))
                {
                    return Err(error("P4已有未知预留，不隐式重发"));
                }
            }
            let role = if request.call.tier == learning_agent::Tier::Flash {
                Role::AgentFlash
            } else {
                Role::AgentPro
            };
            store
                .call(&name, role, &request.call.system, &request.call.user, true)
                .await
        }
    })
    .await
}
