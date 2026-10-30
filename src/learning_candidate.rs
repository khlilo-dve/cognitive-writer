//! Portable draft inputs and immutable saved-version identities.
use crate::{
    error::AppError,
    voice_learning::{VoiceDraft, sha256, sources, validate},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DraftOrigin {
    ModelOutput,
    ImplementerEdit,
    UserEdit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VersionRef {
    pub name: String,
    pub number: u32,
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateInput {
    pub articles: Vec<String>,
    pub draft: VoiceDraft,
    pub origin: DraftOrigin,
    pub note: String,
    #[serde(default)]
    pub review: Option<String>,
    #[serde(default)]
    pub base: Option<VersionRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    pub schema: u32,
    pub id: String,
    pub input: CandidateInput,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LearningVersion {
    pub schema: u32,
    pub id: String,
    pub name: String,
    pub number: u32,
    pub previous: Option<String>,
    pub saved_at: String,
    /// User saving chooses a snapshot; it is not independent semantic/factual acceptance.
    pub status: String,
    pub candidate: Candidate,
}

#[derive(Debug, Serialize)]
pub struct CandidateChanges {
    pub previous_articles: usize,
    pub current_articles: usize,
    pub added_source_hashes: Vec<String>,
    pub reason: String,
    pub text_diff: String,
}

pub(crate) fn name(name: &str) -> Result<String, AppError> {
    crate::style_names::style_file_path(".", name)?;
    Ok(name.trim().to_string())
}

pub(crate) fn valid_id(id: &str) -> Result<(), AppError> {
    if id.len() != 64
        || !id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(AppError::LearningWorkspace("候选/版本标识无效".into()));
    }
    Ok(())
}

pub(crate) fn identity(value: &impl Serialize) -> Result<String, AppError> {
    let mut value =
        serde_json::to_value(value).map_err(|e| AppError::LearningWorkspace(e.to_string()))?;
    value
        .as_object_mut()
        .ok_or_else(|| AppError::LearningWorkspace("版本记录不是对象".into()))?
        .remove("id");
    Ok(sha256(
        &serde_json::to_vec(&value).map_err(|e| AppError::LearningWorkspace(e.to_string()))?,
    ))
}

impl CandidateInput {
    pub(crate) fn validate(&self) -> Result<(), AppError> {
        let sources = sources(&self.articles).map_err(local_error)?;
        if sources
            .iter()
            .map(|s| &s.identity)
            .ne(self.draft.sources.iter())
        {
            return Err(AppError::LearningWorkspace(
                "候选来源与正文快照不一致".into(),
            ));
        }
        validate(&self.draft.spec, &sources).map_err(local_error)?;
        valid_id(&self.draft.fingerprint)?;
        if self.draft.status != "draft_pending_semantic_review"
            || self.draft.method.trim().is_empty()
            || self.note.trim().is_empty()
        {
            return Err(AppError::LearningWorkspace(
                "需要待审学习候选、方法身份与变化理由".into(),
            ));
        }
        if let Some(base) = &self.base {
            if name(&base.name)? != base.name || base.number == 0 {
                return Err(AppError::LearningWorkspace("基准版本无效".into()));
            }
            valid_id(&base.id)?;
        }
        Ok(())
    }
}

impl Candidate {
    pub(crate) fn verify(&self) -> Result<(), AppError> {
        self.input.validate()?;
        if self.schema != 1 || identity(self)? != self.id {
            return Err(AppError::LearningWorkspace("候选记录身份损坏".into()));
        }
        Ok(())
    }
}

impl LearningVersion {
    pub fn reference(&self) -> VersionRef {
        VersionRef {
            name: self.name.clone(),
            number: self.number,
            id: self.id.clone(),
        }
    }

    pub(crate) fn verify(&self) -> Result<(), AppError> {
        self.candidate.verify()?;
        if self.schema != 1
            || identity(self)? != self.id
            || name(&self.name)? != self.name
            || self.number == 0
            || self.status != "saved_draft_not_quality_certified"
        {
            return Err(AppError::LearningWorkspace("学习版本身份损坏".into()));
        }
        Ok(())
    }
}

fn local_error(error: AppError) -> AppError {
    match error {
        AppError::Parse(message) => AppError::LearningWorkspace(message),
        other => other,
    }
}
