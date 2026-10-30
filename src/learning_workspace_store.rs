use crate::{
    error::AppError,
    learning_candidate::{Candidate, LearningVersion, identity, name, valid_id},
    voice_learning::sha256,
};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

pub(crate) fn read<T: DeserializeOwned>(path: &Path) -> Result<T, AppError> {
    serde_json::from_slice(&fs::read(path)?)
        .map_err(|e| AppError::LearningWorkspace(format!("{}: {e}", path.display())))
}

/// Publish a complete file without replacing an existing path, even under another process.
pub(crate) fn publish(path: &Path, bytes: &[u8]) -> Result<(), AppError> {
    let parent = path
        .parent()
        .ok_or_else(|| AppError::FileWrite("目标缺少目录".into()))?;
    fs::create_dir_all(parent)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| AppError::FileWrite(e.to_string()))?
        .as_nanos();
    let pending = parent.join(format!(".learning-{}-{stamp}.pending", std::process::id()));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&pending)?;
    let result = file.write_all(bytes).and_then(|_| file.sync_all());
    drop(file);
    let result = result.and_then(|_| fs::hard_link(&pending, path));
    let cleanup = fs::remove_file(&pending);
    match (result, cleanup) {
        (Ok(()), Ok(())) => Ok(()),
        (Ok(()), Err(e)) => Err(AppError::FileWrite(format!(
            "已发布{}，但临时文件清理失败: {e}",
            path.display()
        ))),
        (Err(e), Ok(())) => Err(AppError::FileWrite(format!(
            "未覆盖{}: {e}",
            path.display()
        ))),
        (Err(e), Err(cleanup)) => Err(AppError::FileWrite(format!(
            "发布失败: {e}；临时文件清理失败: {cleanup}"
        ))),
    }
}

pub(crate) fn publish_json(path: &Path, value: &impl Serialize) -> Result<(), AppError> {
    let bytes =
        serde_json::to_vec_pretty(value).map_err(|e| AppError::LearningWorkspace(e.to_string()))?;
    publish(path, &bytes)
}

pub(crate) fn lock(root: &Path) -> Result<File, AppError> {
    fs::create_dir_all(root)?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join(".workspace.lock"))?;
    file.try_lock()
        .map_err(|_| AppError::FileWrite("另一个草稿操作正在进行，请稍后重试".into()))?;
    Ok(file)
}

pub(crate) fn candidate_path(root: &Path, id: &str) -> Result<PathBuf, AppError> {
    valid_id(id)?;
    Ok(root.join("candidates").join(format!("{id}.json")))
}

pub(crate) fn candidate(root: &Path, id: &str) -> Result<Candidate, AppError> {
    let result: Candidate = read(&candidate_path(root, id)?)?;
    result.verify()?;
    if result.id != id {
        return Err(AppError::LearningWorkspace("候选文件名身份不符".into()));
    }
    Ok(result)
}

pub(crate) fn version_dir(root: &Path, style_name: &str) -> Result<PathBuf, AppError> {
    Ok(root
        .join("versions")
        .join(sha256(name(style_name)?.as_bytes())))
}

pub(crate) fn history(root: &Path, style_name: &str) -> Result<Vec<LearningVersion>, AppError> {
    let style_name = name(style_name)?;
    let dir = version_dir(root, &style_name)?;
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e.into()),
    };
    let mut versions = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if path.extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        let version: LearningVersion = read(&path)?;
        version.verify()?;
        if version.name != style_name
            || path.file_name().and_then(|s| s.to_str())
                != Some(&format!("v{:06}.json", version.number))
        {
            return Err(AppError::LearningWorkspace(
                "学习版本文件名或名称不一致".into(),
            ));
        }
        versions.push(version);
    }
    versions.sort_by_key(|v| v.number);
    for (index, version) in versions.iter().enumerate() {
        let expected_previous = index.checked_sub(1).map(|i| versions[i].id.as_str());
        let expected_base = index.checked_sub(1).map(|i| versions[i].reference());
        if version.number as usize != index + 1
            || version.previous.as_deref() != expected_previous
            || version.candidate.input.base != expected_base
        {
            return Err(AppError::LearningWorkspace(
                "学习版本历史断链或基准版本不符".into(),
            ));
        }
    }
    Ok(versions)
}

pub(crate) fn finalize(version: &mut LearningVersion) -> Result<(), AppError> {
    version.id = identity(version)?;
    version.verify()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn publication_never_overwrites_and_pending_files_do_not_survive_failures() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("existing.md");
        fs::write(&target, "旧版").unwrap();
        assert!(publish(&target, "新版".as_bytes()).is_err());
        assert_eq!(fs::read_to_string(&target).unwrap(), "旧版");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
        let blocked = dir.path().join("blocked");
        fs::write(&blocked, "阻止建目录").unwrap();
        assert!(publish(&blocked.join("new.json"), b"{}").is_err());
    }
}
