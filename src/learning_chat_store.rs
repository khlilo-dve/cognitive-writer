//! Immutable parent snapshots and a durable task link before domain execution.
use crate::{
    error::AppError,
    learning_candidate::{identity, valid_id},
    learning_chat_types::*,
    learning_workspace::LearningWorkspace,
    learning_workspace_store as io,
};
use std::path::{Path, PathBuf};
pub(crate) fn error(s: &str) -> AppError {
    AppError::LearningWorkspace(s.into())
}
pub(crate) fn directory(r: &ChatRef) -> Result<PathBuf, AppError> {
    valid_id(&r.id)?;
    Ok(r.workspace.join("chats").join(&r.id))
}
pub(crate) fn create(root: &Path) -> Result<ChatRef, AppError> {
    std::fs::create_dir_all(root)?;
    let mut profile = crate::learning_transport::LearningClient::from_env()?.profile();
    profile.model = "mimo-v2.6-flash".into();
    create_with(
        root,
        profile,
        std::fs::read_to_string("prompts/learning_chat.md")?,
        std::fs::read_to_string("prompts/learning_agent.md")?,
    )
}
pub(crate) fn create_with(
    root: &Path,
    profile: crate::learning_job::CallProfile,
    prompt: String,
    agent_prompt: String,
) -> Result<ChatRef, AppError> {
    std::fs::create_dir_all(root)?;
    let mut p = Plan {
        schema: 1,
        id: String::new(),
        created_at: chrono::Utc::now().to_rfc3339(),
        base_dir: std::env::current_dir()?,
        profile,
        prompt,
        agent_prompt,
    };
    valid_id(&p.profile.endpoint_sha256)?;
    if p.profile.model != "mimo-v2.6-flash"
        || p.prompt.trim().is_empty()
        || p.agent_prompt.trim().is_empty()
    {
        return Err(error("主会话需要Flash身份和完整提示词"));
    }
    p.id = identity(&p)?;
    let r = ChatRef {
        workspace: std::fs::canonicalize(root)?,
        id: p.id.clone(),
    };
    let dir = directory(&r)?;
    let _lock = io::lock(&dir.join("state-lock"))?;
    io::publish_json(&dir.join("plan.json"), &p)?;
    publish(
        &dir,
        None,
        0,
        State {
            delivery: None,
            selected_writer: None,
            active: None,
            paused: false,
            turn: 0,
            pending: None,
            event: Event::Idle,
        },
    )?;
    Ok(r)
}
pub(crate) fn plan(r: &ChatRef) -> Result<Plan, AppError> {
    let p: Plan = io::read(&directory(r)?.join("plan.json"))?;
    if p.id != r.id || p.schema != 1 || identity(&p)? != r.id {
        return Err(error("主会话计划身份损坏"));
    }
    Ok(p)
}
pub(crate) fn load(r: &ChatRef) -> Result<Record, AppError> {
    plan(r)?;
    let mut files = std::fs::read_dir(directory(r)?.join("states"))?
        .map(|e| e.map(|e| e.path()))
        .collect::<Result<Vec<_>, _>>()?;
    files.retain(|p| p.extension().is_some_and(|s| s == "json"));
    files.sort();
    let mut prior: Option<Record> = None;
    for (n, path) in files.iter().enumerate() {
        let v: Record = io::read(path)?;
        if path.file_name().and_then(|s| s.to_str()) != Some(&format!("{n:08}.json"))
            || v.sequence != n as u32
            || identity(&v)? != v.id
            || v.previous.as_deref() != prior.as_ref().map(|r| r.id.as_str())
        {
            return Err(error("主会话状态链损坏，未建立替代任务"));
        }
        prior = Some(v);
    }
    prior.ok_or_else(|| error("主会话状态缺失"))
}
fn publish(
    root: &Path,
    previous: Option<String>,
    sequence: u32,
    state: State,
) -> Result<Record, AppError> {
    let mut r = Record {
        id: String::new(),
        previous,
        sequence,
        state,
    };
    r.id = identity(&r)?;
    io::publish_json(&root.join("states").join(format!("{sequence:08}.json")), &r)?;
    Ok(r)
}
pub(crate) fn advance(r: &ChatRef, old: &Record, state: State) -> Result<Record, AppError> {
    let dir = directory(r)?;
    let _lock = io::lock(&dir.join("state-lock"))?;
    let current = load(r)?;
    if current.id != old.id {
        return Err(error("主会话已被取消或修订，旧动作未采用"));
    }
    publish(&dir, Some(old.id.clone()), old.sequence + 1, state)
}
pub(crate) fn ws(r: &ChatRef) -> Result<LearningWorkspace, AppError> {
    LearningWorkspace::open(&r.workspace)
}
pub(crate) fn binding(r: &ChatRef) -> Result<Option<Binding>, AppError> {
    let state = load(r)?.state;
    state
        .active
        .map(|id| {
            let s = crate::learning_agent::load(&ws(r)?, &id)?.state;
            Ok(Binding {
                task_id: id,
                revision: s.revision,
                question_id: if s.status == crate::learning_agent::Status::WaitingInput {
                    s.questions.last().map(|q| q.id.clone())
                } else {
                    None
                },
            })
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    #[test]
    fn damaged_parent_chain_is_not_recreated() {
        let (_d, r) = crate::learning_chat_types::tests::fixture();
        std::fs::remove_file(super::directory(&r).unwrap().join("states/00000000.json")).unwrap();
        assert!(super::load(&r).is_err());
        assert!(crate::learning_chat::submit(&r, "学习正文：短文").is_err());
        assert!(!r.workspace.join("agent-tasks").exists());
    }
}
