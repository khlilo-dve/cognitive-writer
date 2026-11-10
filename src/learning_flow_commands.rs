//! Explicit candidate/version controls; no model is needed to save or inspect.
use crate::{
    error::AppError,
    learning_chat::ChatRef,
    learning_chat_store as store,
    learning_flow::{self, WriterInput},
};
pub(crate) fn command(r: &ChatRef, input: &str) -> Result<bool, AppError> {
    let input = input.trim();
    if matches!(input, "/preview" | "预览候选" | "看看候选") {
        learning_flow::display(r)?;
        return Ok(true);
    }
    let ws = store::ws(r)?;
    if let Some(name) = input.strip_prefix("/versions ") {
        for v in ws.versions(name.trim())? {
            println!(
                "{} v{}：{}篇；{}",
                v.name,
                v.number,
                v.candidate.input.articles.len(),
                v.status
            );
        }
        return Ok(true);
    }
    if let Some(args) = input.strip_prefix("/show ") {
        let (name, n) = args
            .rsplit_once(' ')
            .ok_or_else(|| store::error("用法 /show <名称> <版本>"))?;
        let n = n.parse().map_err(|_| store::error("版本须为整数"))?;
        println!("{}", ws.version(name.trim(), n)?.candidate.input.draft.spec);
        return Ok(true);
    }
    if let Some(args) = input.strip_prefix("/use-version ") {
        let (name, rest) = if let Some(quoted) = args.strip_prefix('"') {
            quoted
                .split_once('"')
                .ok_or_else(|| store::error("名称引号未闭合"))?
        } else {
            args.split_once(' ')
                .ok_or_else(|| store::error("需要名称、版本与写作要求"))?
        };
        let (number, request) = rest
            .trim()
            .split_once(' ')
            .ok_or_else(|| store::error("需要版本与写作要求"))?;
        let n = number.parse().map_err(|_| store::error("版本须为整数"))?;
        if request.trim().is_empty() {
            return Err(store::error("需要本次写作要求"));
        }
        let v = ws.version(name, n)?;
        let old = store::load(r)?;
        if old.state.pending.is_some() || old.state.paused {
            return Err(store::error("请先恢复或取消当前未完成输入"));
        }
        if let Some(id) = &old.state.active {
            if crate::learning_agent::load(&ws, id)?.state.status
                != crate::learning_agent::Status::ReadyForLearning
            {
                return Err(store::error("请先完成或取消当前材料任务"));
            }
        }
        let mut state = old.state.clone();
        state.selected_writer = Some(WriterInput {
            label: format!("{} v{}", v.name, v.number),
            spec: v.candidate.input.draft.spec,
            request: request.into(),
        });
        store::advance(r, &old, state)?;
        return Ok(true);
    }
    let saving = input
        .strip_prefix("/save ")
        .or_else(|| input.strip_prefix("保存为"))
        .or_else(|| input.strip_prefix("命名为"));
    let diff = matches!(input, "/diff" | "查看差异" | "看看变化");
    let writing = input.strip_prefix("/use-candidate ");
    if saving.is_none() && !diff && writing.is_none() {
        return Ok(false);
    }
    let old = store::load(r)?;
    if old.state.paused || old.state.pending.is_some() {
        return Err(store::error("请先恢复或取消未完成输入"));
    }
    let mut d = old
        .state
        .delivery
        .clone()
        .ok_or_else(|| store::error("尚无学习交付，请先完成学习"))?;
    learning_flow::current(r, &d)?;
    let id = d
        .candidate_id
        .clone()
        .ok_or_else(|| store::error("学习尚未生成候选，请继续学习"))?;
    if diff {
        let delta = ws.changes(&id)?;
        println!(
            "来源：{}→{}篇；新增{}篇\n理由：{}\n{}",
            delta.previous_articles,
            delta.current_articles,
            delta.added_source_hashes.len(),
            delta.reason,
            delta.text_diff
        );
        return Ok(true);
    }
    if let Some(name) = saving {
        let name = name
            .trim()
            .trim_matches(['：', ':', '“', '”', '"', '《', '》']);
        crate::learning_candidate::name(name)?;
        if d.saved.as_ref().is_some_and(|v| v.name != name) {
            return Err(store::error(
                "此候选已保存；另存不同名称请使用独立候选，未重复建版",
            ));
        }
        let guard =
            crate::learning_workspace_store::lock(&store::directory(r)?.join("state-lock"))?;
        if store::load(r)?.id != old.id {
            return Err(store::error("会话变化，未保存旧候选"));
        }
        learning_flow::current(r, &d)?;
        let version = ws.save(&id, name)?;
        d.save_name = Some(name.into());
        d.saved = Some(version.reference());
        drop(guard);
        println!("已保存：{} v{}（待审）", version.name, version.number);
    }
    let mut state = old.state.clone();
    state.delivery = Some(d);
    if let Some(request) = writing {
        if request.trim().is_empty() {
            return Err(store::error("需要本次写作要求"));
        }
        state.selected_writer = Some(WriterInput {
            label: "本次候选".into(),
            spec: ws.preview(&id)?.input.draft.spec,
            request: request.into(),
        });
    }
    store::advance(r, &old, state)?;
    Ok(true)
}
