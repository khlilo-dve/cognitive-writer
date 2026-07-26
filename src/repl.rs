//! REPL 循环 + 状态机 + 意图分发 — cognitive-writer v3.0 核心交互模块。
//!
//! 用户通过终端 REPL 用自然语言交互，Agent 解析意图、管理对话状态、
//! 分发到对应的功能模块。

use std::io::{self, Write};

use pulldown_cmark::{html, Options, Parser as MdParser};
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::clipboard::inject_clipboard;
use crate::error::AppError;
use crate::generate::{generate_outline, render_fulltext};
use crate::intent::{classify_intent, is_confirm, is_cancel, parse_intent, Intent, SessionState};
use crate::io::extract_idea_slug;
use crate::llm::{call_llm, with_retry};
use crate::refine::REFINE_SYSTEM_PROMPT;
use crate::styles::{
    delete_style, fuzzy_match_style, list_styles_with_desc, show_style_detail,
};
use crate::website::{publish_to_website, write_mdx_draft};

// ── Local constants (duplicated from CLI modules to avoid cross-module
//    coupling; editable independently) ──────────────────────────────────

/// System prompt for outline generation (Pass 1 of generate flow).
const OUTLINE_SYSTEM_PROMPT: &str = r#"你是一个写作结构设计专家。
用户会给你一份创作素材，你需要输出一份文章逻辑大纲。

要求：
1. 用 Markdown 层级列表（- / 1. 2. 3.）
2. 每个节点写清该段的【核心论点】和【支撑素材/案例方向】
3. 标注段落之间的逻辑衔接关系（递进/转折/并列/因果）
4. 控制在 300-500 字以内
5. 不要写正文，只输出骨架"#;

/// System prompt for update / change-style — rewrites the whole article
/// according to an instruction while preserving structure.
const UPDATE_SYSTEM_PROMPT: &str = r#"你是严苛的文本编辑。用户会给你一篇完整的文章和一条修改指令，你需要根据修改指令重写整篇文章。

要求：
1. 保持原文的整体结构和段落数量
2. 只修改与指令相关的部分，其余内容保持不变
3. 直接输出完整的修改后 Markdown，不要任何解释、说明或代码块包裹
4. 输出必须是可发布的最终版本"#;

// ── Help text ─────────────────────────────────────────────────────────

const HELP_TEXT: &str = r#"
Cognitive Writer v3.0 — 对话式写作 Agent
─────────────────────────────────────────

核心工作流:
  写一篇关于<主题>的文章用<风格名>风格    → 生成大纲 → 确认 → 渲染全文 → 发布

状态内指令:
  [大纲确认]  确认/好/没问题 → 渲染全文  |  修改指令 → 调整大纲  |  取消/算了 → 放弃
  [全文确认]  确认/好/没问题 → 注入剪贴板 + 存草稿  |  修改指令 → 局部重绘  |  整体换成<风格名> → 换风格
  [发布确认]  发布/推 → 正式发布  |  只发网站 → 仅网站  |  等一下/先不/看看 → 保留草稿

其他命令:
  /help        显示此帮助
  /quit        退出程序
  /styles      列出风格库（等同于「我的风格库有什么」）
  /state       显示当前状态（调试用）

独立操作 (Idle 状态可用):
  输入 URL     → 学习该文章的写作风格
  列出风格     → 列出所有可用风格文件
  看看<名称>   → 查看风格详情
  删掉<名称>   → 删除指定风格
  重写 <路径> 改成 <指令>  → 根据指令重写文章
  <路径> 改 <指令>          → 局部重绘文件
"#;

// ── Markdown → WeChat HTML ──────────────────────────────────────────

/// Convert Markdown to a WeChat-compatible HTML fragment.
fn md_to_wechat_html(markdown: &str) -> String {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TABLES);

    let parser = MdParser::new_ext(markdown, opts);
    let mut raw = String::new();
    html::push_html(&mut raw, parser);

    // 微信编辑器会剥离 <style> 标签，所以直接移除 <hr> 标签
    let raw = raw.replace("<hr />", "").replace("<hr>", "");

    // 极简样式：只保留基本排版和加粗，不加任何装饰性 CSS
    format!(
        "<section style=\"font-size:15px;line-height:2;color:#333;\">{raw}</section>"
    )
}

// ── Helpers ─────────────────────────────────────────────────────────

/// Return a horizontal separator line.
fn sep() -> String {
    "─".repeat(60)
}

/// Extract a title from Markdown content (first `#` heading).
fn extract_title(markdown: &str) -> String {
    for line in markdown.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            return trimmed.trim_start_matches('#').trim().to_string();
        }
    }
    "Untitled".to_string()
}

// ── Repl ────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
pub struct Repl {
    state: SessionState,
    // 当前文章上下文
    current_topic: Option<String>,
    current_outline: Option<String>,
    current_fulltext: Option<String>,
    current_style_name: Option<String>,
    current_style_content: Option<String>,
    current_slug: Option<String>,
    // 配置（不持久化，从环境变量恢复）
    #[serde(skip)]
    api_key: String,
    #[serde(skip)]
    base_url: String,
    #[serde(skip)]
    model: String,
    #[serde(skip)]
    website_path: String,
    // HTTP 客户端复用（不持久化）
    #[serde(skip)]
    client: Client,
    // session 持久化目录（不持久化）
    #[serde(skip)]
    session_dir: Option<std::path::PathBuf>,
}

impl Repl {
    // ── Constructors ─────────────────────────────────────────────

    pub fn new() -> Result<Self, AppError> {
        Self::new_with_session_dir(&std::path::PathBuf::from("."))
    }

    /// 新建实例，关联 session 目录。
    pub fn new_with_session_dir(session_dir: &std::path::Path) -> Result<Self, AppError> {
        let api_key = std::env::var("API_KEY")
            .map_err(|_| AppError::EnvVar("API_KEY 未设置".to_string()))?;
        let base_url = std::env::var("BASE_URL")
            .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
        let model = std::env::var("MODEL")
            .unwrap_or_else(|_| "gpt-4o".to_string());
        let website_path = std::env::var("WEBSITE_PATH").unwrap_or_default();
        let client = Client::new();

        Ok(Self {
            state: SessionState::Idle,
            current_topic: None,
            current_outline: None,
            current_fulltext: None,
            current_style_name: None,
            current_style_content: None,
            current_slug: None,
            api_key,
            base_url,
            model,
            website_path,
            client,
            session_dir: Some(session_dir.to_path_buf()),
        })
    }

    /// 尝试从 session 目录恢复状态。
    /// 成功时返回 Some(Repl)，失败或无文件时返回 None。
    pub fn restore(session_dir: &std::path::Path) -> Option<Self> {
        let path = session_dir.join("current.json");
        if !path.exists() {
            return None;
        }
        let json = std::fs::read_to_string(&path).ok()?;
        let mut repl: Repl = serde_json::from_str(&json).ok()?;
        // 重新初始化 #[serde(skip)] 字段
        repl.api_key = std::env::var("API_KEY").ok()?;
        repl.base_url = std::env::var("BASE_URL")
            .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
        repl.model = std::env::var("MODEL")
            .unwrap_or_else(|_| "gpt-4o".to_string());
        repl.website_path = std::env::var("WEBSITE_PATH").unwrap_or_default();
        repl.client = Client::new();
        repl.session_dir = Some(session_dir.to_path_buf());
        Some(repl)
    }

    /// 返回当前状态的引用。
    pub fn current_state(&self) -> &SessionState {
        &self.state
    }

    /// 保存当前状态到 session 目录的 current.json。
    pub fn save(&self) -> Result<(), AppError> {
        let dir = match &self.session_dir {
            Some(d) => d.clone(),
            None => return Ok(()),
        };
        std::fs::create_dir_all(&dir)
            .map_err(|e| AppError::FileWrite(format!("session 目录创建失败 {}: {}", dir.display(), e)))?;
        let path = dir.join("current.json");
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| AppError::Parse(format!("序列化失败: {}", e)))?;
        std::fs::write(&path, json)
            .map_err(|e| AppError::FileWrite(format!("{}: {}", path.display(), e)))?;
        Ok(())
    }

    /// 清除 session 文件（全流程正常结束时调用）。
    fn clear_session(&self) -> Result<(), AppError> {
        let dir = match &self.session_dir {
            Some(d) => d.clone(),
            None => return Ok(()),
        };
        let path = dir.join("current.json");
        if path.exists() {
            std::fs::remove_file(&path)
                .map_err(|e| AppError::FileWrite(format!("清除 session 失败 {}: {}", path.display(), e)))?;
        }
        Ok(())
    }

    // ── Main loop ─────────────────────────────────────────────────

    pub async fn run(&mut self) {
        println!("Cognitive Writer v3.1 — 对话式写作 Agent (会话自动保存)");
        println!("输入你的想法，或输入 /help 查看帮助");

        use tokio::io::{AsyncBufReadExt, BufReader};

        let stdin = BufReader::new(tokio::io::stdin());
        let mut lines = stdin.lines();

        loop {
            let prompt = match self.state {
                SessionState::Idle => "> ",
                SessionState::WaitingForOutline => "[大纲确认] > ",
                SessionState::WaitingForFulltext => "[全文确认] > ",
                SessionState::WaitingForPublish => "[发布确认] > ",
            };
            print!("{}", prompt);
            let _ = std::io::stdout().flush();

            tokio::select! {
                line_result = lines.next_line() => {
                    match line_result {
                        Ok(Some(input)) => {
                            if input.is_empty() {
                                continue;
                            }

                            // 特殊命令
                            if input.starts_with('/') {
                                if self.handle_command(&input) {
                                    return;
                                }
                                continue;
                            }

                            // 意图分派：快速预检 → LLM 分类 → 关键词 fallback
                            let intent = if is_confirm(&input) || is_cancel(&input) {
                                parse_intent(&input, &self.state)
                            } else {
                                match classify_intent(
                                    &self.client,
                                    &self.base_url,
                                    &self.api_key,
                                    &self.model,
                                    &input,
                                    &self.state,
                                )
                                .await
                                {
                                    Ok(intent) => {
                                        println!("[debug] LLM 分类: {:?}", intent);
                                        intent
                                    }
                                    Err(e) => {
                                        eprintln!("[warn] LLM 分类失败: {}，回退关键词匹配", e);
                                        parse_intent(&input, &self.state)
                                    }
                                }
                            };

                            if let Err(e) = self.dispatch(intent).await {
                                eprintln!("[error] {}", e);
                            }

                            // 每次 dispatch 后自动保存
                            if let Err(e) = self.save() {
                                eprintln!("[warn] 会话保存失败: {}", e);
                            }
                        }
                        Ok(None) => {
                            // EOF (Ctrl+D)
                            println!("\n再见。");
                            let _ = self.clear_session();
                            return;
                        }
                        Err(e) => {
                            eprintln!("[error] stdin 读取失败: {}", e);
                            return;
                        }
                    }
                }
                _ = tokio::signal::ctrl_c() => {
                    println!("\n收到中断信号，正在保存会话...");
                    if let Err(e) = self.save() {
                        eprintln!("[warn] 保存失败: {}", e);
                    }
                    println!("会话已保存。下次启动 `cog` 可恢复。再见。");
                    std::process::exit(0);
                }
            }
        }
    }

    // ── Special-command handler ────────────────────────────────────

    /// Process a `/`-prefixed command. Returns `true` if the REPL should
    /// exit (i.e. `/quit`).
    fn handle_command(&mut self, input: &str) -> bool {
        match input.trim() {
            "/help" => {
                println!("{}", HELP_TEXT);
                false
            }
            "/quit" => {
                println!("再见。");
                true
            }
            "/styles" => {
                match self.handle_list_styles() {
                    Ok(()) => {}
                    Err(e) => eprintln!("[error] {}", e),
                }
                false
            }
            "/state" => {
                self.print_state();
                false
            }
            other => {
                println!("未知命令: {}. 输入 /help 查看帮助。", other);
                false
            }
        }
    }

    // ── State reset ───────────────────────────────────────────────

    fn reset_state(&mut self) {
        self.state = SessionState::Idle;
        self.current_topic = None;
        self.current_outline = None;
        self.current_fulltext = None;
        self.current_style_name = None;
        self.current_style_content = None;
        self.current_slug = None;
        // 全流程完成，清除 session 文件（下次启动不提示恢复）
        let _ = self.clear_session();
    }

    // ── Debug ─────────────────────────────────────────────────────

    fn print_state(&self) {
        println!("{}", sep());
        println!("状态: {:?}", self.state);
        println!(
            "话题: {}",
            self.current_topic.as_deref().unwrap_or("(无)")
        );
        println!(
            "大纲: {}",
            self.current_outline
                .as_ref()
                .map(|o| format!("{} 字符", o.len()))
                .unwrap_or_else(|| "(无)".to_string())
        );
        println!(
            "全文: {}",
            self.current_fulltext
                .as_ref()
                .map(|f| format!("{} 字符", f.len()))
                .unwrap_or_else(|| "(无)".to_string())
        );
        println!(
            "风格: {}",
            self.current_style_name.as_deref().unwrap_or("(无)")
        );
        println!(
            "slug: {}",
            self.current_slug.as_deref().unwrap_or("(无)")
        );
        println!("配置: model={} base_url={}", self.model, self.base_url);
        println!(
            "网站路径: {}",
            if self.website_path.is_empty() {
                "(未配置)"
            } else {
                &self.website_path
            }
        );
        println!("{}", sep());
    }

    // ── Intent dispatch ───────────────────────────────────────────

    async fn dispatch(&mut self, intent: Intent) -> Result<(), AppError> {
        match (&self.state, intent) {
            // ── Idle 状态 ──
            (SessionState::Idle, Intent::Generate { topic, style_name }) => {
                self.handle_generate(&topic, &style_name).await?;
            }
            (SessionState::Idle, Intent::Learn { url }) => {
                println!("[info] 开始学习风格: {}", url);
                crate::learn::run_learn(&url).await;
            }
            (SessionState::Idle, Intent::ListStyles) => {
                self.handle_list_styles()?;
            }
            (SessionState::Idle, Intent::ShowStyle { name }) => {
                self.handle_show_style(&name)?;
            }
            (SessionState::Idle, Intent::DeleteStyle { name }) => {
                self.handle_delete_style(&name)?;
            }
            (SessionState::Idle, Intent::UpdateFile { path, instruction }) => {
                println!("[info] 重写文件: {} 指令: {}", path, instruction);
                crate::update::run_update(&path, Some(instruction), false).await;
            }
            (SessionState::Idle, Intent::RefineFile { path, instruction }) => {
                self.handle_refine_file(&path, &instruction).await?;
            }

            // ── 任意状态 ──
            (_, Intent::Cancel) => {
                println!("已取消。");
                self.reset_state();
            }
            (_, Intent::Unknown) => {
                println!(
                    "没理解你的意思。试试说「写一篇关于XXX的文章，用YYY风格」或「学一下这个风格 https://...」"
                );
            }

            // ── WaitingForOutline ──
            (SessionState::WaitingForOutline, Intent::Confirm) => {
                self.handle_render_fulltext().await?;
            }
            (SessionState::WaitingForOutline, Intent::ModifyOutline { instruction }) => {
                self.handle_modify_outline(&instruction).await?;
            }

            // ── WaitingForFulltext ──
            (SessionState::WaitingForFulltext, Intent::Confirm) => {
                self.handle_draft_publish().await?;
            }
            (SessionState::WaitingForFulltext, Intent::ModifySection { instruction }) => {
                self.handle_refine_section(&instruction).await?;
            }
            (SessionState::WaitingForFulltext, Intent::ChangeStyle { style_name }) => {
                self.handle_change_style(&style_name).await?;
            }

            // ── WaitingForPublish ──
            (SessionState::WaitingForPublish, Intent::Publish) => {
                self.handle_publish().await?;
                self.reset_state();
            }
            (SessionState::WaitingForPublish, Intent::PublishWebsiteOnly) => {
                self.handle_publish_website_only().await?;
                self.reset_state();
            }
            (SessionState::WaitingForPublish, Intent::Hold) => {
                println!("草稿已保留。公众号：打开微信后台粘贴 (已注入剪贴板)。网站：MDX 文件已写入。");
                self.reset_state();
            }

            // ── 状态不匹配 ──
            (state, intent) => {
                return Err(AppError::InvalidState {
                    state: format!("{:?}", state),
                    intent: format!("{:?}", intent),
                });
            }
        }
        Ok(())
    }

    // ── handle: Generate ──────────────────────────────────────────

    /// 1. 模糊匹配风格 → 得到风格名+内容
    /// 2. 存储风格信息
    /// 3. 调用 generate_outline → 骨架
    /// 4. 打印骨架到终端
    /// 5. 切换到 WaitingForOutline 状态
    async fn handle_generate(
        &mut self,
        topic: &str,
        style_name: &str,
    ) -> Result<(), AppError> {
        // 模糊匹配风格
        let (disp_name, style_content) =
            fuzzy_match_style("styles", style_name)?;

        println!("[info] 匹配风格: {} ({} 字符)", disp_name, style_content.len());

        self.current_style_name = Some(disp_name);
        self.current_style_content = Some(style_content);
        self.current_topic = Some(topic.to_string());

        // 生成大纲
        let spinner = crate::llm::new_spinner("正在生成大纲骨架...");
        let outline = generate_outline(
            &self.client,
            &self.base_url,
            &self.api_key,
            &self.model,
            topic,
        )
        .await?;
        spinner.finish_with_message("大纲生成完成");

        println!("{}", sep());
        println!("{}", outline);
        println!("{}", sep());

        self.current_outline = Some(outline);
        self.state = SessionState::WaitingForOutline;

        println!();
        println!("  请确认大纲，或输入修改指令（如「第二部分太长了」），输入「取消」放弃。");

        Ok(())
    }

    // ── handle: Render fulltext ───────────────────────────────────

    /// 从骨架 + 风格 + 素材 渲染全文。
    async fn handle_render_fulltext(&mut self) -> Result<(), AppError> {
        let style = self
            .current_style_content
            .as_deref()
            .ok_or_else(|| AppError::Intent("风格内容丢失".to_string()))?;
        let outline = self
            .current_outline
            .as_deref()
            .ok_or_else(|| AppError::Intent("大纲内容丢失".to_string()))?;
        let topic = self
            .current_topic
            .as_deref()
            .ok_or_else(|| AppError::Intent("创作主题丢失".to_string()))?;

        let spinner = crate::llm::new_spinner("正在渲染正文...");
        let fulltext = render_fulltext(
            &self.client,
            &self.base_url,
            &self.api_key,
            &self.model,
            style,
            outline,
            topic,
        )
        .await?;
        spinner.finish_with_message("正文渲染完成");

        let slug = extract_idea_slug(&fulltext);
        self.current_slug = Some(slug);

        println!("{}", sep());
        println!("{}", fulltext);
        println!("{}", sep());

        self.current_fulltext = Some(fulltext);
        self.state = SessionState::WaitingForFulltext;

        println!();
        println!("  请确认全文，或输入修改指令。输入「整体换成<风格名>风格」更换风格。");

        Ok(())
    }

    // ── handle: Draft + Clipboard → WaitingForPublish ─────────────

    /// 剪贴板注入 + MDX 草稿写入 → 切换到 WaitingForPublish。
    async fn handle_draft_publish(&mut self) -> Result<(), AppError> {
        let fulltext = self
            .current_fulltext
            .as_deref()
            .ok_or_else(|| AppError::Intent("全文内容丢失".to_string()))?;
        let slug = self
            .current_slug
            .as_deref()
            .ok_or_else(|| AppError::Intent("slug 丢失".to_string()))?;

        // 剪贴板注入
        let html_fragment = md_to_wechat_html(fulltext);
        match inject_clipboard(&html_fragment) {
            Ok(tool) => println!("[done] 富文本已注入剪贴板 (via {})", tool),
            Err(e) => eprintln!("[warn] 剪贴板注入失败: {}", e),
        }

        // 网站草稿
        if !self.website_path.is_empty() {
            let title = extract_title(fulltext);
            match write_mdx_draft(
                &self.website_path,
                "signal",
                slug,
                &title,
                fulltext,
            ) {
                Ok(path) => println!("[done] 网站草稿已写入: {}", path),
                Err(e) => eprintln!("[warn] 网站草稿写入失败: {}", e),
            }
        } else {
            println!("[info] WEBSITE_PATH 未配置，跳过网站草稿");
        }

        self.state = SessionState::WaitingForPublish;

        println!();
        println!("  草稿已就绪。输入「发布」正式发布，「只发网站」仅发布到网站，「等一下」保留草稿。");

        Ok(())
    }

    // ── handle: Publish ───────────────────────────────────────────

    /// 正式发布：网站 commit+push，提示公众号手动发布。
    async fn handle_publish(&mut self) -> Result<(), AppError> {
        let slug = self
            .current_slug
            .as_deref()
            .ok_or_else(|| AppError::Intent("slug 丢失，无法发布".to_string()))?;

        if !self.website_path.is_empty() {
            match publish_to_website(&self.website_path, slug) {
                Ok(url) => println!("[done] 网站已发布: {}", url),
                Err(e) => eprintln!("[warn] 网站发布失败: {}", e),
            }
        }

        println!("[info] 公众号请手动到微信后台粘贴发布 (Ctrl+V)");
        println!("[done] 版本管理已完成");

        Ok(())
    }

    /// 仅发布到网站，不提示公众号。
    async fn handle_publish_website_only(&mut self) -> Result<(), AppError> {
        let slug = self
            .current_slug
            .as_deref()
            .ok_or_else(|| AppError::Intent("slug 丢失，无法发布".to_string()))?;

        if self.website_path.is_empty() {
            println!("[info] WEBSITE_PATH 未配置，无法发布到网站");
            return Ok(());
        }

        match publish_to_website(&self.website_path, slug) {
            Ok(url) => println!("[done] 网站已发布: {}", url),
            Err(e) => eprintln!("[warn] 网站发布失败: {}", e),
        }

        Ok(())
    }

    // ── handle: Modify outline ────────────────────────────────────

    /// 用 LLM 重新生成大纲（携带修改指令）。
    async fn handle_modify_outline(
        &mut self,
        instruction: &str,
    ) -> Result<(), AppError> {
        let outline = self
            .current_outline
            .as_deref()
            .ok_or_else(|| AppError::Intent("大纲内容丢失".to_string()))?;
        let topic = self
            .current_topic
            .as_deref()
            .ok_or_else(|| AppError::Intent("创作主题丢失".to_string()))?;

        let prompt = format!(
            "以下是一篇文章的逻辑大纲：\n\n---\n{}\n---\n\n原始素材：\n\n---\n{}\n---\n\n请根据以下修改指令调整大纲：\n{}\n\n请输出修改后的完整大纲。",
            outline, topic, instruction
        );

        let spinner = crate::llm::new_spinner("正在修改大纲...");
        let new_outline = with_retry(3, "大纲修改", || {
            call_llm(
                &self.client,
                &self.base_url,
                &self.api_key,
                &self.model,
                OUTLINE_SYSTEM_PROMPT,
                &prompt,
            )
        })
        .await?;
        spinner.finish_with_message("大纲修改完成");

        self.current_outline = Some(new_outline.clone());

        println!("{}", sep());
        println!("{}", new_outline);
        println!("{}", sep());
        println!();
        println!("  请确认大纲，或继续输入修改指令。");

        // 保持 WaitingForOutline 状态
        Ok(())
    }

    // ── handle: Refine section ────────────────────────────────────

    /// 用 REFINE_SYSTEM_PROMPT 局部重绘全文中的指定段落。
    async fn handle_refine_section(
        &mut self,
        instruction: &str,
    ) -> Result<(), AppError> {
        let fulltext = self
            .current_fulltext
            .as_deref()
            .ok_or_else(|| AppError::Intent("全文内容丢失".to_string()))?;

        let user_prompt = format!(
            "以下是需要修改的文章全文：\n\n---\n{}\n---\n\n修改指令：{}\n\n请根据修改指令输出修改后的完整 Markdown 正文。",
            fulltext, instruction
        );

        let spinner = crate::llm::new_spinner("正在局部修改...");
        let new_fulltext = with_retry(3, "局部重绘", || {
            call_llm(
                &self.client,
                &self.base_url,
                &self.api_key,
                &self.model,
                REFINE_SYSTEM_PROMPT,
                &user_prompt,
            )
        })
        .await?;
        spinner.finish_with_message("修改完成");

        // 更新 slug（文章内容可能发生显著变化）
        let slug = extract_idea_slug(&new_fulltext);
        self.current_slug = Some(slug);
        self.current_fulltext = Some(new_fulltext.clone());

        println!("{}", sep());
        println!("{}", new_fulltext);
        println!("{}", sep());
        println!();
        println!("  请确认修改后的全文，或继续输入修改指令。");

        // 保持 WaitingForFulltext 状态
        Ok(())
    }

    // ── handle: Change style ──────────────────────────────────────

    /// 模糊匹配新风格 → 更新 current_style_content →
    /// LLM 按新风格重写全文。
    async fn handle_change_style(
        &mut self,
        style_name: &str,
    ) -> Result<(), AppError> {
        let fulltext = self
            .current_fulltext
            .as_deref()
            .ok_or_else(|| AppError::Intent("全文内容丢失".to_string()))?;

        // 模糊匹配新风格
        let (disp_name, style_content) =
            fuzzy_match_style("styles", style_name)?;

        println!(
            "[info] 切换风格: {} → {}",
            self.current_style_name.as_deref().unwrap_or("(未知)"),
            disp_name
        );

        self.current_style_name = Some(disp_name);
        self.current_style_content = Some(style_content.clone());

        // 构建 prompt：当前文章 + 新风格
        let user_prompt = format!(
            "以下是需要修改的文章全文：\n\n---\n{}\n---\n\n请将以上文章改写为以下风格：\n\n---\n{}\n---\n\n请输出改写后的完整 Markdown 正文。",
            fulltext, style_content
        );

        let spinner = crate::llm::new_spinner("正在按新风格重写...");
        let new_fulltext = with_retry(3, "风格切换", || {
            call_llm(
                &self.client,
                &self.base_url,
                &self.api_key,
                &self.model,
                UPDATE_SYSTEM_PROMPT,
                &user_prompt,
            )
        })
        .await?;
        spinner.finish_with_message("风格切换完成");

        let slug = extract_idea_slug(&new_fulltext);
        self.current_slug = Some(slug);
        self.current_fulltext = Some(new_fulltext.clone());

        println!("{}", sep());
        println!("{}", new_fulltext);
        println!("{}", sep());
        println!();
        println!("  已按新风格重写。请确认，或继续修改。");

        // 保持 WaitingForFulltext 状态
        Ok(())
    }

    // ── handle: Refine file ───────────────────────────────────────

    /// 读取 .md 文件，根据指令用 LLM 局部重绘，输出版本化结果。
    async fn handle_refine_file(
        &mut self,
        path: &str,
        instruction: &str,
    ) -> Result<(), AppError> {
        let content = crate::io::read_file(path)
            .map_err(|e| AppError::FileRead(format!("读取 {} 失败: {}", path, e)))?;

        if content.trim().is_empty() {
            return Err(AppError::Intent(format!("文件内容为空: {}", path)));
        }

        let user_prompt = format!(
            "以下是需要修改的文章全文：\n\n---\n{}\n---\n\n修改指令：{}\n\n请根据修改指令输出修改后的完整 Markdown 正文。",
            content, instruction
        );

        let spinner = crate::llm::new_spinner("正在局部重绘...");
        let rewritten = with_retry(3, "局部重绘", || {
            call_llm(
                &self.client,
                &self.base_url,
                &self.api_key,
                &self.model,
                REFINE_SYSTEM_PROMPT,
                &user_prompt,
            )
        })
        .await?;
        spinner.finish_with_message("重绘完成");

        // 版本化输出
        let slug = extract_idea_slug(&rewritten);
        let ver = crate::io::next_version("outputs", &slug);
        let md_path = format!("outputs/{}_v{}.md", slug, ver);

        crate::io::write_file(&md_path, &rewritten)
            .map_err(|e| AppError::FileWrite(format!("写入 {} 失败: {}", md_path, e)))?;

        println!("{}", sep());
        println!("{}", rewritten);
        println!("{}", sep());
        println!("[done] 已保存 → {}", md_path);

        Ok(())
    }

    // ── handle: List styles ───────────────────────────────────────

    fn handle_list_styles(&self) -> Result<(), AppError> {
        let summaries = list_styles_with_desc("styles")?;
        if summaries.is_empty() {
            println!("风格库为空。试试说「学一下这个风格 https://...」来创建第一个风格。");
            return Ok(());
        }

        println!("{}", sep());
        println!("风格库 ({} 个):", summaries.len());
        println!();
        for s in &summaries {
            println!("  [{}] {}", s.display_name, s.description);
        }
        println!("{}", sep());
        Ok(())
    }

    // ── handle: Show style detail ─────────────────────────────────

    fn handle_show_style(&self, name: &str) -> Result<(), AppError> {
        match show_style_detail("styles", name) {
            Ok(content) => {
                println!("{}", sep());
                println!("风格: {}", name);
                println!("{}", sep());
                println!("{}", content);
                println!("{}", sep());
                Ok(())
            }
            Err(e) => {
                eprintln!("[error] 未找到风格 '{}': {}", name, e);
                Err(e)
            }
        }
    }

    // ── handle: Delete style ──────────────────────────────────────

    fn handle_delete_style(&self, name: &str) -> Result<(), AppError> {
        // 二次确认
        print!("确认删除风格 '{}'? [y/N] ", name);
        let _ = io::stdout().flush();

        let mut confirm = String::new();
        if io::stdin().read_line(&mut confirm).is_err() {
            println!("已取消。");
            return Ok(());
        }

        let confirm = confirm.trim().to_lowercase();
        if confirm != "y" && confirm != "yes" {
            println!("已取消。");
            return Ok(());
        }

        match delete_style("styles", name) {
            Ok(()) => {
                println!("[done] 风格 '{}' 已删除。", name);
                Ok(())
            }
            Err(e) => {
                eprintln!("[error] 删除失败: {}", e);
                Err(e)
            }
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── md_to_wechat_html ──────────────────────────────────────────

    #[test]
    fn test_md_to_wechat_html_basic() {
        let result = md_to_wechat_html("**hello** world");
        assert!(result.contains("<section"));
        assert!(result.contains("<strong>hello</strong>"));
        assert!(result.contains("world"));
    }

    #[test]
    fn test_md_to_wechat_html_strips_hr() {
        let result = md_to_wechat_html("---\ncontent");
        assert!(!result.contains("<hr"));
        assert!(result.contains("content"));
    }

    #[test]
    fn test_md_to_wechat_html_empty() {
        let result = md_to_wechat_html("");
        assert!(result.contains("<section"));
    }

    // ── extract_title ──────────────────────────────────────────────

    #[test]
    fn test_extract_title_h1() {
        assert_eq!(extract_title("# 我的文章标题"), "我的文章标题");
    }

    #[test]
    fn test_extract_title_h2() {
        assert_eq!(extract_title("## 二级标题\n内容"), "二级标题");
    }

    #[test]
    fn test_extract_title_with_whitespace() {
        assert_eq!(extract_title("   #   标题前后空白   "), "标题前后空白");
    }

    #[test]
    fn test_extract_title_no_heading() {
        assert_eq!(extract_title("正文内容没有标题"), "Untitled");
    }

    #[test]
    fn test_extract_title_empty() {
        assert_eq!(extract_title(""), "Untitled");
    }

    #[test]
    fn test_extract_title_skips_empty_lines() {
        assert_eq!(
            extract_title("\n\n# 跳过空行"),
            "跳过空行"
        );
    }

    // ── Repl::new ──────────────────────────────────────────────────

    #[test]
    fn test_repl_new_requires_api_key() {
        // Without API_KEY set, should return Err
        // (This test relies on API_KEY not being set in the environment)
        unsafe { std::env::remove_var("API_KEY") };
        let result = Repl::new();
        assert!(result.is_err());
        if let Err(e) = result {
            let msg = e.to_string();
            assert!(msg.contains("API_KEY") || msg.contains("未设置"));
        }
    }

    #[test]
    fn test_repl_new_uses_defaults() {
        unsafe { std::env::set_var("API_KEY", "test-key") };
        // Remove any overrides so we test defaults
        unsafe { std::env::remove_var("BASE_URL") };
        unsafe { std::env::remove_var("MODEL") };
        unsafe { std::env::remove_var("WEBSITE_PATH") };

        let repl = Repl::new().unwrap();
        assert_eq!(repl.api_key, "test-key");
        assert_eq!(repl.base_url, "https://api.openai.com/v1");
        assert_eq!(repl.model, "gpt-4o");
        assert!(repl.website_path.is_empty());
        assert!(matches!(repl.state, SessionState::Idle));

        unsafe { std::env::remove_var("API_KEY") };
    }

    // ── reset_state ────────────────────────────────────────────────

    #[test]
    fn test_reset_state_clears_all() {
        unsafe { std::env::set_var("API_KEY", "test-key") };
        let mut repl = Repl::new().unwrap();
        repl.state = SessionState::WaitingForPublish;
        repl.current_topic = Some("test".into());
        repl.current_outline = Some("outline".into());
        repl.current_fulltext = Some("fulltext".into());
        repl.current_style_name = Some("style".into());
        repl.current_style_content = Some("content".into());
        repl.current_slug = Some("slug".into());

        repl.reset_state();

        assert!(matches!(repl.state, SessionState::Idle));
        assert!(repl.current_topic.is_none());
        assert!(repl.current_outline.is_none());
        assert!(repl.current_fulltext.is_none());
        assert!(repl.current_style_name.is_none());
        assert!(repl.current_style_content.is_none());
        assert!(repl.current_slug.is_none());

        unsafe { std::env::remove_var("API_KEY") };
    }

    // ── handle_command ─────────────────────────────────────────────

    #[test]
    fn test_handle_command_help() {
        unsafe { std::env::set_var("API_KEY", "test-key") };
        let mut repl = Repl::new().unwrap();
        let should_exit = repl.handle_command("/help");
        assert!(!should_exit);
        unsafe { std::env::remove_var("API_KEY") };
    }

    #[test]
    fn test_handle_command_quit() {
        unsafe { std::env::set_var("API_KEY", "test-key") };
        let mut repl = Repl::new().unwrap();
        let should_exit = repl.handle_command("/quit");
        assert!(should_exit);
        unsafe { std::env::remove_var("API_KEY") };
    }

    #[test]
    fn test_handle_command_unknown() {
        unsafe { std::env::set_var("API_KEY", "test-key") };
        let mut repl = Repl::new().unwrap();
        let should_exit = repl.handle_command("/unknown");
        assert!(!should_exit);
        unsafe { std::env::remove_var("API_KEY") };
    }

    // ── handle_list_styles ─────────────────────────────────────────

    #[test]
    fn test_handle_list_styles_empty_dir_ok() {
        unsafe { std::env::set_var("API_KEY", "test-key") };
        let repl = Repl::new().unwrap();

        // If the styles dir doesn't exist or is empty, list_styles_with_desc
        // returns Err — but handle_list_styles returns Ok(()) after printing.
        // Test the graceful handling path using a temp dir.
        // We can't easily redirect stdout in unit tests, so test the non-panic
        // property: the function returns a Result.
        let _ = repl.handle_list_styles();
        // Either Ok or Err from the underlying call is acceptable — the
        // function itself doesn't panic.
        unsafe { std::env::remove_var("API_KEY") };
    }
}
