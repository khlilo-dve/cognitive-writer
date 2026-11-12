//! Independent, grounded issue reports and a single bounded draft repair.
use crate::{
    error::AppError,
    learning_chat::ChatRef,
    learning_chat_store as store,
    learning_flow::{Call, WriterInput},
    learning_workspace_store as io,
    voice_learning::sha256,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{future::Future, path::Path};
pub(crate) const METHOD: &str = "writer-facts-v1";
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Issue {
    pub kind: String,
    pub quote: String,
    pub reason: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Missing {
    pub requirement: String,
    pub reason: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Review {
    pub issues: Vec<Issue>,
    #[serde(default)]
    pub missing: Vec<Missing>,
}
impl Review {
    pub(crate) fn clear(&self) -> bool {
        self.issues.is_empty() && self.missing.is_empty()
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Prompts {
    pub review: String,
    pub repair: String,
}
impl Prompts {
    pub(crate) fn load() -> Result<Self, AppError> {
        Ok(Self {
            review: std::fs::read_to_string("prompts/writer_fact_review.md")?,
            repair: std::fs::read_to_string("prompts/writer_fact_repair.md")?,
        })
    }
}
#[derive(Serialize, Deserialize)]
pub(crate) struct Audit {
    pub original_sha256: String,
    pub final_sha256: String,
    pub first: Review,
    pub final_review: Option<Review>,
    pub repaired: bool,
    pub status: String,
}
fn parse(raw: &str, request: &str, draft: &str) -> Result<Review, AppError> {
    let r: Review = serde_json::from_str(raw)
        .map_err(|_| store::error("事实核对JSON无效，原稿和回执保留，未当作已通过"))?;
    let mut seen = std::collections::HashSet::new();
    if r.issues.len() + r.missing.len() > 24
        || r.issues.iter().any(|i| {
            !matches!(
                i.kind.as_str(),
                "unsupported_fact" | "contradiction" | "uncertain"
            ) || i.quote.trim().is_empty()
                || i.reason.trim().is_empty()
                || !draft.contains(&i.quote)
                || !seen.insert(i.quote.clone())
        })
        || r.missing.iter().any(|m| {
            m.requirement.trim().is_empty()
                || m.reason.trim().is_empty()
                || !request.contains(&m.requirement)
        })
    {
        return Err(store::error(
            "事实核对引用不属于本稿/本任务或结构无效，未当作已通过",
        ));
    }
    Ok(r)
}
fn publish<T: Serialize>(path: &Path, value: &T) -> Result<(), AppError> {
    if path.exists() {
        let old: serde_json::Value = io::read(path)?;
        if old != serde_json::to_value(value).map_err(|e| store::error(&e.to_string()))? {
            return Err(store::error("已有事实核对产物身份变化"));
        }
        Ok(())
    } else {
        io::publish_json(path, value)
    }
}
fn text(path: &Path, body: &str) -> Result<(), AppError> {
    if path.exists() {
        if std::fs::read_to_string(path)? != body {
            return Err(store::error("既有稿件身份变化"));
        }
        Ok(())
    } else {
        io::publish(path, body.as_bytes())
    }
}
pub(crate) async fn review_with<F, Fut>(
    root: &Path,
    profile: &crate::learning_job::CallProfile,
    prompt: &str,
    stage: &str,
    request: &str,
    draft: &str,
    call: &mut F,
) -> Result<Review, AppError>
where
    F: FnMut(Call) -> Fut,
    Fut: Future<Output = Result<String, AppError>>,
{
    let mut profile = profile.clone();
    profile.model = "mimo-v2.6-pro".into();
    let raw = call(Call {
        root: root.into(),
        profile,
        stage: stage.into(),
        system: prompt.into(),
        user: json!({"request":request,"draft":draft}).to_string(),
    })
    .await?;
    parse(&raw, request, draft)
}
fn unchanged_issue_paragraphs(first: &Review, original: &str, revised: &str) -> Vec<Issue> {
    first
        .issues
        .iter()
        .filter(|issue| {
            original.split("\n\n").any(|paragraph| {
                paragraph.contains(&issue.quote) && revised.split("\n\n").any(|p| p == paragraph)
            })
        })
        .cloned()
        .collect()
}
pub(crate) async fn check_with<F, Fut>(
    root: &Path,
    profile: &crate::learning_job::CallProfile,
    input: &WriterInput,
    draft: &str,
    prompts: &Prompts,
    call: &mut F,
) -> Result<String, AppError>
where
    F: FnMut(Call) -> Fut,
    Fut: Future<Output = Result<String, AppError>>,
{
    text(&root.join("draft-original.md"), draft)?;
    let first = review_with(
        root,
        profile,
        &prompts.review,
        "writer-fact-review",
        &input.request,
        draft,
        call,
    )
    .await?;
    publish(&root.join("facts-first.json"), &first)?;
    if first.clear() {
        publish(
            &root.join("facts-audit.json"),
            &Audit {
                original_sha256: sha256(draft.as_bytes()),
                final_sha256: sha256(draft.as_bytes()),
                first,
                final_review: None,
                repaired: false,
                status: "model_found_no_issue_not_truth_certified".into(),
            },
        )?;
        return Ok(draft.into());
    }
    let mut flash = profile.clone();
    flash.model = "mimo-v2.6-flash".into();
    let fixed = call(Call {
        root: root.into(),
        profile: flash,
        stage: "writer-fact-repair".into(),
        system: prompts.repair.clone(),
        user: json!({"request":input.request,"style":input.spec,"draft":draft,"review":first})
            .to_string(),
    })
    .await?;
    crate::spec_quality::validate_execution_spec(&fixed)?;
    text(&root.join("draft-revised.md"), &fixed)?;
    text(
        &root.join("draft-diff.md"),
        &crate::learning_diff::diff(draft, &fixed),
    )?;
    let verified = review_with(
        root,
        profile,
        &prompts.review,
        "writer-fact-verify",
        &input.request,
        &fixed,
        call,
    )
    .await?;
    let clear = verified.clear();
    let first_for_gate = first.clone();
    publish(
        &root.join("facts-audit.json"),
        &Audit {
            original_sha256: sha256(draft.as_bytes()),
            final_sha256: sha256(fixed.as_bytes()),
            first,
            final_review: Some(verified),
            repaired: true,
            status: if clear {
                "model_found_no_issue_after_one_repair"
            } else {
                "needs_review_after_one_repair"
            }
            .into(),
        },
    )?;
    let unchanged = unchanged_issue_paragraphs(&first_for_gate, draft, &fixed);
    publish(
        &root.join("facts-decision.json"),
        &json!({
            "model_found_no_issue":clear,"unchanged_issue_paragraphs":unchanged,
            "status":if clear && unchanged.is_empty(){"model_clear_no_unchanged_flagged_paragraph"}else{"needs_review_after_one_repair"}
        }),
    )?;
    if clear && unchanged.is_empty() {
        Ok(fixed)
    } else {
        Err(store::error(
            "一次修订后事实核对仍有问题，草稿与问题保留；输入 /writer-review 查看，未交付为已核对成稿",
        ))
    }
}
pub(crate) fn outline_system(spec: &str) -> String {
    format!(
        "{}\n\n本次大纲只组织用户任务已有信息。支撑素材方向必须来自任务；没有依据的设施、流程、人物、行为和因果不列入大纲。篇幅按实际材料决定，材料少时可短。建议或假设须明确性质，不能写成已知事实。",
        crate::generate::outline_system_prompt(spec)
    )
}
pub(crate) fn fulltext_user(outline: &str, request: &str) -> String {
    format!(
        "本次任务与唯一事实依据：\n{request}\n\n结构建议（不是新增事实来源，若有越界自行舍弃）：\n{outline}\n\n按任务权限写完整文章。仅组织已有材料；新增建议/观点要明确为建议/观点，授权假设应明确为假设。不得把大纲中的设施、行为、推断当已给事实，不为篇幅凑场景。只输出Markdown正文。"
    )
}
pub(crate) fn display(r: &ChatRef) -> Result<(), AppError> {
    let state = store::load(r)?.state;
    let input = state
        .selected_writer
        .or_else(|| state.delivery.and_then(|d| d.writer))
        .ok_or_else(|| store::error("没有当前写作核对任务"))?;
    let root = crate::learning_flow_writer::directory(r, &input)?;
    for (file, label) in [
        ("draft-original.md", "原稿"),
        ("draft-revised.md", "一次修订稿"),
    ] {
        let path = root.join(file);
        if path.exists() {
            println!("{label}：\n{}", std::fs::read_to_string(path)?);
        }
    }
    if root.join("facts-audit.json").exists() {
        let audit: Audit = io::read(&root.join("facts-audit.json"))?;
        println!("事实核对状态：{}（模型判断）", audit.status);
        println!(
            "{}",
            serde_json::to_string_pretty(&audit).map_err(|e| store::error(&e.to_string()))?
        );
    } else if root.join("facts-first.json").exists() {
        let v: Review = io::read(&root.join("facts-first.json"))?;
        println!(
            "首审问题：{}",
            serde_json::to_string_pretty(&v).map_err(|e| store::error(&e.to_string()))?
        );
    } else {
        println!("事实核对尚未完成，原请求/响应记录已保留；未知请求仅显式重试。");
    }
    let decision = root.join("facts-decision.json");
    if decision.exists() {
        println!("交付检查：{}", std::fs::read_to_string(decision)?);
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn review_must_point_to_actual_draft_and_request_and_reject_unknown_fields() {
        let raw = r#"{"issues":[{"kind":"unsupported_fact","quote":"排队还书","reason":"未提供流程"}],"missing":[]}"#;
        assert!(parse(raw, "图书馆十点关门", "有人排队还书").is_ok());
        assert!(parse(r#"{"issues":[]}"#, "任务", "稿").unwrap().clear());
        assert!(parse(r#"{}"#, "任务", "稿").is_err());
        assert!(parse(raw, "图书馆十点关门", "原稿没有那句话").is_err());
        assert!(
            parse(
                r#"{"issues":[],"missing":[{"requirement":"不存在的要求","reason":"遗漏"}]}"#,
                "原任务",
                "稿"
            )
            .is_err()
        );
        assert!(parse(r#"{"issues":[],"missing":[],"score":10}"#, "原任务", "稿").is_err());
    }
    #[test]
    fn unchanged_flagged_paragraph_is_not_cleared_by_a_second_model() {
        let r = Review {
            issues: vec![Issue {
                kind: "unsupported_fact".into(),
                quote: "现场没有中间人".into(),
                reason: "任务未给".into(),
            }],
            missing: vec![],
        };
        assert_eq!(
            unchanged_issue_paragraphs(
                &r,
                "开头\n\n现场没有中间人。",
                "改了开头\n\n现场没有中间人。"
            )
            .len(),
            1
        );
        assert!(
            unchanged_issue_paragraphs(
                &r,
                "现场没有中间人。",
                "假设现场没有中间人，可以直接沟通。"
            )
            .is_empty()
        );
    }
    #[tokio::test]
    async fn one_repair_then_remaining_issue_blocks_delivery_and_preserves_both() {
        let d = tempfile::tempdir().unwrap();
        let p = crate::learning_job::CallProfile {
            model: "mimo-v2.6-flash".into(),
            endpoint_sha256: "a".repeat(64),
        };
        let input = WriterInput {
            method: Some(METHOD.into()),
            label: "test".into(),
            spec: "写法".into(),
            request: "十点关门".into(),
        };
        let prompts = Prompts {
            review: "核对".into(),
            repair: "修订".into(),
        };
        let mut stages = vec![];
        let result=check_with(d.path(),&p,&input,"原稿说有人排队还书",&prompts,&mut |c:Call|{stages.push((c.stage.clone(),c.profile.model.clone()));async move{Ok(if c.stage=="writer-fact-repair"{"修订稿仍有人排队还书".into()}else{r#"{"issues":[{"kind":"unsupported_fact","quote":"排队还书","reason":"未提供流程"}],"missing":[]}"#.into()})}}).await;
        assert!(result.is_err());
        assert_eq!(stages.len(), 3);
        assert_eq!(stages[0].1, "mimo-v2.6-pro");
        assert_eq!(stages[1].1, "mimo-v2.6-flash");
        assert_eq!(stages[2].1, "mimo-v2.6-pro");
        assert!(d.path().join("draft-diff.md").exists());
        assert!(
            std::fs::read_to_string(d.path().join("facts-audit.json"))
                .unwrap()
                .contains("needs_review")
        );
    }
}
