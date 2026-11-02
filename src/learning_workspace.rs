//! Draft previews and immutable named learning versions, independent of source acquisition.
use crate::{
    error::AppError,
    learning_candidate::{
        Candidate, CandidateChanges, CandidateInput, LearningVersion, VersionRef, identity, name,
    },
    learning_workspace_store as store,
};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

pub struct LearningWorkspace {
    root: PathBuf,
}

impl LearningWorkspace {
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }
    pub fn open(root: &Path) -> Result<Self, AppError> {
        if root.as_os_str().is_empty() {
            return Err(AppError::FileRead("工作区目录为空".into()));
        }
        Ok(Self {
            root: if root.is_absolute() {
                root.into()
            } else {
                std::env::current_dir()?.join(root)
            },
        })
    }

    /// Idempotently retain a ready-to-preview candidate; no model calls or style-library writes.
    pub fn stage(&self, input: CandidateInput) -> Result<Candidate, AppError> {
        input.validate()?;
        self.check_base(&input)?;
        let mut candidate = Candidate {
            schema: 1,
            id: String::new(),
            input,
        };
        candidate.id = identity(&candidate)?;
        let _lock = store::lock(&self.root)?;
        let path = store::candidate_path(&self.root, &candidate.id)?;
        if path.exists() {
            return self.preview(&candidate.id);
        }
        store::publish_json(&path, &candidate)?;
        Ok(candidate)
    }

    pub fn preview(&self, id: &str) -> Result<Candidate, AppError> {
        store::candidate(&self.root, id)
    }

    pub fn candidates(&self) -> Result<Vec<Candidate>, AppError> {
        let entries = match std::fs::read_dir(self.root.join("candidates")) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => return Err(e.into()),
        };
        let mut result = Vec::new();
        for entry in entries {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == "json") {
                let id = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .ok_or_else(|| AppError::LearningWorkspace("候选文件名无效".into()))?;
                result.push(self.preview(id)?);
            }
        }
        result.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(result)
    }

    pub fn catalog(&self) -> Result<Vec<LearningVersion>, AppError> {
        let entries = match std::fs::read_dir(self.root.join("versions")) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => return Err(e.into()),
        };
        let mut result = Vec::new();
        for entry in entries {
            let dir = entry?.path();
            let first = dir.join("v000001.json");
            if !first.exists() {
                if std::fs::read_dir(&dir)?
                    .any(|e| e.map_or(true, |e| e.path().extension().is_some_and(|s| s == "json")))
                {
                    return Err(AppError::LearningWorkspace("学习历史缺少第一版".into()));
                }
                continue;
            }
            let version: LearningVersion = store::read(&first)?;
            if dir != store::version_dir(&self.root, &version.name)? {
                return Err(AppError::LearningWorkspace("版本目录身份不符".into()));
            }
            if let Some(latest) = self.versions(&version.name)?.pop() {
                result.push(latest);
            }
        }
        result.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(result)
    }

    pub fn versions(&self, style_name: &str) -> Result<Vec<LearningVersion>, AppError> {
        store::history(&self.root, style_name)
    }

    pub fn version(&self, style_name: &str, number: u32) -> Result<LearningVersion, AppError> {
        self.versions(style_name)?
            .into_iter()
            .find(|v| v.number == number)
            .ok_or_else(|| {
                AppError::FileRead(format!("未找到{name}的第{number}版", name = style_name))
            })
    }

    pub fn save(&self, candidate_id: &str, style_name: &str) -> Result<LearningVersion, AppError> {
        let style_name = name(style_name)?;
        let _lock = store::lock(&self.root)?;
        let candidate = self.preview(candidate_id)?;
        self.check_base(&candidate.input)?;
        let history = self.versions(&style_name)?;
        let latest = history.last();
        match (&candidate.input.base, latest) {
            (None, None) => {}
            (Some(base), Some(current))
                if base == &current.reference() && base.name == style_name => {}
            (None, Some(_)) => {
                return Err(AppError::FileWrite(
                    "同名风格已存在，请以明确旧版本生成更新候选".into(),
                ));
            }
            _ => {
                return Err(AppError::FileWrite(
                    "候选基准已过期或名称不符，请查看最新版本后重新生成候选".into(),
                ));
            }
        }
        let number = latest
            .map_or(Some(1), |v| v.number.checked_add(1))
            .ok_or_else(|| AppError::FileWrite("版本数量超限".into()))?;
        let mut version = LearningVersion {
            schema: 1,
            id: String::new(),
            name: style_name.clone(),
            number,
            previous: latest.map(|v| v.id.clone()),
            saved_at: chrono::Utc::now().to_rfc3339(),
            status: "saved_draft_not_quality_certified".into(),
            candidate,
        };
        store::finalize(&mut version)?;
        store::publish_json(
            &store::version_dir(&self.root, &style_name)?.join(format!("v{number:06}.json")),
            &version,
        )?;
        Ok(version)
    }

    /// Export exactly the chosen spec; analysis, review and article snapshots remain in the workspace.
    pub fn export(
        &self,
        style_name: &str,
        number: u32,
        target: &Path,
    ) -> Result<PathBuf, AppError> {
        let version = self.version(style_name, number)?;
        if target.extension().is_none_or(|s| s != "md") {
            return Err(AppError::FileWrite("风格导出目标须为新.md文件".into()));
        }
        store::publish(target, version.candidate.input.draft.spec.as_bytes())?;
        Ok(target.into())
    }

    pub fn changes(&self, id: &str) -> Result<CandidateChanges, AppError> {
        let candidate = self.preview(id)?;
        let previous = self.check_base(&candidate.input)?;
        let known = previous
            .as_ref()
            .map(|v| {
                v.candidate
                    .input
                    .draft
                    .sources
                    .iter()
                    .map(|s| s.text_sha256.clone())
                    .collect::<HashSet<_>>()
            })
            .unwrap_or_default();
        let added_source_hashes = candidate
            .input
            .draft
            .sources
            .iter()
            .filter(|s| !known.contains(&s.text_sha256))
            .map(|s| s.text_sha256.clone())
            .collect();
        Ok(CandidateChanges {
            previous_articles: known.len(),
            current_articles: candidate.input.articles.len(),
            added_source_hashes,
            reason: candidate.input.note.clone(),
            text_diff: crate::learning_diff::diff(
                previous
                    .as_ref()
                    .map(|v| v.candidate.input.draft.spec.as_str())
                    .unwrap_or(""),
                &candidate.input.draft.spec,
            ),
        })
    }

    fn check_base(&self, input: &CandidateInput) -> Result<Option<LearningVersion>, AppError> {
        let Some(VersionRef { name, number, id }) = &input.base else {
            return Ok(None);
        };
        let parent = self.version(name, *number)?;
        if parent.id != *id {
            return Err(AppError::LearningWorkspace("候选基准版本身份不一致".into()));
        }
        let current = input
            .draft
            .sources
            .iter()
            .map(|s| s.text_sha256.as_str())
            .collect::<HashSet<_>>();
        if parent
            .candidate
            .input
            .draft
            .sources
            .iter()
            .any(|s| !current.contains(s.text_sha256.as_str()))
        {
            return Err(AppError::LearningWorkspace(
                "追加学习必须保留原有正文快照，不能减少或替换来源".into(),
            ));
        }
        Ok(Some(parent))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        learning_candidate::DraftOrigin,
        voice_learning::{VoicePrompts, learn_voice_with},
    };
    use std::fs;

    async fn input(count: usize) -> CandidateInput {
        let articles = (0..count)
            .map(|i| format!("第{i}篇不同的原始文本，需要保留这份来源。"))
            .collect::<Vec<_>>();
        let draft = learn_voice_with(
            &articles,
            &VoicePrompts {
                analysis: "分析".into(),
                compile: "编译".into(),
            },
            |stage, _, user| async move {
                Ok(if stage == "analysis" {
                    let input:serde_json::Value=serde_json::from_str(&user).unwrap();
                    serde_json::json!({"analysis":"AUDIT_ONLY_ANALYSIS","evidence":input["articles"].as_array().unwrap().iter().map(|a|serde_json::json!({"article":a["article"],"quote":a["text"]})).collect::<Vec<_>>()}).to_string()
                } else {"由已知材料支撑判断，根据表达责任调整叙述与推理。".into()})
            },
        )
        .await
        .unwrap();
        CandidateInput {
            articles,
            draft,
            origin: DraftOrigin::ModelOutput,
            note: "首次学习".into(),
            review: Some("AUDIT_ONLY_REVIEW".into()),
            base: None,
        }
    }

    #[tokio::test]
    async fn restart_preview_append_history_changes_and_writer_isolation() {
        let dir = tempfile::tempdir().unwrap();
        let ws = LearningWorkspace::open(dir.path()).unwrap();
        let one = ws.stage(input(1).await).unwrap();
        assert_eq!(ws.stage(one.input.clone()).unwrap().id, one.id);
        assert!(ws.versions("测试风格").unwrap().is_empty());
        let first = ws.save(&one.id, "测试风格").unwrap();
        let reopened = LearningWorkspace::open(dir.path()).unwrap();
        assert_eq!(reopened.preview(&one.id).unwrap().id, one.id);
        assert_eq!(reopened.candidates().unwrap().len(), 1);
        assert_eq!(reopened.catalog().unwrap()[0].id, first.id);
        let mut update = input(3).await;
        update.base = Some(first.reference());
        update.note = "新增两篇，保留原文并重新综合".into();
        update.draft.spec = "以判断和具体依据相互支撑，保留长段推理与随事短评。".into();
        let staged = reopened.stage(update).unwrap();
        let delta = reopened.changes(&staged.id).unwrap();
        assert_eq!(
            (
                delta.previous_articles,
                delta.current_articles,
                delta.added_source_hashes.len()
            ),
            (1, 3, 2)
        );
        assert!(delta.text_diff.contains("+以判断和具体依据"));
        let second = reopened.save(&staged.id, "测试风格").unwrap();
        assert_eq!(second.previous, Some(first.id.clone()));
        assert_eq!(
            reopened
                .version("测试风格", 1)
                .unwrap()
                .candidate
                .input
                .articles
                .len(),
            1
        );
        let out = dir.path().join("writer.md");
        reopened.export("测试风格", 2, &out).unwrap();
        assert_eq!(fs::read_to_string(&out).unwrap(), staged.input.draft.spec);
        assert!(!fs::read_to_string(&out).unwrap().contains("AUDIT_ONLY"));
        assert!(reopened.export("测试风格", 1, &out).is_err());
        assert_eq!(fs::read_to_string(&out).unwrap(), staged.input.draft.spec);
    }

    #[tokio::test]
    async fn stale_candidates_cannot_overwrite_newer_versions_and_names_cannot_escape() {
        let dir = tempfile::tempdir().unwrap();
        let ws = LearningWorkspace::open(dir.path()).unwrap();
        let initial = ws.stage(input(1).await).unwrap();
        assert!(ws.save(&initial.id, "../越界").is_err());
        assert!(ws.preview("../candidate").is_err());
        let first = ws.save(&initial.id, "同名").unwrap();
        assert!(ws.save(&initial.id, "同名").is_err());
        let mut a = input(2).await;
        a.base = Some(first.reference());
        a.note = "更新甲".into();
        let mut b = a.clone();
        b.note = "更新乙".into();
        b.draft.spec = "保留不同的议论与描写条件，不套固定开头。".into();
        let a = ws.stage(a).unwrap();
        let b = ws.stage(b).unwrap();
        let latest = ws.save(&a.id, "同名").unwrap();
        assert!(ws.save(&b.id, "同名").is_err());
        assert_eq!(ws.versions("同名").unwrap().last().unwrap().id, latest.id);
        assert_eq!(ws.preview(&b.id).unwrap().id, b.id);
    }

    #[tokio::test]
    async fn reject_source_changes_duplicates_and_tampering_without_losing_previous_version() {
        let dir = tempfile::tempdir().unwrap();
        let ws = LearningWorkspace::open(dir.path()).unwrap();
        let first = ws.stage(input(2).await).unwrap();
        let saved = ws.save(&first.id, "原文集合").unwrap();
        let mut reduced = input(1).await;
        reduced.base = Some(saved.reference());
        assert!(ws.stage(reduced).is_err());
        let mut duplicate = input(1).await;
        duplicate.articles.push(duplicate.articles[0].clone());
        assert!(ws.stage(duplicate).is_err());
        let mut mismatch = input(2).await;
        mismatch.articles[0].push_str("被替换");
        assert!(ws.stage(mismatch).is_err());
        let path = store::candidate_path(dir.path(), &first.id).unwrap();
        let mut broken = fs::read_to_string(&path).unwrap();
        broken = broken.replace("首次学习", "篡改记录");
        fs::write(path, broken).unwrap();
        assert!(ws.preview(&first.id).is_err());
        assert_eq!(ws.version("原文集合", 1).unwrap().id, saved.id);
        let vpath = store::version_dir(dir.path(), "原文集合")
            .unwrap()
            .join("v000001.json");
        fs::write(vpath, "{}").unwrap();
        assert!(ws.versions("原文集合").is_err());
    }

    #[tokio::test]
    async fn busy_lock_and_failed_version_publication_leave_candidate_recoverable() {
        let dir = tempfile::tempdir().unwrap();
        let ws = LearningWorkspace::open(dir.path()).unwrap();
        let staged = ws.stage(input(1).await).unwrap();
        let lock = store::lock(dir.path()).unwrap();
        assert!(ws.save(&staged.id, "写入失败").is_err());
        drop(lock);
        fs::write(dir.path().join("versions"), "阻止建目录").unwrap();
        assert!(ws.save(&staged.id, "写入失败").is_err());
        assert_eq!(ws.preview(&staged.id).unwrap().id, staged.id);
    }

    #[tokio::test]
    async fn unchanged_sources_can_be_revised_but_missing_history_is_not_silently_recreated() {
        let dir = tempfile::tempdir().unwrap();
        let ws = LearningWorkspace::open(dir.path()).unwrap();
        let staged = ws.stage(input(1).await).unwrap();
        let first = ws.save(&staged.id, "修订").unwrap();
        let mut revised = staged.input.clone();
        revised.base = Some(first.reference());
        revised.origin = DraftOrigin::UserEdit;
        revised.note = "只修订说明，不改变文章来源".into();
        revised.draft.spec = "材料薄时保留判断边界，不用新场景补足叙述。".into();
        let revised = ws.stage(revised).unwrap();
        assert!(
            ws.changes(&revised.id)
                .unwrap()
                .added_source_hashes
                .is_empty()
        );
        let second = ws.save(&revised.id, "修订").unwrap();
        assert_eq!(second.number, 2);
        fs::remove_file(
            store::version_dir(dir.path(), "修订")
                .unwrap()
                .join("v000001.json"),
        )
        .unwrap();
        assert!(ws.versions("修订").is_err());
        assert!(ws.catalog().is_err());
        assert_eq!(ws.preview(&revised.id).unwrap().id, revised.id);
    }

    #[tokio::test]
    async fn invalid_imports_do_not_create_workspace_and_ten_articles_are_supported() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("not-created-yet");
        let ws = LearningWorkspace::open(&root).unwrap();
        let ten = input(10).await;
        let mut too_many = ten.clone();
        too_many.articles.push("第十一篇不同的正文".into());
        assert!(matches!(
            ws.stage(too_many),
            Err(AppError::LearningWorkspace(_))
        ));
        assert!(!root.exists());
        let mut falsely_approved = ten.clone();
        falsely_approved.draft.status = "approved".into();
        assert!(ws.stage(falsely_approved).is_err());
        assert!(!root.exists());
        assert_eq!(ws.stage(ten).unwrap().input.articles.len(), 10);
    }
}
