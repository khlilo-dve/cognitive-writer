use thiserror::Error;

#[allow(dead_code)]
#[derive(Error, Debug)]
pub enum AppError {
    #[error("环境变量 `{0}` 未设置")]
    EnvVar(String),
    #[error("文件读取失败: {0}")]
    FileRead(String),
    #[error("文件写入失败: {0}")]
    FileWrite(String),
    #[error("LLM API 错误 (HTTP {status}): {body}")]
    ApiError { status: u16, body: String },
    #[error("网络请求失败: {0}")]
    Network(String),
    #[error("API 响应解析失败: {0}")]
    Parse(String),
    #[error("API 返回空 choices")]
    EmptyChoices,
    #[error("剪贴板不可用: {0}")]
    Clipboard(String),
    #[error("风格目录无 .md 文件: {0}")]
    NoStyles(String),
    #[error("AI_EDIT 解析错误: {0}")]
    AiEditParse(String),
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
}
