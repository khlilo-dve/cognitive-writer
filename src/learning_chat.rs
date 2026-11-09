//! Main conversation delegates a single learning task; P5 consumes the ready reference later.
pub use crate::learning_chat_types::{Action, ChatRef, Event, LearnInput, ModelCall};
use crate::{
    error::AppError, learning_agent, learning_chat_parse as parse, learning_chat_reply as reply,
    learning_chat_store as store, learning_chat_types::*, learning_workspace_store as io,
};
use serde_json::json;
use std::future::Future;
pub fn create(root: &std::path::Path) -> Result<ChatRef, AppError> {
    store::create(root)
}
pub fn event(r: &ChatRef) -> Result<Event, AppError> {
    Ok(store::load(r)?.state.event)
}
pub fn has_pending(r: &ChatRef) -> Result<bool, AppError> {
    Ok(store::load(r)?.state.pending.is_some())
}
pub fn active(r: &ChatRef) -> Result<Option<String>, AppError> {
    Ok(store::load(r)?.state.active)
}
pub fn submit(r: &ChatRef, input: &str) -> Result<(), AppError> {
    if input.len() > crate::learning_sources::MAX_BYTES {
        return Err(store::error("会话输入超过8MiB，未截断或建立任务"));
    }
    if input.trim().is_empty() {
        return Ok(());
    }
    let old = store::load(r)?;
    let mut s = old.state.clone();
    if s.paused {
        return Err(store::error("学习已暂停；输入继续恢复，或取消当前学习"));
    }
    if s.active.is_some()
        && reply::fast(input, &reply::view(r)?).is_none()
        && parse::learning(parse::control(input).0)
        && !["目标改为", "改学", "改成学习", "补充", "再加", "改用"]
            .iter()
            .any(|w| input.contains(w))
    {
        s.event = Event::Message {
            text: format!(
                "已有学习任务，未替换其材料或原写作需求。新请求已保留：{input}。明确替换当前学习或先取消后可新建。"
            ),
        };
        store::advance(r, &old, s)?;
        return Err(store::error(
            "已有学习任务；新需求已记录，明确替换或先取消才能新建",
        ));
    }
    s.turn += 1;
    s.pending = Some(Turn {
        number: s.turn,
        input: input.into(),
        binding: store::binding(r)?,
        action: None,
    });
    store::advance(r, &old, s)?;
    Ok(())
}
pub fn submit_url(r: &ChatRef, input: &str, url: &str) -> Result<(), AppError> {
    parse::validate_source(
        &crate::learning_sources::SourceInput::Url { url: url.into() },
        input,
    )?;
    submit(r, input)?;
    let old = store::load(r)?;
    let mut s = old.state.clone();
    let turn = s
        .pending
        .as_mut()
        .ok_or_else(|| store::error("缺原始输入"))?;
    turn.action = Some(Action::Learn {
        input: LearnInput {
            sources: vec![crate::learning_sources::SourceInput::Url { url: url.into() }],
            write_after: input.contains("写"),
            ..Default::default()
        },
    });
    store::advance(r, &old, s)?;
    Ok(())
}
pub fn cancel(r: &ChatRef) -> Result<(), AppError> {
    let old = store::load(r)?;
    if let Some(id) = &old.state.active {
        learning_agent::cancel(&store::ws(r)?, id)?;
    }
    let mut s = old.state.clone();
    s.active = None;
    s.pending = None;
    s.paused = false;
    s.event = Event::Idle;
    store::advance(r, &old, s)?;
    Ok(())
}
pub fn pause(r: &ChatRef) -> Result<(), AppError> {
    let old = store::load(r)?;
    if old.state.active.is_none() && old.state.pending.is_none() {
        return Err(store::error("没有可暂停的学习任务"));
    }
    let mut s = old.state.clone();
    s.paused = true;
    s.event = Event::Message {
        text: "学习已暂停，材料、原写作要求与未完成步骤保留；继续后恢复同一任务".into(),
    };
    store::advance(r, &old, s)?;
    Ok(())
}
pub async fn resume(r: &ChatRef, retry: bool) -> Result<Event, AppError> {
    run_with(r, |request| model(r, request, retry)).await
}
async fn model(r: &ChatRef, request: ModelCall, retry: bool) -> Result<String, AppError> {
    if request.parent {
        crate::learning_agent_model::call_at(
            &store::directory(r)?,
            &request.owner_id,
            request.call,
            retry,
        )
        .await
    } else {
        crate::learning_agent_model::call(&store::ws(r)?, &request.owner_id, request.call, retry)
            .await
    }
}
pub async fn run_with<F, Fut>(r: &ChatRef, mut decide: F) -> Result<Event, AppError>
where
    F: FnMut(ModelCall) -> Fut,
    Fut: Future<Output = Result<String, AppError>>,
{
    let _execution = io::lock(&store::directory(r)?.join("run-lock"))?;
    let p = store::plan(r)?;
    let mut record = store::load(r)?;
    if record.state.paused {
        let mut s = record.state.clone();
        s.paused = false;
        record = store::advance(r, &record, s)?;
    }
    loop {
        let mut s = record.state.clone();
        if let Some(turn) = s.pending.as_ref() {
            let mut turn = turn.clone();
            if turn.action.is_none() {
                let v = reply::view(r)?;
                let simple = if turn.binding.is_some() {
                    reply::fast(&turn.input, &v)
                } else {
                    parse::fast_start(&turn.input)
                };
                let action = if let Some(action) = simple {
                    action
                } else {
                    let call=learning_agent::DecisionCall{step:format!("t{:04}-interpret",turn.number),tier:learning_agent::Tier::Flash,system:p.prompt.clone(),user:json!({"mode":if turn.binding.is_some(){"reply"}else{"start"},"input":turn.input,"task":v}).to_string(),endpoint_sha256:p.profile.endpoint_sha256.clone(),budget_micro_cny:500000};
                    let raw = decide(ModelCall {
                        owner_id: r.id.clone(),
                        parent: true,
                        call,
                    })
                    .await?;
                    if store::load(r)?.id != record.id {
                        return Err(store::error("主会话已变化，旧输入解释未采用"));
                    }
                    serde_json::from_str::<Action>(&raw).map_err(|e| {
                        store::error(&format!(
                            "输入解释JSON无效：{e}；原回执保留，可补充明确输入或取消"
                        ))
                    })?
                };
                if turn.binding.is_some() {
                    reply::validate(&action, &turn.input, &v)?;
                }
                turn.action = Some(action);
                s.pending = Some(turn);
                record = store::advance(r, &record, s)?;
                continue;
            }
            match turn
                .action
                .as_ref()
                .ok_or_else(|| store::error("缺解释动作"))?
            {
                Action::Learn { input } => {
                    if s.active.is_some() {
                        return Err(store::error("已有学习任务，未创建第二个"));
                    }
                    let ws = store::ws(r)?;
                    let delegate = parse::make_delegate(&ws, input.clone(), &turn.input)?;
                    let link =
                        store::directory(r)?.join(format!("turns/t{:04}-task.json", turn.number));
                    let id = if link.exists() {
                        let v: serde_json::Value = io::read(&link)?;
                        let id = v["task_id"]
                            .as_str()
                            .ok_or_else(|| store::error("任务链接损坏"))?;
                        let prior = crate::learning_agent_store::plan(&ws, id)?;
                        if serde_json::to_value(&prior.delegate)
                            .map_err(|e| store::error(&e.to_string()))?
                            != serde_json::to_value(&delegate)
                                .map_err(|e| store::error(&e.to_string()))?
                        {
                            return Err(store::error("任务链接与委派不符"));
                        }
                        id.to_string()
                    } else {
                        let plan = learning_agent::prepare(
                            &ws,
                            delegate,
                            &p.base_dir,
                            p.profile.clone(),
                            p.agent_prompt.clone(),
                        )?;
                        io::publish_json(&link, &json!({"task_id":plan.id}))?;
                        plan.id
                    };
                    s.active = Some(id.clone());
                    s.pending = None;
                    s.event = Event::Learning { task_id: id };
                    record = store::advance(r, &record, s)?;
                    continue;
                }
                Action::Reply { .. } => {
                    reply::apply(r, &turn, turn.action.as_ref().unwrap())?;
                    s.pending = None;
                    record = store::advance(r, &record, s)?;
                    continue;
                }
                Action::Use { style_name } => {
                    if !style_name.is_empty()
                        && (!parse::control(&turn.input).0.contains(style_name)
                            || !parse::positive(parse::control(&turn.input).0, style_name))
                    {
                        return Err(store::error("风格名不在原始写作需求中"));
                    }
                    s.pending = None;
                    s.event = Event::Use {
                        style_name: style_name.clone(),
                        original: turn.input.clone(),
                    };
                    return Ok(store::advance(r, &record, s)?.state.event);
                }
                Action::Other => {
                    s.pending = None;
                    s.event = Event::Idle;
                    return Ok(store::advance(r, &record, s)?.state.event);
                }
            }
        }
        if let Some(id) = &s.active {
            let ws = store::ws(r)?;
            let id = id.clone();
            let result = learning_agent::run_with(&ws, &id, |call| {
                decide(ModelCall {
                    owner_id: id.clone(),
                    parent: false,
                    call,
                })
            })
            .await?;
            if store::load(r)?.id != record.id {
                return Err(store::error("主会话已变化，旧子任务结果未采用"));
            }
            if result.state.status == learning_agent::Status::Cancelled {
                s.active = None;
                s.event = Event::Idle;
            } else {
                s.event = Event::Learning { task_id: id };
            }
            return Ok(store::advance(r, &record, s)?.state.event);
        }
        return Ok(s.event);
    }
}
/// Display a saved question only when entering/restoring the wait, never on progress queries.
pub fn display(r: &ChatRef, show_question: bool) -> Result<(), AppError> {
    let s = store::load(r)?.state;
    if s.paused {
        println!("学习已暂停；输入继续恢复同一任务。");
        return Ok(());
    }
    if let Some(id) = s.active {
        let a = learning_agent::load(&store::ws(r)?, &id)?.state;
        println!(
            "学习任务：{id}\n状态：{:?}；决策{}/12；主动澄清{}/2\n{}",
            a.status,
            a.decision_steps,
            a.questions.len(),
            a.reason
        );
        if show_question && a.status == learning_agent::Status::WaitingInput {
            if let Some(q) = a.questions.last() {
                println!("{}", q.text);
            }
        }
        if a.status == learning_agent::Status::ReadyForLearning {
            println!("材料已准备好；尚未生成风格说明，保存名和后续写作要求已保留。");
        }
    } else if s.pending.is_some() {
        println!("主会话有未完成输入，原话和请求记录已保留；继续可恢复，未知请求仅显式重试。");
    } else if let Event::Message { text } = s.event {
        println!("{text}");
    } else {
        println!("当前没有活动学习任务。");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::learning_chat_types::tests::fixture;
    #[tokio::test]
    async fn combined_brief_and_missing_sources_share_the_domain_task_without_extra_models() {
        let (_d, r) = fixture();
        let original =
            "学习下面写法，再写一篇关于图书馆的文章，800字；正文：我把书签留在昨天那一页。";
        submit(&r, original).unwrap();
        run_with(&r, |_| async { panic!("清楚材料不应调用") })
            .await
            .unwrap();
        let id = active(&r).unwrap().unwrap();
        let p = crate::learning_agent_store::plan(&store::ws(&r).unwrap(), &id).unwrap();
        assert_eq!(p.delegate.followup_request.as_deref(), Some(original));
        assert_eq!(p.delegate.request.expected_articles, None);
        assert!(!r.workspace.join("candidates").exists());
        assert!(!r.workspace.join("versions").exists());
        submit(&r, "目标改为学习段落推进").unwrap();
        run_with(&r, |_| async { panic!("已明确修订无需模型") })
            .await
            .unwrap();
        let s = learning_agent::load(&store::ws(&r).unwrap(), &id)
            .unwrap()
            .state;
        assert_eq!(s.revision, 1);
        assert_eq!(active(&r).unwrap().as_deref(), Some(id.as_str()));
        cancel(&r).unwrap();
        submit(&r, "学习一种风格").unwrap();
        run_with(&r, |_| async { panic!("缺材料问题可直接呈现") })
            .await
            .unwrap();
        let missing = active(&r).unwrap().unwrap();
        let before = learning_agent::load(&store::ws(&r).unwrap(), &missing)
            .unwrap()
            .state;
        assert_eq!(before.questions.len(), 1);
        submit(&r, "正文：新提供的完整短文。").unwrap();
        run_with(&r, |_| async {
            panic!("补充明确正文不增加调用")
        })
        .await
        .unwrap();
        let after = learning_agent::load(&store::ws(&r).unwrap(), &missing)
            .unwrap()
            .state;
        assert_eq!(after.questions.len(), 1);
        assert_eq!(after.answers.len(), 1);
        assert_eq!(after.status, learning_agent::Status::ReadyForLearning);
    }
    #[tokio::test]
    async fn optional_interpreter_is_grounded_and_preserves_save_intent() {
        let (_d, r) = fixture();
        let original = "学习下面内容，命名为研读；正文：完整短文。";
        submit(&r, original).unwrap();
        let mut calls = 0;
        run_with(&r,|request|{calls+=1;assert!(request.parent);async{Ok(json!({"kind":"learn","sources":[{"kind":"text","label":"正文","text":"完整短文。"}],"save_name":"研读"}).to_string())}}).await.unwrap();
        assert_eq!(calls, 1);
        let p = crate::learning_agent_store::plan(
            &store::ws(&r).unwrap(),
            &active(&r).unwrap().unwrap(),
        )
        .unwrap();
        assert_eq!(p.delegate.save_name.as_deref(), Some("研读"));
    }
    #[tokio::test]
    async fn waiting_pause_resume_and_new_request_do_not_replace_the_task() {
        let (_d, r) = fixture();
        submit(&r, "学习正文：# 甲\n\n甲文。\n\n# 乙\n\n乙文。").unwrap();
        run_with(&r,|_|async{Ok(json!({"tool":"clarify","topic":"选文","question":"选甲还是乙？","reason":"两篇独立文章未选择"}).to_string())}).await.unwrap();
        let id = active(&r).unwrap();
        pause(&r).unwrap();
        assert!(submit(&r, "学习另一种风格").is_err());
        run_with(&r, |_| async { panic!("恢复等待不得调用") })
            .await
            .unwrap();
        assert!(submit(&r, "学习另一个目标").is_err());
        assert_eq!(active(&r).unwrap(), id);
        submit(&r, "选第二篇").unwrap();
        run_with(&r, |_| async { panic!("编号明确不得调用") })
            .await
            .unwrap();
        assert_eq!(active(&r).unwrap(), id);
        assert_eq!(
            learning_agent::ready_texts(&store::ws(&r).unwrap(), id.as_deref().unwrap()).unwrap(),
            vec!["# 乙\n\n乙文。"]
        );
    }
    #[tokio::test]
    async fn cancelled_late_parent_interpretation_cannot_create_a_task() {
        let (_d, r) = fixture();
        submit(&r, "学习一种写法，命名为待定").unwrap();
        let result = run_with(&r, |_| {
            cancel(&r).unwrap();
            async { Ok(json!({"kind":"learn","sources":[]}).to_string()) }
        })
        .await;
        assert!(result.is_err());
        assert!(active(&r).unwrap().is_none());
        assert!(!r.workspace.join("agent-tasks").exists());
    }
    #[tokio::test]
    async fn applied_reply_recovers_before_parent_checkpoint_without_applying_twice() {
        let (_d, r) = fixture();
        submit(&r, "学习正文：# 甲\n\n甲文。\n\n# 乙\n\n乙文。").unwrap();
        run_with(&r,|_|async{Ok(json!({"tool":"clarify","topic":"选文","question":"甲还是乙？","reason":"需要范围"}).to_string())}).await.unwrap();
        submit(&r, "选第二篇").unwrap();
        let old = store::load(&r).unwrap();
        let mut state = old.state.clone();
        let action = reply::fast("选第二篇", &reply::view(&r).unwrap()).unwrap();
        state.pending.as_mut().unwrap().action = Some(action.clone());
        let saved = store::advance(&r, &old, state).unwrap();
        reply::apply(&r, saved.state.pending.as_ref().unwrap(), &action).unwrap();
        run_with(&r, |_| async { panic!("不能重复解释或重复问") })
            .await
            .unwrap();
        let s = learning_agent::load(&store::ws(&r).unwrap(), &active(&r).unwrap().unwrap())
            .unwrap()
            .state;
        assert_eq!(s.answers.len(), 1);
        assert_eq!(s.revision, 1);
    }
}
