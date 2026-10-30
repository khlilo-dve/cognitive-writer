//! Offline CLI for candidates already produced by the text-learning core.
use crate::{
    error::AppError,
    learning_candidate::{Candidate, CandidateInput, DraftOrigin},
    learning_workspace::LearningWorkspace,
};
use std::path::{Path, PathBuf};

const HELP: &str = "用法：cognitive-writer style-draft [--root <工作区>] <命令>\n\
  import <候选包.json> [--base <风格名> <版本>]  导入并预览待审候选，不调用模型\n\
  preview <候选ID>                             重新查看候选\n\
  candidates                                  查找保留的候选（含已保存候选）\n\
  list                                        查看已命名风格及最新版本\n\
  diff <候选ID>                                查看来源数量、说明差异及变化理由\n\
  save <候选ID> <风格名>                        保存新的不可覆盖版本\n\
  versions <风格名>                            查看历史\n\
  show <风格名> <版本>                         查看指定版本\n\
  export <风格名> <版本> <新文件.md>            仅导出执行说明，拒绝覆盖\n\
候选包包含articles正文数组、draft、origin、note，可选review/base。\n\
文件/URL采集仍在后续入口；保存与导出均不表示质量已验收。";

fn origin(origin: &DraftOrigin) -> &'static str {
    match origin {
        DraftOrigin::ModelOutput => "模型输出",
        DraftOrigin::ImplementerEdit => "实现者修订",
        DraftOrigin::UserEdit => "用户修订",
    }
}

fn preview(candidate: &Candidate) {
    println!(
        "candidate_id: {}\n状态：待审草稿\n正文：{}篇\n来源：{}\n变化理由：{}",
        candidate.id,
        candidate.input.articles.len(),
        origin(&candidate.input.origin),
        candidate.input.note
    );
    if let Some(base) = &candidate.input.base {
        println!("基准：{} 第{}版", base.name, base.number);
    }
    for warning in &candidate.input.draft.warnings {
        println!("提示：{warning}");
    }
    println!("\n{}", candidate.input.draft.spec);
}

fn number(text: &str) -> Result<u32, AppError> {
    text.parse::<u32>()
        .ok()
        .filter(|n| *n > 0)
        .ok_or_else(|| AppError::LearningWorkspace("版本须为正整数".into()))
}

fn import(ws: &LearningWorkspace, file: &str, base: Option<(&str, &str)>) -> Result<(), AppError> {
    let mut input: CandidateInput = serde_json::from_slice(&std::fs::read(file)?)
        .map_err(|e| AppError::LearningWorkspace(format!("候选包无效: {e}")))?;
    if let Some((name, version)) = base {
        if input.base.is_some() {
            return Err(AppError::LearningWorkspace(
                "候选包已有基准，不能用参数覆盖".into(),
            ));
        }
        input.base = Some(ws.version(name, number(version)?)?.reference());
    }
    preview(&ws.stage(input)?);
    Ok(())
}

pub fn run(args: &[&str]) -> Result<(), AppError> {
    if args.is_empty() || args == ["--help"] {
        println!("{HELP}");
        return Ok(());
    }
    let (root, args) = match args {
        ["--root", root, rest @ ..] => (PathBuf::from(root), rest),
        _ => (
            dirs::home_dir()
                .ok_or_else(|| AppError::FileRead("无法定位用户目录，请用--root指定工作区".into()))?
                .join(".cognitive-writer/learning"),
            args,
        ),
    };
    let ws = LearningWorkspace::open(&root)?;
    match args {
        ["--help"] => println!("{HELP}"),
        ["import", file] => import(&ws, file, None)?,
        ["import", file, "--base", name, version] => import(&ws, file, Some((name, version)))?,
        ["preview", id] => preview(&ws.preview(id)?),
        ["candidates"] => {
            for candidate in ws.candidates()? {
                println!(
                    "{}｜{}篇｜{}｜{}",
                    candidate.id,
                    candidate.input.articles.len(),
                    origin(&candidate.input.origin),
                    candidate.input.note
                );
            }
        }
        ["list"] => {
            for version in ws.catalog()? {
                println!(
                    "{}｜第{}版｜{}篇｜保存的待审草稿",
                    version.name,
                    version.number,
                    version.candidate.input.articles.len()
                );
            }
        }
        ["diff", id] => {
            let changes = ws.changes(id)?;
            println!(
                "正文：{} → {}篇；新增{}篇\n变化理由：{}\n- 表示旧说明，+ 表示新说明\n{}",
                changes.previous_articles,
                changes.current_articles,
                changes.added_source_hashes.len(),
                changes.reason,
                changes.text_diff
            );
        }
        ["save", id, name] => {
            let version = ws.save(id, name)?;
            println!(
                "version: {}\n已保存：{} 第{}版（待审，未自动导出）\ncandidate_id: {}",
                version.number, version.name, version.number, version.candidate.id
            );
        }
        ["versions", name] => {
            let versions = ws.versions(name)?;
            if versions.is_empty() {
                println!("尚无保存版本：{name}");
            }
            for version in versions {
                println!(
                    "第{}版｜{}篇｜{}｜{}｜{}",
                    version.number,
                    version.candidate.input.articles.len(),
                    origin(&version.candidate.input.origin),
                    version.saved_at,
                    version.candidate.input.note
                );
            }
        }
        ["show", name, version] => {
            let version = ws.version(name, number(version)?)?;
            println!("{} 第{}版：保存的待审草稿\n", version.name, version.number);
            preview(&version.candidate);
        }
        ["export", name, version, target] => {
            let path = ws.export(name, number(version)?, Path::new(target))?;
            println!(
                "已导出选定版本的执行说明：{}；未导出原文、分析或审阅。",
                path.display()
            );
        }
        _ => return Err(AppError::LearningWorkspace(HELP.into())),
    }
    Ok(())
}
