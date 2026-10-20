//! Development-only ending audit with program-owned source offsets.
use crate::{
    error::AppError,
    eval_store::write_json,
    research_compare::finish_tasks,
    research_dataset::sha256,
    research_profile::{self, ROOT},
    research_store::ResearchStore,
    research_validation::validated,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::HashSet, path::Path, sync::Arc};

fn verify_anchors(row: &Value, source: &str) -> Result<Vec<Value>, AppError> {
    if row["source_sha256"] != sha256(source.as_bytes()) {
        return Err(AppError::Parse("结尾定位来源SHA变化".into()));
    }
    let chars: Vec<_> = source.chars().collect();
    let segments = row["final_punctuation_segments"]
        .as_array()
        .ok_or_else(|| AppError::Parse("缺少结尾定位".into()))?;
    if segments.is_empty() || segments.len() > 2 {
        return Err(AppError::Parse("结尾片段数量无效".into()));
    }
    let mut result = Vec::new();
    let mut previous_end = 0;
    for (i, s) in segments.iter().enumerate() {
        let start = s["start_character"]
            .as_u64()
            .ok_or_else(|| AppError::Parse("缺少起点".into()))? as usize;
        let end = s["end_character"]
            .as_u64()
            .ok_or_else(|| AppError::Parse("缺少终点".into()))? as usize;
        if start < previous_end || end <= start || end > chars.len() {
            return Err(AppError::Parse("结尾片段越界或乱序".into()));
        }
        let quote: String = chars[start..end].iter().collect();
        if s["quote"] != quote {
            return Err(AppError::Parse("结尾引文与字符偏移不一致".into()));
        }
        previous_end = end;
        result.push(json!({"id":format!("end-{}",i+1),"start_character":start,"end_character":end,"quote":quote}));
    }
    if chars[previous_end..].iter().any(|c| c.is_alphanumeric()) {
        return Err(AppError::Parse("所选结尾之后仍有正文文字".into()));
    }
    Ok(result)
}

fn decode(raw: &str, anchors: &[Value]) -> Result<Value, AppError> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Judgment {
        status: String,
        evidence_ids: Vec<String>,
        reason: String,
    }
    let j: Judgment = serde_json::from_str(raw).map_err(|e| AppError::Parse(e.to_string()))?;
    if !["support", "counter", "unknown"].contains(&j.status.as_str())
        || j.reason.trim().is_empty()
        || (j.status != "unknown" && j.evidence_ids.is_empty())
    {
        return Err(AppError::Parse("结尾状态/理由/证据覆盖无效".into()));
    }
    let mut seen = HashSet::new();
    let mut quotes = Vec::new();
    if j.status != "unknown"
        && !j
            .evidence_ids
            .iter()
            .any(|id| anchors.last().is_some_and(|a| a["id"] == *id))
    {
        return Err(AppError::Parse(
            "结尾支持或反例必须包含实际最后片段，不能只选倒数第二片段".into(),
        ));
    }
    for id in &j.evidence_ids {
        let anchor = anchors
            .iter()
            .find(|a| a["id"] == *id)
            .ok_or_else(|| AppError::Parse("不能引用程序提供范围外的结尾".into()))?;
        if !seen.insert(id) {
            return Err(AppError::Parse("结尾ID重复".into()));
        }
        quotes.push(anchor.clone());
    }
    Ok(
        json!({"status":j.status,"reason":j.reason,"evidence_ids":j.evidence_ids,"source_evidence":quotes}),
    )
}

pub(crate) async fn run(run: &str) -> Result<(), AppError> {
    let sources = research_profile::load_sources("p2-views-02")?;
    let old: Value = serde_json::from_slice(&std::fs::read(format!(
        "{ROOT}/runs/p3-evidence-audit-01/inputs.json"
    ))?)
    .map_err(|e| AppError::Parse(e.to_string()))?;
    let anchor_bytes = std::fs::read(format!("{ROOT}/reports/evidence-anchors-v1.json"))?;
    let anchors: Value =
        serde_json::from_slice(&anchor_bytes).map_err(|e| AppError::Parse(e.to_string()))?;
    let mut cases = Vec::new();
    for id in ["wxb-03", "wzq-02", "wzq-05b", "wzq-07"] {
        let (author, source, _) = &sources[id];
        let row = anchors["rows"]
            .as_array()
            .ok_or_else(|| AppError::Parse("缺少定位rows".into()))?
            .iter()
            .find(|a| a["article"] == id)
            .ok_or_else(|| AppError::Parse("缺少指定文章定位".into()))?;
        let candidates: Vec<_> = old["candidates"][author]
            .as_array()
            .ok_or_else(|| AppError::Parse("缺少原候选".into()))?
            .iter()
            .filter(|c| c["dimension"] == "ending")
            .collect();
        if candidates.len() != 1 {
            return Err(AppError::Parse("要求唯一冻结结尾候选".into()));
        }
        cases.push(json!({"article":id,"text":source.text,"candidate":candidates[0],"ending_anchors":verify_anchors(row,&source.text)?}));
    }
    let prompt = std::fs::read_to_string(format!("{ROOT}/prompts/ending-anchor-audit-v1.md"))?;
    let store = Arc::new(ResearchStore::open(Path::new(ROOT), run, false)?);
    store.limit_run(1_000_000).await?;
    store.freeze(
        &json!({"suite":"ending_anchor_audit_v1","cases":cases,"prompt":prompt,
        "anchor_file_sha256":sha256(&anchor_bytes),"run_cap_micro_cny":1000000,
        "protocol_sha256":sha256(&std::fs::read(format!("{ROOT}/method-revision-10.md"))?)}),
    )?;
    std::fs::create_dir_all(store.root.join("cases"))?;
    let mut jobs = tokio::task::JoinSet::new();
    for case in cases {
        let store = Arc::clone(&store);
        let prompt = prompt.clone();
        jobs.spawn(async move {
            let id = case["article"].as_str().unwrap();
            let result = validated(
                &store,
                &format!("ending-{id}"),
                &prompt,
                &json!({"text":case["text"],"candidate":case["candidate"],"ending_anchors":case["ending_anchors"]}).to_string(),
                |raw| decode(raw, case["ending_anchors"].as_array().unwrap()),
            )
            .await;
            match result {
                Ok(value) => {
                    write_json(
                        &store.root.join(format!("cases/{id}.json")),
                        &json!({"input":case,"judgment":value}),
                    )?;
                    Ok::<_, AppError>(
                        json!({"id":id,"status":"complete","judgment":value["status"]}),
                    )
                }
                Err(e) => Ok(json!({"id":id,"status":"failed","error":e.to_string()})),
            }
        });
    }
    finish_tasks(&store, &mut jobs, "ending_anchor_audit").await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_offsets_reject_middle_sentence_and_tampered_source() {
        let source = "首句。末句。\n";
        let mut row = json!({"source_sha256":sha256(source.as_bytes()),"final_punctuation_segments":[{"start_character":3,"end_character":6,"quote":"末句。"}]});
        let anchors = verify_anchors(&row, source).unwrap();
        assert_eq!(anchors[0]["quote"], "末句。");
        let two = vec![
            json!({"id":"end-1","quote":"首句。"}),
            json!({"id":"end-2","quote":"末句。"}),
        ];
        assert!(
            decode(
                r#"{"status":"support","evidence_ids":["end-1"],"reason":"只拿倒数第二句"}"#,
                &two
            )
            .is_err()
        );
        assert!(
            decode(
                r#"{"status":"counter","evidence_ids":["end-2"],"reason":"最后一句与规则相反"}"#,
                &two
            )
            .is_ok()
        );
        assert!(verify_anchors(&row, "首句。末句被改。\n").is_err());
        row["final_punctuation_segments"][0] =
            json!({"start_character":0,"end_character":3,"quote":"首句。"});
        assert!(verify_anchors(&row, source).is_err());
        assert!(
            decode(
                r#"{"status":"support","evidence_ids":["invented"],"reason":"错误位置"}"#,
                &anchors
            )
            .is_err()
        );
    }
}
