//! Frozen, resumable raw-text learning tasks. A successful task stages a candidate, never a version.
use crate::{
    error::AppError,
    learning_candidate::{Candidate, CandidateInput, DraftOrigin, VersionRef, identity, valid_id},
    learning_workspace::LearningWorkspace,
    learning_workspace_store as store,
    voice_learning::{self, VoicePrompts, learn_voice_with},
    voice_revision::{self, RevisionPrompts, revise_voice_with},
};
use serde::{Deserialize, Serialize};
use std::{future::Future, path::PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CallProfile {
    pub model: String,
    pub endpoint_sha256: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobPrompts {
    pub learn: VoicePrompts,
    pub revision: RevisionPrompts,
}
impl JobPrompts {
    pub fn load() -> Result<Self, AppError> {
        Ok(Self {
            learn: VoicePrompts::load()?,
            revision: RevisionPrompts::load()?,
        })
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LearningJob {
    pub schema: u32,
    pub id: String,
    pub articles: Vec<String>,
    pub note: String,
    pub base: Option<VersionRef>,
    pub profile: CallProfile,
    pub prompts: JobPrompts,
    pub learning_method: String,
    pub revision_method: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobState {
    pub status: String,
    pub candidate_id: Option<String>,
}

pub(crate) fn directory(ws: &LearningWorkspace, id: &str) -> Result<PathBuf, AppError> {
    valid_id(id)?;
    Ok(ws.root().join("jobs").join(id))
}

fn current_base(
    ws: &LearningWorkspace,
    base: &Option<VersionRef>,
    articles: &[String],
) -> Result<(), AppError> {
    if let Some(base) = base {
        let parent = ws.version(&base.name, base.number)?;
        if parent.reference() != *base
            || ws.versions(&base.name)?.last().map(|v| v.id.as_str()) != Some(base.id.as_str())
        {
            return Err(AppError::LearningWorkspace(
                "学习基准已过期，请从最新版本追加".into(),
            ));
        }
        let current = voice_learning::sources(articles)?;
        if parent.candidate.input.draft.sources.iter().any(|s| {
            !current
                .iter()
                .any(|c| c.identity.text_sha256 == s.text_sha256)
        }) {
            return Err(AppError::LearningWorkspace(
                "学习任务必须保留旧版完整正文".into(),
            ));
        }
    }
    Ok(())
}

pub fn prepare(
    ws: &LearningWorkspace,
    articles: Vec<String>,
    note: String,
    base: Option<VersionRef>,
    profile: CallProfile,
    prompts: JobPrompts,
) -> Result<LearningJob, AppError> {
    let sources = voice_learning::sources(&articles)?;
    valid_id(&profile.endpoint_sha256)?;
    if note.trim().is_empty()
        || profile.model != "mimo-v2.6-pro"
        || [
            &prompts.learn.analysis,
            &prompts.learn.compile,
            &prompts.revision.review,
            &prompts.revision.revise,
        ]
        .iter()
        .any(|p| p.trim().is_empty())
    {
        return Err(AppError::LearningWorkspace(
            "需要变化理由、Pro学习配置和完整提示词".into(),
        ));
    }
    let articles = sources.into_iter().map(|s| s.text).collect::<Vec<_>>();
    current_base(ws, &base, &articles)?;
    let mut job = LearningJob {
        schema: 1,
        id: String::new(),
        articles,
        note,
        base,
        profile,
        prompts,
        learning_method: voice_learning::METHOD.into(),
        revision_method: voice_revision::METHOD.into(),
    };
    job.id = identity(&job)?;
    let _lock = store::lock(ws.root())?;
    let path = directory(ws, &job.id)?.join("plan.json");
    if path.exists() {
        return load(ws, &job.id);
    }
    store::publish_json(&path, &job)?;
    Ok(job)
}

pub fn load(ws: &LearningWorkspace, id: &str) -> Result<LearningJob, AppError> {
    let job: LearningJob = store::read(&directory(ws, id)?.join("plan.json"))?;
    voice_learning::sources(&job.articles)?;
    if job.schema != 1 || job.id != id || identity(&job)? != id {
        return Err(AppError::LearningWorkspace("学习任务身份损坏".into()));
    }
    Ok(job)
}

pub fn state(ws: &LearningWorkspace, id: &str) -> Result<JobState, AppError> {
    let job = load(ws, id)?;
    let path = directory(ws, id)?.join("state.json");
    if !path.exists() {
        return Ok(JobState {
            status: "prepared".into(),
            candidate_id: None,
        });
    }
    let state: JobState = store::read(&path)?;
    if let Some(candidate) = &state.candidate_id {
        let candidate = ws.preview(candidate)?;
        if candidate.input.articles != job.articles
            || candidate.input.base != job.base
            || candidate.input.note != job.note
        {
            return Err(AppError::LearningWorkspace("任务与候选身份不符".into()));
        }
    } else if state.status == "complete" {
        return Err(AppError::LearningWorkspace("完整任务缺少候选".into()));
    }
    Ok(state)
}

pub fn list(ws: &LearningWorkspace) -> Result<Vec<(LearningJob, JobState)>, AppError> {
    let entries = match std::fs::read_dir(ws.root().join("jobs")) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e.into()),
    };
    let mut result = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if path.join("plan.json").exists() {
            let id = path
                .file_name()
                .and_then(|s| s.to_str())
                .ok_or_else(|| AppError::LearningWorkspace("任务路径无效".into()))?;
            result.push((load(ws, id)?, state(ws, id)?));
        }
    }
    result.sort_by(|a, b| a.0.id.cmp(&b.0.id));
    Ok(result)
}

fn set_state(
    ws: &LearningWorkspace,
    id: &str,
    status: &str,
    candidate: Option<String>,
) -> Result<(), AppError> {
    let state = JobState {
        status: status.into(),
        candidate_id: candidate,
    };
    crate::io::write_file_atomic(
        &directory(ws, id)?.join("state.json"),
        &serde_json::to_string_pretty(&state)
            .map_err(|e| AppError::LearningWorkspace(e.to_string()))?,
    )
}

/// Caller routes and journals requests; both live transport and research budget adapters use this path.
pub async fn run_with<F, Fut>(
    ws: &LearningWorkspace,
    id: &str,
    mut call: F,
) -> Result<Candidate, AppError>
where
    F: FnMut(String, String, String) -> Fut,
    Fut: Future<Output = Result<String, AppError>>,
{
    let job = load(ws, id)?;
    let _lock = store::lock(&directory(ws, id)?)?;
    // Completed output is immutable and can be recovered after later versions were saved.
    let prior = state(ws, id)?;
    if prior.status == "complete" {
        return ws.preview(
            prior
                .candidate_id
                .as_deref()
                .ok_or_else(|| AppError::LearningWorkspace("完整任务缺少候选".into()))?,
        );
    }
    if job.learning_method != voice_learning::METHOD
        || job.revision_method != voice_revision::METHOD
    {
        return Err(AppError::LearningWorkspace(
            "学习方法已变化，不能悄悄复用旧任务；请保留旧记录并新建任务".into(),
        ));
    }
    current_base(ws, &job.base, &job.articles)?;
    set_state(ws, id, "running", None)?;
    let result = async {
        let draft = learn_voice_with(&job.articles, &job.prompts.learn, |stage, system, user| {
            call(format!("learn-{stage}"), system, user)
        })
        .await?;
        let revised = revise_voice_with(
            &job.articles,
            &draft,
            &job.prompts.revision,
            |stage, system, user| call(format!("scope-{stage}"), system, user),
        )
        .await?;
        ws.stage(CandidateInput {
            articles: job.articles.clone(),
            draft: revised.result,
            origin: DraftOrigin::ModelOutput,
            note: job.note.clone(),
            review: Some(revised.review),
            base: job.base.clone(),
        })
    }
    .await;
    match result {
        Ok(candidate) => {
            set_state(ws, id, "complete", Some(candidate.id.clone()))?;
            Ok(candidate)
        }
        Err(error) => {
            set_state(ws, id, "failed", None)?;
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn prompts() -> JobPrompts {
        JobPrompts {
            learn: VoicePrompts {
                analysis: "分析".into(),
                compile: "编译".into(),
            },
            revision: RevisionPrompts {
                review: "复读".into(),
                revise: "修订".into(),
            },
        }
    }
    fn profile() -> CallProfile {
        CallProfile {
            model: "mimo-v2.6-pro".into(),
            endpoint_sha256: "0".repeat(64),
        }
    }

    #[tokio::test]
    async fn learning_stages_candidate_only_append_preserves_parent_and_frozen_prompts() {
        let temp = tempfile::tempdir().unwrap();
        let ws = LearningWorkspace::open(temp.path()).unwrap();
        let a = "独特的第一份原文".to_string();
        let b = "不同的第二份原文".to_string();
        let job = prepare(
            &ws,
            vec![a.clone()],
            "初次学习".into(),
            None,
            profile(),
            prompts(),
        )
        .unwrap();
        let candidate = run_with(&ws, &job.id, |stage, _, _| async move {
            Ok(if stage == "learn-analysis" {
                "内部分析"
            } else if stage == "scope-review" {
                "内部复读"
            } else {
                "在事实之内组织细节，按需要给出判断。"
            }
            .into())
        })
        .await
        .unwrap();
        assert!(ws.catalog().unwrap().is_empty());
        assert_eq!(candidate.input.review.as_deref(), Some("内部复读"));
        let first = ws.save(&candidate.id, "样例").unwrap();
        let update = prepare(
            &ws,
            vec![b.clone(), a.clone()],
            "追加正文".into(),
            Some(first.reference()),
            profile(),
            prompts(),
        )
        .unwrap();
        let reordered = prepare(
            &ws,
            vec![a, b],
            "追加正文".into(),
            Some(first.reference()),
            profile(),
            prompts(),
        )
        .unwrap();
        assert_eq!(update.id, reordered.id);
        assert_eq!(
            load(&ws, &update.id).unwrap().prompts.learn.analysis,
            "分析"
        );
        let result = run_with(&ws, &update.id, |_, _, _| async {
            Err(AppError::Network("模拟中断".into()))
        })
        .await;
        assert!(result.is_err());
        assert_eq!(state(&ws, &update.id).unwrap().status, "failed");
        assert_eq!(ws.versions("样例").unwrap().len(), 1);
    }
    #[tokio::test]
    async fn completed_old_method_is_readable_but_unfinished_old_method_cannot_send() {
        let temp = tempfile::tempdir().unwrap();
        let ws = LearningWorkspace::open(temp.path()).unwrap();
        let job = prepare(
            &ws,
            vec!["用于兼容验证的独特原文".into()],
            "首次学习".into(),
            None,
            profile(),
            prompts(),
        )
        .unwrap();
        let candidate = run_with(&ws, &job.id, |_, _, _| async {
            Ok("依照有据材料组织细节和判断。".into())
        })
        .await
        .unwrap();
        let mut historical = job.clone();
        historical.revision_method = "historical-method".into();
        historical.id = identity(&historical).unwrap();
        store::publish_json(
            &directory(&ws, &historical.id).unwrap().join("plan.json"),
            &historical,
        )
        .unwrap();
        set_state(&ws, &historical.id, "complete", Some(candidate.id.clone())).unwrap();
        assert_eq!(
            run_with(&ws, &historical.id, |_, _, _| async {
                panic!("完整任务不可重发")
            })
            .await
            .unwrap()
            .id,
            candidate.id
        );
        set_state(&ws, &historical.id, "failed", None).unwrap();
        assert!(
            run_with(&ws, &historical.id, |_, _, _| async {
                panic!("旧方法未完成任务不可重发")
            })
            .await
            .is_err()
        );
    }
}
