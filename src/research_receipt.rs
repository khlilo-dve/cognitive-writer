use crate::{error::AppError, research_budget::cost, research_dataset::sha256};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Receipt {
    pub content: String,
    pub model: String,
    pub finish_reason: String,
    pub usage: Option<Value>,
}
#[derive(Serialize, Deserialize)]
pub(crate) struct Checkpoint {
    pub request: Value,
    pub receipt: Receipt,
    pub receipt_sha256: String,
    pub completed_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reused_from: Option<String>,
}

pub(crate) fn parse_receipt(raw: &str) -> Result<Receipt, AppError> {
    let v: Value = serde_json::from_str(raw)
        .map_err(|_| AppError::Parse("模型响应不是有效JSON；费用预留保留".into()))?;
    Ok(Receipt {
        content: v
            .pointer("/choices/0/message/content")
            .and_then(Value::as_str)
            .unwrap_or("")
            .into(),
        model: v["model"].as_str().unwrap_or("").into(),
        finish_reason: v
            .pointer("/choices/0/finish_reason")
            .and_then(Value::as_str)
            .unwrap_or("")
            .into(),
        usage: v.get("usage").filter(|u| !u.is_null()).cloned(),
    })
}

pub(crate) fn charge(model: &str, r: &Receipt) -> Result<u64, AppError> {
    let u = r
        .usage
        .as_ref()
        .ok_or_else(|| AppError::Parse("usage缺失，费用保留预留值，停止请求链".into()))?;
    let input = u["prompt_tokens"]
        .as_u64()
        .ok_or_else(|| AppError::Parse("prompt_tokens无效".into()))?;
    let output = u["completion_tokens"]
        .as_u64()
        .ok_or_else(|| AppError::Parse("completion_tokens无效".into()))?;
    if input == 0 {
        return Err(AppError::Parse("输入usage为零，不能核定费用".into()));
    }
    cost(model, input, output)
}

pub(crate) fn verify(c: &Checkpoint) -> Result<(), AppError> {
    if sha256(&serde_json::to_vec(&c.receipt).map_err(|e| AppError::Parse(e.to_string()))?)
        != c.receipt_sha256
    {
        return Err(AppError::Parse("响应检查点哈希不匹配".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn receipt_tampering_and_missing_usage_are_not_reusable() {
        let r=parse_receipt(r#"{"model":"mimo-v2.6-pro","choices":[{"finish_reason":"stop","message":{"content":"test"}}],"usage":{"prompt_tokens":20,"completion_tokens":30}}"#).unwrap();
        let mut c = Checkpoint {
            request: Value::Null,
            receipt_sha256: sha256(&serde_json::to_vec(&r).unwrap()),
            receipt: r,
            completed_at: "now".into(),
            reused_from: None,
        };
        verify(&c).unwrap();
        c.receipt.content = "tampered".into();
        assert!(verify(&c).is_err());
        assert!(charge("mimo-v2.6-pro", &parse_receipt("{}").unwrap()).is_err());
    }
}
