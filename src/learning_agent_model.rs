//! P3 live decision adapter: reuse durable transport and keep fees in the user task workspace.
use crate::{
    error::AppError,
    learning_agent_store::{directory, error},
    learning_agent_types::DecisionCall,
    learning_transport::{self, LearningClient, LearningResponse},
    learning_workspace::LearningWorkspace,
    learning_workspace_store as store,
    llm::request_body,
    research_budget::{Budget, cost},
    research_receipt::{Receipt, charge},
};
use serde_json::{Value, json};
use std::sync::Mutex;
pub(crate) async fn call(
    ws: &LearningWorkspace,
    id: &str,
    call: DecisionCall,
    retry: bool,
) -> Result<String, AppError> {
    call_at(&directory(ws, id)?, id, call, retry).await
}
pub(crate) async fn call_at(
    root: &std::path::Path,
    id: &str,
    call: DecisionCall,
    retry: bool,
) -> Result<String, AppError> {
    let client = LearningClient::from_env()?;
    if client.profile().endpoint_sha256 != call.endpoint_sha256 {
        return Err(error("Agent端点身份变化，未发送或复用决策"));
    }
    let root = root.to_path_buf();
    let _lock = store::lock(&root.join("billing-lock"))?;
    let billing = Mutex::new(Budget::open(
        root.join("usage-ledger.json"),
        id,
        call.budget_micro_cny,
        call.budget_micro_cny,
    )?);
    let format = json!({"type":"json_object"});
    let mut body = request_body(call.tier.model(), &call.system, &call.user, Some(&format))?;
    body["max_completion_tokens"] = json!(2048);
    body["thinking"] = json!({"type":"disabled"});
    let request = json!({"schema":1,"endpoint_sha256":call.endpoint_sha256,"body":body});
    let reserve = cost(
        call.tier.model(),
        serde_json::to_vec(&body)
            .map_err(|e| error(&e.to_string()))?
            .len() as u64
            + 1024,
        2048,
    )?;
    let result = learning_transport::call_with(
        &root,
        &call.step,
        request,
        retry,
        || async {
            client.preflight()?;
            let b = billing.lock().map_err(|_| error("任务费用锁异常"))?;
            if reserve > call.budget_micro_cny.saturating_sub(b.occupied(false)) {
                return Err(error("任务决策预算不足，未发送请求"));
            }
            Ok(())
        },
        || async {
            let attempt = (1..=3)
                .filter(|n| {
                    root.join(format!("requests/{}-attempt-{n}.intent.json", call.step))
                        .exists()
                })
                .last()
                .ok_or_else(|| error("缺少决策意图"))?;
            billing
                .lock()
                .map_err(|_| error("任务费用锁异常"))?
                .reserve(&format!("{id}/{}/attempt-{attempt}", call.step), reserve)?;
            client.response(&body).await
        },
    )
    .await;
    let path = root.join(format!("requests/{}.response.json", call.step));
    if path.exists() {
        let saved: Value = store::read(&path)?;
        let receipt: LearningResponse =
            serde_json::from_value(saved["response"].clone()).map_err(|_| error("决策回执无效"))?;
        if receipt.model != call.tier.model() {
            return Err(error("决策返回模型身份不符，费用保留未知预留"));
        }
        let output_over_limit = receipt
            .usage
            .as_ref()
            .and_then(|u| u["completion_tokens"].as_u64())
            .is_some_and(|n| n > 2048);
        let amount = charge(
            call.tier.model(),
            &Receipt {
                content: receipt.content,
                model: receipt.model,
                finish_reason: receipt.finish_reason,
                usage: receipt.usage,
            },
        )?;
        let attempt = saved["attempt"]
            .as_u64()
            .ok_or_else(|| error("缺少决策尝试号"))?;
        billing
            .lock()
            .map_err(|_| error("任务费用锁异常"))?
            .settle(&format!("{id}/{}/attempt-{attempt}", call.step), amount)?;
        if output_over_limit {
            return Err(error("决策输出usage超过2048上限，已记账并停止"));
        }
    }
    result
}
