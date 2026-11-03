//! Bounded real P3 checks reuse the domain engine and the shared research ledger.
use crate::{
    error::AppError,
    eval_store::write_json,
    learning_agent::{self, Delegate, Reply, ScopePatch, Status, Tier},
    learning_job::CallProfile,
    learning_transport::LearningClient,
    learning_workspace::LearningWorkspace,
    learning_workspace_store as local,
    research_budget::cost,
    research_receipt::{Checkpoint, verify},
    research_store::{ResearchStore, Role},
};
use serde_json::{Value, json};
use std::{path::Path, sync::Arc};
const ROOT: &str = "research/style-aggregation";
fn error(s: &str) -> AppError {
    AppError::LearningWorkspace(s.into())
}
fn cases() -> Result<Vec<Value>, AppError> {
    let mixed = "# 纸上候车\n\n我在车站捡起一张旧车票。列车还没到，纸角却早已软了。我把它夹进笔记里，等下一个站名。\n\n# 给团队的项目备忘\n\n本周上线前先检查错误日志，再核对数据回写。任何一项失败都保留回执，不以请求成功替代产品验收。";
    let continuous = "# 早晨的桌面\n\n我先把散在桌面的纸片排好。半张车票夹在书里，另一张纸记着今天要找的书名。这是同一天整理笔记的开始。\n\n# 午后的桌面\n\n同一份笔记还在整理。我把早晨那张车票翻过来，接着写下刚找到的页码。下午的光移走，桌面才总算空出一小块。";
    Ok(vec![
        json!({"id":"clear","expected":"ready_for_learning","delegate":{"user_request":"学习我分别提供的这两篇文章的综合写法。","request":{"sources":[{"kind":"text","label":"候车","text":"我把车票夹进书里。等车时先读两页，站名响了再合上。"},{"kind":"text","label":"读书","text":"书架最下面的一册落了灰。我把它取出来，发现旧书签还停在去年读过的地方。"}],"expected_articles":2,"note":"固定清楚来源，正常零额外决策"}}}),
        json!({"id":"choice","expected":"ready_for_learning","delegate":{"user_request":"这里只想学习其中一篇文章的写法，目前没有选定具体哪篇。两篇是独立文章，不能合并。","request":{"sources":[{"kind":"text","label":"两篇独立文章","text":mixed}],"expected_articles":1,"note":"用户未选择前不能擅自选文"}}}),
        json!({"id":"sections","expected":"ready_for_learning","delegate":{"user_request":"学习这同一篇连续文章的写法；一级标题只是早晨与午后的章节，都属于同一天的桌面笔记，不是让我二选一。","request":{"sources":[{"kind":"text","label":"桌面笔记全文","text":continuous}],"expected_articles":1,"note":"内部判断章节边界，无需用户确认明显信息"}}}),
        json!({"id":"bounded","expected":"blocked","delegate":{"user_request":"学习其中一篇独立文章，暂未决定哪篇，不能随便选，也不要把两篇合并。","request":{"sources":[{"kind":"text","label":"两篇独立文章","text":mixed}],"expected_articles":1,"note":"有限追问，不擅自代选"}}}),
    ])
}
pub(crate) async fn run(run: &str) -> Result<(), AppError> {
    let store = Arc::new(ResearchStore::open(Path::new(ROOT), run, false)?);
    let phase_path = store.root.join("phase-budget.json");
    let phase: Value = if phase_path.exists() {
        local::read(&phase_path)?
    } else {
        json!({"prior_runs":[]})
    };
    let usage = store.usage().await;
    let mut cap = 1_000_000u64;
    let priors = phase["prior_runs"]
        .as_array()
        .ok_or_else(|| error("阶段预算来源无效"))?;
    let mut seen = std::collections::HashSet::new();
    for prior in priors {
        let prior = prior
            .as_str()
            .filter(|s| crate::research_dataset::valid_id(s) && *s != run)
            .ok_or_else(|| error("阶段预算来源身份无效"))?;
        if !seen.insert(prior) {
            return Err(error("阶段预算来源重复"));
        }
        for e in usage["entries"]
            .as_object()
            .ok_or_else(|| error("账本无效"))?
            .values()
            .filter(|e| e["run_id"] == prior)
        {
            cap = cap.saturating_sub(
                e["actual_micro"]
                    .as_u64()
                    .or(e["reserved_micro"].as_u64())
                    .unwrap_or(u64::MAX),
            );
        }
    }
    store.limit_run(cap).await?;
    let mut profile = LearningClient::from_env()?.profile();
    profile.model = "mimo-v2.6-flash".into();
    let prompt = std::fs::read_to_string("prompts/learning_agent.md")?;
    let mut cases = cases()?;
    if let Some(ids) = phase.get("selected_case_ids") {
        let ids = ids.as_array().ok_or_else(|| error("复验用例列表无效"))?;
        if ids.is_empty() || ids.iter().any(|id| !cases.iter().any(|c| c["id"] == *id)) {
            return Err(error("未知复验用例"));
        }
        cases.retain(|c| ids.contains(&c["id"]));
    }
    store.freeze(&json!({"suite":"domain_learning_agent_v1","method":"style-input-agent-v1","prompt":prompt,"profile":profile,"cases":cases,"run_cap_micro_cny":cap,"phase_budget":phase,"case_cap_micro_cny":250_000}))?;
    std::fs::create_dir_all(store.root.join("links"))?;
    let mut rows = Vec::new();
    for case in &cases {
        let code = case["id"].as_str().ok_or_else(|| error("缺用例身份"))?;
        let ws = LearningWorkspace::open(&store.root.join("workspaces").join(code))?;
        let link = store.root.join(format!("links/{code}.json"));
        let id = if link.exists() {
            let saved: Value = local::read(&link)?;
            saved["task_id"]
                .as_str()
                .ok_or_else(|| error("任务链接无效"))?
                .to_string()
        } else {
            let delegate: Delegate = serde_json::from_value(case["delegate"].clone())
                .map_err(|e| error(&e.to_string()))?;
            let plan = learning_agent::prepare(
                &ws,
                delegate,
                Path::new("."),
                profile.clone(),
                prompt.clone(),
            )?;
            local::publish_json(&link, &json!({"task_id":plan.id}))?;
            plan.id
        };
        let outcome = drive(&store, &ws, &id, code, &profile).await;
        let state = learning_agent::load(&ws, &id)?.state;
        let expected = case["expected"].as_str().unwrap_or("");
        let actual = serde_json::to_value(&state.status).map_err(|e| error(&e.to_string()))?;
        let mut passed =
            actual == expected && state.questions.len() <= 2 && state.decision_steps <= 12;
        let mut texts = Vec::new();
        if state.status == Status::ReadyForLearning {
            texts = learning_agent::ready_texts(&ws, &id)?;
            passed &= if code == "clear" {
                texts.len() == 2 && state.decision_steps == 0 && state.questions.is_empty()
            } else if code == "choice" {
                texts.len() == 1
                    && texts[0].contains("本周上线前先检查错误日志")
                    && state.questions.len() == 1
            } else {
                texts.len() == 1 && texts[0].contains("早晨那张车票") && state.questions.is_empty()
            };
        }
        rows.push(json!({"id":code,"task_id":id,"status":state.status,"expected":expected,"passed":passed,"questions":state.questions,"answers":state.answers,"decisions":state.decision_steps,"pro_used":state.pro_used,"reason":state.reason,"selected":state.selected,"ready_text_sha256":texts.iter().map(|s|crate::voice_learning::sha256(s.as_bytes())).collect::<Vec<_>>(),"error":outcome.err().map(|e|e.to_string())}));
        write_json(
            &store.root.join("summary.json"),
            &json!({"status":"running","rows":rows,"usage":store.usage().await}),
        )?;
    }
    // Verify the saved original receipts even when finished tasks require no model callback.
    let mut receipts = 0;
    for entry in std::fs::read_dir(store.root.join("requests"))? {
        let path = entry?.path();
        if path.extension().is_some_and(|e| e == "json") {
            let saved: Checkpoint = local::read(&path)?;
            verify(&saved)?;
            receipts += 1;
        }
    }
    let passed = rows
        .iter()
        .all(|r| r["passed"] == true && r["error"].is_null());
    write_json(
        &store.root.join("summary.json"),
        &json!({"status":if passed{"complete"}else{"partial"},"scope":"P3 domain decisions; synthetic inputs; no P4 main conversation or P5 style generation","rows":rows,"verified_receipts":receipts,"usage":store.usage().await,"finished_at":chrono::Utc::now().to_rfc3339()}),
    )?;
    if passed {
        Ok(())
    } else {
        Err(error("部分P3真实用例未达预期，完整状态和响应保留"))
    }
}
async fn drive(
    store: &Arc<ResearchStore>,
    ws: &LearningWorkspace,
    id: &str,
    code: &str,
    profile: &CallProfile,
) -> Result<(), AppError> {
    loop {
        let store = Arc::clone(store);
        let code_owned = code.to_string();
        let endpoint = profile.endpoint_sha256.clone();
        let r = learning_agent::run_with(ws, id, move |call| {
            let store = Arc::clone(&store);
            let code = code_owned.clone();
            let endpoint = endpoint.clone();
            async move {
                if endpoint != call.endpoint_sha256 {
                    return Err(error("研究端点身份不一致"));
                }
                let name = format!("{code}-{}", call.step);
                let role = if call.tier == Tier::Flash {
                    Role::AgentFlash
                } else {
                    Role::AgentPro
                };
                let path = store.root.join(format!("requests/{name}.json"));
                if !path.exists() {
                    let usage = store.usage().await;
                    let entries = usage["entries"]
                        .as_object()
                        .ok_or_else(|| error("账本条目无效"))?;
                    let prefix = format!(
                        "{}/{}-",
                        store
                            .root
                            .file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or(""),
                        code
                    );
                    let identity = format!(
                        "{}/{}",
                        store
                            .root
                            .file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or(""),
                        name
                    );
                    if entries
                        .keys()
                        .any(|k| k == &identity || k.starts_with(&format!("{identity}/")))
                    {
                        return Err(error("研究请求已有未知预留，本小试不隐式重发"));
                    }
                    let used = entries
                        .iter()
                        .filter(|(k, _)| k.starts_with(&prefix))
                        .map(|(_, v)| {
                            v["actual_micro"]
                                .as_u64()
                                .or(v["reserved_micro"].as_u64())
                                .unwrap_or(u64::MAX)
                        })
                        .fold(0u64, u64::saturating_add);
                    let upper = call.system.len() as u64 + call.user.len() as u64 + 4096;
                    if cost(role.model(), upper, 2048)? > 250_000u64.saturating_sub(used) {
                        return Err(error("小试单任务0.25元预算不足"));
                    }
                }
                store
                    .call(&name, role, &call.system, &call.user, true)
                    .await
            }
        })
        .await?;
        if r.state.status != Status::WaitingInput {
            return Ok(());
        }
        let answer = if code == "choice" {
            "选择《给团队的项目备忘》这篇。"
        } else if r.state.answers.is_empty() {
            "暂时还没决定选择哪篇。"
        } else {
            "我现在仍然无法确定具体选文。"
        };
        if r.state.answers.len() >= 2 {
            return Err(error("用例超过两次回答，未继续"));
        }
        let scope = if code == "choice" {
            ScopePatch {
                target_titles: Some(vec!["给团队的项目备忘".into()]),
                expected_articles: Some(1),
                ..Default::default()
            }
        } else {
            ScopePatch::default()
        };
        learning_agent::answer(
            ws,
            id,
            Reply {
                expected_revision: r.state.revision,
                question_id: r.state.questions.last().map(|q| q.id.clone()),
                text: answer.into(),
                user_request: None,
                scope,
            },
        )?;
    }
}
