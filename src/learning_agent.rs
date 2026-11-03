//! Bounded, durable domain decisions. P3 ends at source readiness, before P5 style generation.
pub use crate::learning_agent_types::{
    AgentPlan, AgentState, Classification, Decision, DecisionCall, Delegate, Reply, ScopePatch,
    StateRecord, Status, Tier,
};
use crate::learning_agent_types::{METHOD, Pending, Question};
use crate::{
    error::AppError, learning_agent_scope as scope, learning_agent_store as journal,
    learning_agent_tools as tools, learning_workspace::LearningWorkspace,
    learning_workspace_store as store, voice_learning::sha256,
};
use std::{future::Future, path::Path};

pub fn prepare(
    ws: &LearningWorkspace,
    delegate: Delegate,
    base: &Path,
    profile: crate::learning_job::CallProfile,
    prompt: String,
) -> Result<AgentPlan, AppError> {
    journal::prepare(ws, delegate, base, profile, prompt)
}
pub fn load(ws: &LearningWorkspace, id: &str) -> Result<StateRecord, AppError> {
    journal::load(ws, id)
}
pub fn answer(ws: &LearningWorkspace, id: &str, reply: Reply) -> Result<StateRecord, AppError> {
    tools::reply(ws, id, reply)
}
pub fn cancel(ws: &LearningWorkspace, id: &str) -> Result<StateRecord, AppError> {
    let r = load(ws, id)?;
    if r.state.status == Status::Cancelled {
        return Ok(r);
    }
    let mut s = r.state.clone();
    s.revision += 1;
    s.status = Status::Cancelled;
    s.pending = None;
    s.reason = "用户已取消；既有材料与回执保留，迟到结果不会采用".into();
    Ok(journal::advance(ws, &r, s)?.0)
}
pub fn ready_texts(ws: &LearningWorkspace, id: &str) -> Result<Vec<String>, AppError> {
    let p = journal::plan(ws, id)?;
    let r = load(ws, id)?;
    if r.state.status != Status::ReadyForLearning {
        return Err(journal::error("尚未准备好学习正文"));
    }
    scope::selected_texts(
        ws,
        &p,
        &r.state,
        &tools::collection(ws, &p, &r.state)?,
        &r.state.selected,
    )
}
fn question_key(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).collect()
}
fn apply(
    ws: &LearningWorkspace,
    p: &AgentPlan,
    s: &mut AgentState,
    decision: Decision,
) -> Result<bool, AppError> {
    let c = tools::collection(ws, p, s)?;
    match decision {
        Decision::Collect => Err(journal::error("已有来源集合，不能无进展重复采集")),
        Decision::Inspect {
            source_ids,
            offset_chars,
            ..
        } => tools::inspect(&c, s, source_ids, offset_chars),
        Decision::Resolve {
            source_id,
            classification,
            evidence,
            reason,
        } => scope::resolve(&c, s, &source_id, classification, evidence, reason),
        Decision::Select {
            article_ids,
            reason,
        } => {
            scope::selected_texts(ws, p, s, &c, &article_ids)?;
            s.selected = article_ids;
            s.status = Status::ReadyForLearning;
            s.reason = format!("正文范围已准备好，尚未生成风格。{reason}");
            Ok(true)
        }
        Decision::Clarify {
            topic,
            question,
            reason,
        } => {
            if s.questions.len() >= 2 {
                s.status = Status::Blocked;
                s.reason =
                    "已到两轮主动澄清上限；任务保留，等待用户主动提供明确范围，不再生成第三个问题"
                        .into();
                return Ok(true);
            }
            if question.trim().is_empty()
                || topic.trim().is_empty()
                || reason.trim().is_empty()
                || s.questions
                    .iter()
                    .any(|q| question_key(&q.text) == question_key(&question))
            {
                return Err(journal::error("问题为空或已经问过，不能换标签重复追问"));
            }
            if let Some(previous) = s.questions.last() {
                if !s
                    .answers
                    .iter()
                    .any(|a| a.reply.question_id.as_deref() == Some(&previous.id))
                {
                    return Err(journal::error("上一问题尚未回答，不能追加追问"));
                }
            }
            let id = sha256(
                format!("{}:{}:{}:{question}", p.id, s.revision, s.questions.len()).as_bytes(),
            );
            s.questions.push(Question {
                id,
                revision: s.revision,
                topic,
                text: question,
                reason,
            });
            s.status = Status::WaitingInput;
            s.reason = "需要用户决定关键范围；等待期间不调用模型".into();
            Ok(true)
        }
        Decision::Escalate { reason } => {
            if s.pro_used {
                return Err(journal::error("本任务已使用一次Pro升级，不能重复升级"));
            }
            if reason.trim().is_empty() {
                return Err(journal::error("升级需要具体困难原因"));
            }
            s.pro_used = true;
            s.next_tier = Tier::Pro;
            s.reason = reason;
            Ok(true)
        }
        Decision::Stop { reason } => {
            if reason.trim().is_empty() {
                return Err(journal::error("停止必须说明具体缺口"));
            }
            s.status = Status::Blocked;
            s.reason = reason;
            Ok(true)
        }
    }
}
/// The caller supplies a journaled model adapter. Completed/waiting tasks never call it again.
pub async fn run_with<F, Fut>(
    ws: &LearningWorkspace,
    id: &str,
    mut decide: F,
) -> Result<StateRecord, AppError>
where
    F: FnMut(DecisionCall) -> Fut,
    Fut: Future<Output = Result<String, AppError>>,
{
    let p = journal::plan(ws, id)?;
    let _run = store::lock(&journal::directory(ws, id)?.join("run-lock"))?;
    let mut r = load(ws, id)?;
    if r.state.status == Status::Failed && r.state.pending.is_some() {
        let mut s = r.state.clone();
        s.status = Status::Active;
        let (next, applied) = journal::advance(ws, &r, s)?;
        if !applied {
            return Ok(next);
        }
        r = next;
    }
    loop {
        if r.state.status != Status::Active {
            return Ok(r);
        }
        if p.method != METHOD {
            return Err(journal::error("Agent方法已变化，保留旧任务，不静默重跑"));
        }
        if r.state.pending.is_none() {
            let mut s = r.state.clone();
            if s.collection.is_none() && s.no_progress >= 2 {
                s.status = Status::Blocked;
                s.reason = "采集连续两次没有进展，保留失败原因并停止".into();
                return Ok(journal::advance(ws, &r, s)?.0);
            }
            if s.collection.is_none() {
                s.pending = Some(Pending::Tool {
                    step: format!("r{}-collect", s.revision),
                    decision: Decision::Collect,
                });
            } else {
                let c = tools::collection(ws, &p, &s)?;
                if let Some(ids) = scope::automatic(ws, &p, &s, &c) {
                    s.selected = ids;
                    s.status = Status::ReadyForLearning;
                    s.reason =
                        "来源和范围已满足，正文已准备好；本步骤未追加追问或决策模型调用".into();
                    return Ok(journal::advance(ws, &r, s)?.0);
                }
                if scope::pool(&c, &s).is_empty()
                    && !c.records.iter().any(|v| {
                        v.status == crate::learning_sources::SourceStatus::NeedsInspection
                            && !s.resolutions.contains_key(&v.id)
                    })
                {
                    s.status = Status::Blocked;
                    s.reason =
                        "未取得可学习正文；来源失败与已完成材料保留，需补充有效文章才能继续".into();
                    return Ok(journal::advance(ws, &r, s)?.0);
                }
                if s.decision_steps >= 12 || s.no_progress >= 2 {
                    s.status = Status::Blocked;
                    s.reason="决策次数已到12步或连续两次无进展，未继续调用；已保存材料、回答及具体工具结果".into();
                    return Ok(journal::advance(ws, &r, s)?.0);
                }
                let user = tools::observation(&p, &s, &c)?;
                s.decision_steps += 1;
                let tier = s.next_tier;
                s.next_tier = Tier::Flash;
                s.pending = Some(Pending::Decision {
                    step: format!("r{}-d{}", s.revision, s.decision_steps),
                    tier,
                    user,
                });
            }
            let (next, applied) = journal::advance(ws, &r, s)?;
            if !applied {
                return Ok(next);
            }
            r = next;
            continue;
        }
        let pending = r
            .state
            .pending
            .clone()
            .ok_or_else(|| journal::error("待执行步骤缺失"))?;
        match pending {
            Pending::Decision { step, tier, user } => {
                let call = DecisionCall {
                    step: step.clone(),
                    tier,
                    system: p.prompt.clone(),
                    user,
                    endpoint_sha256: p.profile.endpoint_sha256.clone(),
                    budget_micro_cny: p.delegate.budget_micro_cny,
                };
                let response = decide(call).await;
                let current = load(ws, id)?;
                if current.id != r.id {
                    return Ok(current);
                }
                let mut s = r.state.clone();
                match response {
                    Ok(raw) => match serde_json::from_str::<Decision>(&raw) {
                        Ok(action) => {
                            s.pending = Some(Pending::Tool {
                                step,
                                decision: action,
                            })
                        }
                        Err(error) => {
                            s.pending = None;
                            s.no_progress += 1;
                            s.reason = format!(
                                "模型动作JSON无效：{error}；请返回带tool字段的完整动作；原响应保留"
                            );
                            s.observation = serde_json::json!({"tool_error":s.reason});
                        }
                    },
                    Err(error) => {
                        s.status = Status::Failed;
                        s.reason = error.to_string();
                        let (latest, applied) = journal::advance(ws, &r, s)?;
                        if !applied {
                            return Ok(latest);
                        }
                        return Err(error);
                    }
                }
                let (next, applied) = journal::advance(ws, &r, s)?;
                if !applied {
                    return Ok(next);
                }
                r = next;
            }
            Pending::Tool { step, decision } => {
                let mut s = r.state.clone();
                let result = if matches!(decision, Decision::Collect) {
                    match tools::collect(ws, &p, &s, &step).await {
                        Ok(reference) => {
                            s.collection = Some(reference);
                            s.observation = serde_json::json!({"collected":true});
                            Ok(true)
                        }
                        Err(e) => Err(e),
                    }
                } else {
                    apply(ws, &p, &mut s, decision)
                };
                let current = load(ws, id)?;
                if current.id != r.id {
                    return Ok(current);
                }
                s.pending = None;
                match result {
                    Ok(progress) => {
                        if progress {
                            s.no_progress = 0;
                        } else {
                            s.no_progress += 1;
                        }
                    }
                    Err(error) => {
                        s.reason = error.to_string();
                        s.observation = serde_json::json!({"tool_error":s.reason});
                        s.no_progress += 1;
                    }
                }
                let (next, applied) = journal::advance(ws, &r, s)?;
                if !applied {
                    return Ok(next);
                }
                r = next;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::learning_agent_types::tests::{fixture, question, reply};
    #[tokio::test]
    async fn clear_sources_are_ready_without_models_and_completed_resume_stays_keyless() {
        let f = fixture("这是明确提供的完整短文。", None);
        let result = run_with(&f.ws, &f.plan.id, |_| async {
            panic!("不应增加模型调用")
        })
        .await
        .unwrap();
        assert_eq!(result.state.status, Status::ReadyForLearning);
        assert_eq!(
            ready_texts(&f.ws, &f.plan.id).unwrap(),
            vec!["这是明确提供的完整短文。"]
        );
        assert_eq!(result.state.questions.len(), 0);
        assert_eq!(
            run_with(&f.ws, &f.plan.id, |_| async {
                panic!("完成结果不重发")
            })
            .await
            .unwrap()
            .id,
            result.id
        );
        let frozen = journal::plan(&f.ws, &f.plan.id).unwrap();
        assert_eq!(frozen.delegate.save_name.as_deref(), Some("以后使用的风格"));
        assert!(frozen.delegate.followup_request.is_some());
    }
    #[tokio::test]
    async fn clarification_is_bounded_answers_survive_and_duplicate_reply_does_not_rewind() {
        let f = fixture("# 甲\n\n甲文章。\n\n# 乙\n\n乙文章。", None);
        let one = run_with(&f.ws, &f.plan.id, |_| async {
            Ok(serde_json::to_string(&question("请选甲还是乙？")).unwrap())
        })
        .await
        .unwrap();
        assert_eq!(one.state.status, Status::WaitingInput);
        assert_eq!(
            run_with(&f.ws, &f.plan.id, |_| async {
                panic!("等待时不再调用")
            })
            .await
            .unwrap()
            .id,
            one.id
        );
        let response = reply(&one, "我偏向第二篇，但还不能确定标题");
        answer(&f.ws, &f.plan.id, response.clone()).unwrap();
        let two = run_with(&f.ws, &f.plan.id, |_| async {
            Ok(serde_json::to_string(&question("第二篇是乙，是否选择乙这一篇？")).unwrap())
        })
        .await
        .unwrap();
        assert_eq!(two.state.questions.len(), 2);
        assert_eq!(two.state.answers.len(), 1);
        assert_eq!(answer(&f.ws, &f.plan.id, response).unwrap().id, two.id);
        answer(&f.ws, &f.plan.id, reply(&two, "标题仍不能确定")).unwrap();
        let stopped = run_with(&f.ws, &f.plan.id, |_| async {
            Ok(serde_json::to_string(&question("第三次提问不应显示")).unwrap())
        })
        .await
        .unwrap();
        assert_eq!(stopped.state.status, Status::Blocked);
        assert_eq!(stopped.state.questions.len(), 2);
        assert_eq!(stopped.state.answers.len(), 2);
        assert_eq!(stopped.state.decision_steps, 3);
    }
    #[tokio::test]
    async fn model_observation_carries_read_history_and_specific_schema_failure() {
        let f = fixture(
            "# 早晨\n\n连续文章的早晨。\n\n# 午后\n\n同一篇文章接着写午后。",
            Some(1),
        );
        let mut calls = 0;
        let result = run_with(&f.ws, &f.plan.id, |call| {
            calls += 1;
            let user: serde_json::Value = serde_json::from_str(&call.user).unwrap();
            let raw = match calls {
                1 => serde_json::json!({"tool":"inspect","source_ids":["s001"],"reason":"查看边界"}).to_string(),
                2 => {
                    assert_eq!(user["inspected_sources"], serde_json::json!(["s001"]));
                    assert_eq!(user["inspected_windows"], serde_json::json!(["s001@0"]));
                    serde_json::json!({"source_id":"s001","classification":"article","evidence":["同一篇文章"],"reason":"连续"}).to_string()
                }
                3 => {
                    assert!(user["last_tool_result"]["tool_error"].as_str().unwrap().contains("missing field `tool`"));
                    serde_json::json!({"tool":"resolve","source_id":"s001","classification":"article","evidence":["同一篇文章"],"reason":"连续"}).to_string()
                }
                _ => panic!("无需第四次调用"),
            };
            async move { Ok(raw) }
        }).await.unwrap();
        assert_eq!(result.state.status, Status::ReadyForLearning);
        assert!(result.state.questions.is_empty());
        assert_eq!(calls, 3);
    }
    #[tokio::test]
    async fn cancellation_can_commit_while_a_model_result_is_in_flight() {
        let f = fixture("# 甲\n\n正文甲。\n\n# 乙\n\n正文乙。", None);
        let result = run_with(&f.ws, &f.plan.id, |_| {
            cancel(&f.ws, &f.plan.id).unwrap();
            async { Ok(serde_json::to_string(&question("迟到的问题")).unwrap()) }
        })
        .await
        .unwrap();
        assert_eq!(result.state.status, Status::Cancelled);
        assert_eq!(result.state.revision, 1);
        assert!(result.state.questions.is_empty());
    }
}
