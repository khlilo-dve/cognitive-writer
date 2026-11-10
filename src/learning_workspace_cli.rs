//! Text-learning jobs and offline candidate/version commands.
use crate::learning_job::{self, JobPrompts, LearningJob};
use crate::learning_transport::{self, LearningClient};
use crate::{
    error::AppError,
    learning_candidate::{Candidate, CandidateInput, DraftOrigin},
    learning_workspace::LearningWorkspace,
};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::{io::Write, sync::Arc};

const HELP: &str = "用法：cognitive-writer style-draft [--root <工作区>] <命令>\n\
  chat                                       P5学习/候选/版本/写作开发入口，共用唯一stdin\n\
  agent-start <委派.json>                      启动材料决策Agent，复杂情况调用Flash\n\
  agent <任务ID> / agent-cancel <任务ID>         查看／取消领域任务，不读取stdin\n\
  agent-resume <任务ID> [--retry-uncertain]      恢复任务，保留已完成决策与材料\n\
  agent-answer <任务ID> <回复.json>              回答当前范围问题并继续\n\
  collect <来源清单.json>                      采集TXT/MD/正文/URL并保存来源快照\n\
  collection <来源集合ID>                      查看快照与选文／完整性状态\n\
  export-texts <来源集合ID> <新正文包.json>      仅在范围满足时导出，不调用模型\n\
  learn <正文包.json>                          从1–10篇正文开始学习\n\
  append <风格名> <版本> <新正文包.json>         保留旧正文、追加并重新学习\n\
  jobs / job <任务ID>                          查看保留任务与阶段用量\n\
  resume <任务ID> [--retry-uncertain]            恢复；不确定请求需显式允许重试\n\
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
来源清单为sources/note，可选expected_articles/allow_partial；多文选文与子Agent对话请用chat；DOCX/PDF仍待P6。保存与导出均不表示质量已验收。";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TextSet {
    articles: Vec<String>,
    note: String,
}

fn text_set(path: &str) -> Result<TextSet, AppError> {
    serde_json::from_slice(&std::fs::read(path)?)
        .map_err(|e| AppError::LearningWorkspace(format!("正文包无效: {e}")))
}

async fn run_job(ws: &LearningWorkspace, job: LearningJob, retry: bool) -> Result<(), AppError> {
    let client = Arc::new(LearningClient::from_env()?);
    if client.profile() != job.profile {
        return Err(AppError::LearningWorkspace(
            "端点/模型变化，未恢复此任务".into(),
        ));
    }
    let root = learning_job::directory(ws, &job.id)?;
    learning_transport::statistics(&root)?;
    println!(
        "job_id: {}\n学习任务已保留；完成后请预览候选再保存。",
        job.id
    );
    std::io::stdout().flush()?;
    let id = job.id.clone();
    let shared = Arc::new(job);
    let result = tokio::select! {
        result=learning_job::run_with(ws,&id,|stage,system,user| {
            let client=Arc::clone(&client);let root=root.clone();let job=Arc::clone(&shared);
            async move {client.call(&root,&job,stage,system,user,retry).await}
        })=>result,
        _=tokio::signal::ctrl_c()=>Err(AppError::LearningWorkspace("学习已中断，阶段记录保留；可用resume恢复，未保存新版本".into())),
    };
    preview(&result?);
    Ok(())
}

fn show_job(ws: &LearningWorkspace, id: &str) -> Result<(), AppError> {
    let job = learning_job::load(ws, id)?;
    let state = learning_job::state(ws, id)?;
    let stats = learning_transport::statistics(&learning_job::directory(ws, id)?)?;
    println!(
        "job_id: {id}\n状态快照：{}（running不保证进程仍在运行）\n正文：{}篇\n模型：{}\n已收到响应：{}\n响应未知的请求：{}\n已记录输入/输出tokens：{} / {}",
        state.status,
        job.articles.len(),
        job.profile.model,
        stats["responses"],
        stats["uncertain_attempts"],
        stats["known_prompt_tokens"],
        stats["known_completion_tokens"]
    );
    if let Some(candidate) = state.candidate_id {
        println!("candidate_id: {candidate}");
    }
    Ok(())
}

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

pub async fn run(args: &[&str]) -> Result<(), AppError> {
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
        ["chat"] => crate::repl::run_learning_session(ws.root()).await?,
        ["agent-start", file] => crate::learning_agent_cli::start(&ws, Path::new(file)).await?,
        ["agent", id] => crate::learning_agent_cli::inspect(&ws, id)?,
        ["agent-resume", id] => crate::learning_agent_cli::resume(&ws, id, false).await?,
        ["agent-resume", id, "--retry-uncertain"] => {
            crate::learning_agent_cli::resume(&ws, id, true).await?
        }
        ["agent-answer", id, file] => {
            crate::learning_agent_cli::answer(&ws, id, Path::new(file)).await?
        }
        ["agent-cancel", id] => crate::learning_agent_cli::cancel(&ws, id)?,
        ["collect", file] => crate::learning_source_cli::collect(&ws, Path::new(file)).await?,
        ["collection", id] => crate::learning_source_cli::show(&ws, id)?,
        ["export-texts", id, target] => {
            crate::learning_source_cli::export(&ws, id, Path::new(target))?
        }
        ["learn", file] => {
            let set = text_set(file)?;
            let client = LearningClient::from_env()?;
            let job = learning_job::prepare(
                &ws,
                set.articles,
                set.note,
                None,
                client.profile(),
                JobPrompts::load()?,
            )?;
            run_job(&ws, job, false).await?;
        }
        ["append", name, version, file] => {
            let mut set = text_set(file)?;
            if set.articles.is_empty() {
                return Err(AppError::LearningWorkspace("追加至少需要一篇新正文".into()));
            }
            let parent = ws.version(name, number(version)?)?;
            let base = parent.reference();
            let mut articles = parent.candidate.input.articles;
            articles.append(&mut set.articles);
            let client = LearningClient::from_env()?;
            let job = learning_job::prepare(
                &ws,
                articles,
                set.note,
                Some(base),
                client.profile(),
                JobPrompts::load()?,
            )?;
            run_job(&ws, job, false).await?;
        }
        ["resume", id] => run_job(&ws, learning_job::load(&ws, id)?, false).await?,
        ["resume", id, "--retry-uncertain"] => {
            run_job(&ws, learning_job::load(&ws, id)?, true).await?
        }
        ["jobs"] => {
            for (job, state) in learning_job::list(&ws)? {
                println!(
                    "{}｜{}篇｜{}｜{}",
                    job.id,
                    job.articles.len(),
                    state.status,
                    job.note
                );
            }
        }
        ["job", id] => show_job(&ws, id)?,
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
