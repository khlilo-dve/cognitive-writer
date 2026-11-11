//! Immutable input collections. Structural readiness is not semantic style approval.
pub(crate) use crate::learning_input_limits::validate_request;
pub use crate::learning_source_binary::BinarySnapshot;
use crate::{
    error::AppError,
    learning_candidate::{identity, valid_id},
    learning_source_fetch as fetch,
    learning_source_parse::PageLink,
    learning_workspace::LearningWorkspace,
    learning_workspace_store as store,
    voice_learning::sha256,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};
pub const MAX_BYTES: usize = 8 * 1024 * 1024;
const METHOD: &str = "source-collection-v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SourceInput {
    File { path: String },
    Text { label: String, text: String },
    Url { url: String },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceRequest {
    pub sources: Vec<SourceInput>,
    pub note: String,
    #[serde(default)]
    pub expected_articles: Option<usize>,
    #[serde(default)]
    pub allow_partial: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceStatus {
    Extracted,
    NeedsInspection,
    NonArticle,
    Failed,
    Unsupported,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attempt {
    pub method: String,
    pub http_status: Option<u16>,
    pub outcome: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub method: String,
    pub content_type: String,
    pub resolved_locator: String,
    pub body: String,
    pub raw_sha256: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArticleCandidate {
    pub id: String,
    pub source_id: String,
    pub title: String,
    pub text: String,
    pub text_sha256: String,
    pub normalized_sha256: String,
    pub boundary_method: String,
    pub duplicate_of: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceRecord {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binary: Option<BinarySnapshot>,
    pub id: String,
    pub resolved_locator: String,
    pub status: SourceStatus,
    pub reason: String,
    pub attempts: Vec<Attempt>,
    pub snapshots: Vec<Snapshot>,
    pub candidates: Vec<ArticleCandidate>,
    pub links: Vec<PageLink>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceCollection {
    pub schema: u32,
    pub method: String,
    pub id: String,
    pub collected_at: String,
    pub base_dir: PathBuf,
    pub request: SourceRequest,
    pub records: Vec<SourceRecord>,
}
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Readiness {
    Ready,
    NeedsInspection,
    NeedsInput,
    Blocked,
}
#[derive(Debug, Serialize)]
pub struct Assessment {
    pub status: Readiness,
    pub unique_articles: usize,
    pub duplicate_articles: usize,
    pub reasons: Vec<String>,
}
fn error(message: &str) -> AppError {
    AppError::LearningWorkspace(message.into())
}
fn normalized(text: &str) -> String {
    sha256(
        text.chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
            .as_bytes(),
    )
}

impl SourceCollection {
    pub fn assessment(&self) -> Result<Assessment, AppError> {
        self.verify()?;
        let unique = self
            .records
            .iter()
            .flat_map(|r| &r.candidates)
            .filter(|a| a.duplicate_of.is_none())
            .count();
        let duplicates = self
            .records
            .iter()
            .flat_map(|r| &r.candidates)
            .filter(|a| a.duplicate_of.is_some())
            .count();
        let mut reasons: Vec<_> = self
            .records
            .iter()
            .filter(|r| !r.reason.is_empty())
            .map(|r| format!("{}: {}", r.id, r.reason))
            .collect();
        let inspection = self
            .records
            .iter()
            .any(|r| r.status == SourceStatus::NeedsInspection);
        let multiple = self.records.iter().any(|r| r.candidates.len() > 1);
        let failed = self
            .records
            .iter()
            .any(|r| r.status != SourceStatus::Extracted);
        let mismatch = self.request.expected_articles.is_some_and(|n| n != unique);
        let status = if inspection {
            Readiness::NeedsInspection
        } else if unique == 0 {
            Readiness::Blocked
        } else if multiple || unique > 10 || mismatch || (failed && !self.request.allow_partial) {
            Readiness::NeedsInput
        } else {
            Readiness::Ready
        };
        if multiple {
            reasons.push("一项来源含多个正文候选，需要明确选文；未默认全选".into());
        }
        if unique > 10 {
            reasons.push("独立正文超过10篇，未截取前10篇".into());
        }
        if mismatch {
            reasons.push(format!(
                "明确独立篇数为{}，实际为{unique}篇",
                self.request.expected_articles.unwrap()
            ));
        }
        if failed && !self.request.allow_partial {
            reasons.push("存在未取得明确正文的来源，未获准缩小范围".into());
        }
        if unique == 0 && !inspection {
            reasons.push("没有可学习的正文".into());
        }
        Ok(Assessment {
            status,
            unique_articles: unique,
            duplicate_articles: duplicates,
            reasons,
        })
    }
    pub fn verify(&self) -> Result<(), AppError> {
        validate_request(&self.request)?;
        valid_id(&self.id)?;
        if self.schema != 1
            || self.method != METHOD
            || identity(self)? != self.id
            || self.records.len() != self.request.sources.len()
        {
            return Err(error("来源集合身份或版本损坏"));
        }
        let mut seen = HashMap::new();
        for (i, record) in self.records.iter().enumerate() {
            if record.id != format!("s{:03}", i + 1) {
                return Err(error("来源记录顺序身份不一致"));
            }
            if let Some(blob) = &record.binary {
                crate::learning_source_binary::reference(blob)?;
            }
            for snapshot in &record.snapshots {
                if sha256(snapshot.body.as_bytes()) != snapshot.raw_sha256 {
                    return Err(error("原始来源快照损坏"));
                }
            }
            for (j, a) in record.candidates.iter().enumerate() {
                let duplicate = seen.get(&a.normalized_sha256).cloned();
                if a.id != format!("{}-a{:03}", record.id, j + 1)
                    || a.source_id != record.id
                    || a.text.trim().is_empty()
                    || sha256(a.text.as_bytes()) != a.text_sha256
                    || normalized(&a.text) != a.normalized_sha256
                    || a.duplicate_of != duplicate
                {
                    return Err(error("来源正文、哈希或重复标记不一致"));
                }
                seen.entry(a.normalized_sha256.clone())
                    .or_insert_with(|| a.id.clone());
            }
        }
        Ok(())
    }
}

/// Acquires sources without any model request. Immutable output can be inspected after restart.
pub async fn collect(
    ws: &LearningWorkspace,
    request: SourceRequest,
    base_dir: &Path,
) -> Result<SourceCollection, AppError> {
    collect_with_reader(ws, request, base_dir, Some("https://r.jina.ai/")).await
}
pub(crate) async fn collect_with_reader(
    ws: &LearningWorkspace,
    request: SourceRequest,
    base_dir: &Path,
    reader_prefix: Option<&str>,
) -> Result<SourceCollection, AppError> {
    validate_request(&request)?;
    let base_dir = std::fs::canonicalize(base_dir)?;
    let mut records = Vec::new();
    let mut seen = HashMap::new();
    for (i, input) in request.sources.iter().enumerate() {
        let fetched = fetch::fetch(input, &base_dir, reader_prefix).await;
        let binary = fetched
            .binary
            .as_deref()
            .map(|bytes| crate::learning_source_binary::retain(ws, bytes))
            .transpose()?;
        let source_id = format!("s{:03}", i + 1);
        let mut candidates = Vec::new();
        let mut links = Vec::new();
        if let Some(extraction) = fetched.extraction {
            links = extraction.links;
            for (j, a) in extraction.articles.into_iter().enumerate() {
                let normalized_sha256 = normalized(&a.text);
                let duplicate_of = seen.get(&normalized_sha256).cloned();
                let id = format!("{source_id}-a{:03}", j + 1);
                seen.entry(normalized_sha256.clone())
                    .or_insert_with(|| id.clone());
                candidates.push(ArticleCandidate {
                    id,
                    source_id: source_id.clone(),
                    title: a.title,
                    text_sha256: sha256(a.text.as_bytes()),
                    text: a.text,
                    normalized_sha256,
                    boundary_method: extraction.method.clone(),
                    duplicate_of,
                });
            }
        }
        records.push(SourceRecord {
            binary,
            id: source_id,
            resolved_locator: fetched.resolved_locator,
            status: fetched.status,
            reason: fetched.reason,
            attempts: fetched.attempts,
            snapshots: fetched.snapshots,
            candidates,
            links,
        });
    }
    let mut collection = SourceCollection {
        schema: 1,
        method: METHOD.into(),
        id: String::new(),
        collected_at: chrono::Utc::now().to_rfc3339(),
        base_dir,
        request,
        records,
    };
    collection.id = identity(&collection)?;
    collection.verify()?;
    let _lock = store::lock(ws.root())?;
    store::publish_json(&collection_path(ws, &collection.id)?, &collection)?;
    Ok(collection)
}
fn collection_path(ws: &LearningWorkspace, id: &str) -> Result<PathBuf, AppError> {
    valid_id(id)?;
    Ok(ws
        .root()
        .join("source-collections")
        .join(format!("{id}.json")))
}
pub fn load(ws: &LearningWorkspace, id: &str) -> Result<SourceCollection, AppError> {
    let collection: SourceCollection = store::read(&collection_path(ws, id)?)?;
    collection.verify()?;
    crate::learning_source_binary::verify(ws, &collection.records)?;
    if collection.id != id {
        return Err(error("来源集合文件名与身份不符"));
    }
    Ok(collection)
}
/// Writes exactly the articles/note contract consumed by existing style-draft learn; never overwrites.
pub fn export_texts(ws: &LearningWorkspace, id: &str, target: &Path) -> Result<(), AppError> {
    let collection = load(ws, id)?;
    if collection.assessment()?.status != Readiness::Ready {
        return Err(error("来源仍需范围处理或没有正文，未导出不完整学习包"));
    }
    let articles: Vec<_> = collection
        .records
        .iter()
        .flat_map(|r| &r.candidates)
        .filter(|a| a.duplicate_of.is_none())
        .map(|a| &a.text)
        .collect();
    store::publish_json(
        target,
        &serde_json::json!({"articles":articles,"note":collection.request.note}),
    )
}
pub fn read_request(path: &Path) -> Result<(SourceRequest, PathBuf), AppError> {
    let file = std::fs::canonicalize(path)?;
    let request: SourceRequest =
        serde_json::from_slice(&fetch::read_limited(&file)?).map_err(|_| {
            error("来源清单JSON无效；需要sources/note，可选expected_articles/allow_partial")
        })?;
    validate_request(&request)?;
    Ok((
        request,
        file.parent()
            .ok_or_else(|| error("来源清单缺少目录"))?
            .to_path_buf(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(sources: Vec<SourceInput>) -> SourceRequest {
        SourceRequest {
            sources,
            note: "学习这些文章的写法".into(),
            expected_articles: None,
            allow_partial: false,
        }
    }
    fn text(body: &str) -> SourceInput {
        SourceInput::Text {
            label: "文章".into(),
            text: body.into(),
        }
    }
    #[tokio::test]
    async fn relative_files_duplicates_and_export_survive_source_deletion_without_reacquisition() {
        let dir = tempfile::tempdir().unwrap();
        let ws = LearningWorkspace::open(&dir.path().join("workspace")).unwrap();
        std::fs::write(dir.path().join("样文.md"), "段落甲。\n\n段落乙。").unwrap();
        let c = collect_with_reader(
            &ws,
            request(vec![
                SourceInput::File {
                    path: "样文.md".into(),
                },
                text("段落甲。 段落乙。"),
            ]),
            dir.path(),
            None,
        )
        .await
        .unwrap();
        assert_eq!(c.assessment().unwrap().status, Readiness::Ready);
        assert_eq!(c.assessment().unwrap().duplicate_articles, 1);
        std::fs::remove_file(dir.path().join("样文.md")).unwrap();
        assert_eq!(load(&ws, &c.id).unwrap().id, c.id);
        let target = dir.path().join("texts.json");
        export_texts(&ws, &c.id, &target).unwrap();
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&target).unwrap()).unwrap();
        assert_eq!(saved["articles"].as_array().unwrap().len(), 1);
        assert!(export_texts(&ws, &c.id, &target).is_err());
        let path = collection_path(&ws, &c.id).unwrap();
        let mut damaged = c.clone();
        damaged.records[0].candidates[0].text = "替换正文".into();
        std::fs::write(path, serde_json::to_vec(&damaged).unwrap()).unwrap();
        assert!(load(&ws, &c.id).is_err());
    }
    #[tokio::test]
    async fn missing_sources_need_scope_change_and_partial_does_not_erase_explicit_count() {
        let dir = tempfile::tempdir().unwrap();
        let ws = LearningWorkspace::open(dir.path()).unwrap();
        let mut r = request(vec![
            text("有效正文。"),
            SourceInput::File {
                path: "missing.txt".into(),
            },
        ]);
        let c = collect_with_reader(&ws, r.clone(), dir.path(), None)
            .await
            .unwrap();
        assert_eq!(c.assessment().unwrap().status, Readiness::NeedsInput);
        assert!(export_texts(&ws, &c.id, &dir.path().join("invalid.json")).is_err());
        r.allow_partial = true;
        let c = collect_with_reader(&ws, r.clone(), dir.path(), None)
            .await
            .unwrap();
        assert_eq!(c.assessment().unwrap().status, Readiness::Ready);
        r.expected_articles = Some(2);
        let c = collect_with_reader(&ws, r, dir.path(), None).await.unwrap();
        assert_eq!(c.assessment().unwrap().status, Readiness::NeedsInput);
    }
    #[tokio::test]
    async fn multisection_and_eleven_articles_are_not_silently_selected() {
        let dir = tempfile::tempdir().unwrap();
        let ws = LearningWorkspace::open(dir.path()).unwrap();
        let c = collect_with_reader(
            &ws,
            request(vec![text("# 甲\n\n正文甲。\n\n# 乙\n\n正文乙。")]),
            dir.path(),
            None,
        )
        .await
        .unwrap();
        assert_eq!(c.assessment().unwrap().status, Readiness::NeedsInput);
        let c = collect_with_reader(
            &ws,
            request((0..11).map(|i| text(&format!("独立正文{i}。"))).collect()),
            dir.path(),
            None,
        )
        .await
        .unwrap();
        assert_eq!(c.assessment().unwrap().unique_articles, 11);
        assert_eq!(c.assessment().unwrap().status, Readiness::NeedsInput);
        assert!(export_texts(&ws, &c.id, &dir.path().join("eleven.json")).is_err());
    }
    #[tokio::test]
    async fn ten_articles_are_ready_and_no_text_is_blocked_not_fabricated() {
        let dir = tempfile::tempdir().unwrap();
        let ws = LearningWorkspace::open(dir.path()).unwrap();
        let c = collect_with_reader(
            &ws,
            request((0..10).map(|i| text(&format!("短文{i}。"))).collect()),
            dir.path(),
            None,
        )
        .await
        .unwrap();
        assert_eq!(c.assessment().unwrap().status, Readiness::Ready);
        let c = collect_with_reader(&ws, request(vec![text(" \n ")]), dir.path(), None)
            .await
            .unwrap();
        assert_eq!(c.assessment().unwrap().status, Readiness::Blocked);
        assert!(
            collect_with_reader(
                &ws,
                request(vec![SourceInput::Url {
                    url: "https://user:secret@example.org".into()
                }]),
                dir.path(),
                None
            )
            .await
            .is_err()
        );
    }
}
