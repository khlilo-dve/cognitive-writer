//! 共享工具函数：环境变量读取、Markdown→微信 HTML、致命错误。

use pulldown_cmark::{Options, Parser as MdParser, html};

use crate::error::AppError;

/// 读取必需环境变量，缺失时返回错误。
pub fn env_var(key: &str) -> Result<String, AppError> {
    std::env::var(key).map_err(|_| AppError::EnvVar(key.into()))
}

/// 读取可选环境变量，缺失时用默认值。
pub fn env_var_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

/// Markdown → 微信公众号 HTML 片段（极简样式，仅保留排版与加粗）。
pub fn md_to_wechat_html(markdown: &str) -> String {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TABLES);

    let parser = MdParser::new_ext(markdown, opts);
    let mut raw = String::new();
    html::push_html(&mut raw, parser);

    // 微信编辑器会剥离 <style> 标签，所以直接移除 <hr> 标签
    let raw = raw.replace("<hr />", "").replace("<hr>", "");

    format!("<section style=\"font-size:15px;line-height:2;color:#333;\">{raw}</section>")
}

/// 打印错误并退出进程（仅 CLI 模式使用；REPL 路径应返回 Result）。
pub fn fatal(msg: impl std::fmt::Display) -> ! {
    eprintln!("[error] {}", msg);
    std::process::exit(1);
}

/// 历史兼容入口；新代码应使用具体 AppError 变体，已有 AppError 直接传播。
pub fn to_app_error(msg: impl std::fmt::Display) -> AppError {
    AppError::Intent(format!("{}", msg))
}
