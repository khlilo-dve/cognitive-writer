//! Ground user choices in the current domain task; never create another question budget.
use super::learning_chat_parse as parse;
use crate::{
    error::AppError,
    learning_agent::{self, ScopePatch, Status},
    learning_agent_scope, learning_agent_store, learning_agent_tools,
    learning_chat_store::{self as store, error},
    learning_chat_types::*,
};
use serde_json::{Value, json};
pub(crate) fn view(r: &ChatRef) -> Result<Value, AppError> {
    let Some(binding) = store::binding(r)? else {
        return Ok(Value::Null);
    };
    let ws = store::ws(r)?;
    let p = learning_agent_store::plan(&ws, &binding.task_id)?;
    let s = learning_agent::load(&ws, &binding.task_id)?.state;
    let candidates = if s.collection.is_some() {
        learning_agent_scope::pool(&learning_agent_tools::collection(&ws,&p,&s)?,&s).into_iter().enumerate().map(|(i,e)|json!({"number":i+1,"id":e.id,"title":e.title,"preview":e.text.chars().take(160).collect::<String>()})).collect::<Vec<_>>()
    } else {
        vec![]
    };
    Ok(
        json!({"task_id":binding.task_id,"revision":s.revision,"status":s.status,"user_request":s.user_request,"expected_articles":s.request.expected_articles,"questions":s.questions,"answers_count":s.answers.len(),"candidates":candidates,"sources":s.request.sources.iter().enumerate().map(|(i,_)|format!("s{:03}",i+1)).collect::<Vec<_>>()}),
    )
}
fn positive_reference(s: &str, term: &str) -> bool {
    s.match_indices(term).any(|(at, _)| {
        let prefix = s[..at]
            .rsplit(['，', ',', '。', '；', ';', '、'])
            .next()
            .unwrap_or("")
            .rsplit("而是")
            .next()
            .unwrap_or("");
        ![
            "不要",
            "不选",
            "别选",
            "不是",
            "排除",
            "不想",
            "不学",
            "不保留",
        ]
        .iter()
        .any(|w| prefix.contains(w))
    })
}
fn ordinals(s: &str) -> Vec<usize> {
    let mut out = Vec::new();
    for (i, w) in ["一", "二", "三", "四", "五", "六", "七", "八", "九", "十"]
        .iter()
        .enumerate()
    {
        if positive_reference(s, &format!("第{w}篇"))
            || positive_reference(s, &format!("第{}篇", i + 1))
        {
            out.push(i + 1);
        }
    }
    if out.is_empty() {
        if let Ok(n) = s.trim().parse::<usize>() {
            if (1..=10).contains(&n) {
                out.push(n);
            }
        }
    }
    out
}
pub(crate) fn fast(input: &str, v: &Value) -> Option<Action> {
    let candidates = v["candidates"].as_array()?;
    let header = parse::control(input).0;
    let mut ids = Vec::new();
    if !["没决定", "未决定", "不知道", "不确定"]
        .iter()
        .any(|w| header.contains(w))
    {
        let numbers = ordinals(header);
        if !numbers.is_empty() {
            for n in numbers {
                if let Some(c) = candidates.get(n - 1) {
                    ids.push(c["id"].as_str()?.into());
                }
            }
        } else {
            for c in candidates {
                let id = c["id"].as_str()?;
                let title = c["title"].as_str()?;
                if positive_reference(header, id)
                    || !title.is_empty() && positive_reference(header, title)
                {
                    ids.push(id.into());
                }
            }
        }
    }
    let mut scope = ScopePatch::default();
    if !ids.is_empty() {
        scope.article_ids = Some(ids);
        return Some(Action::Reply { scope, goal: None });
    }
    if matches!(
        header.trim(),
        "我还没决定" | "还没决定" | "暂时没有" | "不确定" | "不知道" | "还没找到文章"
    ) {
        return Some(Action::Reply { scope, goal: None });
    }
    let sources = parse::sources(input);
    if !sources.is_empty() {
        let old = v["sources"].as_array()?;
        if old.len() == 1
            && (header.contains("改用")
                || candidates.is_empty()
                    && !["再加", "加上", "补充一篇"]
                        .iter()
                        .any(|w| header.contains(w)))
        {
            scope.replacements = vec![crate::learning_agent_types::Replacement {
                source_id: old[0].as_str()?.into(),
                input: sources[0].clone(),
            }];
            if sources.len() > 1 {
                scope.additions = sources[1..].to_vec();
            }
        } else if old.is_empty()
            || ["补充", "加上", "再加", "提供"]
                .iter()
                .any(|w| header.contains(w))
            || header.trim().starts_with('/')
            || header.trim().starts_with("http")
        {
            scope.additions = sources;
        } else {
            return None;
        }
        return Some(Action::Reply { scope, goal: None });
    }
    if ["目标改为", "改学", "改成学习"]
        .iter()
        .any(|w| header.contains(w))
    {
        return Some(Action::Reply {
            scope,
            goal: Some(input.into()),
        });
    }
    None
}
pub(crate) fn validate(action: &Action, input: &str, v: &Value) -> Result<(), AppError> {
    let Action::Reply { scope, goal } = action else {
        return Err(error("当前等待回复只能返回reply，不能替换为新学习任务"));
    };
    let candidates = v["candidates"]
        .as_array()
        .ok_or_else(|| error("缺少当前候选"))?;
    if let Some(ids) = &scope.article_ids {
        if ids.is_empty()
            || ["没决定", "未决定", "不知道", "不确定"]
                .iter()
                .any(|w| input.contains(w))
        {
            return Err(error("回复没有明确选文，不能替用户猜测"));
        }
        for id in ids {
            let c = candidates
                .iter()
                .find(|c| c["id"] == *id)
                .ok_or_else(|| error("解释器选择了不存在的正文"))?;
            let title = c["title"].as_str().unwrap_or("");
            let chars: Vec<_> = title.chars().collect();
            let supported = positive_reference(input, id)
                || !title.is_empty() && positive_reference(input, title)
                || ordinals(input)
                    .iter()
                    .any(|n| candidates.get(n - 1).is_some_and(|v| v["id"] == *id))
                || chars
                    .windows(3)
                    .any(|s| positive_reference(input, &s.iter().collect::<String>()));
            if !supported {
                return Err(error("选文缺少可定位的编号或标题指代，未替用户选择"));
            }
        }
    }
    if scope.clear_expected_articles
        && !["不限篇数", "不限定篇数", "不指定数量"]
            .iter()
            .any(|w| input.contains(w))
        || scope.allow_partial == Some(false)
            && !["不允许部分", "全部必须", "缺一不可"]
                .iter()
                .any(|w| input.contains(w))
        || scope.all_articles == Some(false)
            && !["不要全部", "不学全部", "取消全部"]
                .iter()
                .any(|w| input.contains(w))
    {
        return Err(error("不能无用户明确指令清除篇数或更改已给范围"));
    }
    parse::controls(
        input,
        scope.expected_articles,
        scope.allow_partial == Some(true),
        scope.all_articles == Some(true),
        scope.target_titles.as_deref().unwrap_or(&[]),
    )?;
    for r in &scope.replacements {
        if !v["sources"]
            .as_array()
            .is_some_and(|v| v.iter().any(|s| s == &r.source_id))
        {
            return Err(error("替换来源不存在"));
        }
        parse::validate_source(&r.input, input)?;
    }
    for s in &scope.additions {
        parse::validate_source(s, input)?;
    }
    if goal.as_ref().is_some_and(|g| {
        g != input
            || !["目标改为", "改学", "改成学习"]
                .iter()
                .any(|w| input.contains(w))
    }) {
        return Err(error("目标修改未对应用户明确原话"));
    }
    Ok(())
}
pub(crate) fn apply(r: &ChatRef, turn: &Turn, action: &Action) -> Result<(), AppError> {
    let Some(binding) = &turn.binding else {
        return Err(error("回复缺少原任务身份"));
    };
    let ws = store::ws(r)?;
    let s = learning_agent::load(&ws, &binding.task_id)?.state;
    if store::load(r)?.state.active.as_deref() != Some(&binding.task_id) {
        return Err(error("当前学习任务已变，旧回复未采用"));
    }
    let Action::Reply { scope, goal } = action else {
        return Err(error("不是回复动作"));
    };
    let reply = learning_agent::Reply {
        expected_revision: binding.revision,
        question_id: binding.question_id.clone(),
        text: turn.input.clone(),
        user_request: goal.clone(),
        scope: scope.clone(),
    };
    if s.revision != binding.revision {
        let hash = crate::learning_candidate::identity(&reply)?;
        if s.answers.iter().any(|a| a.id == hash) {
            return Ok(());
        }
        return Err(error("当前学习修订已变，旧回复未采用"));
    }
    if matches!(s.status, Status::WaitingInput | Status::Blocked) {
        learning_agent::answer(&ws, &binding.task_id, reply)?;
    } else {
        learning_agent::revise(&ws, &binding.task_id, reply)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unsupported_choice_and_control_invention_are_rejected() {
        let v = json!({"candidates":[{"id":"a1","title":"纸上候车"},{"id":"a2","title":"给团队的项目备忘"}],"sources":["s001"]});
        let action: Action =
            serde_json::from_value(json!({"kind":"reply","scope":{"article_ids":["a2"]}})).unwrap();
        assert!(validate(&action, "我没决定", &v).is_err());
        assert!(validate(&action, "保留那篇项目备忘的", &v).is_ok());
        assert!(validate(&action, "随便", &v).is_err());
        let action: Action =
            serde_json::from_value(json!({"kind":"reply","scope":{"allow_partial":true}})).unwrap();
        assert!(validate(&action, "继续", &v).is_err());
    }
    #[test]
    fn negated_and_multiple_references_preserve_the_users_actual_set() {
        let v = json!({"candidates":[{"id":"a1","title":"纸上候车"},{"id":"a2","title":"项目备忘"}],"sources":["s001"]});
        for text in [
            "不要第一篇，选第二篇",
            "不是第一篇而是第二篇",
            "不要纸上候车，选项目备忘",
        ] {
            let action = fast(text, &v).unwrap();
            let Action::Reply { scope, .. } = &action else {
                panic!()
            };
            assert_eq!(scope.article_ids.as_ref().unwrap(), &vec!["a2".to_string()]);
            validate(&action, text, &v).unwrap();
        }
        let action = fast("第一篇和第二篇", &v).unwrap();
        let Action::Reply { scope, .. } = action else {
            panic!()
        };
        assert_eq!(scope.article_ids.unwrap(), vec!["a1", "a2"]);
        let bad: Action =
            serde_json::from_value(json!({"kind":"reply","scope":{"article_ids":["a1"]}})).unwrap();
        assert!(validate(&bad, "不要第一篇，选第二篇", &v).is_err());
    }
}
