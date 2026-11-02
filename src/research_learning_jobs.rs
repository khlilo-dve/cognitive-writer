//! Four real development jobs use product orchestration and journals with the shared research budget.
use crate::{
    error::AppError,
    eval_store::write_json,
    learning_job::{self, JobPrompts},
    learning_transport::{self, LearningClient, LearningResponse},
    learning_workspace::LearningWorkspace,
    learning_workspace_store as local,
    llm::request_body,
    research_compare::finish_tasks,
    research_receipt::{Checkpoint, verify},
    research_store::{ResearchStore, Role},
    voice_learning::sha256,
};
use serde_json::{Value, json};
use std::{path::Path, sync::Arc};

const ROOT: &str = "research/style-aggregation";

pub(crate) async fn run(run_id: &str, retry: bool) -> Result<(), AppError> {
    let bytes = std::fs::read(format!("{ROOT}/runs/voice-learning-core-01/inputs.json"))?;
    let source: Value =
        serde_json::from_slice(&bytes).map_err(|e| AppError::Parse(e.to_string()))?;
    let mut pairs = Vec::new();
    for (code, ids) in [("wxb", ["wxb-02", "wxb-03"]), ("wzq", ["wzq-02", "wzq-04"])] {
        let job = source["jobs"]
            .as_array()
            .and_then(|jobs| jobs.iter().find(|j| j["id"] == format!("{code}-k3")))
            .ok_or_else(|| AppError::Parse("缺少冻结三篇来源".into()))?;
        let source_ids = job["source_ids"]
            .as_array()
            .ok_or_else(|| AppError::Parse("缺来源身份".into()))?;
        let texts = ids
            .iter()
            .map(|id| {
                let index = source_ids
                    .iter()
                    .position(|v| v == id)
                    .ok_or_else(|| AppError::Parse("缺指定正文".into()))?;
                job["texts"][index]
                    .as_str()
                    .map(str::to_string)
                    .ok_or_else(|| AppError::Parse("正文无效".into()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        pairs.push(json!({"id":code,"source_ids":ids,"texts":texts}));
    }
    let client = LearningClient::from_env()?;
    let profile = client.profile();
    let prompts = JobPrompts::load()?;
    let store = Arc::new(ResearchStore::open(Path::new(ROOT), run_id, false)?);
    store.limit_run(3_000_000).await?;
    store.freeze(&json!({"suite":"product_learning_jobs_live_v1","pairs":pairs,"profile":profile,"prompts":prompts,
        "workspace_layout":"per-author-v1",
        "source_inputs_sha256":sha256(&bytes),"learning_method":crate::voice_learning::METHOD,
        "revision_method":crate::voice_revision::METHOD,"run_cap_micro_cny":3_000_000}))?;
    std::fs::create_dir_all(store.root.join("links"))?;
    let mut tasks = tokio::task::JoinSet::new();
    for pair in pairs {
        let store = Arc::clone(&store);
        let prompts = prompts.clone();
        let profile = profile.clone();
        tasks.spawn(async move {
            let code=pair["id"].as_str().ok_or_else(||AppError::Parse("缺作者链身份".into()))?;
            let ws=LearningWorkspace::open(&store.root.join("workspaces").join(code))?;
            let texts:Vec<String>=serde_json::from_value(pair["texts"].clone()).map_err(|e|AppError::Parse(e.to_string()))?;
            let mut results=Vec::new();let mut base=None;
            for count in [1usize,2] {
                let link=store.root.join(format!("links/{code}-k{count}.json"));
                let job=if link.exists() {
                    let saved:Value=local::read(&link)?;
                    learning_job::load(&ws,saved["job_id"].as_str().ok_or_else(||AppError::Parse("任务链接损坏".into()))?)?
                } else {
                    let job=learning_job::prepare(&ws,texts[..count].to_vec(),format!("开发验证：{count}篇完整正文，结果待审"),
                        base.clone(),profile.clone(),prompts.clone())?;
                    local::publish_json(&link,&json!({"job_id":job.id}))?;job
                };
                let job_root=learning_job::directory(&ws,&job.id)?;
                learning_transport::statistics(&job_root)?;
                let result=learning_job::run_with(&ws,&job.id,|stage,system,user| {
                    let store=Arc::clone(&store);let root=job_root.clone();let profile=profile.clone();
                    let name=format!("{code}-k{count}-{stage}");
                    async move {
                        let mut body=request_body(&profile.model,&system,&user,None)?;
                        body["max_completion_tokens"]=json!(16384);body["thinking"]=json!({"type":"disabled"});
                        let request=json!({"schema":1,"endpoint_sha256":profile.endpoint_sha256,"body":body});
                        let expected=request.clone();
                        learning_transport::call_with(&root,&stage,request,retry,||async {Ok(())},||async {
                            store.call(&name,Role::Structured,&system,&user,false).await?;
                            let saved:Checkpoint=local::read(&store.root.join(format!("requests/{name}.json")))?;
                            verify(&saved)?;
                            if saved.request!=expected {return Err(AppError::Parse("研究与产品请求身份不一致".into()));}
                            Ok(LearningResponse{content:saved.receipt.content,model:saved.receipt.model,
                                finish_reason:saved.receipt.finish_reason,usage:saved.receipt.usage})
                        }).await
                    }
                }).await;
                let candidate=match result {
                    Ok(c)=>c,
                    Err(error)=>return Ok(json!({"id":code,"status":"failed","jobs":results,"error":error.to_string()})),
                };
                let name=format!("开发声音-{code}");let versions=ws.versions(&name)?;
                let version=if let Some(existing)=versions.get(count-1) {
                    if existing.candidate.id!=candidate.id {return Err(AppError::Parse("已保存开发版本身份变化".into()));}
                    existing.clone()
                } else {ws.save(&candidate.id,&name)?};
                base=Some(version.reference());
                results.push(json!({"sources":count,"job_id":job.id,"candidate_id":candidate.id,"version":version.number,
                    "status":"complete","requests":learning_transport::statistics(&job_root)?}));
            }
            Ok::<_,AppError>(json!({"id":code,"status":"complete","jobs":results}))
        });
    }
    finish_tasks(&store, &mut tasks, "product_learning_jobs_live").await?;
    write_json(
        &store.root.join("workflow-boundary.json"),
        &json!({"saved_versions":"research_workspace_only",
        "adopted_production_style":false,"quality":"draft_pending_semantic_review"}),
    )
}
