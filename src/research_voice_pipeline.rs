//! Content separation and independent new-topic probes for the prose learner.
use crate::{
    error::AppError,
    eval_store::write_json,
    generate::style_system_prompt,
    io::write_file_atomic,
    research_compare::finish_tasks,
    research_dataset::{sha256, valid_id},
    research_store::{ResearchStore, Role},
    research_validation::validated_as,
    research_voice_prose::{self, Job},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{path::Path, sync::Arc};

const ROOT: &str = "research/style-aggregation";
const PROSE: &str = "voice-learning-prose-01";

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Removed {
    excerpt: String,
    reason: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Cleaned {
    spec: String,
    removed: Vec<Removed>,
    scope_notes: String,
}

fn markers(id: &str) -> &'static [&'static str] {
    match id {
        "wxb-k1" => &[
            "医院",
            "病人",
            "卫生员",
            "手术",
            "猪大肠",
            "钟表",
            "电话机",
            "化验",
        ],
        "wxb-k2" => &[
            "你猜怎么说",
            "我怀疑这么理解是不对的",
            "孟子",
            "格调",
            "医院",
            "手术",
        ],
        "wzq-k1" => &[
            "我想念",
            "不是怀人",
            "不是思乡",
            "雨珠",
            "缅桂",
            "菌子",
            "昆明",
            "带着水珠的花",
        ],
        "wzq-k2" => &["编辑工作", "一道菜", "烹饪", "菜肴", "昆明", "知味"],
        _ => &[],
    }
}

fn parse_clean(raw: &str, draft: &str, job: &Job) -> Result<Cleaned, AppError> {
    let cleaned: Cleaned = serde_json::from_str(raw.trim())
        .map_err(|e| AppError::Parse(format!("内容剥离JSON无效: {e}")))?;
    research_voice_prose::validate(&cleaned.spec, job)?;
    if cleaned.scope_notes.trim().is_empty()
        || cleaned.removed.iter().any(|r| {
            r.excerpt.trim().is_empty()
                || r.reason.trim().is_empty()
                || !draft.contains(&r.excerpt)
                || cleaned.spec.contains(&r.excerpt)
        })
        || markers(&job.id)
            .iter()
            .any(|marker| cleaned.spec.contains(marker))
    {
        return Err(AppError::Parse(
            "删除记录无法回查、仍在正文或源文内容哨点未去除".into(),
        ));
    }
    Ok(cleaned)
}

fn require_complete(run: &str, count: usize) -> Result<(), AppError> {
    let summary: Value =
        serde_json::from_slice(&std::fs::read(format!("{ROOT}/runs/{run}/summary.json"))?)
            .map_err(|e| AppError::Parse(e.to_string()))?;
    if summary["status"] != "complete"
        || summary["rows"]
            .as_array()
            .is_none_or(|rows| rows.len() != count)
    {
        return Err(AppError::Parse("依赖run未完整，不启动后续步骤".into()));
    }
    Ok(())
}

pub(crate) async fn clean(run_id: &str) -> Result<(), AppError> {
    require_complete(PROSE, 4)?;
    let jobs = research_voice_prose::jobs()?;
    let prompt = std::fs::read_to_string(format!("{ROOT}/prompts/voice-clean-v1.md"))?;
    let mut inputs = Vec::new();
    for job in &jobs {
        let path = format!("{ROOT}/runs/{PROSE}/profiles/{}", job.id);
        let draft = std::fs::read_to_string(format!("{path}.md"))?;
        let saved: Value = serde_json::from_slice(&std::fs::read(format!("{path}.json"))?)
            .map_err(|e| AppError::Parse(e.to_string()))?;
        if saved["spec"].as_str() != Some(&draft) {
            return Err(AppError::Parse("分析阅读副本身份不符".into()));
        }
        inputs.push(json!({"job":job,"draft":draft,"markers":markers(&job.id)}));
    }
    let store = Arc::new(ResearchStore::open(Path::new(ROOT), run_id, false)?);
    store.limit_run(1_000_000).await?;
    store.freeze(
        &json!({"suite":"voice_content_separation_v1","source_run":PROSE,"inputs":inputs,
        "prompt":prompt,"model":Role::FlashAnalysis.model(),"run_cap_micro_cny":1_000_000,
        "method_sha256":sha256(&std::fs::read(format!("{ROOT}/method-revision-23.md"))?)}),
    )?;
    std::fs::create_dir_all(store.root.join("profiles"))?;
    let mut tasks = tokio::task::JoinSet::<Result<Value, AppError>>::new();
    for (job, input) in jobs.into_iter().zip(inputs) {
        let store = Arc::clone(&store);
        let prompt = prompt.clone();
        tasks.spawn(async move {
            let name = job.id.clone();
            let draft = input["draft"].as_str().ok_or_else(|| AppError::Parse("缺少分析稿".into()))?;
            let user = json!({"analysis":draft,"article_count":job.sources.len()}).to_string();
            match validated_as(&store, &name, Role::FlashAnalysis, &prompt, &user,
                |raw| parse_clean(raw, draft, &job)).await {
                Ok(cleaned) => {
                    write_json(&store.root.join(format!("profiles/{name}.json")),
                        &json!({"id":name,"source_run":PROSE,"draft_sha256":sha256(draft.as_bytes()),
                            "source_ids":job.sources.iter().map(|s| &s.source_id).collect::<Vec<_>>(),"result":cleaned}))?;
                    write_file_atomic(&store.root.join(format!("profiles/{name}.md")), &cleaned.spec)?;
                    Ok(json!({"id":name,"status":"complete","removed":cleaned.removed.len()}))
                }
                Err(error) => Ok(json!({"id":name,"status":"failed","error":error.to_string()})),
            }
        });
    }
    finish_tasks(&store, &mut tasks, "voice_content_separation").await
}

pub(crate) async fn clean_pro(run_id: &str) -> Result<(), AppError> {
    require_complete(PROSE, 4)?;
    let jobs = research_voice_prose::jobs()?;
    let prompt = std::fs::read_to_string(format!("{ROOT}/prompts/voice-clean-pro-v2.md"))?;
    let mut inputs = Vec::new();
    for job in &jobs {
        let path = format!("{ROOT}/runs/{PROSE}/profiles/{}", job.id);
        let draft = std::fs::read_to_string(format!("{path}.md"))?;
        let saved: Value = serde_json::from_slice(&std::fs::read(format!("{path}.json"))?)
            .map_err(|e| AppError::Parse(e.to_string()))?;
        if saved["spec"].as_str() != Some(&draft) {
            return Err(AppError::Parse("分析阅读副本身份不符".into()));
        }
        inputs.push(json!({"job":job,"draft":draft,"markers":markers(&job.id)}));
    }
    let store = Arc::new(ResearchStore::open(Path::new(ROOT), run_id, false)?);
    store.limit_run(2_000_000).await?;
    store.freeze(
        &json!({"suite":"voice_content_separation_pro_v2","source_run":PROSE,"inputs":inputs,
        "prompt":prompt,"model":Role::Structured.model(),"run_cap_micro_cny":2_000_000,
        "method_sha256":sha256(&std::fs::read(format!("{ROOT}/method-revision-24.md"))?)}),
    )?;
    std::fs::create_dir_all(store.root.join("profiles"))?;
    let mut tasks = tokio::task::JoinSet::<Result<Value, AppError>>::new();
    for (job, input) in jobs.into_iter().zip(inputs) {
        let store = Arc::clone(&store);
        let prompt = prompt.clone();
        tasks.spawn(async move {
            let name = job.id.clone();
            let draft = input["draft"].as_str().ok_or_else(|| AppError::Parse("缺分析稿".into()))?;
            let original = json!({"analysis":draft,"article_count":job.sources.len(),
                "diagnosed_content":markers(&name)}).to_string();
            let mut user = original.clone();
            for attempt in 0..2 {
                let raw = match store.call(&format!("{name}-{attempt}"), Role::Structured,
                    &prompt, &user, false).await {
                    Ok(raw) => raw,
                    Err(error) => return Ok(json!({"id":name,"status":"failed","error":error.to_string()})),
                };
                let spec = raw.trim();
                let check = research_voice_prose::validate(spec, &job).and_then(|()| {
                    if markers(&name).iter().any(|marker| spec.contains(marker)) {
                        Err(AppError::Parse("已定位的源文内容仍在执行说明".into()))
                    } else { Ok(()) }
                });
                match check {
                    Ok(()) => {
                        write_json(&store.root.join(format!("profiles/{name}.json")),
                            &json!({"id":name,"source_run":PROSE,"draft_sha256":sha256(draft.as_bytes()),
                                "source_ids":job.sources.iter().map(|s| &s.source_id).collect::<Vec<_>>(),
                                "result":{"spec":spec},"status":"draft_pending_semantic_review"}))?;
                        write_file_atomic(&store.root.join(format!("profiles/{name}.md")), spec)?;
                        return Ok(json!({"id":name,"status":"complete"}));
                    }
                    Err(error) if attempt == 0 => {
                        user = format!("原始任务：{original}\n上次候选：{spec}\n程序诊断：{error}\n请重新输出完整Markdown，只修复内容泄漏或证据范围，不增加规则。")
                    }
                    Err(error) => return Ok(json!({"id":name,"status":"failed","error":error.to_string()})),
                }
            }
            Err(AppError::Parse("内容编译次数耗尽".into()))
        });
    }
    finish_tasks(&store, &mut tasks, "voice_content_separation_pro").await
}

pub(crate) async fn probe(run_id: &str, source_run: &str) -> Result<(), AppError> {
    if !valid_id(source_run) {
        return Err(AppError::Parse("Style来源run无效".into()));
    }
    require_complete(source_run, 4)?;
    let task_bytes = std::fs::read(format!("{ROOT}/datasets/voice-probe-tasks-v1.json"))?;
    let suite: Value =
        serde_json::from_slice(&task_bytes).map_err(|e| AppError::Parse(e.to_string()))?;
    let tasks = suite["tasks"]
        .as_array()
        .filter(|tasks| tasks.len() == 2)
        .ok_or_else(|| AppError::Parse("新题材计划必须两题".into()))?;
    let mut jobs = Vec::new();
    for id in ["wxb-k1", "wxb-k2", "wzq-k1", "wzq-k2"] {
        let path = format!("{ROOT}/runs/{source_run}/profiles/{id}");
        let spec = std::fs::read_to_string(format!("{path}.md"))?;
        let saved: Value = serde_json::from_slice(&std::fs::read(format!("{path}.json"))?)
            .map_err(|e| AppError::Parse(e.to_string()))?;
        if saved["result"]["spec"].as_str() != Some(&spec) {
            return Err(AppError::Parse("执行Style副本身份不符".into()));
        }
        for task in tasks {
            let task_id = task["id"]
                .as_str()
                .ok_or_else(|| AppError::Parse("缺题目ID".into()))?;
            let user = format!(
                "{}\n素材：{}\n要求：{}",
                suite["instruction"].as_str().unwrap_or(""),
                task["facts"].as_str().unwrap_or(""),
                task["requirements"].as_str().unwrap_or("")
            );
            jobs.push(
                json!({"id":format!("{id}-{task_id}"),"style_id":id,"topic":task_id,
                "spec":spec,"system":style_system_prompt(&spec),"user":user}),
            );
        }
    }
    let store = Arc::new(ResearchStore::open(Path::new(ROOT), run_id, false)?);
    store.limit_run(1_000_000).await?;
    store.freeze(&json!({"suite":"voice_learning_probe_v1","jobs":jobs,"source_run":source_run,
        "tasks_sha256":sha256(&task_bytes),"model":Role::Writer.model(),"run_cap_micro_cny":1_000_000}))?;
    std::fs::create_dir_all(store.root.join("articles"))?;
    let mut pending = tokio::task::JoinSet::<Result<Value, AppError>>::new();
    for job in jobs {
        let store = Arc::clone(&store);
        pending.spawn(async move {
            let id = job["id"]
                .as_str()
                .ok_or_else(|| AppError::Parse("缺少用稿ID".into()))?;
            match store
                .call(
                    id,
                    Role::Writer,
                    job["system"].as_str().unwrap_or(""),
                    job["user"].as_str().unwrap_or(""),
                    false,
                )
                .await
            {
                Ok(body) => {
                    write_json(
                        &store.root.join(format!("articles/{id}.json")),
                        &json!({"job":job,"body":body}),
                    )?;
                    write_file_atomic(&store.root.join(format!("articles/{id}.md")), &body)?;
                    Ok(json!({"id":id,"status":"complete"}))
                }
                Err(error) => Ok(json!({"id":id,"status":"failed","error":error.to_string()})),
            }
        });
    }
    finish_tasks(&store, &mut pending, "voice_learning_probe").await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn removed_examples_must_be_traceable_and_absent_from_spec() {
        let job = Job {
            id: "wxb-k1".into(),
            sources: vec![],
            cards: Default::default(),
        };
        let draft = "用手术室的现场解释叙述与判断。";
        let v = json!({"spec":"用任务已给的过程带出判断，缺少过程就保留清楚的推理。",
            "removed":[{"excerpt":"手术室的现场","reason":"来源场景"}],"scope_notes":"单篇局部观察"});
        assert!(parse_clean(&v.to_string(), draft, &job).is_ok());
        let mut broken = v;
        broken["spec"] = json!("用医院里的过程带出判断。");
        assert!(parse_clean(&broken.to_string(), draft, &job).is_err());
    }
}
