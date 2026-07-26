# Cognitive Writer — 系统架构文档

> 面向微信公众号 + 个人网站的 AI 文章生成 Agent，Rust 实现。

---

## 版本演进

| 版本 | 形态 | 交互方式 | 状态 |
|------|------|---------|------|
| v2.1 | CLI 工具 | `cargo run -- <subcommand>` 4 个子命令 | 稳定 |
| v3.0 (当前) | 对话式 Agent | REPL 自然语言 + 状态机，可选 CLI 快捷模式 | 2026-07-26 已实现 |

---

## v3.0 架构总览

```
┌─────────────────────────────────────────────────────────┐
│                 REPL Loop (repl.rs: 1024 行)               │
│  stdin → Intent Parser → State Machine → Handler dispatch │
└──────────────────────┬──────────────────────────────────┘
                       │
         ┌─────────────┼─────────────┬──────────────┐
         ▼             ▼             ▼              ▼
     generate.rs   learn.rs     refine.rs     update.rs
         │             │             │              │
         ▼             ▼             ▼              ▼
      llm.rs ◄────── 共用 ──────► clipboard.rs
      io.rs                        website.rs
      styles.rs                    intent.rs
      error.rs
      lib.rs
```

**核心变更：** `repl.rs` 成为新入口（1024 行），`main.rs` 退化为启动分流器（终端 → REPL，管道/CLI 参数 → 旧 CLI 快捷模式）。

---

## 项目结构（v3.0 已实现）

```
cognitive-writer/
├── Cargo.toml
├── .env
├── src/
│   ├── main.rs              # 100 行 — REPL/CLI 分流器
│   ├── repl.rs              # 1024 行 — REPL 循环 + 状态机 + 意图分发
│   ├── intent.rs            # 706 行 — 意图枚举 + 关键词匹配 (41 tests)
│   ├── generate.rs          # 207 行 — 骨架→渲染双通道
│   ├── learn.rs             # 256 行 — 风格逆向学习 (含 strip_html_tags, 7 tests)
│   ├── update.rs            # 153 行 — 全文重写
│   ├── refine.rs            # 275 行 — 局部重绘 + run_refine 公开入口
│   ├── website.rs           # 386 行 — MDX 生成 + git push (12 tests)
│   ├── styles.rs            # 416 行 — 风格库管理 (18 tests)
│   ├── llm.rs               # 140 行 — LLM 客户端
│   ├── clipboard.rs         # 247 行 — 剪贴板注入
│   ├── io.rs                # 263 行 — 文件 I/O + delete_file
│   ├── error.rs             # 36 行 — AppError (新增 4 变体)
│   └── lib.rs               # 15 行 — 库入口 (支持 cargo test --lib)
├── inputs/
│   └── idea_01.md
├── styles/
│   ├── wechat_base.md
│   └── qingbian.md
├── outputs/
│   ├── {slug}_v{N}.md
│   └── {slug}_v{N}.html
└── docs/
    └── ARCHITECTURE.md
```

> **测试总计：124 tests, 0 failed**（`cargo test --lib`）

---

## 模块职责（v3.0）

### `src/main.rs` — 启动分流器

- 检测 stdin 是否为终端 → 是 → 启动 REPL
- 检测是否有 CLI 参数 → 是 → 走旧 CLI 子命令路径（generate/learn/refine/update）
- 管道输入 → 走旧 CLI 路径

### `src/repl.rs` — REPL 循环 + 状态机

**REPL 循环：**
```
loop {
    print prompt → read stdin → IntentParser::parse(line, &state)
    → match (intent, state):
        (Intent::Generate, Idle) → generate::run()
        (Intent::Confirm, WaitingForOutline) → generate::render_fulltext()
        (Intent::Confirm, WaitingForFulltext) → clipboard + website::write_mdx_draft()
        (Intent::Confirm, WaitingForPublish) → website::publish()
        ...
    → update state → loop
}
```

**状态机（SessionState）：**

```
                 ┌──────────┐
        ┌───────►   Idle   ◄───────────────┐
        │        └────┬─────┘               │
        │             │                     │
        │    「写一篇XX，用YY风格」           │ 「取消/算了」
        │             ▼                     │
        │   ┌──────────────────┐            │
        │   │ WaitingForOutline │───────────┘
        │   └────────┬─────────┘
        │            │
        │    「OK/继续」│「第二段改XX」
        │            │
        │            ▼
        │   ┌───────────────────┐
        │   │ WaitingForFulltext │◄────「整体改成XX风格」
        │   └────────┬──────────┘      (触发全文重写，回到本状态)
        │            │
        │    「OK/发布」  │「第三段加案例」
        │            │   (触发局部重绘，回到本状态)
        │            ▼
        │   ┌────────────────────┐
        │   │ WaitingForPublish  │──「只发网站」(单平台发布)
        │   └────────┬───────────┘──「等一下」(保留草稿，回 Idle)
        │            │
        │    「发布」
        │            │
        │            ▼
        │        发布完成 → Idle
        │
        └── 从任意状态「取消」→ Idle
```

四个状态，状态间转换由 `intent.rs` 的解析结果 + 当前状态联合决定。

### `src/intent.rs` — 意图解析器

**Intent 枚举：**

```rust
pub enum Intent {
    // ── 状态无关 ──
    Generate { topic: String, style_name: String },
    Learn { url: String },
    ListStyles,
    ShowStyle { name: String },
    DeleteStyle { name: String },
    RefineFile { path: String, instruction: String },
    UpdateFile { path: String, instruction: String },

    // ── 状态相关（仅在特定状态下有效）──
    Confirm,                              // OK/没问题/继续/就这样
    Cancel,                               // 算了/取消/不写了
    ModifyOutline { instruction: String }, // 仅在 WaitingForOutline 下
    ModifySection { instruction: String }, // 仅在 WaitingForFulltext 下
    ChangeStyle { style_name: String },    // 仅在 WaitingForFulltext 下
    Publish,                              // 仅在 WaitingForPublish 下
    PublishWebsiteOnly,                   // 仅在 WaitingForPublish 下
    Hold,                                 // 仅在 WaitingForPublish 下
    Unknown,                              // 无法识别
}
```

**匹配策略：** 纯关键词 + 规则优先级，不用 NLP 库。

```rust
fn parse_intent(input: &str, state: &SessionState) -> Intent {
    // 1. 先匹配状态无关的高置信度模式
    //   → 包含 URL → Learn
    //   → "写一篇" + "风格" → Generate
    //   → "风格库有什么" → ListStyles
    //   → "看看" + "风格" → ShowStyle
    //   → "删掉" + "风格" → DeleteStyle
    //   → "重写" + ".md" → UpdateFile
    //   → ".md" + "改成" → RefineFile

    // 2. 根据当前状态匹配
    //   match state {
    //     WaitingForOutline => is_confirm / is_cancel / ModifyOutline
    //     WaitingForFulltext => is_confirm / is_cancel / ModifySection / ChangeStyle
    //     WaitingForPublish => Publish / PublishWebsiteOnly / Hold / Cancel
    //     Idle => /* fall through */
    //   }

    // 3. 回退 → Unknown
}
```

### `src/generate.rs` — 骨架→渲染双通道

**从 main.rs 迁移，行为不变：**

```
素材 + 风格 → Pass 1 (OUTLINE_SYSTEM_PROMPT) → 骨架(300-500字)
           → Pass 2 (style system prompt + 大纲 + 素材) → 全文 Markdown
           → 版本管理 → outputs/{slug}_v{N}.md + .html + 剪贴板
```

**REPL 集成适配：**
- `generate_outline()` — 只做 Pass 1，返回骨架文本
- `render_fulltext(outline, style, idea)` — 做 Pass 2，返回全文
- REPL 在两个 pass 之间插入检查点等待用户确认

### `src/learn.rs` — 风格逆向学习

**从 main.rs 迁移，行为不变：**

```
URL → Jina Reader (降级: strip-tags) → MD 正文
    → LLM 分析 (LEARN_SYSTEM_PROMPT) → 风格分析报告
    → 用户命名 → styles/{name}.md
```

**REPL 集成适配：**
- `fetch_and_analyze(url)` → 返回风格分析文本
- 展示摘要（前 500 字符）
- 用户在 REPL 中输入名称 → 保存

### `src/refine.rs` — 局部重绘

**无改动。** REPL 集成时，不再依赖文件中的 AI_EDIT 标记，而是在 SessionState 中持有当前全文，直接构造 prompt 调用 LLM。

### `src/update.rs` — 全文重写

**从 main.rs 迁移，行为不变：**

```
原文 + 修改指令 → UPDATE_SYSTEM_PROMPT → LLM → 新全文
→ 版本管理 → outputs/{slug}_v{N}.md + .html + 剪贴板
```

### `src/website.rs` — 个人网站发布

**核心函数：**

```rust
/// 写入 MDX 草稿（检查点 2 通过后调用）
pub fn write_mdx_draft(
    website_path: &str,
    taxonomy: &str,  // "signal" / "node" / "pow"，默认 "signal"
    slug: &str,
    title: &str,
    markdown_body: &str,
) -> Result<String, AppError> {
    // 1. 构造 MDX frontmatter
    //    ---
    //    title: "标题"
    //    date: "2026-07-26"
    //    summary: "前 100 字摘要"
    //    ---
    // 2. 写入 {website_path}/content/{taxonomy}/{slug}.mdx
    // 3. 返回写入路径
}

/// 发布到网站（检查点 3「发布」后调用）
pub fn publish_to_website(
    website_path: &str,
    slug: &str,
) -> Result<String, AppError> {
    // 1. git add content/signal/{slug}.mdx
    // 2. git commit -m "post: {title}"
    // 3. git push origin main
    // 4. 返回：部署已触发 → https://khlilo.xyz/signal/{slug}
}
```

**不依赖 Vercel Deploy Hook。** Vercel 的 Git 集成在 push 后自动触发 build + deploy。

### `src/styles.rs` — 风格库管理

```rust
/// 模糊匹配风格文件名
/// 用户说"轻辩" → 扫描 styles/*.md → 匹配 styles/qingbian.md
pub fn fuzzy_match_style(name: &str) -> Result<(String, String), AppError> {
    // 1. 精确匹配文件名（去掉 .md）
    // 2. 包含匹配（name 是文件名的子串）
    // 3. 读文件内容标题行匹配
    // 4. 最近修改的（用户说"刚学的风格"）
}

/// 列出所有风格 + 一句话描述
pub fn list_styles_with_desc() -> Vec<StyleSummary>

/// 读取并摘要展示某个风格
pub fn show_style_detail(name: &str) -> Result<String, AppError>

/// 删除风格文件
pub fn delete_style(name: &str) -> Result<(), AppError>
```

### `src/lib.rs` — 库入口

提供 `pub mod` 声明，将所有模块暴露为库 crate。支持 `cargo test --lib` 运行全部 124 个单元测试，无需编译二进制。

---

## 功能对话触发表（完整版）

### 功能 1：风格逆向学习

| 用户输入 | Agent 行为 |
|---------|-----------|
| 「学一下这篇文章的风格：https://...」 | 抓取 → LLM 分析 → 展示摘要 → 问命名 → 保存 styles/{name}.md |
| 「分析这篇的文风 https://...」 | 同上 |

### 功能 2：文章生成 + 双平台草稿

| 用户输入 | Agent 行为 |
|---------|-----------|
| 「写一篇关于 XXX 的文章，用轻辩风格」 | 查风格库 → Pass 1: 大纲 → 展示 |
| 「OK」 | Pass 2: 全文 → 展示 |
| 「OK」 | 剪贴板 + MDX 草稿 → 「确认后说『发布』」 |
| 「发布」 | git push → Vercel 部署 |

### 功能 3：局部重绘（检查点 2 阶段）

| 用户输入 | Agent 行为 |
|---------|-----------|
| 「第三段加一个具体案例」 | 全文上下文 + 指令 → LLM 重写 → 展示 → 回 WaitingForFulltext |
| 「结尾太弱了，加强一下」 | 同上 |

### 功能 4：整文重写（检查点 2 阶段）

| 用户输入 | Agent 行为 |
|---------|-----------|
| 「整体语气太严肃了，放松一点」 | 全文上下文 + 指令 → UPDATE_SYSTEM_PROMPT → 展示 → 回 WaitingForFulltext |

### 功能 5：风格库管理

| 用户输入 | Agent 行为 |
|---------|-----------|
| 「我的风格库有哪些？」 | 列出 styles/*.md + 一句话描述 |
| 「看看轻辩风格的摘要」 | 读取并展示 |
| 「删掉 XXX 这个风格」 | 确认 → 删除 |

---

## 对话状态机完整意图表

| 用户输入 | 当前状态 | 解析结果 |
|---------|---------|---------|
| 「写一篇关于 XXX 的文章，用 YYY 风格」 | Idle | Generate |
| 「学一下这个风格 https://...」 | Idle | Learn |
| 「分析这篇的文风 https://...」 | Idle | Learn |
| 「我的风格库有什么」 | Idle | ListStyles |
| 「看看 YYY 风格的详情」 | Idle | ShowStyle |
| 「删掉 YYY 风格」 | Idle | DeleteStyle |
| 「重写 outputs/xxx.md，改成更犀利的风格」 | Idle | UpdateFile |
| 「把 outputs/xxx.md 第三段改短」 | Idle | RefineFile |
| 「OK / 没问题 / 继续 / 就这样」 | WaitingForOutline | Confirm |
| 「第N个论点换一下 / 加一个关于XXX的分论点」 | WaitingForOutline | ModifyOutline |
| 「算了 / 取消 / 不写了」 | 任意 | Cancel |
| 「OK / 没问题 / 发布 / 就这样」 | WaitingForFulltext | Confirm |
| 「第二段逻辑有问题，重写」 | WaitingForFulltext | ModifySection |
| 「结尾加个案例」 | WaitingForFulltext | ModifySection |
| 「整体语气太严肃了，放松一点」 | WaitingForFulltext | ChangeStyle |
| 「发布」 | WaitingForPublish | Publish |
| 「只发网站」 | WaitingForPublish | PublishWebsiteOnly |
| 「等一下 / 我再看看」 | WaitingForPublish | Hold |

---

## 三个检查点

### 检查点 1：大纲确认
- 展示后等待用户响应
- 通过：「OK / 没问题 / 继续」→ 进入全文渲染
- 修改：「第N个论点换一下 / 加一个关于XXX的分论点」→ Agent 重新生成大纲
- 放弃：「算了 / 换一个选题」→ 回到 Idle

### 检查点 2：全文确认
- 展示后等待用户响应
- 通过：「OK / 没问题 / 发布」→ 剪贴板 + MDX 草稿，进入 WaitingForPublish
- 局部修改：「第N段加个案例 / 结尾太弱了加强一下」→ 局部重绘（复用 refine 逻辑）
- 换风格：「整体换成更口语化的感觉」→ 全文重写（复用 update 逻辑）

### 检查点 3：发布确认
- 双平台草稿就绪后
- 「发布」→ git push → Vercel 自动部署
- 「等一下 / 我再看看」→ 保持草稿状态，用户可手动去微信后台粘贴
- 「只发网站」→ git push，公众号草稿保留但不额外操作（已在剪贴板）

---

## 双平台输出

### 目标 1：微信公众号
- 保持现有方案：CF_HTML 注入系统剪贴板
- 用户手动到微信后台粘贴
- 不使用微信公众号 API（个人订阅号不支持）

### 目标 2：个人网站（khlilo.xyz）
- 草稿：写入 `content/signal/{slug}.mdx`（带 frontmatter）
- 发布：`git add + commit + push` → Vercel Git 集成自动部署
- 不生成英文版本 .en.mdx
- 默认分类：signal

---

## 环境变量（.env）

```env
# 现有（不变）
API_KEY=sk-...
BASE_URL=https://api.deepseek.com
MODEL=deepseek-v4-pro

# v3.0 新增
WEBSITE_PATH=/home/khlilo/Genesis_Workspace/04_Arena_Output/Portfolio_Website/my-website
```

不需要的（已移除设计）：
- ~~WECHAT_APPID / WECHAT_APPSECRET~~ → 个人订阅号无 API 权限
- ~~WEBSITE_DEPLOY_HOOK~~ → Vercel Git 自动部署

---

## 依赖栈（v3.0 目标）

| Crate | 用途 | 变更 |
|-------|------|------|
| `tokio` | 异步运行时 | 不变 |
| `reqwest` | HTTP 客户端 | 不变 |
| `serde` / `serde_json` | JSON 序列化 | 不变 |
| `dotenvy` | 加载 `.env` | 不变 |
| `pulldown-cmark` | Markdown → HTML | 不变 |
| `thiserror` | 强类型错误枚举 | 不变 |
| `indicatif` | LLM 调用进度指示器 (spinner) | 不变 |
| `chrono` | MDX frontmatter 日期格式化 | 已添加 |
| `clap` | CLI 参数解析 | 保留（可选快捷模式） |
| `dialoguer` | 交互式 CLI 输入 | 保留（可选快捷模式） |

**Dev dependencies:** `tempfile` 不变。

---

## 错误处理扩展（v3.0）

新增 `AppError` 变体：

```rust
#[derive(Error, Debug)]
pub enum AppError {
    // ... 现有变体不变 ...

    #[error("网站集成失败: {0}")]
    Website(String),

    #[error("Git 操作失败: {0}")]
    Git(String),

    #[error("无法理解输入: {0}")]
    Intent(String),

    #[error("当前状态下不支持此操作: {state:?} ← {intent:?}")]
    InvalidState { state: SessionState, intent: Intent },
}
```

---

## v2.1 旧版模块参考（CLI 模式保留）

v3.0 REPL 模式下保留 CLI 快捷模式，以下 v2.1 子命令继续可用：

```
cog generate [OPTIONS]      # 快捷生成
cog learn <URL>             # 快捷学习
cog refine [OPTIONS] <FILE> # 局部重绘
cog update [OPTIONS] <FILE> # 整文重写
```

> **CLI 参数解析：** 使用手工参数解析（`std::env::args`），不依赖 clap derive。`clap` 和 `dialoguer` 依赖保留在 `Cargo.toml` 中但实际未使用。

---

## 实现阶段

| 阶段 | 内容 | 预计改动 | 状态 |
|------|------|---------|------|
| Phase 1 | 模块搬迁：main.rs → generate/learn/update/styles | 重构，不改变行为 | ✅ 已完成 |
| Phase 2 | intent.rs + repl.rs：意图解析 + 状态机 + REPL 循环 | 新增核心能力 | ✅ 已完成 |
| Phase 3 | website.rs：MDX 生成 + git push | 新增发布能力 | ✅ 已完成 |
| Phase 4 | REPL 集成：所有功能挂到 REPL 状态机 | 整合 | ✅ 已完成 |
| Phase 5 | 清理 + 测试 | 移除死代码 | ✅ 已完成 |

**实现总结：** 全部 5 个阶段已完成。总计 124 tests, 0 failed（`cargo test --lib`）。

## 已知技术债

以下问题在 v3.0 实现中确认存在，记录以供后续版本处理：

1. **`run_learn` / `run_update` 内部调用 `process::exit(1)`** — 错误路径会直接杀死 REPL 进程而非返回错误让 REPL 循环继续。应改为返回 `Result` 让调用方决定如何处理。
2. **私有函数副本** — 多个模块中存在私有的 `env_var`、`md_to_wechat_html` 等函数副本。应提取到共享模块（如 `utils.rs`）避免代码重复。
3. **Dead code warnings** — `delete_file`（`io.rs`）和 `StyleSummary.filename` 字段存在 `#[allow(dead_code)]` 或编译器 warning。需评估是移除死代码还是暴露为公开 API。

---

## 不做

- 不做选题推荐/自动选题
- 不做多轮闲聊
- 不做数据统计（阅读量等）
- 不做定时发布
- 不做 Web UI
- 不做多平台分发（只做公众号剪贴板 + 个人网站 git push）
- 不做英文版本 .en.mdx 同步生成
- 不做微信公众号 API 集成（个人订阅号无权限）

---

## 验收标准

1. 「学一下这个风格 https://...」→ 风格文件出现在 styles/ 目录
2. 「写一篇关于 XXX 的文章，用刚学的风格」→ 大纲 → 确认 → 全文 → 确认 → 剪贴板就绪 + 网站 MDX 草稿写入
3. 「第三段加个案例」→ 文章只改第三段，其余不变
4. 「发布」→ git push 已执行 + Vercel 自动部署，Agent 返回网站链接
5. 「我的风格库有什么」→ 列出所有可用风格 + 一句话描述
6. 「只发网站」→ git push 执行，公众号草稿保留在剪贴板不额外处理
