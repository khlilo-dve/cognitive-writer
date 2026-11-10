//! Durable P5 bridge: prepared material -> existing learning job -> candidate/version -> Writer.
use crate::{
    error::AppError,
    learning_agent,
    learning_candidate::VersionRef,
    learning_chat::ChatRef,
    learning_chat_store as store,
    learning_job::{self, JobPrompts},
    learning_workspace_store as io,
};
use serde::{Deserialize, Serialize};
use std::{future::Future, path::PathBuf};
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Delivery {
    pub task_id: String,
    pub revision: u32,
    pub job_id: String,
    pub candidate_id: Option<String>,
    pub save_name: Option<String>,
    pub saved: Option<VersionRef>,
    pub writer: Option<WriterInput>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriterInput {
    pub label: String,
    pub spec: String,
    pub request: String,
}
#[derive(Debug, Clone)]
pub struct Written {
    pub label: String,
    pub spec: String,
    pub request: String,
    pub outline: String,
    pub fulltext: String,
}
/// A transport request whose owner journal is fixed before sending. No credentials are serialized.
#[derive(Clone)]
pub struct Call {
    pub root: PathBuf,
    pub profile: crate::learning_job::CallProfile,
    pub stage: String,
    pub system: String,
    pub user: String,
}
pub(crate) fn current(r: &ChatRef, d: &Delivery) -> Result<(), AppError> {
    let b = store::binding(r)?.ok_or_else(|| store::error("学习已取消，旧交付未采用"))?;
    if b.task_id != d.task_id || b.revision != d.revision {
        return Err(store::error("材料已修订，旧学习结果未采用"));
    }
    Ok(())
}
fn checkpoint(
    r: &ChatRef,
    old: &crate::learning_chat_types::Record,
    d: Delivery,
) -> Result<crate::learning_chat_types::Record, AppError> {
    current(r, &d)?;
    let mut s = old.state.clone();
    s.delivery = Some(d);
    store::advance(r, old, s)
}
pub async fn resume(r: &ChatRef, retry: bool) -> Result<Option<Written>, AppError> {
    if store::load(r)?.state.selected_writer.is_none() {
        crate::learning_chat::resume(r, retry).await?;
    }
    run_with(r, |call| async move {
        crate::learning_flow_writer::send(call, retry).await
    })
    .await
}
pub async fn run_with<F, Fut>(r: &ChatRef, mut call: F) -> Result<Option<Written>, AppError>
where
    F: FnMut(Call) -> Fut,
    Fut: Future<Output = Result<String, AppError>>,
{
    let _lock = io::lock(&store::directory(r)?.join("run-lock"))?;
    let ws = store::ws(r)?;
    let mut old = store::load(r)?;
    if old.state.paused || old.state.pending.is_some() {
        return Ok(None);
    }
    if let Some(input) = &old.state.selected_writer {
        let written = crate::learning_flow_writer::run_with(r, input, &mut call).await?;
        if store::load(r)?.id != old.id {
            return Err(store::error("会话变化，旧成稿未采用"));
        }
        return Ok(Some(written));
    }
    let Some(id) = old.state.active.clone() else {
        return Ok(None);
    };
    let material = learning_agent::load(&ws, &id)?;
    if material.state.status != learning_agent::Status::ReadyForLearning {
        return Ok(None);
    }
    let mut d = match old.state.delivery.clone() {
        Some(d) if d.task_id == id && d.revision == material.state.revision => d,
        _ => {
            let plan = crate::learning_agent_store::plan(&ws, &id)?;
            let mut profile = plan.profile.clone();
            profile.model = "mimo-v2.6-pro".into();
            let job = learning_job::prepare(
                &ws,
                learning_agent::ready_texts(&ws, &id)?,
                format!(
                    "{}\n学习目标：{}\n材料任务：{} 修订{}",
                    plan.delegate.user_request,
                    material.state.user_request,
                    id,
                    material.state.revision
                ),
                plan.delegate.base.clone(),
                profile,
                JobPrompts::load()?,
            )?;
            let d = Delivery {
                task_id: id.clone(),
                revision: material.state.revision,
                job_id: job.id,
                candidate_id: None,
                save_name: plan.delegate.save_name,
                saved: None,
                writer: None,
            };
            old = checkpoint(r, &old, d.clone())?;
            d
        }
    };
    current(r, &d)?;
    let job = learning_job::load(&ws, &d.job_id)?;
    if d.candidate_id.is_none() {
        let root = learning_job::directory(&ws, &job.id)?;
        let expected = old.id.clone();
        let candidate = learning_job::run_with(&ws, &job.id, |stage, system, user| {
            let check = current(r, &d).and_then(|_| {
                if store::load(r)?.id != expected {
                    Err(store::error("会话变化，未继续旧学习阶段"))
                } else {
                    Ok(())
                }
            });
            let pending = check.map(|_| {
                call(Call {
                    root: root.clone(),
                    profile: job.profile.clone(),
                    stage,
                    system,
                    user,
                })
            });
            async move {
                match pending {
                    Ok(future) => future.await,
                    Err(error) => Err(error),
                }
            }
        })
        .await?;
        d.candidate_id = Some(candidate.id);
        old = checkpoint(r, &old, d.clone())?;
    }
    let candidate = ws.preview(
        d.candidate_id
            .as_deref()
            .ok_or_else(|| store::error("候选引用缺失"))?,
    )?;
    if candidate.input.articles != job.articles || candidate.input.base != job.base {
        return Err(store::error("交付候选与学习任务身份不符"));
    }
    if d.saved.is_none() {
        if let Some(name) = &d.save_name {
            // Short state lock prevents cancel racing the irreversible version publication.
            let guard = io::lock(&store::directory(r)?.join("state-lock"))?;
            if store::load(r)?.id != old.id {
                return Err(store::error("会话变化，未保存旧候选"));
            }
            current(r, &d)?;
            d.saved = Some(ws.save(&candidate.id, name)?.reference());
            drop(guard);
            old = checkpoint(r, &old, d.clone())?;
        }
    }
    if d.writer.is_none() {
        let plan = crate::learning_agent_store::plan(&ws, &id)?;
        if let Some(original) = plan.delegate.followup_request {
            let request =
                crate::learning_chat_parse::writing_request(&original).ok_or_else(|| {
                    store::error("后续写作要求不明确，候选已保留，请直接提供写作要求")
                })?;
            d.writer = Some(WriterInput {
                label: d
                    .saved
                    .as_ref()
                    .map(|v| format!("{} v{}", v.name, v.number))
                    .unwrap_or_else(|| "本次未命名候选".into()),
                spec: candidate.input.draft.spec.clone(),
                request,
            });
            old = checkpoint(r, &old, d.clone())?;
        }
    }
    if let Some(input) = &d.writer {
        let result = crate::learning_flow_writer::run_with(r, input, &mut call).await?;
        if store::load(r)?.id != old.id {
            return Err(store::error("会话变化，旧成稿未采用"));
        }
        current(r, &d)?;
        return Ok(Some(result));
    }
    Ok(None)
}
pub fn command(r: &ChatRef, input: &str) -> Result<bool, AppError> {
    crate::learning_flow_commands::command(r, input)
}
pub fn display(r: &ChatRef) -> Result<(), AppError> {
    let state = store::load(r)?.state;
    if state.paused {
        return crate::learning_chat::display(r, false);
    }
    let Some(d) = state.delivery else {
        return crate::learning_chat::display(r, false);
    };
    current(r, &d)?;
    let ws = store::ws(r)?;
    if let Some(id) = d.candidate_id {
        let c = ws.preview(&id)?;
        println!(
            "风格候选已生成（{}篇，待审）：\n{}",
            c.input.articles.len(),
            c.input.draft.spec
        );
        if let Some(v) = d.saved {
            println!("已保存：{} v{}", v.name, v.number);
        } else {
            println!("候选尚未命名；可输入 保存为X。无需命名也可先预览。 ");
        }
        println!(
            "/preview 查看候选；/diff 查看变化；/versions <名称> 查看历史；取消学习结束当前任务。"
        );
    } else {
        let stats =
            crate::learning_transport::statistics(&learning_job::directory(&ws, &d.job_id)?)?;
        println!("正在学习，阶段回执：{stats}；继续可恢复，未知请求仅 /retry-learning 显式重试。");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::learning_chat::{self, Action, LearnInput};
    use crate::learning_chat_types::tests::fixture;
    use serde_json::{Value, json};
    async fn ready(r: &ChatRef, text: &str) {
        learning_chat::submit(r, text).unwrap();
        learning_chat::run_with(r, |_| async { panic!("明确材料不调用解释器") })
            .await
            .unwrap();
    }
    fn answer(c: &Call) -> String {
        match c.stage.as_str() {
            "learn-analysis" => {
                let v: Value = serde_json::from_str(&c.user).unwrap();
                json!({"analysis":"INTERNAL_ANALYSIS", "evidence":v["articles"].as_array().unwrap().iter().map(|a|json!({"article":a["article"],"quote":a["text"]})).collect::<Vec<_>>()}).to_string()
            }
            "scope-review" => "INTERNAL_REVIEW".into(),
            "writer-outline" => "- 新主题的大纲".into(),
            "writer-fulltext" => "# 新文章\n只按给定新材料写。".into(),
            _ => "从具体感受进入判断，推理要有已知材料，不把偶见手法变成每段必须动作。".into(),
        }
    }
    #[tokio::test]
    async fn candidate_only_save_is_idempotent_and_writer_receives_only_spec_and_new_brief() {
        let (_tmp, r) = fixture();
        ready(
            &r,
            "学习下面文章：正文：SOURCE_SENTINEL，独特原文不进入Writer。",
        )
        .await;
        let mut n = 0;
        assert!(
            run_with(&r, |c| {
                n += 1;
                async move { Ok(answer(&c)) }
            })
            .await
            .unwrap()
            .is_none()
        );
        assert_eq!(n, 4);
        let ws = store::ws(&r).unwrap();
        assert!(ws.catalog().unwrap().is_empty());
        command(&r, "保存为本次写法").unwrap();
        command(&r, "保存为本次写法").unwrap();
        assert_eq!(ws.versions("本次写法").unwrap().len(), 1);
        run_with(&r, |_| async { panic!("恢复已完成任务不调用") })
            .await
            .unwrap();
        command(
            &r,
            "/use-version 本次写法 1 写一篇图书馆文章，800字，给学生看",
        )
        .unwrap();
        let result = run_with(&r, |c| async move {
            assert_eq!(c.profile.model, "mimo-v2.6-flash");
            assert!(!format!("{}{}", c.system, c.user).contains("SOURCE_SENTINEL"));
            assert!(!format!("{}{}", c.system, c.user).contains("INTERNAL_"));
            assert!(c.user.contains("800字"));
            Ok(answer(&c))
        })
        .await
        .unwrap()
        .unwrap();
        assert!(result.fulltext.contains("新文章"));
    }
    #[tokio::test]
    async fn combined_auto_save_and_nameless_followup_both_preserve_new_request() {
        for save in ["", "，保存为“示例”"] {
            let (_tmp, r) = fixture();
            ready(&r,&format!("学习下面文章{save}，再写一篇关于自行车的文章，600字；正文：SOURCE_ONLY，特有原文。" )).await;
            let w = run_with(&r, |c| async move {
                if c.stage.starts_with("writer-") {
                    assert!(!c.user.contains("SOURCE_ONLY"));
                    assert!(c.user.contains("600字"));
                }
                Ok(answer(&c))
            })
            .await
            .unwrap()
            .unwrap();
            assert!(w.request.starts_with("写一篇"));
            assert_eq!(
                store::ws(&r).unwrap().catalog().unwrap().len(),
                usize::from(!save.is_empty())
            );
        }
    }
    #[tokio::test]
    async fn cancelled_late_model_output_never_saves_or_writes() {
        let (_tmp, r) = fixture();
        ready(
            &r,
            "学习保存为“不能保存”，再写一篇新文章；正文：取消边界的独特材料。",
        )
        .await;
        let mut calls = 0;
        let result = run_with(&r, |c| {
            calls += 1;
            learning_chat::cancel(&r).unwrap();
            async move { Ok(answer(&c)) }
        })
        .await;
        assert!(result.is_err());
        assert_eq!(calls, 1, "取消后不得再发下一学习阶段");
        assert!(store::ws(&r).unwrap().catalog().unwrap().is_empty());
        assert!(store::load(&r).unwrap().state.delivery.is_none());
    }
    #[tokio::test]
    async fn append_preserves_old_text_and_version_and_stale_candidate_never_overwrites() {
        let (_tmp, r) = fixture();
        ready(&r, "学习保存为“更新例子”；正文：原来的独立完整文章。").await;
        run_with(&r, |c| async move { Ok(answer(&c)) })
            .await
            .unwrap();
        let ws = store::ws(&r).unwrap();
        let first = ws.version("更新例子", 1).unwrap();
        let original = "追加到更新例子，保存为更新例子，正文：第二篇不同的独立文章。";
        learning_chat::submit(&r, original).unwrap();
        learning_chat::run_with(&r, |_| async {
            Ok(serde_json::to_string(&Action::Learn {
                input: LearnInput {
                    sources: vec![crate::learning_sources::SourceInput::Text {
                        label: "正文".into(),
                        text: "第二篇不同的独立文章。".into(),
                    }],
                    base_name: Some("更新例子".into()),
                    base_number: None,
                    save_name: Some("更新例子".into()),
                    ..Default::default()
                },
            })
            .unwrap())
        })
        .await
        .unwrap();
        run_with(&r, |c| async move { Ok(answer(&c)) })
            .await
            .unwrap();
        let second = ws.version("更新例子", 2).unwrap();
        assert_eq!(second.previous, Some(first.id.clone()));
        assert_eq!(second.candidate.input.articles.len(), 2);
        assert_eq!(ws.version("更新例子", 1).unwrap().id, first.id);
        assert_eq!(
            ws.save(&first.candidate.id, "更新例子").unwrap().id,
            first.id
        );
        assert_eq!(ws.versions("更新例子").unwrap().len(), 2);
    }
    #[tokio::test]
    async fn save_conflict_preserves_candidate_and_never_dispatches_writer() {
        let (_tmp, r) = fixture();
        ready(&r, "学习保存为“冲突名”；正文：第一份完整独立材料。").await;
        run_with(&r, |c| async move { Ok(answer(&c)) })
            .await
            .unwrap();
        let ws = store::ws(&r).unwrap();
        let first = ws.version("冲突名", 1).unwrap();
        ready(
            &r,
            "学习保存为“冲突名”，再写一篇新题材文章；正文：另一份独立完整新文章。",
        )
        .await;
        assert!(
            run_with(&r, |c| async move {
                assert!(!c.stage.starts_with("writer-"));
                Ok(answer(&c))
            })
            .await
            .is_err()
        );
        let state = store::load(&r).unwrap().state;
        let d = state.delivery.unwrap();
        assert!(d.candidate_id.is_some());
        assert!(d.saved.is_none());
        assert_eq!(ws.versions("冲突名").unwrap().len(), 1);
        assert_eq!(ws.version("冲突名", 1).unwrap().id, first.id);
        assert!(
            run_with(&r, |_| async {
                panic!("保存冲突恢复不重抽模型结果")
            })
            .await
            .is_err()
        );
        command(&r, "保存为另存新名").unwrap();
        let written = run_with(&r, |c| async move {
            assert!(c.stage.starts_with("writer-"));
            Ok(answer(&c))
        })
        .await
        .unwrap()
        .unwrap();
        assert!(written.label.contains("另存新名"));
    }
}
