//! Iterate the free-form learner under fixed judges, then confirm on a preregistered topic.
use crate::error::AppError;
use crate::eval::{parse_learn_judgment, score_learn_judgment};
use crate::eval_store::{Store, write_json};
use crate::eval_transfer_audit::{Job, Row, run_job};
use crate::free_style_gate::Spec;
use crate::generate::{outline_system_prompt, render_prompt, style_system_prompt};
use serde_json::{Value, json};
use std::sync::Arc;

fn read(path: &str) -> Result<String, AppError> {
    Ok(std::fs::read_to_string(path)?)
}
fn read_json(path: &str) -> Result<Value, AppError> {
    serde_json::from_str(&read(path)?).map_err(|e| AppError::Parse(e.to_string()))
}
fn text_valid(s: &str) -> Result<(), AppError> {
    if s.trim().is_empty() {
        Err(AppError::Parse("空响应".into()))
    } else {
        Ok(())
    }
}
fn run_id(raw: &str) -> Result<&str, AppError> {
    if !raw.is_empty()
        && raw
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        Ok(raw)
    } else {
        Err(AppError::Parse("来源run_id无效".into()))
    }
}
struct Input {
    phase: String,
    source_run: Option<String>,
    learn: String,
    judge: String,
    transfer: String,
    sources: Vec<String>,
    tasks: Value,
    confirmed_specs: Option<Vec<Spec>>,
}
impl Input {
    fn load() -> Result<Self, AppError> {
        let phase = std::env::var("EVAL_RELEASE_PHASE").unwrap_or_else(|_| "development".into());
        if !["development", "confirmation"].contains(&phase.as_str()) {
            return Err(AppError::EnvVar(
                "EVAL_RELEASE_PHASE须为development或confirmation".into(),
            ));
        }
        let learn = read("prompts/learn_style_direct.md")?;
        let source_run = if phase == "confirmation" {
            Some(
                std::env::var("EVAL_RELEASE_SOURCE_RUN")
                    .map_err(|_| AppError::EnvVar("确认阶段需EVAL_RELEASE_SOURCE_RUN".into()))?,
            )
        } else {
            None
        };
        let confirmed_specs = if let Some(id) = &source_run {
            let root = format!("eval/runs/{}", run_id(id)?);
            let summary = read_json(&format!("{root}/summary.json"))?;
            let frozen = read_json(&format!("{root}/inputs.json"))?;
            if summary["status"] != "complete"
                || summary["gate"]["passed"] != true
                || frozen["phase"] != "development"
                || frozen["learn_prompt"] != learn
            {
                return Err(AppError::Parse(
                    "确认阶段必须使用已过开发门槛且提示词未变的Spec".into(),
                ));
            }
            Some(
                serde_json::from_value(summary["specs"].clone())
                    .map_err(|e| AppError::Parse(e.to_string()))?,
            )
        } else {
            None
        };
        Ok(Self {
            phase,
            source_run,
            learn,
            confirmed_specs,
            judge: read("prompts/eval_judge_learn.md")?,
            transfer: read("prompts/eval_judge_transfer_v2.md")?,
            sources: (1..=5)
                .map(|n| read(&format!("eval/samples/sample_{n:02}.md")))
                .collect::<Result<_, _>>()?,
            tasks: read_json("eval/fixtures/free_style_release_tasks.json")?,
        })
    }
}
async fn learn_case(store: &Store, input: &Input, sample: usize) -> Result<Vec<Spec>, AppError> {
    let mut specs = Vec::new();
    for variant in if sample % 2 == 1 {
        ["direct_free", "single_stage"]
    } else {
        ["single_stage", "direct_free"]
    } {
        let name = format!("learn_{variant}_{sample:02}");
        let source = &input.sources[sample - 1];
        let call = |stage, system: String, user: String| {
            let id = format!("{name}_{stage}");
            async move { store.call(&id, &system, &user, text_valid).await }
        };
        let learned = if variant == "direct_free" {
            crate::style_learning::extract_direct_with(source, &input.learn, call).await?
        } else {
            crate::style_learning::extract_legacy_with(source, call).await?
        };
        write_json(&store.root.join(format!("{name}.json")), &learned)?;
        let raw = store
            .call(
                &format!("spec_{variant}_{sample:02}_judge"),
                &input.judge,
                &format!("原始样文：\n{source}\n\n待评风格指令：\n{}", learned.spec),
                |s| parse_learn_judgment(s).map(|_| ()),
            )
            .await?;
        specs.push(Spec {
            sample,
            variant: variant.into(),
            judgment: score_learn_judgment(&raw, &learned.spec)?,
            text: learned.spec,
            extraction_calls: learned.calls,
        });
    }
    Ok(specs)
}

pub async fn run() -> Result<(), AppError> {
    let input = Arc::new(Input::load()?);
    let store = Arc::new(Store::from_env()?);
    if let Some(id) = &input.source_run {
        if read_json(&format!("eval/runs/{id}/summary.json"))?["model"] != store.model {
            return Err(AppError::Parse("确认模型必须与开发阶段一致".into()));
        }
    }
    store.freeze_inputs(&json!({"schema":1,"suite":"free_style_release","phase":input.phase,"source_run":input.source_run,
        "model":store.model,"learn_prompt":input.learn,"legacy_prompt":read("eval/fixtures/learn_style_single_v1.md")?,"spec_judge":input.judge,"transfer_judge":input.transfer,
        "sources":input.sources,"all_preregistered_tasks":input.tasks,"confirmed_specs":input.confirmed_specs,
        "outline_template":outline_system_prompt("{style}"),"body_template":style_system_prompt("{style}"),"render_template":render_prompt("{outline}","{idea}"),
        "criteria":"free-release-v1: Spec 400-600, actionability/distinctiveness mean>=4, every cleanliness>=4; per task style>=4, source>=4.8, facts strict>=4.4/creative>=4.8, naturalness>=3.8, completion>=4.8, key scores>2, length>=4/5"}))?;
    let result=async{
        let mut specs=if let Some(specs)=&input.confirmed_specs{specs.clone()}else{
            // One complete case smoke-tests the configured API before scheduling the rest.
            let mut specs=learn_case(&store,&input,1).await?;
            let mut queue=tokio::task::JoinSet::new();
            for sample in 2..=5 {let store=store.clone();let input=input.clone();queue.spawn(async move{learn_case(&store,&input,sample).await});}
            let mut errors=Vec::new();
            while let Some(r)=queue.join_next().await{match r{Ok(Ok(v))=>specs.extend(v),Ok(Err(e))=>errors.push(e.to_string()),Err(e)=>errors.push(e.to_string())}}
            if !errors.is_empty(){write_json(&store.root.join("learning_failures.json"),&errors)?;return Err(AppError::Parse("部分学习链失败，见learning_failures.json".into()));}
            specs
        };
        specs.sort_by(|a,b|(a.sample,&a.variant).cmp(&(b.sample,&b.variant)));
        for spec in &specs {std::fs::write(store.root.join(format!("spec_{}_{:02}.md",spec.variant,spec.sample)),&spec.text)?;}
        write_json(&store.root.join("specs.json"),&specs)?;
        let mut jobs=Vec::new();
        for mode in ["strict","creative"]{for sample in 1..=5{for variant in if sample%2==0 {["direct_free","single_stage"]}else{["single_stage","direct_free"]}{
            let spec=specs.iter().find(|s|s.sample==sample && s.variant==variant).ok_or_else(||AppError::Parse("缺少Spec".into()))?;
            let topic=input.tasks[&input.phase][mode].as_str().ok_or_else(||AppError::Parse("缺少预登记任务".into()))?.to_string();
            jobs.push(Job{id:format!("{mode}_{variant}_{sample:02}"),group:mode.into(),sample,variant:variant.into(),source:input.sources[sample-1].clone(),spec:spec.text.clone(),
                topic,mode:mode.into(),existing_body:None,provenance:json!({"spec_file":format!("spec_{variant}_{sample:02}.md"),"phase":input.phase,"source_run":input.source_run}),previous_transfer:Value::Null});
        }}}
        write_json(&store.root.join("jobs.json"),&jobs)?;
        let mut queue=tokio::task::JoinSet::new();
        for job in jobs {let store=store.clone();let input=input.clone();queue.spawn(async move{(job.id.clone(),run_job(&store,&input.transfer,&job).await)});}
        let mut rows:Vec<Row>=Vec::new();let mut errors=Vec::new();
        while let Some(r)=queue.join_next().await {match r {Ok((_,Ok(row)))=>rows.push(row),Ok((id,Err(e)))=>errors.push(json!({"id":id,"error":e.to_string()})),Err(e)=>errors.push(json!({"error":e.to_string()}))}}
        rows.sort_by(|a,b|a.id.cmp(&b.id));
        if !errors.is_empty(){write_json(&store.root.join("partial.json"),&json!({"status":"incomplete","rows":rows,"errors":errors}))?;return Err(AppError::Parse("部分写作/评分失败，见partial.json；保留检查点后按失败类型续跑".into()));}
        let gate=crate::free_style_gate::evaluate(&specs,&rows)?;
        write_json(&store.root.join("summary.json"),&json!({"status":"complete","phase":input.phase,"model":store.model,"at":chrono::Utc::now().to_rfc3339(),
            "human_reviewed":false,"specs":specs,"rows":rows,"gate":gate,"runtime_default_changed":false}))?;
        std::fs::write(store.root.join("summary.md"),crate::free_style_gate::report(&specs,&rows,&gate,&input.phase,&store.model))?;
        println!("[complete] {} gate={}",store.root.display(),gate["passed"]);Ok::<_,AppError>(())
    }.await;
    if let Err(e) = &result {
        write_json(
            &store.root.join("failure-release.json"),
            &json!({"status":"failed","at":chrono::Utc::now().to_rfc3339(),"error":e.to_string()}),
        )?;
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_run_id_cannot_escape_directory() {
        for id in ["", "../x", "a/b", "a\\b"] {
            assert!(run_id(id).is_err());
        }
        assert!(run_id("20260921-free-v2").is_ok());
    }
    #[test]
    #[ignore = "自由格式发布对照；真实API；EVAL_RUN_ID续跑，EVAL_RELEASE_PHASE选择开发/确认"]
    fn eval_free_style_release_suite() {
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(run())
            .unwrap();
    }
}
