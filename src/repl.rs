//! REPL 循环 + 状态机 + 意图分发 — cognitive-writer v3.0 核心交互模块。
//!
//! 用户通过终端 REPL 用自然语言交互，Agent 解析意图、管理对话状态、
//! 分发到对应的功能模块。

use crate::repl_input::{self, Input};

use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::clipboard::inject_clipboard;
use crate::error::AppError;
use crate::generate::{generate_outline, render_fulltext};
use crate::intent::{Intent, SessionState, classify_intent, is_cancel, is_confirm, parse_intent};
use crate::io::extract_idea_slug;
use crate::llm::{call_llm, with_retry};
use crate::refine::REFINE_SYSTEM_PROMPT;
use crate::style_selection::{self, named_matches, read_options, selection_query};
use crate::styles::{
    delete_style, fuzzy_match_style, list_styles_with_desc, save_new_style, show_style_detail,
};
use crate::utils::md_to_wechat_html;
use crate::website::{publish_to_website, write_mdx_draft};

// ── Local constants ─────────────────────────────────────────────────

/// System prompt for update / change-style — rewrites the whole article
/// according to an instruction while preserving structure.
const UPDATE_SYSTEM_PROMPT: &str = r#"你是严苛的文本编辑。用户会给你一篇完整的文章和一条修改指令，你需要根据修改指令重写整篇文章。

要求：
1. 保持原文的整体结构和段落数量
2. 只修改与指令相关的部分，其余内容保持不变
3. 直接输出完整的修改后 Markdown，不要任何解释、说明或代码块包裹
4. 输出必须是可发布的最终版本"#;

/// 状态的中文标签，用于「状态不对」时准确报错。
fn state_label(state: &SessionState) -> &'static str {
    match state {
        SessionState::Idle => "空闲",
        SessionState::WaitingForGenerationStyle => "等待选择风格",
        SessionState::WaitingForStyleName => "等待风格命名",
        SessionState::WaitingForDeleteStyleConfirmation => "等待删除确认",
        SessionState::WaitingForOutline => "等待确认大纲",
        SessionState::WaitingForFulltext => "等待确认全文",
        SessionState::WaitingForPublish => "等待确认发布",
    }
}

/// 意图的中文标签，用于「状态不对」时准确报错。
fn intent_label(intent: &Intent) -> &'static str {
    match intent {
        Intent::Generate { .. } => "写文章",
        Intent::Learn { .. } => "学风格",
        Intent::ListStyles => "列出风格",
        Intent::ShowStyle { .. } => "查看风格",
        Intent::DeleteStyle { .. } => "删除风格",
        Intent::RefineFile { .. } => "局部修改文件",
        Intent::UpdateFile { .. } => "重写文件",
        Intent::Confirm => "确认",
        Intent::Cancel => "取消",
        Intent::ModifySection { .. } => "修改段落",
        Intent::ChangeStyle { .. } => "换风格",
        Intent::Publish => "发布",
        Intent::PublishWebsiteOnly => "只发网站",
        Intent::Hold => "暂缓",
        Intent::Unknown { .. } => "无法识别",
    }
}

// ── Help text ─────────────────────────────────────────────────────────

const HELP_TEXT: &str = concat!(
    "\nCognitive Writer v",
    env!("CARGO_PKG_VERSION"),
    r#" — 对话式写作 Agent
─────────────────────────────────────────

核心工作流:
  写一篇关于<主题>的文章用<风格名>风格    → 生成大纲 → 自动渲染全文 → 发布

状态内指令:
  [选择风格]  未明确风格时先追问；回复名称或编号继续，/styles 查看候选
  [全文确认]  确认/好/没问题 → 注入剪贴板 + 存草稿  |  修改指令 → 局部重绘  |  整体换成<风格名> → 换风格
  [发布确认]  发布/推 → 正式发布  |  只发网站 → 仅网站  |  等一下/先不/看看 → 保留草稿

其他命令:
  /help        显示此帮助
  /quit        保存未完成进度并退出
  /cancel      取消当前操作
  /styles      列出风格库（等同于「我的风格库有什么」）
  /state       显示当前状态（调试用）

独立操作 (Idle 状态可用):
  输入 URL     → 学习该文章的写作风格
  列出风格     → 列出所有可用风格文件
  看看<名称>   → 查看风格详情
  删掉<名称>   → 删除指定风格
  重写 <路径> 改成 <指令>  → 根据指令重写文章
  <路径> 改 <指令>          → 局部重绘文件
"#
);

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
    #[serde(default)]
    generation_request: Option<String>,
    #[serde(default)]
    pending_style_choices: Vec<String>,
    current_outline: Option<String>,
    current_fulltext: Option<String>,
    current_style_name: Option<String>,
    current_style_content: Option<String>,
    current_slug: Option<String>,
    #[serde(default)]
    pending_style_content: Option<String>,
    #[serde(default)]
    pending_delete_name: Option<String>,
    #[serde(skip, default = "default_styles_dir")]
    styles_dir: String,
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

fn default_styles_dir() -> String {
    "styles".into()
}

/// 恢复提示与 REPL 共用同一个 stdin 读取器；Ctrl+C/EOF 保留旧会话。
pub async fn run_session(session_dir: &std::path::Path) -> Result<(), AppError> {
    let saved = Repl::restore(session_dir)?;
    let mut input = repl_input::stdin_input();
    let mut repl = if let Some(saved) = saved {
        println!(
            "检测到未完成会话（{}）。Enter 恢复，n 放弃。",
            state_label(saved.current_state())
        );
        loop {
            let answer = tokio::select! {
                line = input.recv() => line,
                _ = tokio::signal::ctrl_c() => return Ok(()),
            };
            match answer {
                None => return Ok(()),
                Some(Err(e)) => return Err(e),
                Some(Ok(answer)) => match answer.trim().to_lowercase().as_str() {
                    "" | "y" | "yes" => break saved,
                    "n" | "no" => {
                        let fresh = Repl::new_with_session_dir(session_dir)?;
                        saved.clear_session()?;
                        break fresh;
                    }
                    _ => println!("Enter 恢复，n 放弃；其他输入不会清除进度。"),
                },
            }
        }
    } else {
        Repl::new_with_session_dir(session_dir)?
    };
    repl.run_with_input(input).await;
    Ok(())
}

impl Repl {
    // ── Constructors ─────────────────────────────────────────────

    /// 新建不持久化的实例；需要恢复进度时使用 new_with_session_dir。
    pub fn new() -> Result<Self, AppError> {
        let mut repl = Self::new_with_session_dir(std::path::Path::new("."))?;
        repl.session_dir = None;
        Ok(repl)
    }

    /// 新建实例，关联 session 目录。
    pub fn new_with_session_dir(session_dir: &std::path::Path) -> Result<Self, AppError> {
        let api_key =
            std::env::var("API_KEY").map_err(|_| AppError::EnvVar("API_KEY 未设置".to_string()))?;
        let base_url =
            std::env::var("BASE_URL").unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
        let model = std::env::var("MODEL").unwrap_or_else(|_| "gpt-4o".to_string());
        let website_path = std::env::var("WEBSITE_PATH").unwrap_or_default();
        let client = Client::new();

        Ok(Self {
            state: SessionState::Idle,
            current_topic: None,
            generation_request: None,
            pending_style_choices: Vec::new(),
            current_outline: None,
            current_fulltext: None,
            current_style_name: None,
            current_style_content: None,
            current_slug: None,
            pending_style_content: None,
            pending_delete_name: None,
            styles_dir: default_styles_dir(),
            api_key,
            base_url,
            model,
            website_path,
            client,
            session_dir: Some(session_dir.to_path_buf()),
        })
    }

    /// 无会话时返回 None；恢复失败返回错误，调用方不得以新会话覆盖原文件。
    pub fn restore(session_dir: &std::path::Path) -> Result<Option<Self>, AppError> {
        let path = session_dir.join("current.json");
        let json = match std::fs::read_to_string(&path) {
            Ok(json) => json,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => {
                return Err(AppError::FileRead(format!(
                    "无法恢复会话 {}: {e}；原文件未改动，请检查文件或权限后重试",
                    path.display()
                )));
            }
        };
        let mut repl: Repl = serde_json::from_str(&json).map_err(|e| {
            AppError::FileRead(format!(
                "无法解析会话 {}: {e}；原文件未改动，请修复文件，或将其移走备份后重新启动",
                path.display()
            ))
        })?;
        // 重新初始化 #[serde(skip)] 字段；配置错误同样不能丢弃旧会话。
        repl.api_key = std::env::var("API_KEY").map_err(|_| {
            AppError::EnvVar(format!("API_KEY（会话 {} 保持原样）", path.display()))
        })?;
        repl.base_url =
            std::env::var("BASE_URL").unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
        repl.model = std::env::var("MODEL").unwrap_or_else(|_| "gpt-4o".to_string());
        repl.website_path = std::env::var("WEBSITE_PATH").unwrap_or_default();
        repl.client = Client::new();
        repl.session_dir = Some(session_dir.to_path_buf());
        Ok(Some(repl))
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
        std::fs::create_dir_all(&dir).map_err(|e| {
            AppError::FileWrite(format!("session 目录创建失败 {}: {}", dir.display(), e))
        })?;
        let path = dir.join("current.json");
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| AppError::Parse(format!("序列化失败: {}", e)))?;
        crate::io::write_file_atomic(&path, &json)?;
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
            std::fs::remove_file(&path).map_err(|e| {
                AppError::FileWrite(format!("清除 session 失败 {}: {}", path.display(), e))
            })?;
        }
        Ok(())
    }

    // ── Main loop ─────────────────────────────────────────────────

    pub async fn run(&mut self) {
        self.run_with_input(repl_input::stdin_input()).await;
    }

    async fn run_with_input(&mut self, mut input: Input) {
        println!(
            "Cognitive Writer v{} — 对话式写作 Agent (会话自动保存)",
            env!("CARGO_PKG_VERSION")
        );
        println!("输入内容后按 Enter 发送。行末加 \\ 可以换行继续输入。");
        println!("输入 /help 查看帮助，/quit 保存并退出。");
        if self.state == SessionState::WaitingForGenerationStyle {
            if let Err(e) = self.show_generation_choices("", false) {
                eprintln!("[error] {e}");
            }
        }
        loop {
            repl_input::prompt(&format!("[{}] > ", state_label(&self.state)));
            let received = tokio::select! {
                line = input.recv() => line,
                _ = tokio::signal::ctrl_c() => { if self.save_on_exit() { return; } else { continue; } }
            };
            let line = match received {
                Some(Ok(line)) => line,
                Some(Err(e)) => {
                    eprintln!("[error] stdin 读取失败: {e}");
                    self.save_on_exit();
                    return;
                }
                None => {
                    self.save_on_exit();
                    return;
                }
            };
            // 信号同时包住分类与整个 dispatch，模型等待期间也能中断。
            let result = tokio::select! {
                result = self.process_input(&line) => Some(result),
                _ = tokio::signal::ctrl_c() => None,
            };
            match result {
                None => {
                    if self.save_on_exit() {
                        return;
                    } else {
                        continue;
                    }
                }
                Some(Ok(true)) => return,
                Some(Err(e)) => eprintln!("[error] {e}"),
                Some(Ok(false)) => {}
            }
            if let Err(e) = self.persist_progress() {
                eprintln!("[warn] 会话保存失败: {e}");
            }
        }
    }

    fn persist_progress(&self) -> Result<(), AppError> {
        if self.state == SessionState::Idle {
            self.clear_session()
        } else {
            self.save()
        }
    }

    fn save_on_exit(&self) -> bool {
        match self.persist_progress() {
            Ok(()) => {
                println!("进度已保存。再见。");
                true
            }
            Err(e) => {
                eprintln!("[error] 会话保存失败: {e}；请保存内容或修复目录后重试退出。");
                false
            }
        }
    }

    async fn process_input(&mut self, input: &str) -> Result<bool, AppError> {
        let input = input.trim();
        if self.state == SessionState::WaitingForGenerationStyle
            && (!input.starts_with('/') || input == "/styles")
        {
            self.handle_generation_style_input(input).await?;
            return Ok(false);
        }
        if input.starts_with('/') {
            return Ok(self.handle_command(input));
        }
        // 命名不做意图识别，避免把风格名当作写作指令或多付一次分类调用。
        if self.state == SessionState::WaitingForStyleName {
            if is_cancel(input) {
                self.reset_state();
                return Ok(false);
            }
            let content = self
                .pending_style_content
                .as_deref()
                .ok_or_else(|| AppError::Parse("待保存风格缺失；输入 /cancel 取消".into()))?;
            let path = save_new_style(&self.styles_dir, input, content)?;
            println!("[done] 风格已保存 → {}", path.display());
            self.reset_state();
            return Ok(false);
        }
        if self.state == SessionState::WaitingForDeleteStyleConfirmation {
            if is_cancel(input) || input.is_empty() {
                self.reset_state();
                return Ok(false);
            }
            if !matches!(input.to_lowercase().as_str(), "y" | "yes" | "确认删除") {
                println!("输入 y / yes / 确认删除 才会删除，输入 /cancel 取消。");
                return Ok(false);
            }
            let name = self
                .pending_delete_name
                .as_deref()
                .ok_or_else(|| AppError::Parse("待删除名称缺失；输入 /cancel 取消".into()))?;
            delete_style(&self.styles_dir, name)?;
            println!("[done] 风格 '{name}' 已删除。");
            self.reset_state();
            return Ok(false);
        }
        if input.is_empty() {
            return Ok(false);
        }
        let intent = if is_confirm(input) || is_cancel(input) {
            parse_intent(input, &self.state)
        } else {
            classify_intent(
                &self.client,
                &self.base_url,
                &self.api_key,
                &self.model,
                input,
                &self.state,
            )
            .await?
        };
        self.dispatch(intent, input).await?;
        Ok(false)
    }

    // ── Special-command handler ────────────────────────────────────

    /// Process a `/`-prefixed command. Returns `true` if the REPL should
    /// exit (i.e. `/quit`).
    fn handle_command(&mut self, input: &str) -> bool {
        match input.trim() {
            "/cancel" => {
                self.reset_state();
                println!("已取消。");
                false
            }
            "/help" => {
                println!("{}", HELP_TEXT);
                false
            }
            "/quit" => self.save_on_exit(),
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
        self.generation_request = None;
        self.pending_style_choices.clear();
        self.current_outline = None;
        self.current_fulltext = None;
        self.current_style_name = None;
        self.current_style_content = None;
        self.current_slug = None;
        self.pending_style_content = None;
        self.pending_delete_name = None;
        // 全流程完成，清除 session 文件（下次启动不提示恢复）
        let _ = self.clear_session();
    }

    // ── Debug ─────────────────────────────────────────────────────

    fn print_state(&self) {
        println!("{}", sep());
        println!("状态: {:?}", self.state);
        println!("话题: {}", self.current_topic.as_deref().unwrap_or("(无)"));
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
        println!("slug: {}", self.current_slug.as_deref().unwrap_or("(无)"));
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

    /// 当前状态下的可用操作提示，用于「没理解」或「状态不对」时给用户指路。
    fn state_hint(&self) -> &'static str {
        match self.state {
            SessionState::WaitingForGenerationStyle => {
                "回复风格名称或编号；/styles 查看候选；/cancel 取消；/quit 保存需求退出。"
            }
            SessionState::WaitingForStyleName => {
                "输入新风格名称保存；/cancel 取消；/quit 保存提取结果退出。"
            }
            SessionState::WaitingForDeleteStyleConfirmation => {
                "输入 y / yes / 确认删除；/cancel 取消。"
            }
            SessionState::Idle => {
                "可以：「写一篇关于XX的文章，用YY风格」「列出风格」「学一下这个风格 <链接>」，或 /help 看全部用法。"
            }
            SessionState::WaitingForOutline => {
                "输入「确认」或「OK」从已有大纲继续正文；内容缺失时可输入「算了」取消重试。"
            }
            SessionState::WaitingForFulltext => {
                "可以：「OK」确认全文、「第X段加案例」修改、「换成XX风格」换风格、「算了」取消。"
            }
            SessionState::WaitingForPublish => {
                "可以：「发布」「只发网站」「等一下」暂缓、「算了」取消。"
            }
        }
    }

    // ── Intent dispatch ───────────────────────────────────────────

    async fn dispatch(&mut self, intent: Intent, input: &str) -> Result<(), AppError> {
        match (&self.state, intent) {
            // ── Idle 状态 ──
            (SessionState::Idle, Intent::Generate { topic, style_name }) => {
                self.begin_generation(&topic, &style_name, input).await?;
            }
            (SessionState::Idle, Intent::Learn { url }) => {
                println!("[info] 开始学习风格: {}", url);
                self.pending_style_content = Some(crate::learn::extract_style(&url).await?);
                self.state = SessionState::WaitingForStyleName;
                println!("风格已提取，请输入名称保存，或 /cancel 取消。");
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
                if let Err(e) = crate::update::run_update(&path, Some(instruction), false).await {
                    eprintln!("[error] 重写文件失败: {}", e);
                }
            }
            (SessionState::Idle, Intent::RefineFile { path, instruction }) => {
                self.handle_refine_file(&path, &instruction).await?;
            }

            // ── 任意状态 ──
            (_, Intent::Cancel) => {
                println!("已取消当前操作，回到空闲状态。你可以重新选题或说其他指令。");
                self.reset_state();
            }
            (_, Intent::Unknown { reason }) => {
                println!("[!] 没处理这条输入，原因：{reason}");
                println!("    你刚才说的是：{input}");
                println!("    {}", self.state_hint());
            }

            (SessionState::WaitingForOutline, Intent::Confirm) => {
                self.handle_render_fulltext().await?;
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
                println!("草稿已保留在本地 outputs 目录，可打开 HTML 文件复制到公众号后台。");
                self.reset_state();
            }

            // ── 状态不匹配 ──
            (state, intent) => {
                println!(
                    "[!] 当前状态「{}」做不了「{}」。",
                    state_label(state),
                    intent_label(&intent)
                );
                println!("    {}", self.state_hint());
            }
        }
        Ok(())
    }

    // ── First-generation style clarification ──────────────────────

    async fn begin_generation(
        &mut self,
        topic: &str,
        style: &str,
        original: &str,
    ) -> Result<(), AppError> {
        self.current_topic = Some(topic.to_string());
        self.generation_request = Some(original.to_string());
        self.pending_style_choices.clear();
        self.state = SessionState::WaitingForGenerationStyle;
        let options = read_options(&self.styles_dir)?;
        let matches = named_matches(&options, style);
        if matches.len() == 1 {
            return self.handle_generate(topic, &matches[0]).await;
        }
        if style.trim().is_empty() {
            println!("这篇文章想用哪种风格？请从下列风格中选择。");
        } else {
            println!("无法唯一确定「{style}」对应的风格，请选择具体名称或编号。");
        }
        self.show_generation_choices(style, true)
    }

    fn show_generation_choices(&mut self, query: &str, refresh: bool) -> Result<(), AppError> {
        let options = read_options(&self.styles_dir)?;
        if refresh || self.pending_style_choices.is_empty() {
            let suggested = style_selection::suggestions(&options, query);
            self.pending_style_choices = if suggested.is_empty() {
                options.iter().map(|o| o.name.clone()).collect()
            } else {
                suggested
            };
        }
        println!(
            "已保留写作需求：{}",
            self.generation_request
                .as_deref()
                .or(self.current_topic.as_deref())
                .unwrap_or("(无)")
        );
        if options.is_empty() {
            println!(
                "风格库为空。请先 /cancel，再输入文章链接学习风格；也可导入风格文件后输入 /styles 刷新。需求可用 /quit 保存。"
            );
            return Ok(());
        }
        for (index, name) in self.pending_style_choices.iter().enumerate() {
            let description = options
                .iter()
                .find(|o| &o.name == name)
                .map(|o| o.description.as_str())
                .unwrap_or("文件已移除，请 /styles 刷新");
            println!("  {}. {} — {}", index + 1, name, description);
        }
        println!("回复名称、编号或「用某某风格」；/styles 查看全部，/cancel 取消。");
        Ok(())
    }

    async fn handle_generation_style_input(&mut self, input: &str) -> Result<(), AppError> {
        if is_cancel(input) {
            self.reset_state();
            println!("已取消当前写作需求。");
            return Ok(());
        }
        if style_selection::is_list_request(input) {
            return self.show_generation_choices("", true);
        }
        if input.is_empty() || is_confirm(input) {
            println!("还需要选择具体风格，请回复名称或编号。");
            return self.show_generation_choices("", false);
        }
        let options = read_options(&self.styles_dir)?;
        let query = selection_query(input);
        let mut matches = named_matches(&options, input);
        if matches.is_empty() {
            matches = named_matches(&options, query);
        }
        let selected = if matches.len() == 1 {
            Some(matches.remove(0))
        } else if let Ok(number) = query.parse::<usize>() {
            number
                .checked_sub(1)
                .and_then(|i| self.pending_style_choices.get(i))
                .cloned()
        } else {
            None
        };
        if let Some(name) = selected {
            let topic = self.current_topic.clone().ok_or_else(|| {
                AppError::Intent("待生成主题丢失，请 /cancel 后重新输入需求".into())
            })?;
            return self.handle_generate(&topic, &name).await;
        }
        println!("还不能确定使用哪种风格，请选择具体名称或有效编号。");
        self.show_generation_choices(query, true)
    }

    fn generation_brief(&self) -> Result<String, AppError> {
        let topic = self
            .current_topic
            .as_deref()
            .ok_or_else(|| AppError::Intent("创作主题丢失".into()))?;
        Ok(match self.generation_request.as_deref() {
            Some(request) => format!(
                "主题：{topic}\n原始写作要求：{request}\n风格以当前选定的风格规范为准；保留原请求的篇幅、受众等其他要求。"
            ),
            None => topic.to_string(),
        })
    }

    // ── handle: Generate ──────────────────────────────────────────

    /// 1. 按已选定的名称读取风格内容
    /// 2. 存储风格信息
    /// 3. 调用 generate_outline → 骨架
    /// 4. 自动调用 render_fulltext → 全文
    /// 5. 切换到 WaitingForFulltext 状态
    async fn handle_generate(&mut self, topic: &str, style_name: &str) -> Result<(), AppError> {
        // 上游已明确选择文件；再次按确切名称读取，避免重排或文件变化导致误选。
        let style_content = show_style_detail(&self.styles_dir, style_name)?;
        let disp_name = style_name.to_string();

        println!(
            "[info] 匹配风格: {} ({} 字符)",
            disp_name,
            style_content.len()
        );

        self.current_style_name = Some(disp_name);
        self.current_style_content = Some(style_content.clone());
        self.current_topic = Some(topic.to_string());
        self.pending_style_choices.clear();
        let brief = self.generation_brief()?;

        self.current_outline = None;
        self.state = SessionState::WaitingForOutline;
        // 生成风格化大纲（风格也注入 Pass 1，让结构贴合风格）
        let spinner = crate::llm::new_spinner("正在生成大纲骨架...");
        let outline = generate_outline(
            &self.client,
            &self.base_url,
            &self.api_key,
            &self.model,
            &style_content,
            &brief,
        )
        .await?;
        spinner.finish_with_message("大纲生成完成");

        self.current_outline = Some(outline);
        self.save()?;

        // 大纲已生成，自动进入全文渲染（检查点 1 已取消）
        self.handle_render_fulltext().await?;

        Ok(())
    }

    // ── handle: Render fulltext ───────────────────────────────────

    /// 从骨架 + 风格 + 素材 渲染全文。
    /// 由 dispatch（旧路径：WaitingForOutline→Confirm）或 handle_generate（新路径：自动）调用。
    async fn handle_render_fulltext(&mut self) -> Result<(), AppError> {
        let style = self
            .current_style_content
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| AppError::Intent("风格内容丢失".to_string()))?;
        let outline = self
            .current_outline
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| AppError::Intent("大纲内容丢失".to_string()))?;
        let brief = self.generation_brief()?;

        let spinner = crate::llm::new_spinner("正在渲染正文...");
        let fulltext = render_fulltext(
            &self.client,
            &self.base_url,
            &self.api_key,
            &self.model,
            style,
            outline,
            &brief,
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

    /// 必需草稿全部落盘后才进入发布态；剪贴板失败不影响已保存文件。
    async fn handle_draft_publish(&mut self) -> Result<(), AppError> {
        let html_fragment = self.prepare_draft("outputs").map_err(|e| {
            AppError::FileWrite(format!(
                "草稿保存未完成: {e}；正文仍保留，修复目录后输入「确认」重试，或 /quit 保存会话"
            ))
        })?;
        match inject_clipboard(&html_fragment) {
            Ok(tool) => println!("[done] 富文本已注入剪贴板 (via {})", tool),
            Err(e) => eprintln!("[warn] 剪贴板注入失败: {}；可打开本地 HTML 文件复制", e),
        }
        println!();
        println!(
            "  草稿已就绪。输入「发布」正式发布，「只发网站」仅发布到网站，「等一下」保留草稿。"
        );
        Ok(())
    }

    /// 路径参数让故障测试使用独立目录，不改进程工作目录或真实文章。
    fn prepare_draft(&mut self, outputs_dir: &str) -> Result<String, AppError> {
        let fulltext = self
            .current_fulltext
            .as_deref()
            .ok_or_else(|| AppError::Intent("全文内容丢失".to_string()))?;
        let slug = self
            .current_slug
            .as_deref()
            .ok_or_else(|| AppError::Intent("slug 丢失".to_string()))?;
        let html_fragment = md_to_wechat_html(fulltext);
        let ver = crate::io::next_version(outputs_dir, slug);
        let md_path = std::path::Path::new(outputs_dir).join(format!("{slug}_v{ver}.md"));
        let html_path = std::path::Path::new(outputs_dir).join(format!("{slug}_v{ver}.html"));
        let html_doc = format!(
            "<!DOCTYPE html>\n<html>\n<head><meta charset=\"utf-8\"></head>\n<body>\n{}\n</body>\n</html>",
            html_fragment
        );
        crate::io::write_file_atomic(&md_path, fulltext)?;
        println!("[done] Markdown → {}", md_path.display());
        crate::io::write_file_atomic(&html_path, &html_doc)?;
        println!("[done] HTML     → {}", html_path.display());

        if !self.website_path.is_empty() {
            let title = extract_title(fulltext);
            let path = write_mdx_draft(&self.website_path, "signal", slug, &title, fulltext)?;
            println!("[done] 网站草稿已写入: {}", path);
        } else {
            println!("[info] WEBSITE_PATH 未配置，跳过网站草稿");
        }
        self.state = SessionState::WaitingForPublish;
        Ok(html_fragment)
    }

    // ── handle: Publish ───────────────────────────────────────────

    /// 正式发布：网站 commit+push，提示公众号手动发布。
    async fn handle_publish(&mut self) -> Result<(), AppError> {
        let slug = self
            .current_slug
            .as_deref()
            .ok_or_else(|| AppError::Intent("slug 丢失，无法发布".to_string()))?;

        if !self.website_path.is_empty() {
            let url = publish_to_website(&self.website_path, slug)?;
            println!("[done] 网站已发布: {}", url);
        }

        println!("[info] 公众号请手动到微信后台粘贴发布 (Ctrl+V)");

        Ok(())
    }

    /// 仅发布到网站，不提示公众号。
    async fn handle_publish_website_only(&mut self) -> Result<(), AppError> {
        let slug = self
            .current_slug
            .as_deref()
            .ok_or_else(|| AppError::Intent("slug 丢失，无法发布".to_string()))?;

        if self.website_path.is_empty() {
            return Err(AppError::Website(
                "WEBSITE_PATH 未配置，无法发布到网站".to_string(),
            ));
        }

        let url = publish_to_website(&self.website_path, slug)?;
        println!("[done] 网站已发布: {}", url);

        Ok(())
    }

    // ── handle: Refine section ────────────────────────────────────

    /// 用 REFINE_SYSTEM_PROMPT 局部重绘全文中的指定段落。
    async fn handle_refine_section(&mut self, instruction: &str) -> Result<(), AppError> {
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
    async fn handle_change_style(&mut self, style_name: &str) -> Result<(), AppError> {
        let fulltext = self
            .current_fulltext
            .as_deref()
            .ok_or_else(|| AppError::Intent("全文内容丢失".to_string()))?;

        // 模糊匹配新风格
        let (disp_name, style_content) = fuzzy_match_style(&self.styles_dir, style_name)?;

        println!(
            "[info] 切换风格: {} → {}",
            self.current_style_name.as_deref().unwrap_or("(未知)"),
            disp_name
        );

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

        self.current_style_name = Some(disp_name);
        self.current_style_content = Some(style_content);
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
    async fn handle_refine_file(&mut self, path: &str, instruction: &str) -> Result<(), AppError> {
        let content = crate::io::read_file(path)?;

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

        crate::io::write_file(&md_path, &rewritten)?;

        println!("{}", sep());
        println!("{}", rewritten);
        println!("{}", sep());
        println!("[done] 已保存 → {}", md_path);

        Ok(())
    }

    // ── handle: List styles ───────────────────────────────────────

    fn handle_list_styles(&self) -> Result<(), AppError> {
        let summaries = list_styles_with_desc(&self.styles_dir)?;
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
        match show_style_detail(&self.styles_dir, name) {
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

    fn handle_delete_style(&mut self, name: &str) -> Result<(), AppError> {
        show_style_detail(&self.styles_dir, name)?;
        self.pending_delete_name = Some(name.trim().into());
        self.state = SessionState::WaitingForDeleteStyleConfirmation;
        println!("确认删除风格 '{name}'？输入 y / yes / 确认删除，/cancel 取消。");
        Ok(())
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// 改动进程级环境变量（API_KEY 等）的测试必须持有此锁，
    /// 避免并行测试互相 set_var/remove_var 造成竞态。
    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    // ── state_label / intent_label ────────────────────────────────

    #[test]
    fn test_state_label_covers_all_states() {
        assert_eq!(state_label(&SessionState::Idle), "空闲");
        assert_eq!(
            state_label(&SessionState::WaitingForFulltext),
            "等待确认全文"
        );
        assert_eq!(
            state_label(&SessionState::WaitingForPublish),
            "等待确认发布"
        );
        assert_eq!(
            state_label(&SessionState::WaitingForOutline),
            "等待确认大纲"
        );
    }

    #[test]
    fn test_intent_label_known_and_unknown() {
        assert_eq!(
            intent_label(&Intent::Generate {
                topic: "t".into(),
                style_name: "s".into()
            }),
            "写文章"
        );
        assert_eq!(intent_label(&Intent::Confirm), "确认");
        assert_eq!(
            intent_label(&Intent::Unknown { reason: "x".into() }),
            "无法识别"
        );
    }

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
        assert_eq!(extract_title("\n\n# 跳过空行"), "跳过空行");
    }

    // ── Repl::new ──────────────────────────────────────────────────

    #[test]
    fn test_repl_new_requires_api_key() {
        let _g = ENV_MUTEX.lock().unwrap();
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
        let _g = ENV_MUTEX.lock().unwrap();
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
        let _g = ENV_MUTEX.lock().unwrap();
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

    #[test]
    fn test_website_only_publish_without_config_preserves_session() {
        let _g = ENV_MUTEX.lock().unwrap();
        unsafe { std::env::set_var("API_KEY", "test-key") };
        unsafe { std::env::remove_var("WEBSITE_PATH") };

        let mut repl = Repl::new().unwrap();
        repl.state = SessionState::WaitingForPublish;
        repl.current_slug = Some("test-post".into());

        let runtime = tokio::runtime::Runtime::new().unwrap();
        let result = runtime.block_on(repl.dispatch(Intent::PublishWebsiteOnly, "只发网站"));
        assert!(result.is_err());
        assert_eq!(repl.state, SessionState::WaitingForPublish);
        assert_eq!(repl.current_slug.as_deref(), Some("test-post"));

        unsafe { std::env::remove_var("API_KEY") };
    }

    // ── handle_command ─────────────────────────────────────────────

    #[test]
    fn test_handle_command_help() {
        let _g = ENV_MUTEX.lock().unwrap();
        unsafe { std::env::set_var("API_KEY", "test-key") };
        let mut repl = Repl::new().unwrap();
        let should_exit = repl.handle_command("/help");
        assert!(!should_exit);
        unsafe { std::env::remove_var("API_KEY") };
    }

    #[test]
    fn test_handle_command_quit() {
        let _g = ENV_MUTEX.lock().unwrap();
        unsafe { std::env::set_var("API_KEY", "test-key") };
        let mut repl = Repl::new().unwrap();
        let should_exit = repl.handle_command("/quit");
        assert!(should_exit);
        unsafe { std::env::remove_var("API_KEY") };
    }

    #[test]
    fn test_handle_command_unknown() {
        let _g = ENV_MUTEX.lock().unwrap();
        unsafe { std::env::set_var("API_KEY", "test-key") };
        let mut repl = Repl::new().unwrap();
        let should_exit = repl.handle_command("/unknown");
        assert!(!should_exit);
        unsafe { std::env::remove_var("API_KEY") };
    }

    // ── handle_list_styles ─────────────────────────────────────────

    #[test]
    fn test_handle_list_styles_empty_dir_reports_no_styles() {
        let dir = tempfile::tempdir().unwrap();
        let repl = isolated_repl(dir.path());
        std::fs::create_dir_all(&repl.styles_dir).unwrap();
        assert!(matches!(
            repl.handle_list_styles(),
            Err(AppError::NoStyles(_))
        ));
    }
    fn isolated_repl(dir: &std::path::Path) -> Repl {
        let mut repl: Repl = serde_json::from_value(serde_json::json!({"state":"Idle"})).unwrap();
        repl.session_dir = Some(dir.join("session"));
        repl.styles_dir = dir.join("styles").to_str().unwrap().into();
        repl.base_url = "http://127.0.0.1:1".into();
        repl.api_key = "local-test".into();
        repl
    }

    #[tokio::test]
    async fn naming_is_local_and_rejects_collision_and_paths() {
        let dir = tempfile::tempdir().unwrap();
        let mut repl = isolated_repl(dir.path());
        save_new_style(&repl.styles_dir, "已有", "不能覆盖").unwrap();
        repl.state = SessionState::WaitingForStyleName;
        repl.pending_style_content = Some("新风格机制".into());
        for name in ["已有", "../逃出", "..", "", "x/y", "x\\y"] {
            assert!(repl.process_input(name).await.is_err(), "{name}");
            assert_eq!(repl.state, SessionState::WaitingForStyleName);
            assert_eq!(repl.pending_style_content.as_deref(), Some("新风格机制"));
        }
        assert_eq!(
            show_style_detail(&repl.styles_dir, "已有").unwrap(),
            "不能覆盖"
        );
        // 看起来像意图的名称仍按名称处理，不访问不可用的分类地址。
        repl.process_input("写一篇").await.unwrap();
        assert_eq!(
            show_style_detail(&repl.styles_dir, "写一篇").unwrap(),
            "新风格机制"
        );
        assert_eq!(repl.state, SessionState::Idle);
        assert!(repl.pending_style_content.is_none());
    }

    #[tokio::test]
    async fn deletion_requires_explicit_confirmation_and_can_cancel() {
        let dir = tempfile::tempdir().unwrap();
        let mut repl = isolated_repl(dir.path());
        save_new_style(&repl.styles_dir, "待删除", "保留直到确认").unwrap();
        repl.handle_delete_style("待删除").unwrap();
        repl.process_input("好").await.unwrap();
        assert_eq!(repl.state, SessionState::WaitingForDeleteStyleConfirmation);
        assert!(show_style_detail(&repl.styles_dir, "待删除").is_ok());
        repl.process_input("/cancel").await.unwrap();
        assert_eq!(repl.state, SessionState::Idle);
        assert!(show_style_detail(&repl.styles_dir, "待删除").is_ok());
        repl.handle_delete_style("待删除").unwrap();
        repl.process_input("yes").await.unwrap();
        assert_eq!(repl.state, SessionState::Idle);
        assert!(show_style_detail(&repl.styles_dir, "待删除").is_err());
    }

    #[tokio::test]
    async fn eof_preserves_both_pending_operations_without_side_effects() {
        let dir = tempfile::tempdir().unwrap();
        let mut repl = isolated_repl(dir.path());
        repl.state = SessionState::WaitingForStyleName;
        repl.pending_style_content = Some("已提取的结果".into());
        let (sender, receiver) = tokio::sync::mpsc::channel(1);
        drop(sender);
        repl.run_with_input(receiver).await;
        let path = repl.session_dir.as_ref().unwrap().join("current.json");
        let saved: Repl = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved.pending_style_content, repl.pending_style_content);
        assert_eq!(saved.state, SessionState::WaitingForStyleName);
        assert!(!std::path::Path::new(&repl.styles_dir).exists());
        repl.process_input("/cancel").await.unwrap();
        assert!(!path.exists());
        save_new_style(&repl.styles_dir, "留着", "内容").unwrap();
        repl.handle_delete_style("留着").unwrap();
        let (sender, receiver) = tokio::sync::mpsc::channel(1);
        drop(sender);
        repl.run_with_input(receiver).await;
        let saved: Repl = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(saved.pending_delete_name.as_deref(), Some("留着"));
        assert!(show_style_detail(&repl.styles_dir, "留着").is_ok());
    }

    #[tokio::test]
    async fn old_outline_session_missing_data_preserves_state_and_can_cancel() {
        let dir = tempfile::tempdir().unwrap();
        let mut repl = isolated_repl(dir.path());
        repl.state = SessionState::WaitingForOutline;
        assert!(repl.pending_style_content.is_none());
        assert!(repl.pending_delete_name.is_none());
        assert!(repl.process_input("ok").await.is_err());
        assert_eq!(repl.state, SessionState::WaitingForOutline);
        repl.process_input("/cancel").await.unwrap();
        assert_eq!(repl.state, SessionState::Idle);
    }

    #[tokio::test]
    async fn saved_outline_continues_rendering_without_classification() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let dir = tempfile::tempdir().unwrap();
        let mut repl = isolated_repl(dir.path());
        repl.state = SessionState::WaitingForOutline;
        repl.current_topic = Some("主题".into());
        repl.current_outline = Some("已有大纲".into());
        repl.current_style_content = Some("短句规则".into());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        repl.base_url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = vec![0; 32768];
            let n = socket.read(&mut request).await.unwrap();
            let text = String::from_utf8_lossy(&request[..n]);
            assert!(text.contains("已有大纲"));
            assert!(!text.contains("intent_type"));
            let body = r##"{"choices":[{"message":{"content":"# 新正文\n正文内容"}}]}"##;
            socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
        });
        repl.process_input("ok").await.unwrap();
        server.await.unwrap();
        assert_eq!(repl.state, SessionState::WaitingForFulltext);
        assert_eq!(repl.current_fulltext.as_deref(), Some("# 新正文\n正文内容"));
    }

    #[tokio::test]
    async fn failed_save_keeps_pending_name_on_quit() {
        let dir = tempfile::tempdir().unwrap();
        let mut repl = isolated_repl(dir.path());
        repl.state = SessionState::WaitingForStyleName;
        repl.pending_style_content = Some("不能丢失的结果".into());
        // 用普通文件占据会话目录，跨平台稳定地产生保存失败。
        std::fs::write(repl.session_dir.as_ref().unwrap(), "blocked").unwrap();
        assert!(!repl.process_input("/quit").await.unwrap());
        assert_eq!(
            repl.pending_style_content.as_deref(),
            Some("不能丢失的结果")
        );
        repl.process_input("另存风格").await.unwrap();
        assert_eq!(
            show_style_detail(&repl.styles_dir, "另存风格").unwrap(),
            "不能丢失的结果"
        );
    }

    #[tokio::test]
    async fn cancelled_style_request_keeps_original_metadata_and_body() {
        use tokio::io::AsyncReadExt;
        let dir = tempfile::tempdir().unwrap();
        let mut repl = isolated_repl(dir.path());
        repl.state = SessionState::WaitingForFulltext;
        repl.current_style_name = Some("原风格".into());
        repl.current_style_content = Some("原规则".into());
        repl.current_fulltext = Some("原正文".into());
        save_new_style(&repl.styles_dir, "新风格", "新规则").unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        repl.base_url = format!("http://{}", listener.local_addr().unwrap());
        let (tx, rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 8192];
            socket.read(&mut request).await.unwrap();
            tx.send(()).unwrap();
            std::future::pending::<()>().await;
        });
        tokio::select! {
            _ = rx => {},
            result = repl.handle_change_style("新风格") => panic!("request finished: {result:?}"),
        }
        server.abort();
        assert_eq!(repl.current_style_name.as_deref(), Some("原风格"));
        assert_eq!(repl.current_style_content.as_deref(), Some("原规则"));
        assert_eq!(repl.current_fulltext.as_deref(), Some("原正文"));
        assert_eq!(repl.state, SessionState::WaitingForFulltext);
    }

    #[test]
    fn draft_failure_preserves_body_and_allows_retry() {
        let dir = tempfile::tempdir().unwrap();
        let mut repl = isolated_repl(dir.path());
        repl.state = SessionState::WaitingForFulltext;
        repl.current_fulltext = Some("# 标题\n不能丢失的正文".into());
        repl.current_slug = Some("draft".into());
        let outputs = dir.path().join("outputs");
        std::fs::write(&outputs, "阻止创建目录").unwrap();
        assert!(repl.prepare_draft(outputs.to_str().unwrap()).is_err());
        assert_eq!(repl.state, SessionState::WaitingForFulltext);
        repl.persist_progress().unwrap();
        let saved: Repl = serde_json::from_str(
            &std::fs::read_to_string(repl.session_dir.as_ref().unwrap().join("current.json"))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(saved.current_fulltext, repl.current_fulltext);
        assert_eq!(saved.state, SessionState::WaitingForFulltext);

        std::fs::remove_file(&outputs).unwrap();
        repl.prepare_draft(outputs.to_str().unwrap()).unwrap();
        assert_eq!(repl.state, SessionState::WaitingForPublish);
        assert_eq!(
            std::fs::read_to_string(outputs.join("draft_v1.md")).unwrap(),
            repl.current_fulltext.as_deref().unwrap()
        );
        assert!(
            std::fs::read_to_string(outputs.join("draft_v1.html"))
                .unwrap()
                .contains("不能丢失的正文")
        );
    }

    #[test]
    fn website_draft_failure_keeps_review_state_and_local_copy() {
        let dir = tempfile::tempdir().unwrap();
        let mut repl = isolated_repl(dir.path());
        repl.state = SessionState::WaitingForFulltext;
        repl.current_fulltext = Some("# 标题\n网站正文".into());
        repl.current_slug = Some("draft".into());
        let outputs = dir.path().join("outputs");
        let website = dir.path().join("website");
        std::fs::create_dir_all(&website).unwrap();
        std::fs::write(website.join("content"), "阻止创建目录").unwrap();
        repl.website_path = website.to_str().unwrap().into();
        assert!(repl.prepare_draft(outputs.to_str().unwrap()).is_err());
        assert_eq!(repl.state, SessionState::WaitingForFulltext);
        let original = std::fs::read(outputs.join("draft_v1.md")).unwrap();
        assert_eq!(
            original,
            repl.current_fulltext.as_deref().unwrap().as_bytes()
        );
        std::fs::remove_file(website.join("content")).unwrap();
        repl.prepare_draft(outputs.to_str().unwrap()).unwrap();
        assert_eq!(repl.state, SessionState::WaitingForPublish);
        assert_eq!(
            std::fs::read(outputs.join("draft_v1.md")).unwrap(),
            original
        );
        assert!(
            std::fs::read_to_string(website.join("content/signal/draft.mdx"))
                .unwrap()
                .contains("网站正文")
        );
        assert!(
            !std::fs::read_dir(website.join("content/signal"))
                .unwrap()
                .any(|p| p
                    .unwrap()
                    .path()
                    .extension()
                    .is_some_and(|ext| ext == "pending"))
        );
    }

    #[test]
    fn restore_distinguishes_missing_invalid_and_unreadable_sessions() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(Repl::restore(dir.path()), Ok(None)));
        let path = dir.path().join("current.json");
        for content in [
            b"{broken".as_slice(),
            b"{\"state\":\"UnknownState\"}",
            b"\xff\xfe",
        ] {
            std::fs::write(&path, content).unwrap();
            let result = Repl::restore(dir.path());
            let error = match result {
                Err(e) => e.to_string(),
                _ => panic!("invalid session accepted"),
            };
            assert!(error.contains(&path.display().to_string()));
            assert!(error.contains("原文件未改动"));
            assert_eq!(std::fs::read(&path).unwrap(), content);
        }
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(matches!(
            Repl::restore(dir.path()),
            Err(AppError::FileRead(_))
        ));
        assert!(path.is_dir());
    }

    #[test]
    fn restore_preserves_session_on_missing_config_and_loads_legacy_state() {
        let _g = ENV_MUTEX.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("current.json");
        let original = r##"{"state":"WaitingForFulltext","current_fulltext":"# 旧文章","current_slug":"old"}"##;
        std::fs::write(&path, original).unwrap();
        let previous_key = std::env::var_os("API_KEY");
        unsafe { std::env::remove_var("API_KEY") };
        assert!(matches!(
            Repl::restore(dir.path()),
            Err(AppError::EnvVar(_))
        ));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        unsafe { std::env::set_var("API_KEY", "local-test") };
        let saved = Repl::restore(dir.path()).unwrap().unwrap();
        assert_eq!(saved.state, SessionState::WaitingForFulltext);
        assert_eq!(saved.current_fulltext.as_deref(), Some("# 旧文章"));
        assert!(saved.pending_style_content.is_none());
        assert_eq!(saved.session_dir.as_deref(), Some(dir.path()));
        assert_eq!(std::fs::read_to_string(path).unwrap(), original);
        unsafe {
            match previous_key {
                Some(value) => std::env::set_var("API_KEY", value),
                None => std::env::remove_var("API_KEY"),
            }
        }
    }

    #[tokio::test]
    async fn missing_unknown_and_ambiguous_styles_wait_without_generation() {
        let dir = tempfile::tempdir().unwrap();
        let mut repl = isolated_repl(dir.path());
        for name in ["轻辩", "清变"] {
            save_new_style(&repl.styles_dir, name, "犀利，短句。").unwrap();
        }
        let request = "写一篇 AI 焦虑，800 字，给学生看，犀利点";
        for query in ["", "未知名称", "犀利", "qingbian", "最近用过的风格"] {
            repl.begin_generation("AI 焦虑", query, request)
                .await
                .unwrap();
            assert_eq!(repl.state, SessionState::WaitingForGenerationStyle);
            assert_eq!(repl.generation_request.as_deref(), Some(request));
            assert_eq!(repl.pending_style_choices.len(), 2);
            assert!(repl.current_outline.is_none());
            for reply in ["好", "", "0", "999", "qingbian", "犀利"] {
                repl.process_input(reply).await.unwrap();
                assert_eq!(repl.state, SessionState::WaitingForGenerationStyle);
            }
        }
        repl.process_input("有哪些风格").await.unwrap();
        assert_eq!(repl.generation_request.as_deref(), Some(request));
        repl.process_input("/styles").await.unwrap();
        assert_eq!(repl.current_topic.as_deref(), Some("AI 焦虑"));
        assert!(repl.process_input("/quit").await.unwrap());
        let path = repl.session_dir.as_ref().unwrap().join("current.json");
        let saved: Repl = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved.pending_style_choices, repl.pending_style_choices);
        assert_eq!(saved.generation_request, repl.generation_request);
        repl.process_input("取消").await.unwrap();
        assert_eq!(repl.state, SessionState::Idle);
        assert!(repl.generation_request.is_none());
        assert!(repl.pending_style_choices.is_empty());
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn empty_and_single_style_libraries_never_auto_select() {
        let dir = tempfile::tempdir().unwrap();
        let mut repl = isolated_repl(dir.path());
        repl.begin_generation("主题", "", "写主题，300 字")
            .await
            .unwrap();
        assert_eq!(repl.state, SessionState::WaitingForGenerationStyle);
        assert!(repl.pending_style_choices.is_empty());
        save_new_style(&repl.styles_dir, "唯一", "规则").unwrap();
        repl.process_input("/styles").await.unwrap();
        assert_eq!(repl.pending_style_choices, vec!["唯一"]);
        assert_eq!(repl.state, SessionState::WaitingForGenerationStyle);
        repl.process_input("好").await.unwrap();
        assert_eq!(repl.state, SessionState::WaitingForGenerationStyle);
        // 选项显示后文件被删除：不能默默选择另一个文件。
        std::fs::remove_file(std::path::Path::new(&repl.styles_dir).join("唯一.md")).unwrap();
        assert!(matches!(
            repl.process_input("1").await,
            Err(AppError::FileRead(_))
        ));
        assert_eq!(repl.generation_request.as_deref(), Some("写主题，300 字"));
        repl.process_input("/cancel").await.unwrap();
        assert_eq!(repl.state, SessionState::Idle);
    }

    #[tokio::test]
    async fn selected_name_and_number_use_exact_file_and_keep_requirements_on_failure() {
        let dir = tempfile::tempdir().unwrap();
        let mut repl = isolated_repl(dir.path());
        save_new_style(&repl.styles_dir, "beta", "beta 原规则").unwrap();
        repl.begin_generation("主题", "", "写主题，800 字，给学生看")
            .await
            .unwrap();
        // 模拟恢复时目录新增排序更靠前的文件，旧编号仍指向 beta。
        save_new_style(&repl.styles_dir, "alpha", "alpha 新规则").unwrap();
        repl.show_generation_choices("", false).unwrap();
        assert_eq!(repl.pending_style_choices, vec!["beta"]);
        // 无效 URL 触发生成失败与现有重试，不访问外部模型。
        repl.base_url = "invalid-url".into();
        for reply in ["1", "用beta风格", "beta"] {
            repl.state = SessionState::WaitingForGenerationStyle;
            repl.pending_style_choices = vec!["beta".into()];
            assert!(repl.process_input(reply).await.is_err());
            assert_eq!(repl.current_style_name.as_deref(), Some("beta"));
            assert_eq!(repl.current_style_content.as_deref(), Some("beta 原规则"));
            assert_eq!(
                repl.generation_request.as_deref(),
                Some("写主题，800 字，给学生看")
            );
            assert!(
                repl.generation_brief()
                    .unwrap()
                    .contains("800 字，给学生看")
            );
        }
    }
}
