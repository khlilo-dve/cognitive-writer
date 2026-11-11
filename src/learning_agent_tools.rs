//! P3 source tools; snapshots are reused, and model observations disclose excerpt boundaries.
use crate::{
    error::AppError,
    learning_agent_scope as scope,
    learning_agent_store::{directory, error},
    learning_agent_types::*,
    learning_candidate::identity,
    learning_sources::{self, SourceCollection},
    learning_workspace::LearningWorkspace,
    learning_workspace_store as store,
};
use serde_json::{Value, json};
fn tool_root(
    ws: &LearningWorkspace,
    task: &str,
    step: &str,
) -> Result<std::path::PathBuf, AppError> {
    if step.is_empty()
        || step.len() > 64
        || !step.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err(error("工具步骤ID无效"));
    }
    Ok(directory(ws, task)?.join("tools").join(step))
}
pub(crate) fn collection(
    ws: &LearningWorkspace,
    p: &AgentPlan,
    s: &AgentState,
) -> Result<SourceCollection, AppError> {
    let r = s
        .collection
        .as_ref()
        .ok_or_else(|| error("尚未取得来源集合"))?;
    let source_ws = LearningWorkspace::open(&tool_root(ws, &p.id, &r.step)?.join("workspace"))?;
    let c = learning_sources::load(&source_ws, &r.id)?;
    Ok(c)
}
/// The step owns a collection workspace, so a published result survives a crash before state advance.
pub(crate) async fn collect(
    ws: &LearningWorkspace,
    p: &AgentPlan,
    s: &AgentState,
    step: &str,
) -> Result<CollectionRef, AppError> {
    let root = tool_root(ws, &p.id, step)?;
    let source_ws = LearningWorkspace::open(&root.join("workspace"))?;
    let dir = source_ws.root().join("source-collections");
    let existing = if dir.exists() {
        std::fs::read_dir(&dir)?
            .map(|e| e.map(|e| e.path()))
            .collect::<Result<Vec<_>, _>>()?
    } else {
        vec![]
    };
    let files: Vec<_> = existing
        .into_iter()
        .filter(|p| p.extension().is_some_and(|s| s == "json"))
        .collect();
    let c = if files.is_empty() {
        learning_sources::collect(&source_ws, s.request.clone(), &p.base_dir).await?
    } else {
        if files.len() != 1 {
            return Err(error("同一步骤出现多个采集结果，拒绝猜测"));
        }
        let id = files[0]
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| error("采集结果文件名无效"))?;
        learning_sources::load(&source_ws, id)?
    };
    if serde_json::to_value(&c.request).map_err(|e| error(&e.to_string()))?
        != serde_json::to_value(&s.request).map_err(|e| error(&e.to_string()))?
        || c.base_dir != p.base_dir
    {
        return Err(error("工具缓存与当前输入修订不符"));
    }
    let result = CollectionRef {
        id: c.id,
        step: step.into(),
    };
    let path = root.join("collection-ref.json");
    if path.exists() {
        let old: CollectionRef = store::read(&path)?;
        if old.id != result.id || old.step != result.step {
            return Err(error("采集工具回执损坏"));
        }
    } else {
        store::publish_json(&path, &result)?;
    }
    Ok(result)
}
fn excerpt(text: &str, offset: usize, limit: usize) -> Value {
    let total = text.chars().count();
    let value: String = text.chars().skip(offset).take(limit).collect();
    let consumed = value.chars().count();
    json!({"text":value,"offset_chars":offset,"excerpted":offset>0 || offset.saturating_add(consumed)<total,"end_chars":offset+consumed})
}
pub(crate) fn describe(
    c: &SourceCollection,
    s: &AgentState,
    detailed: &[String],
    offset: usize,
) -> Value {
    let mut budget = 16000usize;
    let records:Vec<_>=c.records.iter().map(|r|{
        let full=detailed.contains(&r.id);let limit=if full{8000}else{160};
        let candidates:Vec<_>=r.candidates.iter().map(|a|{let take=limit.min(budget);let value=excerpt(&a.text,if full{offset}else{0},take);budget=budget.saturating_sub(value["text"].as_str().unwrap_or("").chars().count());json!({"id":a.id,"title":a.title,"body":value,"utf8_bytes":a.text.len(),"duplicate_of":a.duplicate_of})}).collect();
        let raw=if full && r.candidates.is_empty(){r.snapshots.first().map(|a|{let value=excerpt(&a.body,offset,4000.min(budget));budget=budget.saturating_sub(value["text"].as_str().unwrap_or("").chars().count());value})}else{None};
        json!({"id":r.id,"status":r.status,"reason":r.reason,"candidates":candidates,"raw_page_excerpt":raw,"links":r.links.iter().take(32).collect::<Vec<_>>(),"links_omitted":r.links.len()>32,"resolution":s.resolutions.get(&r.id)})
    }).collect();
    json!({"source_collection_id":c.id,"learning_input_budget_bytes":crate::learning_input_limits::MAX_TEXT_BYTES,"sources":records,"current_pool":scope::pool(c,s).iter().map(|e|json!({"id":e.id,"source_id":e.source_id,"title":e.title,"verified":e.verified})).collect::<Vec<_>>()})
}
/// A short source already fully present in the observation needs no second read call.
pub(crate) fn complete_preview_ids(c: &SourceCollection, s: &AgentState) -> Vec<String> {
    describe(c, s, &[], 0)["sources"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| {
            r["candidates"]
                .as_array()
                .is_some_and(|a| !a.is_empty() && a.iter().all(|v| v["body"]["excerpted"] == false))
        })
        .filter_map(|r| r["id"].as_str().map(str::to_string))
        .collect()
}
pub(crate) fn inspect(
    c: &SourceCollection,
    s: &mut AgentState,
    ids: Vec<String>,
    offset: usize,
) -> Result<bool, AppError> {
    let ids = if ids.is_empty() {
        c.records.iter().map(|r| r.id.clone()).collect()
    } else {
        ids
    };
    if ids.is_empty()
        || ids.len() > 4
        || ids.iter().any(|id| !c.records.iter().any(|r| r.id == *id))
    {
        return Err(error("inspect每次须指定1–4个真实来源ID"));
    }
    if ids.iter().all(|id| {
        c.records.iter().find(|r| r.id == *id).is_none_or(|r| {
            r.candidates
                .iter()
                .all(|a| a.text.chars().count() <= offset)
                && r.snapshots.iter().all(|a| a.body.chars().count() <= offset)
        })
    }) {
        return Err(error("请求位置没有可读材料，未将空片段当新进展"));
    }
    let mut progress = false;
    for id in &ids {
        if !s.inspected.contains(id) {
            s.inspected.push(id.clone());
        }
        let window = format!("{id}@{offset}");
        if !s.inspected_windows.contains(&window) {
            s.inspected_windows.push(window);
            progress = true;
        }
    }
    s.observation = describe(c, s, &ids, offset);
    Ok(progress)
}
pub(crate) fn observation(
    p: &AgentPlan,
    s: &AgentState,
    c: &SourceCollection,
) -> Result<String, AppError> {
    serde_json::to_string(&json!({"task_id":p.id,"revision":s.revision,"user_request":s.user_request,"target_titles":s.target_titles,"all_articles":s.all_articles,"expected_articles":s.request.expected_articles,"allow_partial":s.request.allow_partial,"authorized_selection":s.authorized_selection,"base":p.delegate.base,"questions":s.questions,"answers":s.answers.iter().map(|a|json!({"question_id":a.reply.question_id,"text":a.reply.text,"user_request":a.reply.user_request,"scope":{"article_ids":a.reply.scope.article_ids,"target_titles":a.reply.scope.target_titles,"all_articles":a.reply.scope.all_articles,"expected_articles":a.reply.scope.expected_articles,"clear_expected_articles":a.reply.scope.clear_expected_articles,"allow_partial":a.reply.scope.allow_partial,"replacement_source_ids":a.reply.scope.replacements.iter().map(|r|&r.source_id).collect::<Vec<_>>(),"added_sources":a.reply.scope.additions.len()}})).collect::<Vec<_>>(),"fully_visible_sources":complete_preview_ids(c,s),"inspected_sources":s.inspected,"inspected_windows":s.inspected_windows,"steps_used":s.decision_steps,"clarifications_used":s.questions.len(),"pro_used":s.pro_used,"source_summary":describe(c,s,&[],0),"last_tool_result":s.observation,"last_reason":s.reason})).map_err(|e|error(&e.to_string()))
}
pub(crate) fn reply(
    ws: &LearningWorkspace,
    id: &str,
    reply: Reply,
) -> Result<StateRecord, AppError> {
    apply_reply(ws, id, reply, false)
}
pub(crate) fn revise(
    ws: &LearningWorkspace,
    id: &str,
    reply: Reply,
) -> Result<StateRecord, AppError> {
    apply_reply(ws, id, reply, true)
}
fn apply_reply(
    ws: &LearningWorkspace,
    id: &str,
    reply: Reply,
    revise: bool,
) -> Result<StateRecord, AppError> {
    let plan = crate::learning_agent_store::plan(ws, id)?;
    let current = crate::learning_agent_store::load(ws, id)?;
    let mut s = current.state.clone();
    let hash = identity(&reply)?;
    if s.answers.iter().any(|a| a.id == hash) {
        return Ok(current);
    }
    if s.revision != reply.expected_revision
        || !(matches!(s.status, Status::WaitingInput | Status::Blocked)
            || revise
                && s.status != Status::Cancelled
                && reply.question_id.is_none()
                && (reply.user_request.is_some()
                    || !reply.scope.replacements.is_empty()
                    || !reply.scope.additions.is_empty()
                    || reply.scope.article_ids.is_some()
                    || reply.scope.target_titles.is_some()))
        || reply.text.trim().is_empty()
    {
        return Err(error("回复不是当前等待修订，或没有补充内容"));
    }
    match &reply.question_id {
        Some(q) => {
            if s.answers
                .iter()
                .any(|a| a.reply.question_id.as_ref() == Some(q))
                || s.questions.last().map(|a| &a.id) != Some(q)
            {
                return Err(error("该问题已回答或不是当前问题"));
            }
        }
        None if s.status != Status::Blocked && !revise => {
            return Err(error("等待回答时需要当前question_id"));
        }
        None => {}
    }
    let patch = &reply.scope;
    if patch.clear_expected_articles && patch.expected_articles.is_some() {
        return Err(error("不能同时清除和设置篇数"));
    }
    if let Some(value) = patch.allow_partial {
        s.request.allow_partial = value;
    }
    if patch.clear_expected_articles {
        s.request.expected_articles = None;
    }
    if let Some(value) = patch.expected_articles {
        s.request.expected_articles = Some(value);
    }
    if let Some(value) = &patch.target_titles {
        s.target_titles = value.clone();
    }
    if let Some(value) = patch.all_articles {
        s.all_articles = value;
    }
    let changed_inputs = !patch.replacements.is_empty() || !patch.additions.is_empty();
    if changed_inputs && patch.article_ids.is_some() {
        return Err(error("补充来源后须重新取得候选，不能同时引用旧正文ID"));
    }
    for r in &patch.replacements {
        let c = collection(ws, &plan, &s)?;
        let index = c
            .records
            .iter()
            .position(|a| a.id == r.source_id)
            .ok_or_else(|| error("待替换来源不存在"))?;
        s.request.sources[index] = r.input.clone();
    }
    s.request.sources.extend(patch.additions.clone());
    if !s.request.sources.is_empty() {
        crate::learning_sources::validate_request(&s.request)?;
    }
    if let Some(goal) = &reply.user_request {
        if goal.trim().is_empty() {
            return Err(error("新目标不能为空"));
        }
        s.user_request = goal.clone();
        s.authorized_selection = None;
    }
    if changed_inputs {
        s.collection = None;
        s.inspected.clear();
        s.inspected_windows.clear();
        s.resolutions.clear();
        s.authorized_selection = None;
    }
    if patch.article_ids.is_some() || patch.target_titles.is_some() {
        if let Some(reference) = &s.collection {
            let _ = reference;
            let c = collection(ws, &plan, &s)?;
            s.resolutions.retain(|id, _| {
                c.records
                    .iter()
                    .find(|r| r.id == *id)
                    .is_none_or(|r| r.candidates.len() <= 1)
            });
        }
    }
    if let Some(ids) = &patch.article_ids {
        s.authorized_selection = Some(ids.clone());
    }
    s.revision += 1;
    s.no_progress = 0;
    s.status = Status::Active;
    s.pending = None;
    s.selected.clear();
    s.observation = Value::Null;
    s.reason = "用户补充已保留，继续当前任务；追问与决策次数未清零".into();
    s.answers.push(Answer { id: hash, reply });
    let (next, _) = crate::learning_agent_store::advance(ws, &current, s)?;
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        learning_agent as agent,
        learning_agent_types::tests::{fixture, question, reply as user_reply},
        learning_sources::SourceInput,
    };
    #[tokio::test]
    async fn complete_short_observation_allows_resolution_but_partial_preview_does_not() {
        for (body, ready) in [
            (
                "# 早晨\n\n同一份笔记。\n\n# 午后\n\n还是这份笔记。".to_string(),
                true,
            ),
            (
                format!(
                    "# 早晨\n\n{}\n\n# 午后\n\n仍然继续。",
                    "同一份笔记。".repeat(100)
                ),
                false,
            ),
        ] {
            let f = fixture(&body, Some(1));
            let mut count = 0;
            let r = agent::run_with(&f.ws, &f.plan.id, |call| {
                count += 1;
                let user: Value = serde_json::from_str(&call.user).unwrap();
                let raw = if count == 1 {
                    assert_eq!(user["fully_visible_sources"].as_array().unwrap().contains(&json!("s001")), ready);
                    json!({"tool":"resolve","source_id":"s001","classification":"article","evidence":["同一份笔记"],"reason":"连续文章"})
                } else {
                    assert!(!ready && user["last_tool_result"]["tool_error"].as_str().unwrap().contains("先inspect"));
                    json!({"tool":"stop","reason":"夹具保留未读取状态"})
                };
                async move {Ok(raw.to_string())}
            }).await.unwrap();
            assert_eq!(r.state.status == Status::ReadyForLearning, ready);
        }
    }
    #[tokio::test]
    async fn published_source_result_survives_missing_tool_receipt_and_original_file() {
        let f = fixture("初始正文。", None);
        let mut s = agent::load(&f.ws, &f.plan.id).unwrap().state;
        std::fs::write(f._temp.path().join("source.md"), "冻结原文。").unwrap();
        s.request.sources = vec![SourceInput::File {
            path: "source.md".into(),
        }];
        let first = collect(&f.ws, &f.plan, &s, "r0-collect").await.unwrap();
        std::fs::remove_file(f._temp.path().join("source.md")).unwrap();
        std::fs::remove_file(
            tool_root(&f.ws, &f.plan.id, "r0-collect")
                .unwrap()
                .join("collection-ref.json"),
        )
        .unwrap();
        let again = collect(&f.ws, &f.plan, &s, "r0-collect").await.unwrap();
        assert_eq!(first.id, again.id);
        s.request.note = "输入被改，不可复用旧步骤".into();
        assert!(collect(&f.ws, &f.plan, &s, "r0-collect").await.is_err());
    }
    #[tokio::test]
    async fn source_replacement_creates_new_revision_without_resetting_decision_or_question_budget()
    {
        let f = fixture("# 甲\n\n甲文章。\n\n# 乙\n\n乙文章。", None);
        let waiting = agent::run_with(&f.ws, &f.plan.id, |_| async {
            Ok(serde_json::to_string(&question("请选范围")).unwrap())
        })
        .await
        .unwrap();
        let old = waiting.state.collection.as_ref().unwrap().id.clone();
        let mut answer = user_reply(&waiting, "改用这篇完整正文");
        answer.scope.replacements = vec![Replacement {
            source_id: "s001".into(),
            input: SourceInput::Text {
                label: "替代文章".into(),
                text: "补充的完整正文。".into(),
            },
        }];
        let mut stale = answer.clone();
        stale.expected_revision = 9;
        assert!(reply(&f.ws, &f.plan.id, stale).is_err());
        reply(&f.ws, &f.plan.id, answer).unwrap();
        let result = agent::run_with(&f.ws, &f.plan.id, |_| async {
            panic!("新材料范围已清楚")
        })
        .await
        .unwrap();
        assert_eq!(result.state.revision, 1);
        assert_eq!(result.state.questions.len(), 1);
        assert_eq!(result.state.decision_steps, 1);
        assert_ne!(result.state.collection.as_ref().unwrap().id, old);
        assert_eq!(
            agent::ready_texts(&f.ws, &f.plan.id).unwrap(),
            vec!["补充的完整正文。"]
        );
    }
    #[tokio::test]
    async fn ambiguous_collect_cache_cannot_trigger_an_infinite_zero_model_loop() {
        let f = fixture("初始正文。", None);
        let s = agent::load(&f.ws, &f.plan.id).unwrap().state;
        collect(&f.ws, &f.plan, &s, "r0-collect").await.unwrap();
        let root = tool_root(&f.ws, &f.plan.id, "r0-collect").unwrap();
        let source_ws = LearningWorkspace::open(&root.join("workspace")).unwrap();
        let mut other = s.request.clone();
        other.note = "另一份来源请求".into();
        learning_sources::collect(&source_ws, other, &f.plan.base_dir)
            .await
            .unwrap();
        let result = agent::run_with(&f.ws, &f.plan.id, |_| async {
            panic!("采集错误不能调用模型循环")
        })
        .await
        .unwrap();
        assert_eq!(result.state.status, Status::Blocked);
        assert_eq!(result.state.no_progress, 2);
    }
}
