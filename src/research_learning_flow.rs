//! Fixed P5 file -> candidate -> append -> old-version Writer check under the shared ledger.
use crate::{
    error::AppError,
    eval_store::write_json,
    learning_chat::{self, ChatRef},
    learning_chat_store as chat,
    learning_flow::{self, Call},
    learning_transport::{self, LearningClient, LearningResponse},
    learning_workspace_store as io,
    research_receipt::{Checkpoint, verify},
    research_store::{ResearchStore, Role},
    voice_learning::sha256,
};
use serde_json::{Value, json};
use std::{path::Path, sync::Arc};
const ROOT: &str = "research/style-aggregation";
fn error(s: &str) -> AppError {
    AppError::LearningWorkspace(s.into())
}
pub(crate) async fn run(run: &str) -> Result<(), AppError> {
    let store = Arc::new(ResearchStore::open(Path::new(ROOT), run, false)?);
    store.limit_run(2_000_000).await?;
    let bytes = std::fs::read(format!("{ROOT}/runs/voice-learning-core-01/inputs.json"))?;
    let source: Value = serde_json::from_slice(&bytes).map_err(|e| error(&e.to_string()))?;
    let item = source["jobs"]
        .as_array()
        .and_then(|a| a.iter().find(|v| v["id"] == "wxb-k3"))
        .ok_or_else(|| error("缺冻结作者来源"))?;
    let mut texts = vec![];
    for id in ["wxb-02", "wxb-03"] {
        let at = item["source_ids"]
            .as_array()
            .and_then(|a| a.iter().position(|v| v == id))
            .ok_or_else(|| error("缺指定正文"))?;
        texts.push(
            item["texts"][at]
                .as_str()
                .ok_or_else(|| error("正文无效"))?
                .to_string(),
        );
    }
    let mut profile = LearningClient::from_env()?.profile();
    profile.model = "mimo-v2.6-flash".into();
    let prompt = std::fs::read_to_string("prompts/learning_chat.md")?;
    let agent_prompt = std::fs::read_to_string("prompts/learning_agent.md")?;
    store.freeze(&json!({"suite":"p5-delivery-v1","texts":texts,"source_sha256":sha256(&bytes),"source_ids":["wxb-02","wxb-03"],
        "profile":profile,"prompt":prompt,"agent_prompt":agent_prompt,"learning_prompts":crate::learning_job::JobPrompts::load()?,
        "learning_method":crate::voice_learning::METHOD,"revision_method":crate::voice_revision::METHOD,"run_cap_micro_cny":2000000,
        "writer_request":"写一篇给大学生看的图书馆夜读建议，约600字。只用以下材料：图书馆晚上十点闭馆；我希望留半小时收拾与回宿舍。不虚构亲历、数据、引用或新的事实。"}))?;
    for (i, t) in texts.iter().enumerate() {
        let file = store.root.join(format!("fixtures/article-{}.md", i + 1));
        if file.exists() {
            if std::fs::read_to_string(&file)? != *t {
                return Err(error("冻结文件变化"));
            }
        } else {
            io::publish(&file, t.as_bytes())?;
        }
    }
    let link = store.root.join("chat.json");
    let r: ChatRef = if link.exists() {
        io::read(&link)?
    } else {
        let r = chat::create_with(&store.root.join("workspace"), profile, prompt, agent_prompt)?;
        io::publish_json(&link, &r)?;
        r
    };
    let ws = chat::ws(&r)?;
    let result=async {
        if !store.root.join("single.json").exists(){
            if learning_chat::active(&r)?.is_none(){
                let path=std::fs::canonicalize(store.root.join("fixtures/article-1.md"))?;
                learning_chat::submit(&r,&format!("学习 \"{}\"，保存为“P5夜读”",path.display()))?;
            }
            crate::research_learning_chat::drive(&store,&r,"single").await?;
            flow(&store,&r).await?;
            let v=ws.version("P5夜读",1)?;
            if v.candidate.input.articles.len()!=1{return Err(error("单篇来源数错误"));}
            io::publish_json(&store.root.join("single.json"),&json!({"version":v.reference(),"candidate":v.candidate.id}))?;
        }
        if !store.root.join("append.json").exists(){
            if ws.versions("P5夜读")?.len()==1 && chat::load(&r)?.state.delivery.as_ref().is_some_and(|d|d.saved.is_some()){
                let path=std::fs::canonicalize(store.root.join("fixtures/article-2.md"))?;
                learning_chat::submit(&r,&format!("追加到P5夜读，保存为P5夜读；学习 \"{}\"",path.display()))?;
            }
            crate::research_learning_chat::drive(&store,&r,"append").await?;
            flow(&store,&r).await?;
            let v=ws.version("P5夜读",2)?;
            if v.candidate.input.articles.len()!=2{return Err(error("追加未保留两篇正文"));}
            io::publish_json(&store.root.join("append.json"),&json!({"version":v.reference(),"candidate":v.candidate.id,"changes":ws.changes(&v.candidate.id)?}))?;
        }
        if chat::load(&r)?.state.selected_writer.is_none(){
            let input:Value=io::read(&store.root.join("inputs.json"))?;
            learning_flow::command(&r,&format!("/use-version P5夜读 1 {}",input["writer_request"].as_str().unwrap()))?;
        }
        let written=flow(&store,&r).await?.ok_or_else(||error("未生成Writer稿"))?;
        let first=ws.version("P5夜读",1)?;let second=ws.version("P5夜读",2)?;
        if written.spec!=first.candidate.input.draft.spec || second.previous!=Some(first.id.clone()) || ws.versions("P5夜读")?.len()!=2 {
            return Err(error("版本选择或历史链不符"));
        }
        if ws.save(&first.candidate.id,"P5夜读")?.id!=first.id{return Err(error("重复保存不幂等"));}
        write_json(&store.root.join("writer.json"),&json!({"label":written.label,"request":written.request,"outline":written.outline,"fulltext":written.fulltext,"selected_version":first.reference()}))?;
        Ok::<_,AppError>(())
    }.await;
    let usage = store.usage().await;
    let entries = usage["entries"]
        .as_object()
        .ok_or_else(|| error("账本无效"))?;
    let own: Vec<_> = entries
        .iter()
        .filter(|(_, v)| v["run_id"] == run)
        .map(|(k, v)| json!({"id":k,"entry":v}))
        .collect();
    let mut receipts = 0;
    if store.root.join("requests").exists() {
        for entry in std::fs::read_dir(store.root.join("requests"))? {
            let path = entry?.path();
            if path.extension().is_some_and(|s| s == "json") {
                verify(&io::read::<Checkpoint>(&path)?)?;
                receipts += 1;
            }
        }
    }
    let mut run_usage = usage.clone();
    if let Some(entries) = run_usage["entries"].as_object_mut() {
        entries.retain(|_, v| v["run_id"] == run);
    }
    write_json(
        &store.root.join("summary.json"),
        &json!({"status":if result.is_ok(){"complete"}else{"failed"},"error":result.as_ref().err().map(ToString::to_string),
        "verified_receipts":receipts,"entries":own,"usage":run_usage,"versions":ws.versions("P5夜读")?.iter().map(|v|v.reference()).collect::<Vec<_>>(),
        "scope":"P5 engine with ResearchStore adapter; single file, append two articles, explicit old version Writer; not quality certification or human feedback",
        "finished_at":chrono::Utc::now().to_rfc3339()}),
    )?;
    result
}
async fn flow(
    store: &Arc<ResearchStore>,
    r: &ChatRef,
) -> Result<Option<learning_flow::Written>, AppError> {
    learning_flow::run_with(r, |c: Call| {
        let store = Arc::clone(store);
        async move {
            let expected = crate::learning_flow_writer::request(&c)?;
            let name = format!(
                "flow-{}-{}",
                &sha256(c.root.to_string_lossy().as_bytes())[..12],
                c.stage
            );
            learning_transport::call_with(
                &c.root,
                &c.stage,
                expected.clone(),
                false,
                || async { Ok(()) },
                || async {
                    let identity = format!(
                        "{}/{}",
                        store
                            .root
                            .file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or(""),
                        name
                    );
                    if !store.root.join(format!("requests/{name}.json")).exists()
                        && store.usage().await["entries"]
                            .as_object()
                            .ok_or_else(|| error("账本无效"))?
                            .keys()
                            .any(|k| k == &identity || k.starts_with(&format!("{identity}/")))
                    {
                        return Err(error("已有未知预留，未隐式重发"));
                    }
                    let role = if c.profile.model == "mimo-v2.6-pro" {
                        Role::Structured
                    } else {
                        Role::FlashAnalysis
                    };
                    let json_mode = expected["body"].get("response_format").is_some();
                    store
                        .call(&name, role, &c.system, &c.user, json_mode)
                        .await?;
                    let cp: Checkpoint =
                        io::read(&store.root.join(format!("requests/{name}.json")))?;
                    verify(&cp)?;
                    if cp.request != expected {
                        return Err(error("研究请求与产品请求不一致"));
                    }
                    Ok(LearningResponse {
                        content: cp.receipt.content,
                        model: cp.receipt.model,
                        finish_reason: cp.receipt.finish_reason,
                        usage: cp.receipt.usage,
                    })
                },
            )
            .await
        }
    })
    .await
}
