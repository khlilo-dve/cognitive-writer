//! Source policy gates are enforced outside model output.
use crate::{
    error::AppError,
    learning_agent_store::error,
    learning_agent_types::*,
    learning_sources::{SourceCollection, SourceInput, SourceStatus},
    learning_workspace::LearningWorkspace,
    voice_learning::sha256,
};
use std::collections::HashSet;
pub(crate) struct Entry {
    pub id: String,
    pub source_id: String,
    pub title: String,
    pub text: String,
    pub verified: bool,
}
fn provided(c: &SourceCollection, id: &str) -> bool {
    c.records.iter().position(|r| r.id == id).is_some_and(|i| {
        matches!(
            c.request.sources[i],
            SourceInput::File { .. } | SourceInput::Text { .. }
        )
    })
}
pub(crate) fn pool(c: &SourceCollection, s: &AgentState) -> Vec<Entry> {
    let mut out = Vec::new();
    for r in &c.records {
        let resolution = s.resolutions.get(&r.id);
        if resolution.is_some_and(|v| v.classification == Classification::NotArticle) {
            continue;
        }
        let approved = resolution.is_some_and(|v| v.classification == Classification::Article);
        if approved && r.candidates.len() > 1 && provided(c, &r.id) {
            if let Some(raw) = r.snapshots.first() {
                out.push(Entry {
                    id: format!("{}-whole", r.id),
                    source_id: r.id.clone(),
                    title: r
                        .candidates
                        .first()
                        .map(|a| a.title.clone())
                        .unwrap_or_default(),
                    text: raw.body.trim_start_matches('﻿').trim().into(),
                    verified: true,
                });
            }
        } else {
            for a in &r.candidates {
                out.push(Entry {
                    id: a.id.clone(),
                    source_id: r.id.clone(),
                    title: a.title.clone(),
                    text: a.text.clone(),
                    verified: r.status == SourceStatus::Extracted || approved,
                });
            }
        }
    }
    out
}
pub(crate) fn resolve(
    c: &SourceCollection,
    s: &mut AgentState,
    id: &str,
    classification: Classification,
    evidence: Vec<String>,
    reason: String,
) -> Result<bool, AppError> {
    let r = c
        .records
        .iter()
        .find(|r| r.id == id)
        .ok_or_else(|| error("来源ID不存在"))?;
    if !s.inspected.iter().any(|v| v == id)
        && !crate::learning_agent_tools::complete_preview_ids(c, s)
            .iter()
            .any(|v| v == id)
    {
        return Err(error(
            "此来源未完整出现在观察中；先inspect，再判断是否为文章",
        ));
    }
    if evidence.is_empty()
        || reason.trim().is_empty()
        || evidence.iter().any(|q| {
            q.trim().is_empty()
                || !r.candidates.iter().any(|a| a.text.contains(q))
                    && !r.snapshots.iter().any(|a| a.body.contains(q))
        })
    {
        return Err(error("材料判断需要该来源中可定位的证据与理由"));
    }
    if classification == Classification::Article
        && (r.candidates.is_empty() || (r.candidates.len() > 1 && !provided(c, id)))
    {
        return Err(error("没有可用正文或HTML含多文，不能靠判断合并成一篇"));
    }
    if classification == Classification::Article
        && r.candidates.len() > 1
        && (!s.target_titles.is_empty() || s.authorized_selection.is_some())
    {
        return Err(error("用户已有明确选文，不能合并整个文件来改变范围"));
    }
    let next = Resolution {
        classification,
        evidence,
        reason,
    };
    let changed = s.resolutions.get(id) != Some(&next);
    s.resolutions.insert(id.into(), next);
    Ok(changed)
}
fn key(text: &str) -> String {
    sha256(
        text.chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
            .as_bytes(),
    )
}
fn title_ids(c: &SourceCollection, s: &AgentState) -> Result<Vec<String>, AppError> {
    let entries = pool(c, s);
    let mut ids = Vec::new();
    for title in &s.target_titles {
        let found: Vec<_> = entries.iter().filter(|e| e.title == *title).collect();
        if found.len() != 1 {
            return Err(error("目标标题未唯一匹配，不能替用户选择"));
        }
        ids.push(found[0].id.clone());
    }
    Ok(ids)
}
pub(crate) fn selected_texts(
    ws: &LearningWorkspace,
    p: &AgentPlan,
    s: &AgentState,
    c: &SourceCollection,
    ids: &[String],
) -> Result<Vec<String>, AppError> {
    if ids.is_empty() {
        return Err(error("至少选择一篇真实正文"));
    }
    let entries = pool(c, s);
    let mut chosen = Vec::new();
    let mut seen = HashSet::new();
    for id in ids {
        let e = entries
            .iter()
            .find(|e| e.id == *id)
            .ok_or_else(|| error("选择包含不存在的正文ID"))?;
        if !e.verified {
            return Err(error("正文仍待内部识别，不能直接提交学习"));
        }
        if seen.insert(key(&e.text)) {
            chosen.push(e);
        }
    }
    if chosen.len() > 10 {
        return Err(error("选定独立正文超过10篇"));
    }
    let explicit = s.authorized_selection.as_ref();
    if let Some(allowed) = explicit {
        if ids.iter().any(|id| !allowed.contains(id)) {
            return Err(error("模型不能改变用户明确选择的正文集合"));
        }
    }
    if let Some(allowed) = explicit {
        if allowed.iter().any(|id| !ids.contains(id)) {
            return Err(error("模型不能丢弃用户明确选择的正文"));
        }
    }
    let titles = title_ids(c, s)?;
    if !titles.is_empty()
        && (titles.iter().any(|id| !ids.contains(id)) || ids.iter().any(|id| !titles.contains(id)))
    {
        return Err(error("选择未准确覆盖用户指定标题"));
    }
    let subset = explicit.is_some() || !titles.is_empty();
    for r in &c.records {
        let candidates: Vec<_> = entries.iter().filter(|e| e.source_id == r.id).collect();
        if candidates.len() > 1 && !subset && !s.all_articles {
            return Err(error("同页多文需要明确选文，不默认全部或第一篇"));
        }
        if !subset && !s.request.allow_partial && !chosen.iter().any(|e| e.source_id == r.id) {
            // Exact duplicate material is retained in provenance but counted once.
            if !r.candidates.iter().any(|a| seen.contains(&key(&a.text))) {
                return Err(error("有来源未纳入且未获准缩小范围"));
            }
        }
        if s.all_articles && !subset && candidates.iter().any(|e| !seen.contains(&key(&e.text))) {
            return Err(error("用户要求全部文章，不能遗漏正文候选"));
        }
    }
    let mut texts: Vec<String> = Vec::new();
    let mut old = HashSet::new();
    if let Some(base) = &p.delegate.base {
        let parent = ws.version(&base.name, base.number)?;
        if parent.reference() != *base
            || ws.versions(&base.name)?.last().map(|v| v.id.as_str()) != Some(base.id.as_str())
        {
            return Err(error("追加基准过期或身份变化"));
        }
        for t in parent.candidate.input.articles {
            old.insert(key(&t));
            texts.push(t);
        }
    }
    let fresh: Vec<_> = chosen
        .into_iter()
        .filter(|e| !old.contains(&key(&e.text)))
        .collect();
    if fresh.is_empty() {
        return Err(error("没有新的独立正文，未创建追加任务"));
    }
    if s.request
        .expected_articles
        .is_some_and(|n| n != fresh.len())
    {
        return Err(error("选择不满足用户明确的独立篇数，不能由模型改掉"));
    }
    if texts.len() + fresh.len() > 10 {
        return Err(error("追加后的完整来源超过10篇，旧来源不能删除"));
    }
    texts.extend(fresh.into_iter().map(|e| e.text.clone()));
    crate::learning_input_limits::validate(&texts)?;
    Ok(texts)
}
pub(crate) fn automatic(
    ws: &LearningWorkspace,
    p: &AgentPlan,
    s: &AgentState,
    c: &SourceCollection,
) -> Option<Vec<String>> {
    let ids = if let Some(ids) = &s.authorized_selection {
        ids.clone()
    } else if !s.target_titles.is_empty() {
        title_ids(c, s).ok()?
    } else {
        pool(c, s).into_iter().map(|e| e.id).collect()
    };
    selected_texts(ws, p, s, c, &ids).ok()?;
    Some(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        learning_agent as agent, learning_agent_tools as tools,
        learning_agent_types::tests::{fixture, question, reply},
    };
    #[tokio::test]
    async fn inspected_markdown_sections_can_be_one_article_without_a_user_question() {
        let f = fixture(
            "# 早晨\n\n我先整理桌面的笔记。\n\n# 午后\n\n同一份笔记还在整理。",
            Some(1),
        );
        let mut call = 0;
        let r = agent::run_with(&f.ws, &f.plan.id, |_| {
            call += 1;
            let action = if call == 1 {
                Decision::Inspect {
                    source_ids: vec!["s001".into()],
                    offset_chars: 0,
                    reason: "先读正文判断是否同一篇".into(),
                }
            } else {
                Decision::Resolve {
                    source_id: "s001".into(),
                    classification: Classification::Article,
                    evidence: vec!["同一份笔记还在整理".into()],
                    reason: "两个标题属于同一篇连续叙述".into(),
                }
            };
            async move { Ok(serde_json::to_string(&action).unwrap()) }
        })
        .await
        .unwrap();
        assert_eq!(r.state.status, Status::ReadyForLearning);
        assert_eq!(call, 2);
        assert!(r.state.questions.is_empty());
        assert!(agent::ready_texts(&f.ws, &f.plan.id).unwrap()[0].contains("# 午后"));
    }
    #[tokio::test]
    async fn model_cannot_choose_first_article_or_rewrite_count_but_explicit_answer_can() {
        let f = fixture("# 甲\n\n独立文章甲。\n\n# 乙\n\n独立文章乙。", None);
        let mut calls = 0;
        let waiting = agent::run_with(&f.ws, &f.plan.id, |_| {
            calls += 1;
            let d = if calls == 1 {
                Decision::Select {
                    article_ids: vec!["s001-a001".into()],
                    reason: "模型擅自选第一篇".into(),
                }
            } else {
                question("请明确选择文章范围")
            };
            async move { Ok(serde_json::to_string(&d).unwrap()) }
        })
        .await
        .unwrap();
        assert_eq!(waiting.state.status, Status::WaitingInput);
        assert_eq!(waiting.state.questions.len(), 1);
        let mut a = reply(&waiting, "选择乙");
        a.scope.article_ids = Some(vec!["s001-a002".into()]);
        a.scope.expected_articles = Some(1);
        agent::answer(&f.ws, &f.plan.id, a).unwrap();
        let ready = agent::run_with(&f.ws, &f.plan.id, |_| async {
            panic!("明确用户选择直接处理")
        })
        .await
        .unwrap();
        assert_eq!(ready.state.selected, vec!["s001-a002"]);
        assert_eq!(
            agent::ready_texts(&f.ws, &f.plan.id).unwrap(),
            vec!["# 乙\n\n独立文章乙。"]
        );
        let c = tools::collection(&f.ws, &f.plan, &ready.state).unwrap();
        let mut changed = ready.state.clone();
        changed.request.expected_articles = Some(2);
        assert!(selected_texts(&f.ws, &f.plan, &changed, &c, &changed.selected).is_err());
    }
    #[tokio::test]
    async fn no_progress_and_pro_escalation_have_hard_limits() {
        for upgrade in [false, true] {
            let f = fixture("# 甲\n\n甲文。\n\n# 乙\n\n乙文。", None);
            let mut tiers = Vec::new();
            let r = agent::run_with(&f.ws, &f.plan.id, |call| {
                tiers.push(call.tier);
                let d = if upgrade {
                    Decision::Escalate {
                        reason: "需要再次判断".into(),
                    }
                } else {
                    Decision::Inspect {
                        source_ids: vec!["s001".into()],
                        offset_chars: 0,
                        reason: "重复读取".into(),
                    }
                };
                async move { Ok(serde_json::to_string(&d).unwrap()) }
            })
            .await
            .unwrap();
            assert_eq!(r.state.status, Status::Blocked);
            assert_eq!(r.state.no_progress, 2);
            assert_eq!(tiers.len(), 3);
            assert_eq!(
                tiers.iter().filter(|t| **t == Tier::Pro).count(),
                usize::from(upgrade)
            );
        }
    }
    #[tokio::test]
    async fn twelve_fresh_inspections_cannot_run_forever() {
        let f = fixture(
            &format!(
                "# 甲\n\n{}\n\n# 乙\n\n{}",
                "不同段落甲。".repeat(100),
                "不同段落乙。".repeat(100)
            ),
            None,
        );
        let mut n = 0;
        let r = agent::run_with(&f.ws, &f.plan.id, |_| {
            let d = Decision::Inspect {
                source_ids: vec!["s001".into()],
                offset_chars: n,
                reason: "继续读新片段".into(),
            };
            n += 1;
            async move { Ok(serde_json::to_string(&d).unwrap()) }
        })
        .await
        .unwrap();
        assert_eq!(n, 12);
        assert_eq!(r.state.decision_steps, 12);
        assert_eq!(r.state.status, Status::Blocked);
    }
    #[tokio::test]
    async fn explicit_title_selection_cannot_be_replaced_with_a_whole_file() {
        let f = fixture("# 甲\n\n文章甲。\n\n# 乙\n\n文章乙。", None);
        let waiting = agent::run_with(&f.ws, &f.plan.id, |_| async {
            Ok(serde_json::to_string(&question("请选择一篇")).unwrap())
        })
        .await
        .unwrap();
        let c = tools::collection(&f.ws, &f.plan, &waiting.state).unwrap();
        let mut s = waiting.state.clone();
        s.target_titles = vec!["甲".into()];
        tools::inspect(&c, &mut s, vec!["s001".into()], 0).unwrap();
        assert!(
            resolve(
                &c,
                &mut s,
                "s001",
                Classification::Article,
                vec!["文章甲".into()],
                "把文件作为一篇".into()
            )
            .is_err()
        );
        assert_eq!(pool(&c, &s).len(), 2);
    }
    #[tokio::test]
    async fn append_cannot_drop_old_sources_to_fit_the_ten_article_limit() {
        use crate::{
            learning_candidate::{CandidateInput, DraftOrigin},
            voice_learning::{VoicePrompts, learn_voice_with},
        };
        let f = fixture("新材料。", None);
        let old: Vec<_> = (0..9)
            .map(|i| format!("旧原文{i}，不同的事实材料。"))
            .collect();
        let draft=learn_voice_with(&old,&VoicePrompts{analysis:"分析".into(),compile:"编译".into()},|stage,_,user|async move{
            if stage=="analysis"{let v:serde_json::Value=serde_json::from_str(&user).unwrap();Ok(serde_json::json!({"analysis":"各篇以材料形成判断","evidence":v["articles"].as_array().unwrap().iter().map(|a|serde_json::json!({"article":a["article"],"quote":a["text"]})).collect::<Vec<_>>()}).to_string())}else{Ok("依托已给材料组织判断，按语境保留限定和表达分寸。".into())}
        }).await.unwrap();
        let candidate =
            f.ws.stage(CandidateInput {
                articles: old,
                draft,
                origin: DraftOrigin::ModelOutput,
                note: "旧版本".into(),
                review: None,
                base: None,
            })
            .unwrap();
        let parent = f.ws.save(&candidate.id, "旧风格").unwrap();
        let mut p = f.plan.clone();
        p.delegate.base = Some(parent.reference());
        let mut state = agent::load(&f.ws, &f.plan.id).unwrap().state;
        state.request.sources = vec![
            crate::learning_sources::SourceInput::Text {
                label: "新甲".into(),
                text: "新的独立甲材料。".into(),
            },
            crate::learning_sources::SourceInput::Text {
                label: "新乙".into(),
                text: "新的独立乙材料。".into(),
            },
        ];
        state.request.expected_articles = Some(2);
        let c = crate::learning_sources::collect_with_reader(
            &f.ws,
            state.request.clone(),
            f._temp.path(),
            None,
        )
        .await
        .unwrap();
        let err = selected_texts(
            &f.ws,
            &p,
            &state,
            &c,
            &["s001-a001".into(), "s002-a001".into()],
        )
        .unwrap_err();
        assert!(err.to_string().contains("超过10篇"));
        assert_eq!(
            f.ws.version("旧风格", 1)
                .unwrap()
                .candidate
                .input
                .articles
                .len(),
            9
        );
        assert_eq!(f.ws.versions("旧风格").unwrap().len(), 1);
    }
}
