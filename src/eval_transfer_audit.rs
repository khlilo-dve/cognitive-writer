//! Transfer v2 audit: rescore frozen articles, then compare two factual permissions.
use crate::error::AppError;
use crate::eval_store::{Store, write_json};
use crate::generate::{outline_system_prompt, render_prompt, style_system_prompt};
use crate::transfer_quality::{Judgment, grade_with, length_check};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;

const PREVIOUS: &str = "eval/runs/20260917-single-article-v3";
const DIRECT: &str = "eval/runs/20260917-single-article-direct-v1";
const FACTS: &str = "社区举办旧物修理日，地点是社区活动室。桌上有一盏接触不良的台灯和一台转动缓慢的旧风扇。两名志愿者负责检查，来访者可以旁观并提问。台灯的接线松动，志愿者重新固定接线后，灯恢复正常。风扇需要更换零件，当天没有合适零件，工作人员登记了零件型号和后续联系办法，没有宣称已经修好。活动只提供检查和力所能及的修复，不保证所有物品都能恢复。有人重视继续使用原物，有人更在意等待与购买替代品之间的取舍。请涵盖修好的台灯、未能当场修好的风扇、旁观提问和修复边界，并讨论或通过场景呈现修理与替换的取舍。";
const LENGTH: &str =
    "正文600–900个非空白Unicode字符，含标题、标点和Markdown，空白不计；只输出文章。";
fn topic(creative: bool) -> String {
    let permission = if creative {
        "这是明确标注的虚构场景创作，请在开头注明‘虚构场景’。可加入虚构人物、观察者、动作、对话、物件和独立事件，叙述者经历只属于虚构角色；不能改写给定核心事实或把虚构冒充作者现实经历，不得伪造外部真实统计或真实引用。"
    } else {
        "这是严格素材写作。允许同义转述、逻辑解释和不增加独立事实的连接动作。不得补造人物履历、精确时间/数值、新物件、独立事件、对话主张、外部统计/引用或亲身经历。写法和文体可自由选择，不要求固定结构。"
    };
    format!("{LENGTH}\n主题与核心素材：{FACTS}\n事实权限：{permission}")
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Job {
    pub id: String,
    pub group: String,
    pub sample: usize,
    pub variant: String,
    pub source: String,
    pub spec: String,
    pub topic: String,
    pub mode: String,
    pub existing_body: Option<String>,
    pub provenance: Value,
    pub previous_transfer: Value,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Row {
    pub id: String,
    pub group: String,
    pub sample: usize,
    pub variant: String,
    pub body_artifact: String,
    pub length: Value,
    pub judgment: Judgment,
    pub judge_calls: usize,
    pub generation_calls: usize,
    pub previous_transfer: Value,
}
#[derive(Clone, Serialize)]
struct Control {
    id: String,
    source: String,
    topic: String,
    mode: String,
    body: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct ControlResult {
    pub id: String,
    pub passed: bool,
    pub judgment: Judgment,
    pub judge_calls: usize,
}
fn read(path: &str) -> Result<String, AppError> {
    Ok(std::fs::read_to_string(path)?)
}
fn read_json(path: &str) -> Result<Value, AppError> {
    serde_json::from_str(&read(path)?).map_err(|e| AppError::Parse(e.to_string()))
}
fn field<'a>(v: &'a Value, key: &str) -> Result<&'a str, AppError> {
    v[key]
        .as_str()
        .ok_or_else(|| AppError::Parse(format!("历史记录缺少{key}")))
}
fn load_jobs() -> Result<Vec<Job>, AppError> {
    let baseline = read_json(&format!("{PREVIOUS}/summary.json"))?;
    let direct = read_json(&format!("{DIRECT}/summary.json"))?;
    let frozen = read_json(&format!("{PREVIOUS}/inputs.json"))?;
    if baseline["status"] != "complete"
        || direct["status"] != "complete"
        || baseline["model"] != direct["model"]
    {
        return Err(AppError::Parse("历史对照未完整完成或模型不一致".into()));
    }
    let mut jobs = Vec::new();
    for group in ["historical", "strict", "creative"] {
        for sample in 1..=5 {
            for offset in 0..3 {
                let variant = ["single_stage", "two_stage", "direct_free"][(sample + offset) % 3];
                let (root, summary) = if variant == "direct_free" {
                    (DIRECT, &direct)
                } else {
                    (PREVIOUS, &baseline)
                };
                let old = summary["rows"]
                    .as_array()
                    .ok_or_else(|| AppError::Parse("历史rows缺失".into()))?
                    .iter()
                    .find(|r| r["case"] == sample && r["variant"] == variant)
                    .ok_or_else(|| AppError::Parse("历史方法/样文配对缺失".into()))?;
                let body = read(&format!("{root}/{}", field(old, "body")?))?;
                let source = frozen["samples"][sample - 1]
                    .as_str()
                    .ok_or_else(|| AppError::Parse("历史样文缺失".into()))?
                    .to_string();
                let historical = group == "historical";
                jobs.push(Job {id:format!("item_{:02}",jobs.len()+1),group:group.into(),sample,variant:variant.into(),
                    source,spec:read(&format!("{root}/{}",field(old,"spec")?))?,
                    topic:if historical {field(&frozen,"topic")?.into()} else {topic(group=="creative")},
                    mode:if group=="creative" {"creative"} else {"strict"}.into(),
                    existing_body:historical.then_some(body),
                    provenance:json!({"run":root,"spec":old["spec"],"body":old["body"],"generation_model":summary["model"]}),
                    previous_transfer:if historical {old["transfer"].clone()} else {Value::Null}});
            }
        }
    }
    Ok(jobs)
}
fn controls() -> Vec<Control> {
    let source = "人们把热闹当作解决。我家那盏旧台灯坏了，邻居送来一辆红色自行车。我说，先看哪里断了，再谈换什么。";
    let base = "社区修理日有两名志愿者，活动桌上放着一盏接触不良的台灯。志愿者固定松动接线后，灯恢复正常。";
    [
        ("extra_strict", "strict", false),
        ("extra_creative", "creative", false),
        ("leak_strict", "strict", true),
        ("leak_creative", "creative", true),
    ]
    .into_iter()
    .map(|(id, mode, leak)| {
        let added = if leak {
            "邻居送来一辆红色自行车。"
        } else {
            "一个孩子抱着蓝色木马走进来。志愿者替他接好断开的木轮，孩子转了三圈才离开。"
        };
        let permission = if mode == "strict" {
            "只写素材给定事件，不得增添其他事件，即使标注虚构也不允许。"
        } else {
            "允许加入虚构人物、动作和事件，明确虚构即可，不改变核心素材。"
        };
        Control {
            id: id.into(),
            source: source.into(),
            mode: mode.into(),
            topic: format!("{LENGTH} 核心素材：{base} {permission}"),
            body: format!("虚构场景：{base}{added}"),
        }
    })
    .collect()
}
fn text_valid(text: &str) -> Result<(), AppError> {
    if text.trim().is_empty() {
        Err(AppError::Parse("空模型输出".into()))
    } else {
        Ok(())
    }
}
async fn judge(
    store: &Store,
    prompt: &str,
    id: &str,
    source: &str,
    topic: &str,
    mode: &str,
    body: &str,
) -> Result<(Judgment, usize), AppError> {
    grade_with(source, topic, mode, body, |attempt, user| {
        let name = format!("{id}_judge_{attempt}");
        async move { store.call_json(&name, prompt, &user, text_valid).await }
    })
    .await
}
pub(crate) async fn run_job(store: &Store, prompt: &str, job: &Job) -> Result<Row, AppError> {
    let (body, generation_calls) = if let Some(body) = &job.existing_body {
        (body.clone(), 0)
    } else {
        let outline = store
            .call(
                &format!("{}_outline", job.id),
                &outline_system_prompt(&job.spec),
                &job.topic,
                text_valid,
            )
            .await?;
        let body = store
            .call(
                &format!("{}_body", job.id),
                &style_system_prompt(&job.spec),
                &render_prompt(&outline, &job.topic),
                text_valid,
            )
            .await?;
        (body, 2)
    };
    let artifact = format!("{}.md", job.id);
    std::fs::write(store.root.join(&artifact), &body)?;
    let (judgment, judge_calls) = judge(
        store,
        prompt,
        &job.id,
        &job.source,
        &job.topic,
        &job.mode,
        &body,
    )
    .await?;
    let row = Row {
        id: job.id.clone(),
        group: job.group.clone(),
        sample: job.sample,
        variant: job.variant.clone(),
        body_artifact: artifact,
        length: length_check(&body),
        judgment,
        judge_calls,
        generation_calls,
        previous_transfer: job.previous_transfer.clone(),
    };
    write_json(&store.root.join(format!("{}_result.json", job.id)), &row)?;
    Ok(row)
}

pub async fn run() -> Result<(), AppError> {
    let jobs = load_jobs()?;
    let controls = controls();
    let prompt = Arc::new(read("prompts/eval_judge_transfer_v2.md")?);
    let store = Arc::new(Store::from_env()?);
    let mut extraction_prompts = serde_json::Map::new();
    for path in [
        "prompts/analyze_style.md",
        "prompts/learn_style.md",
        "prompts/learn_style_direct.md",
        "eval/fixtures/learn_style_single_v1.md",
    ] {
        extraction_prompts.insert(path.into(), json!(read(path)?));
    }
    store.freeze_inputs(&json!({"schema":1,"suite":"transfer_v2_audit","model":store.model,"judge_prompt":*prompt,
        "jobs":jobs,"controls":controls,"extraction_prompts_unchanged":extraction_prompts,
        "outline_template":outline_system_prompt("{style}"),"body_template":style_system_prompt("{style}"),
        "render_template":render_prompt("{outline}","{idea}"),"length_policy":"non-whitespace Unicode scalars; 600-900 inclusive; diagnostic only",
        "decision_policy":"report disjoint dimensions by task; no combined score or automatic adoption"}))?;
    let work=async {
        let mut probes=Vec::new();
        for c in &controls {
            let (j,calls)=judge(&store,&prompt,&c.id,&c.source,&c.topic,&c.mode,&c.body).await?;
            let passed=if c.id.starts_with("extra_") {
                j.scores.source_independence==5.0 && if c.mode=="strict" {j.scores.factual_compliance<=3.0} else {j.scores.factual_compliance==5.0}
            } else {j.scores.source_independence<=3.0};
            probes.push(ControlResult{id:c.id.clone(),passed,judgment:j,judge_calls:calls});
        }
        write_json(&store.root.join("controls.json"),&probes)?;
        let mut queue=tokio::task::JoinSet::new();
        for job in jobs.clone() {
            let store=store.clone();let prompt=prompt.clone();
            queue.spawn(async move {(job.id.clone(),run_job(&store,&prompt,&job).await)});
        }
        let mut rows=Vec::new();let mut errors=Vec::new();
        while let Some(result)=queue.join_next().await {
            match result {
                Ok((_,Ok(row)))=>rows.push(row),
                Ok((id,Err(e)))=>errors.push(json!({"id":id,"error":e.to_string()})),
                Err(e)=>errors.push(json!({"error":e.to_string()})),
            }
        }
        rows.sort_by(|a,b|a.id.cmp(&b.id));
        if !errors.is_empty() {
            write_json(&store.root.join("partial.json"),&json!({"status":"incomplete","rows":rows,"errors":errors}))?;
            return Err(AppError::Parse("部分迁移链失败；检查点已保留。运输失败可同ID续跑；评分格式或引用修复耗尽时新建ID，仅复用已验证请求".into()));
        }
        let aggregates=crate::transfer_report::aggregate(&rows)?;
        write_json(&store.root.join("summary.json"),&json!({"status":"complete","at":chrono::Utc::now().to_rfc3339(),
            "model":store.model,"rubric":"Transfer-v2","human_reviewed":false,"controls_passed":probes.iter().all(|p|p.passed),
            "controls":probes,"control_mode_drift":crate::transfer_report::control_drift(&probes),"rows":rows,"aggregates":aggregates,"automatic_adoption":false}))?;
        std::fs::write(store.root.join("summary.md"),crate::transfer_report::report(&rows,&probes,&store.model,&aggregates)?)?;
        println!("[complete] {} (45 articles, 4 controls; no extraction requests)",store.root.display());
        Ok::<_,AppError>(())
    }.await;
    if let Err(e) = &work {
        write_json(
            &store.root.join("failure-transfer-audit.json"),
            &json!({"status":"failed","at":chrono::Utc::now().to_rfc3339(),"error":e.to_string()}),
        )?;
    }
    work
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tasks_share_core_material_and_explicitly_change_only_permissions() {
        assert!(topic(false).contains(FACTS));
        assert!(topic(true).contains(FACTS));
        assert!(topic(false).contains("严格素材"));
        assert!(topic(true).contains("虚构场景"));
        let probes = controls();
        assert_eq!(probes.len(), 4);
        assert_eq!(probes[0].body, probes[1].body);
        assert_eq!(probes[2].body, probes[3].body);
    }
    #[test]
    #[ignore = "Transfer v2历史重评与两类任务补测；真实API；EVAL_RUN_ID续跑"]
    fn eval_transfer_audit_suite() {
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(run())
            .unwrap();
    }
}
