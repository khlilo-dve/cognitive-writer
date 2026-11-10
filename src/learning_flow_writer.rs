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
        "learn-analysis" | "learn-analysis-repair"
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
pub(crate) async fn run_with<F, Fut>(
    r: &ChatRef,
    input: &WriterInput,
    call: &mut F,
) -> Result<Written, AppError>
where
    F: FnMut(Call) -> Fut,
    Fut: Future<Output = Result<String, AppError>>,
{
    let key = sha256(&serde_json::to_vec(input).map_err(|e| store::error(&e.to_string()))?);
    let root = store::directory(r)?.join("writers").join(key);
    let frozen = root.join("input.json");
    if frozen.exists() {
        let prior: WriterInput = io::read(&frozen)?;
        if serde_json::to_value(prior).ok() != serde_json::to_value(input).ok() {
            return Err(store::error("Writer输入变化"));
        }
    } else {
        io::publish_json(&frozen, input)?;
    }
    let mut profile = store::plan(r)?.profile;
    profile.model = "mimo-v2.6-flash".into();
    let before = store::load(r)?.id;
    let outline = call(Call {
        root: root.clone(),
        profile: profile.clone(),
        stage: "writer-outline".into(),
        system: crate::generate::outline_system_prompt(&input.spec),
        user: input.request.clone(),
    })
    .await?;
    if store::load(r)?.id != before {
        return Err(store::error("会话变化，未继续旧Writer"));
    }
    let fulltext = call(Call {
        root,
        profile,
        stage: "writer-fulltext".into(),
        system: crate::generate::style_system_prompt(&input.spec),
        user: crate::generate::render_prompt(&outline, &input.request),
    })
    .await?;
    Ok(Written {
        label: input.label.clone(),
        spec: input.spec.clone(),
        request: input.request.clone(),
        outline,
        fulltext,
    })
}
