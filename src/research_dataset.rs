use crate::error::AppError;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    path::{Component, Path},
};

pub(crate) fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub(crate) fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 100
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Article {
    pub article_id: String,
    pub author_id: String,
    pub title: String,
    pub topic: String,
    pub split: String,
    pub source_url: String,
    pub path: String,
    pub sha256: String,
    pub verified: bool,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Dataset {
    pub schema: u32,
    pub dataset_id: String,
    pub articles: Vec<Article>,
}

impl Dataset {
    pub fn load(path: &Path) -> Result<Self, AppError> {
        let ds: Self = serde_json::from_slice(&std::fs::read(path)?)
            .map_err(|e| AppError::Parse(format!("数据集JSON无效: {e}")))?;
        if ds.schema != 1 || !valid_id(&ds.dataset_id) || ds.articles.is_empty() {
            return Err(AppError::Parse("数据集版本/名称/文章列表无效".into()));
        }
        let mut ids = HashSet::new();
        let mut hashes = HashSet::new();
        for a in &ds.articles {
            if !valid_id(&a.article_id)
                || !valid_id(&a.author_id)
                || !ids.insert(&a.article_id)
                || !["discovery", "reference", "confirmation"].contains(&a.split.as_str())
                || !a.source_url.starts_with("https://")
                || !a.verified
            {
                return Err(AppError::Parse(format!(
                    "样本元数据无效或未核验: {}",
                    a.article_id
                )));
            }
            let text = ds.read_article(path, a)?;
            let normalized: String = text.chars().filter(|c| !c.is_whitespace()).collect();
            if normalized.chars().count() < 300 || !hashes.insert(sha256(normalized.as_bytes())) {
                return Err(AppError::Parse(format!("正文过短或重复: {}", a.article_id)));
            }
        }
        Ok(ds)
    }

    pub fn read_article(&self, manifest: &Path, a: &Article) -> Result<String, AppError> {
        let relative = Path::new(&a.path);
        if relative
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
        {
            return Err(AppError::Parse("正文路径必须位于数据集内".into()));
        }
        let root = manifest.parent().unwrap_or(Path::new(".")).canonicalize()?;
        let p = root.join(relative).canonicalize()?;
        if !p.starts_with(&root) {
            return Err(AppError::Parse("正文符号链接越界".into()));
        }
        let bytes = std::fs::read(p)?;
        if sha256(&bytes) != a.sha256 {
            return Err(AppError::Parse(format!("正文SHA变化: {}", a.article_id)));
        }
        String::from_utf8(bytes).map_err(|_| AppError::Parse("正文不是UTF-8".into()))
    }

    pub fn selected(&self, pilot: bool) -> Vec<&Article> {
        let mut selected = Vec::new();
        let mut counts = std::collections::BTreeMap::<&str, usize>::new();
        for a in &self.articles {
            if a.split != "discovery" {
                continue;
            }
            let n = counts.entry(&a.author_id).or_default();
            if !pilot || *n < 2 {
                selected.push(a);
                *n += 1;
            }
        }
        selected
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reject_drift_escape_duplicates_and_keep_holdout_out() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("manifest.json");
        let text = "研究文章。".repeat(100);
        std::fs::write(d.path().join("a.md"), &text).unwrap();
        let a = Article {
            article_id: "a".into(),
            author_id: "author".into(),
            title: "t".into(),
            topic: "t".into(),
            split: "confirmation".into(),
            source_url: "https://example.com/a".into(),
            path: "a.md".into(),
            sha256: sha256(text.as_bytes()),
            verified: true,
        };
        let mut ds = Dataset {
            schema: 1,
            dataset_id: "test".into(),
            articles: vec![a.clone()],
        };
        std::fs::write(&p, serde_json::to_vec(&ds).unwrap()).unwrap();
        assert!(Dataset::load(&p).unwrap().selected(false).is_empty());
        ds.articles.push(Article {
            article_id: "b".into(),
            ..a.clone()
        });
        std::fs::write(&p, serde_json::to_vec(&ds).unwrap()).unwrap();
        assert!(Dataset::load(&p).is_err());
        assert!(
            ds.read_article(
                &p,
                &Article {
                    path: "../a.md".into(),
                    ..a.clone()
                }
            )
            .is_err()
        );
        std::fs::write(d.path().join("a.md"), "changed").unwrap();
        assert!(ds.read_article(&p, &a).is_err());
    }
}
