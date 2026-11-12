//! Fixed Flash Writer with the same durable receipts as the learning transport.
use crate::{
    error::AppError,
    learning_chat::ChatRef,
    learning_chat_store as store,
    learning_flow::{Call, WriterInput, Written},
    learning_transport::{self, LearningClient},
    learning_workspace_store as io,
    voice_learning::sha256,
};
use serde_json::json;
use std::future::Future;
pub(crate) async fn send(call: Call, retry: bool) -> Result<String, AppError> {
    let client = LearningClient::from_env()?;
    let mut profile = client.profile();
    profile.model = call.profile.model.clone();
    if profile != call.profile {
        return Err(store::error("端点变化，未发送交付请求"));
    }
    let request = request(&call)?;
    learning_transport::call_with(
        &call.root,
        &call.stage,
        request.clone(),
        retry,
        || async { client.preflight() },
        || client.response(&request["body"]),
    )
    .await
}
pub(crate) fn request(call: &Call) -> Result<serde_json::Value, AppError> {
    let format = matches!(
        call.stage.as_str(),
        "learn-analysis" | "learn-analysis-repair" | "writer-fact-review" | "writer-fact-verify"
    )
    .then(|| json!({"type":"json_object"}));
    let mut body = crate::llm::request_body(
        &call.profile.model,
        &call.system,
        &call.user,
        format.as_ref(),
    )?;
    body["max_completion_tokens"] = json!(16384);
    body["thinking"] = json!({"type":"disabled"});
    Ok(json!({"schema":1,"endpoint_sha256":call.profile.endpoint_sha256,"body":body}))
}

pub(crate) fn directory(r: &ChatRef, input: &WriterInput) -> Result<std::path::PathBuf, AppError> {
    let key = sha256(&serde_json::to_vec(input).map_err(|e| store::error(&e.to_string()))?);
    Ok(store::directory(r)?.join("writers").join(key))
}
pub(crate) async fn run_with<F, Fut>(
    r: &ChatRef,
    input: &WriterInput,
    call: &mut F,
) -> Result<Written, AppError>
where
    F: FnMut(Call) -> Fut,
    Fut: Future<Output = Result<String, AppError>>,
{
    let root = directory(r, input)?;
    let frozen = root.join("input.json");
    if frozen.exists() {
        let prior: WriterInput = io::read(&frozen)?;
        if serde_json::to_value(prior).ok() != serde_json::to_value(input).ok() {
            return Err(store::error("Writer输入变化"));
        }
    } else {
        io::publish_json(&frozen, input)?;
    }
    let legacy = input.method.is_none();
    if !legacy && input.method.as_deref() != Some(crate::writer_facts::METHOD) {
        return Err(store::error(
            "写作方法已变化，旧记录保留，请新建明确写作任务",
        ));
    }
    if legacy
        && ["writer-outline", "writer-fulltext"]
            .iter()
            .any(|s| !root.join(format!("requests/{s}.response.json")).exists())
    {
        return Err(store::error(
            "历史写作任务没有完整回执，不能静默升级或重发；请保留记录，新建写作任务以使用事实核对",
        ));
    }
    let mut profile = store::plan(r)?.profile;
    profile.model = "mimo-v2.6-flash".into();
    let before = store::load(r)?.id;
    let mut guarded = |request: Call| {
        let checked = (|| {
            let now = store::load(r)?;
            if now.id != before {
                return Err(store::error("会话变化，未继续旧Writer阶段"));
            }
            if now.state.selected_writer.is_none() {
                if let Some(d) = &now.state.delivery {
                    crate::learning_flow::current(r, d)?;
                }
            }
            Ok(())
        })();
        let future = checked.map(|_| call(request));
        async move {
            match future {
                Ok(f) => f.await,
                Err(e) => Err(e),
            }
        }
    };
    let outline = guarded(Call {
        root: root.clone(),
        profile: profile.clone(),
        stage: "writer-outline".into(),
        system: if legacy {
            crate::generate::outline_system_prompt(&input.spec)
        } else {
            crate::writer_facts::outline_system(&input.spec)
        },
        user: input.request.clone(),
    })
    .await?;
    let original = guarded(Call {
        root: root.clone(),
        profile: profile.clone(),
        stage: "writer-fulltext".into(),
        system: crate::generate::style_system_prompt(&input.spec),
        user: if legacy {
            crate::generate::render_prompt(&outline, &input.request)
        } else {
            crate::writer_facts::fulltext_user(&outline, &input.request)
        },
    })
    .await?;
    let fulltext = if legacy {
        original
    } else {
        let path = root.join("fact-prompts.json");
        let prompts = if path.exists() {
            io::read(&path)?
        } else {
            let p = crate::writer_facts::Prompts::load()?;
            io::publish_json(&path, &p)?;
            p
        };
        crate::writer_facts::check_with(&root, &profile, input, &original, &prompts, &mut guarded)
            .await?
    };
    if store::load(r)?.id != before {
        return Err(store::error("会话变化，旧Writer结果未采用"));
    }
    Ok(Written {
        fact_checked: !legacy,
        label: input.label.clone(),
        spec: input.spec.clone(),
        request: input.request.clone(),
        outline,
        fulltext,
    })
}
