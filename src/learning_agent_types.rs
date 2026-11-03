//! Typed domain-agent boundary. Source content never defines control-plane fields.
use crate::{
    learning_candidate::VersionRef,
    learning_job::CallProfile,
    learning_sources::{SourceInput, SourceRequest},
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};
pub(crate) const METHOD: &str = "style-input-agent-v1";
fn default_budget() -> u64 {
    250_000
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Delegate {
    pub user_request: String,
    pub request: SourceRequest,
    #[serde(default)]
    pub target_titles: Vec<String>,
    #[serde(default)]
    pub all_articles: bool,
    #[serde(default)]
    pub base: Option<VersionRef>,
    #[serde(default)]
    pub save_name: Option<String>,
    #[serde(default)]
    pub followup_request: Option<String>,
    #[serde(default = "default_budget")]
    pub budget_micro_cny: u64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentPlan {
    pub schema: u32,
    pub id: String,
    pub method: String,
    pub created_at: String,
    pub delegate: Delegate,
    pub base_dir: PathBuf,
    pub profile: CallProfile,
    pub prompt: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    Flash,
    Pro,
}
impl Tier {
    pub(crate) fn model(self) -> &'static str {
        match self {
            Self::Flash => "mimo-v2.6-flash",
            Self::Pro => "mimo-v2.6-pro",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Active,
    WaitingInput,
    ReadyForLearning,
    Blocked,
    Failed,
    Cancelled,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Classification {
    Article,
    NotArticle,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resolution {
    pub classification: Classification,
    pub evidence: Vec<String>,
    pub reason: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "tool", rename_all = "snake_case", deny_unknown_fields)]
pub enum Decision {
    Collect,
    Inspect {
        source_ids: Vec<String>,
        #[serde(default)]
        offset_chars: usize,
        reason: String,
    },
    Resolve {
        source_id: String,
        classification: Classification,
        evidence: Vec<String>,
        reason: String,
    },
    Select {
        article_ids: Vec<String>,
        reason: String,
    },
    Clarify {
        topic: String,
        question: String,
        reason: String,
    },
    Escalate {
        reason: String,
    },
    Stop {
        reason: String,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollectionRef {
    pub id: String,
    pub step: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Question {
    pub id: String,
    pub revision: u32,
    pub topic: String,
    pub text: String,
    pub reason: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Replacement {
    pub source_id: String,
    pub input: SourceInput,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ScopePatch {
    pub article_ids: Option<Vec<String>>,
    pub target_titles: Option<Vec<String>>,
    pub all_articles: Option<bool>,
    pub allow_partial: Option<bool>,
    pub expected_articles: Option<usize>,
    pub clear_expected_articles: bool,
    pub replacements: Vec<Replacement>,
    pub additions: Vec<SourceInput>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    pub expected_revision: u32,
    pub question_id: Option<String>,
    pub text: String,
    #[serde(default)]
    pub user_request: Option<String>,
    #[serde(default)]
    pub scope: ScopePatch,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Answer {
    pub id: String,
    pub reply: Reply,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Pending {
    Decision {
        step: String,
        tier: Tier,
        user: String,
    },
    Tool {
        step: String,
        decision: Decision,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentState {
    pub task_id: String,
    pub revision: u32,
    pub status: Status,
    pub request: SourceRequest,
    pub user_request: String,
    pub target_titles: Vec<String>,
    pub all_articles: bool,
    pub collection: Option<CollectionRef>,
    pub inspected: Vec<String>,
    pub inspected_windows: Vec<String>,
    pub resolutions: BTreeMap<String, Resolution>,
    pub authorized_selection: Option<Vec<String>>,
    pub selected: Vec<String>,
    pub questions: Vec<Question>,
    pub answers: Vec<Answer>,
    pub decision_steps: u32,
    pub no_progress: u32,
    pub pro_used: bool,
    pub next_tier: Tier,
    pub pending: Option<Pending>,
    pub observation: serde_json::Value,
    pub reason: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateRecord {
    pub id: String,
    pub previous: Option<String>,
    pub sequence: u32,
    pub state: AgentState,
}
#[derive(Clone)]
pub struct DecisionCall {
    pub step: String,
    pub tier: Tier,
    pub system: String,
    pub user: String,
    pub endpoint_sha256: String,
    pub budget_micro_cny: u64,
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub(crate) struct Fixture {
        pub _temp: tempfile::TempDir,
        pub ws: crate::learning_workspace::LearningWorkspace,
        pub plan: AgentPlan,
    }
    pub(crate) fn fixture(body: &str, expected: Option<usize>) -> Fixture {
        let temp = tempfile::tempdir().unwrap();
        let ws = crate::learning_workspace::LearningWorkspace::open(&temp.path().join("workspace"))
            .unwrap();
        let delegate = Delegate {
            user_request: "学习提供的文章，按真实范围处理".into(),
            request: SourceRequest {
                sources: vec![SourceInput::Text {
                    label: "输入文章".into(),
                    text: body.into(),
                }],
                note: "学习写法".into(),
                expected_articles: expected,
                allow_partial: false,
            },
            target_titles: vec![],
            all_articles: false,
            base: None,
            save_name: Some("以后使用的风格".into()),
            followup_request: Some("之后写一篇新文章".into()),
            budget_micro_cny: 250000,
        };
        let plan = crate::learning_agent::prepare(
            &ws,
            delegate,
            temp.path(),
            CallProfile {
                model: "mimo-v2.6-flash".into(),
                endpoint_sha256: "a".repeat(64),
            },
            "动作提示词".into(),
        )
        .unwrap();
        Fixture {
            _temp: temp,
            ws,
            plan,
        }
    }
    pub(crate) fn question(text: &str) -> Decision {
        Decision::Clarify {
            topic: "选择正文".into(),
            question: text.into(),
            reason: "文章集合不明确，需要用户选择".into(),
        }
    }
    pub(crate) fn reply(record: &StateRecord, text: &str) -> Reply {
        Reply {
            expected_revision: record.state.revision,
            question_id: record.state.questions.last().map(|q| q.id.clone()),
            text: text.into(),
            user_request: None,
            scope: ScopePatch::default(),
        }
    }
}
