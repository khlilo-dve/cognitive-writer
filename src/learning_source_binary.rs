//! Content-addressed binary snapshots, kept outside all model-visible text fields.
use crate::{
    error::AppError,
    learning_candidate::valid_id,
    learning_source_fetch::read_limited,
    learning_sources::{MAX_BYTES, SourceRecord},
    learning_workspace::LearningWorkspace,
    learning_workspace_store as store,
    voice_learning::sha256,
};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BinarySnapshot {
    pub sha256: String,
    pub size: usize,
    pub path: String,
}
fn error(s: &str) -> AppError {
    AppError::LearningWorkspace(s.into())
}
pub(crate) fn reference(blob: &BinarySnapshot) -> Result<(), AppError> {
    valid_id(&blob.sha256)?;
    if blob.path != format!("blobs/{}", blob.sha256) || blob.size > MAX_BYTES {
        return Err(error("二进制快照引用无效"));
    }
    Ok(())
}
pub(crate) fn retain(ws: &LearningWorkspace, bytes: &[u8]) -> Result<BinarySnapshot, AppError> {
    let hash = sha256(bytes);
    let relative = format!("blobs/{hash}");
    let path = ws.root().join(&relative);
    if bytes.len() > MAX_BYTES {
        return Err(error("二进制快照超过8MiB"));
    }
    if path.exists() {
        if read_limited(&path)? != bytes {
            return Err(error("既有二进制快照损坏"));
        }
    } else {
        store::publish(&path, bytes)?;
    }
    Ok(BinarySnapshot {
        sha256: hash,
        size: bytes.len(),
        path: relative,
    })
}
pub(crate) fn verify(ws: &LearningWorkspace, records: &[SourceRecord]) -> Result<(), AppError> {
    for record in records {
        if let Some(blob) = &record.binary {
            reference(blob)?;
            let bytes = read_limited(&ws.root().join(&blob.path))?;
            if bytes.len() != blob.size || sha256(&bytes) != blob.sha256 {
                return Err(error("二进制来源快照损坏，未重读外部原文件"));
            }
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn binary_snapshot_is_idempotent_and_corruption_is_rejected() {
        let d = tempfile::tempdir().unwrap();
        let ws = LearningWorkspace::open(d.path()).unwrap();
        let a = retain(&ws, b"binary bytes").unwrap();
        assert_eq!(retain(&ws, b"binary bytes").unwrap().sha256, a.sha256);
        std::fs::write(d.path().join(&a.path), b"broken").unwrap();
        assert!(retain(&ws, b"binary bytes").is_err());
        let mut escape = a;
        escape.path = "../outside".into();
        assert!(reference(&escape).is_err());
    }
}
