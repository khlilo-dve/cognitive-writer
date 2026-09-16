//! 意图类型与公开入口：快速确认/取消预检和 LLM 分类。

use serde::{Deserialize, Serialize};

pub use crate::intent_classify::classify_intent;
pub use crate::intent_quick::{is_cancel, is_confirm, is_undo, parse_intent};

/// 多轮对话的会话状态。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[allow(dead_code)]
pub enum SessionState {
    /// 空闲状态，等待用户发起新操作。
    Idle,
    /// 已接收写作需求，等待用户明确选择风格。
    WaitingForGenerationStyle,
    /// 等待用户对大纲骨架的反馈。
    WaitingForOutline,
    /// 已保留局部修改指令，等待用户明确目标内容块。
    WaitingForEditTarget,
    /// 等待用户对正文的反馈。
    WaitingForFulltext,
    /// 等待用户确认发布。
    WaitingForPublish,
    /// 等待用户为已学习的风格命名。
    WaitingForStyleName,
    /// 等待用户确认删除指定风格。
    WaitingForDeleteStyleConfirmation,
}

/// 从用户输入解析出的结构化意图。
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum Intent {
    /// 生成文章：主题必需，风格名称可为空并由 REPL 追问。
    Generate { topic: String, style_name: String },
    /// 从 URL 学习写作风格。
    Learn { url: String },
    /// 列出所有可用风格。
    ListStyles,
    /// 查看指定风格的详情。
    ShowStyle { name: String },
    /// 删除指定风格。
    DeleteStyle { name: String },
    /// 局部重绘：修改 .md 文件中的特定段落。
    RefineFile { path: String, instruction: String },
    /// 整体重写：根据指令重写 .md 文件。
    UpdateFile { path: String, instruction: String },
    /// 确认当前待处理操作。
    Confirm,
    /// 撤销上一次成功的局部修改，恢复修改前的正文。
    Undo,
    /// 取消当前待处理操作。
    Cancel,
    /// 修改特定段落：携带修改指令。
    ModifySection { instruction: String },
    /// 更换写作风格。
    ChangeStyle { style_name: String },
    /// 发布文章（公众号 + 网站）。
    Publish,
    /// 仅发布到网站。
    PublishWebsiteOnly,
    /// 暂缓/搁置当前操作。
    Hold,
    /// 无法识别的意图（附带原因，用于准确报错）。
    Unknown { reason: String },
}
