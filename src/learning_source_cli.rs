//! Read-only acquisition commands; no learning model or stdin is used here.
use crate::{
    error::AppError,
    learning_sources::{self, SourceCollection},
    learning_workspace::LearningWorkspace,
};
use std::path::Path;

fn summary(collection: &SourceCollection) -> Result<(), AppError> {
    let assessment = collection.assessment()?;
    println!(
        "collection_id: {}\n结构状态：{}\n独立正文：{}篇；重复：{}篇",
        collection.id,
        serde_json::to_string(&assessment.status).map_err(|e| AppError::Parse(e.to_string()))?,
        assessment.unique_articles,
        assessment.duplicate_articles
    );
    println!(
        "ready仅表示可导出待学习正文；needs_inspection由后续领域Agent检查，不自动要求用户回答。"
    );
    for record in &collection.records {
        println!("{}｜{:?}｜{}", record.id, record.status, record.reason);
        for article in &record.candidates {
            println!(
                "  {}｜{}{}",
                article.id,
                article.title,
                article
                    .duplicate_of
                    .as_ref()
                    .map(|s| format!("（重复于{s}）"))
                    .unwrap_or_default()
            );
        }
        if !record.links.is_empty() {
            println!(
                "  保留{}个页面链接供明确选文；未自动遍历。",
                record.links.len()
            );
        }
    }
    for reason in assessment.reasons {
        println!("提示：{reason}");
    }
    println!("未调用学习模型；来源快照可用collection命令恢复查看。");
    Ok(())
}
pub(crate) async fn collect(ws: &LearningWorkspace, path: &Path) -> Result<(), AppError> {
    let (request, base) = learning_sources::read_request(path)?;
    println!(
        "正在采集{}项来源；不会自动开始模型学习。",
        request.sources.len()
    );
    summary(&learning_sources::collect(ws, request, &base).await?)
}
pub(crate) fn show(ws: &LearningWorkspace, id: &str) -> Result<(), AppError> {
    summary(&learning_sources::load(ws, id)?)
}
pub(crate) fn export(ws: &LearningWorkspace, id: &str, target: &Path) -> Result<(), AppError> {
    learning_sources::export_texts(ws, id, target)?;
    println!(
        "collection_id: {id}\n已导出待学习正文包：{}；未调用模型，未保存风格版本。",
        target.display()
    );
    Ok(())
}
