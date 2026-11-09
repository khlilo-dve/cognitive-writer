//! Parent conversation references and pending user turns; article state stays in P3.
use crate::{
    learning_agent::{Delegate, ScopePatch},
    learning_job::CallProfile,
    learning_sources::SourceInput,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRef {
    pub workspace: PathBuf,
    pub id: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Plan {
    pub schema: u32,
    pub id: String,
    pub created_at: String,
    pub base_dir: PathBuf,
    pub profile: CallProfile,
    pub prompt: String,
    pub agent_prompt: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Binding {
    pub task_id: String,
    pub revision: u32,
    pub question_id: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Turn {
    pub number: u32,
    pub input: String,
    pub binding: Option<Binding>,
    pub action: Option<Action>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LearnInput {
    pub sources: Vec<SourceInput>,
    pub expected_articles: Option<usize>,
    pub target_titles: Vec<String>,
    pub all_articles: bool,
    pub allow_partial: bool,
    pub save_name: Option<String>,
    pub base_name: Option<String>,
    pub base_number: Option<u32>,
    pub write_after: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Learn {
        #[serde(flatten)]
        input: LearnInput,
    },
    Reply {
        scope: ScopePatch,
        #[serde(default)]
        goal: Option<String>,
    },
    Use {
        style_name: String,
    },
    Other,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    Idle,
    Learning {
        task_id: String,
    },
    Use {
        style_name: String,
        original: String,
    },
    Message {
        text: String,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct State {
    pub active: Option<String>,
    pub paused: bool,
    pub turn: u32,
    pub pending: Option<Turn>,
    pub event: Event,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Record {
    pub id: String,
    pub previous: Option<String>,
    pub sequence: u32,
    pub state: State,
}
#[derive(Clone)]
pub struct ModelCall {
    pub owner_id: String,
    pub parent: bool,
    pub call: crate::learning_agent::DecisionCall,
}
pub(crate) fn delegate(
    input: LearnInput,
    original: &str,
    base: Option<crate::learning_candidate::VersionRef>,
) -> Delegate {
    Delegate {
        user_request: original.into(),
        request: crate::learning_sources::SourceRequest {
            sources: input.sources,
            note: original.into(),
            expected_articles: input.expected_articles,
            allow_partial: input.allow_partial,
        },
        target_titles: input.target_titles,
        all_articles: input.all_articles,
        base,
        save_name: input.save_name,
        followup_request: input.write_after.then(|| original.into()),
        budget_micro_cny: 250000,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    pub(crate) fn fixture() -> (tempfile::TempDir, super::ChatRef) {
        let d = tempfile::tempdir().unwrap();
        let r = crate::learning_chat_store::create_with(
            d.path(),
            crate::learning_job::CallProfile {
                model: "mimo-v2.6-flash".into(),
                endpoint_sha256: "a".repeat(64),
            },
            "父提示词".into(),
            "领域提示词".into(),
        )
        .unwrap();
        (d, r)
    }
}
