//! Development commands for P3; main conversation routing and stdin stay in P4.
use crate::{
    error::AppError, learning_agent as agent, learning_agent_model, learning_agent_store,
    learning_agent_types::*, learning_source_fetch::read_limited,
    learning_transport::LearningClient, learning_workspace::LearningWorkspace,
};
use std::{io::Write, path::Path};
fn show(record: &StateRecord) -> Result<(), AppError> {
    let s = &record.state;
    println!(
        "agent_task_id: {}\n状态：{:?}\nrevision: {}\n决策：{}/12；主动澄清：{}/2；Pro升级：{}\n{}",
        s.task_id,
        s.status,
        s.revision,
        s.decision_steps,
        s.questions.len(),
        s.pro_used,
        s.reason
    );
    if s.status == Status::WaitingInput {
        let q = s
            .questions
            .last()
            .ok_or_else(|| learning_agent_store::error("等待态缺少问题"))?;
        println!("question_id: {}\n{}\n原因：{}", q.id, q.text, q.reason);
    }
    if let Some(issue) = s
        .observation
        .get("tool_error")
        .and_then(serde_json::Value::as_str)
    {
        println!("具体工具问题：{issue}");
    }
    if let Some(reference) = &s.collection {
        println!("collection_id: {}", reference.id);
    }
    if s.status == Status::ReadyForLearning {
        println!(
            "已选正文ID：{}\n本阶段未生成风格，后续学习／保存／Writer交接由P5接入。",
            s.selected.join(", ")
        );
    }
    Ok(())
}
async fn run(ws: &LearningWorkspace, id: &str, retry: bool) -> Result<(), AppError> {
    println!("agent_task_id: {id}");
    std::io::stdout().flush()?;
    let r = tokio::select! {
        result=agent::run_with(ws,id,|call|learning_agent_model::call(ws,id,call,retry))=>result,
        _=tokio::signal::ctrl_c()=>return Err(learning_agent_store::error("Agent等待已中断，任务与请求记录保留；可agent-resume恢复，不自动重发未知请求")),
    }?;
    show(&r)
}
pub(crate) async fn start(ws: &LearningWorkspace, file: &Path) -> Result<(), AppError> {
    let path = std::fs::canonicalize(file)?;
    let delegate: Delegate = serde_json::from_slice(&read_limited(&path)?).map_err(|_| {
        learning_agent_store::error("委派JSON无效，需要user_request和request（SourceRequest）")
    })?;
    let mut profile = LearningClient::from_env()?.profile();
    profile.model = Tier::Flash.model().into();
    let prompt = std::fs::read_to_string("prompts/learning_agent.md")?;
    let plan = agent::prepare(
        ws,
        delegate,
        path.parent()
            .ok_or_else(|| learning_agent_store::error("委派缺少目录"))?,
        profile,
        prompt,
    )?;
    run(ws, &plan.id, false).await
}
pub(crate) async fn resume(ws: &LearningWorkspace, id: &str, retry: bool) -> Result<(), AppError> {
    run(ws, id, retry).await
}
pub(crate) fn inspect(ws: &LearningWorkspace, id: &str) -> Result<(), AppError> {
    show(&agent::load(ws, id)?)
}
pub(crate) async fn answer(ws: &LearningWorkspace, id: &str, file: &Path) -> Result<(), AppError> {
    let reply: Reply = serde_json::from_slice(&read_limited(file)?).map_err(|_| {
        learning_agent_store::error(
            "回复JSON无效，需要expected_revision/question_id/text和可选scope",
        )
    })?;
    let state = agent::answer(ws, id, reply)?;
    if state.state.status == Status::Active {
        run(ws, id, false).await
    } else {
        show(&state)
    }
}
pub(crate) fn cancel(ws: &LearningWorkspace, id: &str) -> Result<(), AppError> {
    show(&agent::cancel(ws, id)?)
}
