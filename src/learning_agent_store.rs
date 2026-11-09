//! Immutable state chain with short compare-and-append locks; model waits never hold it.
use crate::{
    error::AppError,
    learning_agent_types::*,
    learning_candidate::{identity, valid_id},
    learning_workspace::LearningWorkspace,
    learning_workspace_store as store,
};
use std::path::{Path, PathBuf};
pub(crate) fn error(s: &str) -> AppError {
    AppError::LearningWorkspace(s.into())
}
pub(crate) fn directory(ws: &LearningWorkspace, id: &str) -> Result<PathBuf, AppError> {
    valid_id(id)?;
    Ok(ws.root().join("agent-tasks").join(id))
}
pub(crate) fn prepare(
    ws: &LearningWorkspace,
    delegate: Delegate,
    base: &Path,
    profile: crate::learning_job::CallProfile,
    prompt: String,
) -> Result<AgentPlan, AppError> {
    if !delegate.request.sources.is_empty() {
        crate::learning_sources::validate_request(&delegate.request)?;
    } else if delegate
        .request
        .expected_articles
        .is_some_and(|n| !(1..=10).contains(&n))
    {
        return Err(error("期望文章数须为1至10"));
    }
    if delegate.user_request.trim().is_empty()
        || prompt.trim().is_empty()
        || !(1..=1_000_000).contains(&delegate.budget_micro_cny)
        || profile.model != "mimo-v2.6-flash"
    {
        return Err(error(
            "需要学习目标、完整提示词、Flash配置及不超过1元的任务预算",
        ));
    }
    valid_id(&profile.endpoint_sha256)?;
    if let Some(name) = &delegate.save_name {
        crate::learning_candidate::name(name)?;
    }
    if let Some(base) = &delegate.base {
        if ws.version(&base.name, base.number)?.reference() != *base {
            return Err(error("委派基准身份不符"));
        }
    }
    let mut plan = AgentPlan {
        schema: 1,
        id: String::new(),
        method: METHOD.into(),
        created_at: chrono::Utc::now().to_rfc3339(),
        delegate,
        base_dir: std::fs::canonicalize(base)?,
        profile,
        prompt,
    };
    plan.id = identity(&plan)?;
    let missing = delegate_missing(&plan);
    let state = AgentState {
        task_id: plan.id.clone(),
        revision: 0,
        status: if missing {
            Status::WaitingInput
        } else {
            Status::Active
        },
        request: plan.delegate.request.clone(),
        user_request: plan.delegate.user_request.clone(),
        target_titles: plan.delegate.target_titles.clone(),
        all_articles: plan.delegate.all_articles,
        collection: None,
        inspected: vec![],
        inspected_windows: vec![],
        resolutions: Default::default(),
        authorized_selection: None,
        selected: vec![],
        questions: if missing {
            vec![Question {
                id: crate::voice_learning::sha256(format!("{}:missing-source", plan.id).as_bytes()),
                revision: 0,
                topic: "文章材料".into(),
                text: "请提供要学习的文章文件、文章网址或完整正文；多篇可以一起提供。".into(),
                reason: "已有学习目标，但尚无文章材料，无法开始学习".into(),
            }]
        } else {
            vec![]
        },
        answers: vec![],
        decision_steps: 0,
        no_progress: 0,
        pro_used: false,
        next_tier: Tier::Flash,
        pending: None,
        observation: serde_json::Value::Null,
        reason: if missing {
            "需要文章材料；本问题计入同一任务的两轮澄清上限".into()
        } else {
            String::new()
        },
    };
    let root = directory(ws, &plan.id)?;
    let _lock = store::lock(&root.join("state-lock"))?;
    store::publish_json(&root.join("plan.json"), &plan)?;
    publish(&root, None, 0, state)?;
    Ok(plan)
}
fn delegate_missing(p: &AgentPlan) -> bool {
    p.delegate.request.sources.is_empty()
}
pub(crate) fn plan(ws: &LearningWorkspace, id: &str) -> Result<AgentPlan, AppError> {
    let plan: AgentPlan = store::read(&directory(ws, id)?.join("plan.json"))?;
    if plan.schema != 1 || plan.id != id || identity(&plan)? != id {
        return Err(error("Agent委派身份损坏"));
    }
    Ok(plan)
}
pub(crate) fn load(ws: &LearningWorkspace, id: &str) -> Result<StateRecord, AppError> {
    plan(ws, id)?;
    let mut files = std::fs::read_dir(directory(ws, id)?.join("states"))?
        .map(|e| e.map(|e| e.path()))
        .collect::<Result<Vec<_>, _>>()?;
    files.retain(|p| p.extension().is_some_and(|s| s == "json"));
    files.sort();
    let mut prior: Option<StateRecord> = None;
    for (n, path) in files.iter().enumerate() {
        let r: StateRecord = store::read(path)?;
        if path.file_name().and_then(|s| s.to_str()) != Some(&format!("{n:08}.json"))
            || r.sequence != n as u32
            || r.state.task_id != id
            || identity(&r)? != r.id
            || r.previous.as_deref() != prior.as_ref().map(|p| p.id.as_str())
            || prior
                .as_ref()
                .is_some_and(|p| r.state.revision < p.state.revision)
        {
            return Err(error("Agent状态链损坏或断链"));
        }
        prior = Some(r);
    }
    prior.ok_or_else(|| error("Agent状态缺失，未静默重建任务"))
}
fn publish(
    root: &Path,
    previous: Option<String>,
    sequence: u32,
    state: AgentState,
) -> Result<StateRecord, AppError> {
    let mut r = StateRecord {
        id: String::new(),
        previous,
        sequence,
        state,
    };
    r.id = identity(&r)?;
    store::publish_json(&root.join("states").join(format!("{sequence:08}.json")), &r)?;
    Ok(r)
}
/// A late model/tool result observes the new revision but cannot apply its own state.
pub(crate) fn advance(
    ws: &LearningWorkspace,
    expected: &StateRecord,
    state: AgentState,
) -> Result<(StateRecord, bool), AppError> {
    let root = directory(ws, &expected.state.task_id)?;
    let _lock = store::lock(&root.join("state-lock"))?;
    let current = load(ws, &expected.state.task_id)?;
    if current.id != expected.id {
        return Ok((current, false));
    }
    Ok((
        publish(&root, Some(current.id), current.sequence + 1, state)?,
        true,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn state_chain_tampering_is_not_recovered_as_a_fresh_task() {
        let d = tempfile::tempdir().unwrap();
        let ws = LearningWorkspace::open(d.path()).unwrap();
        let p = prepare(
            &ws,
            Delegate {
                user_request: "学习".into(),
                request: crate::learning_sources::SourceRequest {
                    sources: vec![crate::learning_sources::SourceInput::Text {
                        label: "文".into(),
                        text: "正文。".into(),
                    }],
                    note: "学习".into(),
                    expected_articles: None,
                    allow_partial: false,
                },
                target_titles: vec![],
                all_articles: false,
                base: None,
                save_name: None,
                followup_request: None,
                budget_micro_cny: 250000,
            },
            d.path(),
            crate::learning_job::CallProfile {
                model: "mimo-v2.6-flash".into(),
                endpoint_sha256: "a".repeat(64),
            },
            "提示".into(),
        )
        .unwrap();
        let r = load(&ws, &p.id).unwrap();
        let mut next = r.state.clone();
        next.reason = "结果".into();
        let (next, applied) = advance(&ws, &r, next).unwrap();
        assert!(applied);
        assert!(!advance(&ws, &r, r.state.clone()).unwrap().1);
        std::fs::remove_file(directory(&ws, &p.id).unwrap().join("states/00000000.json")).unwrap();
        assert!(load(&ws, &p.id).is_err());
        assert_eq!(next.sequence, 1);
    }
}
