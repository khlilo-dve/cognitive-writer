use crate::{
    error::AppError,
    research_store::{ResearchStore, Role},
};

pub(crate) async fn validated<T, F>(
    store: &ResearchStore,
    name: &str,
    prompt: &str,
    input: &str,
    parse: F,
) -> Result<T, AppError>
where
    F: Fn(&str) -> Result<T, AppError>,
{
    validated_as(store, name, Role::Structured, prompt, input, parse).await
}

pub(crate) async fn validated_as<T, F>(
    store: &ResearchStore,
    name: &str,
    role: Role,
    prompt: &str,
    input: &str,
    parse: F,
) -> Result<T, AppError>
where
    F: Fn(&str) -> Result<T, AppError>,
{
    let mut user = input.to_string();
    for attempt in 0..2 {
        let raw = store
            .call(&format!("{name}-{attempt}"), role, prompt, &user, true)
            .await?;
        match parse(&raw){
            Ok(v)=>return Ok(v),
            Err(e) if attempt==0=>user=format!("原始输入：{input}\n上次响应：{raw}\n结构/证据诊断：{e}\n只修复格式或证据定位，返回完整JSON，不改变判定标准。"),
            Err(e)=>return Err(e),
        }
    }
    Err(AppError::Parse("结构修复耗尽".into()))
}
