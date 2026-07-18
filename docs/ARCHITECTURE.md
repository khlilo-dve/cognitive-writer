# Cognitive Writer — 系统架构文档

> 面向微信公众号的 AI 文章生成 CLI 工具，Rust 实现。

---

## 项目结构

```
cognitive-writer/
├── Cargo.toml                  # 依赖声明
├── .env                        # API_KEY / BASE_URL / MODEL
├── src/
│   ├── main.rs                 # CLI 路由 + 5 个子命令入口 (667 行)
│   ├── error.rs                # AppError 枚举 (thiserror)
│   ├── llm.rs                  # LLM 调用 + spinner + with_retry
│   ├── clipboard.rs            # CF_HTML 构建 + 多平台剪贴板注入
│   ├── io.rs                   # 文件 I/O + 风格选择 + 版本管理
│   └── refine.rs               # AI_EDIT 标记解析器
├── inputs/
│   └── idea_01.md              # 用户素材输入（默认）
├── styles/
│   ├── wechat_base.md          # 基础微信风格模板
│   └── qingbian.md             # 轻辩风格（learn 产出）
├── outputs/
│   ├── {slug}_v{N}.md          # Markdown 归档
│   └── {slug}_v{N}.html        # 微信兼容 HTML
├── docs/
│   └── ARCHITECTURE.md          # 本文档
└── feedback.md                  # 用户反馈记录
```

## 依赖栈

| Crate | 用途 |
|-------|------|
| `clap` (derive) | 子命令路由 + CLI 参数 |
| `tokio` | 异步运行时 |
| `reqwest` | HTTP 客户端（LLM API + URL 抓取） |
| `serde` / `serde_json` | JSON 序列化 |
| `dotenvy` | 加载 `.env` |
| `pulldown-cmark` | Markdown → HTML |
| `dialoguer` | 交互式 CLI 选择 / 输入 |
| `thiserror` | 强类型错误枚举 |
| `indicatif` | LLM 调用进度指示器 (spinner) |

**Dev dependencies:**

| Crate | 用途 |
|-------|------|
| `tempfile` | 测试用临时目录 |

## CLI 子命令

```
cognitive-writer generate [OPTIONS]     # 生成文章（默认行为）
cognitive-writer learn <URL>            # 从 URL 逆向分析写作风格
cognitive-writer refine [OPTIONS] <FILE> # 局部重绘：解析 <AI_EDIT> 标记
cognitive-writer update [OPTIONS] <FILE> # 基于修改指令 LLM 重写已有文章
```

### 通用选项

| 子命令 | 选项 | 说明 |
|--------|------|------|
| `generate` | `-i, --input <PATH>` | 素材文件路径（默认 `inputs/idea_01.md`） |
| `generate` | `--no-clipboard` | 跳过剪贴板注入，仅输出文件 |
| `refine` | `--no-clipboard` | 同上 |
| `update` | `-i, --instruction <TEXT>` | 修改指令（不提供则交互式输入） |
| `update` | `--no-clipboard` | 同上 |

---

## 模块职责

### `src/error.rs` — 错误类型

```rust
#[derive(Error, Debug)]
pub enum AppError {
    EnvVar(String),          // 环境变量缺失
    FileRead(String),        // 文件读取失败
    FileWrite(String),       // 文件写入失败
    ApiError { status, body }, // LLM API HTTP 错误
    Network(String),         // 网络请求失败
    Parse(String),           // API 响应解析失败
    EmptyChoices,            // API 返回空 choices
    Clipboard(String),       // 剪贴板工具不可用
    NoStyles(String),        // 风格目录为空
    AiEditParse(String),     // AI_EDIT 标签语法错误
    Io(std::io::Error),      // 标准 I/O 错误
}
```

所有模块使用 `Result<T, AppError>` 而非原始的 `Result<T, String>`。

### `src/llm.rs` — LLM 客户端

- `call_llm()`: OpenAI 兼容 API 调用（单次，无重试）
- `with_retry()`: 泛型重试包装器，统一所有 LLM 调用点的重试逻辑（3 次 / 2s 间隔）
- `new_spinner()`: 创建 indicatif 转圈动画，用于 LLM 调用的进度反馈

### `src/clipboard.rs` — 剪贴板注入

跨平台 CF_HTML 富文本注入：

| 优先级 | 环境 | 方法 | 关键细节 |
|--------|------|------|----------|
| 1 | WSL2 | PowerShell CF_HTML | 构建 CF_HTML 字节偏移格式 → PowerShell ReadAllBytes → MemoryStream 绕过 .NET UTF-16 编码 |
| 2 | Linux X11 | `xclip -selection clipboard -t text/html` | 管道传入 HTML |
| 3 | Linux Wayland | `wl-copy --type text/html` | 管道传入 HTML |

每个工具失败时打印 `[warn] <tool> 失败: <detail>`，然后尝试下一个。

### `src/io.rs` — 文件 I/O + 版本管理

- `list_styles()` / `select_style()`: 扫描 `styles/` 并进行交互选择
- `read_file()` / `write_file()`: 通用文件读写
- `extract_idea_slug()`: 从素材中提取文章标题 slug（支持 `# 文章主题：` 和 `# ` 格式）
- `next_version()`: 扫描 `outputs/` 获取同名 slug 最大版本号 +1

### `src/refine.rs` — AI_EDIT 解析器

纯字符串搜索的状态机，解析 `<AI_EDIT instruction="...">...</AI_EDIT>` 标记：

- `parse_ai_edits()`: 返回 `Vec<AiEdit>`，包含 instruction / original / full_match
- `AiEdit` 结构体: instruction（修改指令）、original（原文本）、full_match（完整匹配串，用于替换）

### `src/main.rs` — CLI 路由 + 子命令

- Clap derive 定义 `Cli` 和 `Commands` enum
- 5 个子命令入口函数: `run_generate()`, `run_learn()`, `run_refine()`, `run_update()`
- 核心常量: `OUTLINE_SYSTEM_PROMPT`, `LEARN_SYSTEM_PROMPT`, `REFINE_SYSTEM_PROMPT`, `UPDATE_SYSTEM_PROMPT`
- `generate_with_outline()`: 骨架-渲染双通道 LLM 调用
- `md_to_wechat_html()`: Markdown → 微信兼容 HTML
- `strip_html_tags()` / `fetch_readable_text()`: URL 内容抓取（Jina Reader + 降级方案）
- `fatal()`, `env_var()`, `env_var_or()`: 通用辅助函数

---

## 核心流程

### 一、`generate` — 文章生成

```
                         Pass 1 (骨架)              Pass 2 (渲染)
inputs/{file}.md ──┬─→ OUTLINE_SYSTEM_PROMPT ─→ outline ──┐
                   │                                       ├─→ style prompt ─→ Markdown ─┬─→ outputs/{slug}_v{N}.md
styles/*.md ───────┘───────────────────────────────────────┘                              ├─→ outputs/{slug}_v{N}.html
                                                                                          └─→ 系统剪贴板 (除非 --no-clipboard)
```

#### Phase 1: 初始化

1. 加载 `.env` 配置（API_KEY / BASE_URL / MODEL）
2. 扫描 `styles/` 并交互选择风格（单个时自动选用）
3. 读取素材文件（默认 `inputs/idea_01.md`，可通过 `--input` 指定）

#### Phase 2: 骨架-渲染双通道 (generate_with_outline)

采用 **CoT（Chain of Thought）** 双通道架构，先规划结构再填充正文，解决单次长文生成的逻辑坍缩/重复问题。

- **Pass 1 — 骨架**: `OUTLINE_SYSTEM_PROMPT` + 原始素材 → 300-500 字 Markdown 大纲（内部流转，不输出文件）
- **Pass 2 — 渲染**: 风格 system prompt + `大纲 + 原始素材 + "严格按大纲展开"` → 完整 Markdown 正文

两轮 LLM 调用均通过 `with_retry(3, ...)` 自动重试。调用期间显示 indicatif spinner。

#### Phase 3: 版本命名

- Slug 提取: 素材中 `文章主题：` 或第一个 `# ` 后的文字，过滤为合法文件名字符
- 版本号: 扫描 `outputs/` 同名 slug 最大版本号 +1

#### Phase 4: 四轨输出

- Track A — Markdown 归档: `outputs/{slug}_v{N}.md`
- Track B — 微信 HTML: `outputs/{slug}_v{N}.html`（pulldown-cmark → 剥离 `<hr>` → 内联样式包裹）
- Track C — 剪贴板注入: 跨平台 CF_HTML（可 `--no-clipboard` 跳过）
- Track D — 终端日志: spinner 进度 + [done] 输出路径

---

### 二、`learn` — 风格逆向分析

```
URL ─→ Jina Reader (r.jina.ai) ─→ Markdown 正文 ─→ LLM 风格分析 ─→ styles/{name}.md
         │ (失败时降级)
         └→ 直接 GET + strip_html_tags() ─→ 纯文本 ──┘
```

#### Step 1: 文章抓取 (fetch_readable_text)

- **首选**: Jina Reader API (`https://r.jina.ai/{url}`), Accept: text/markdown, 超时 30s
- **降级**: 直接 GET → `strip_html_tags()` 移除所有 HTML 标签 + 压缩连续空行

#### Step 2: LLM 风格分析

使用 `LEARN_SYSTEM_PROMPT`，从 10 个维度分析写作风格（见风格模板系统章节），最终输出可直接作为 system prompt 的「风格复刻指令」。

#### Step 3: 保存

交互式输入风格名称 → 保存至 `styles/{name}.md` → 后续 `generate` 可选用。

---

### 三、`refine` — 局部重绘

```
file ─→ read ─→ parse_ai_edits() ─→ [for each edit] ─→ call_llm() ─→ 内存替换
                                                                        │
                                     ┌──────────────────────────────────┘
                                     ├─→ outputs/{slug}_v{N+1}.md
                                     ├─→ outputs/{slug}_v{N+1}.html
                                     └─→ 剪贴板 (除非 --no-clipboard)
```

#### 标记语法

```markdown
<AI_EDIT instruction="把这段改成更口语化的表达">
这里是需要重写的原文本片段。
</AI_EDIT>
```

- `instruction` 属性：修改指令
- 标签内部：要被替换的原文本
- 支持同一文件中多个标记，按顺序处理

#### 解析器 (parse_ai_edits)

纯字符串搜索，无 regex 依赖：
1. `str::find("<AI_EDIT ")` 定位开标签
2. 提取 `instruction="..."` 属性值
3. 找 `>` 确定开标签结束
4. 找 `</AI_EDIT>` 提取原文本和完整匹配串

设计决策：标签格式完全可控，手动切片比 regex 更透明、零额外依赖。

#### 执行流程

- 顺序处理（非并行），避免上下文交叉污染
- 每个标记: `REFINE_SYSTEM_PROMPT` + `"修改指令：{instruction}\n\n原文本：\n{original}"` → LLM 重写
- `content.replacen(&full_match, &rewritten, 1)` 单次替换，防止重复片段误替换
- `with_retry(3, ...)` 自动重试，spinner 显示进度

---

### 四、`update` — 整文重写

```
file ─→ read ─→ instruction (CLI 或交互输入) ─→ LLM 重写 ─┬─→ outputs/{slug}_v{N}.md
                                                              ├─→ outputs/{slug}_v{N}.html
                                                              └─→ 剪贴板 (除非 --no-clipboard)
```

#### 与 refine 的区别

| 维度 | refine | update |
|------|--------|--------|
| 粒度 | 局部（AI_EDIT 标记） | 全文 |
| 交互方式 | 预埋标记 | CLI 参数或交互输入指令 |
| 风格 | 不改变（原风格保留） | 不改变（复用原文风格） |
| 适用场景 | 微调某段落表述 | 整体方向调整、补充/删减内容 |

#### 执行流程

1. 读取目标 Markdown 文件
2. 获取修改指令（CLI `-i` 参数 > 交互式 `dialoguer::Input`）
3. `UPDATE_SYSTEM_PROMPT` + `"原文 + 修改指令"` → LLM 重写全文
4. `with_retry(3, ...)` + spinner → 版本管理 → 四轨输出

---

## 风格模板系统 (styles/*.md)

每个 `.md` 文件是一个完整的 LLM system prompt，控制文章生成的风格。

**`wechat_base.md` 要点**：

- **格式**: H1/H2 层级、段间空行、`**加粗**`、`> 引用`、1500-3000 字
- **人称**: 第一人称"我"为主，"我们"≤1-2 次
- **反 AI 清单**: 禁用套话（"众所周知"等）、禁止"三段并列+升华"、禁止每段末尾金句、短句为主（10-20 字）
- **风格标杆**: 李笑来（概念先行）、万维钢（工程思维）、王小波（真诚幽默）
- **结构**: 开头直给洞察 → 核心概念一句话定义 → 2-3 论点+具体案例 → 最小可行动作

**`qingbian.md`** (learn 产出):

- 来源: 蔡垒磊《从第一个100万，到第一个1亿》
- 特征: 理性分析 + 反直觉论证 + 通俗白话、设问自答、数字密集、冷静现实主义
- 含 11 个维度的完整风格分析报告 + 可用的 system prompt

---

## 错误处理

| 类型 | 行为 |
|------|------|
| API_KEY 缺失 | Fatal exit(1) |
| 素材文件空 | Fatal exit(1) |
| 无风格文件 | Fatal exit(1) |
| 修改指令为空 (update) | Fatal exit(1) |
| LLM 调用失败 | with_retry 重试 3 次（2s 间隔），全部失败后 fatal |
| 文件写入失败 | Fatal exit(1) |
| .env 不存在 | Warning，降级到系统环境变量 |
| 剪贴板工具不可用 | 逐工具打印 warn + 详情，全部失败后提示手动打开 HTML |
| AI_EDIT 标签格式错误 | Fatal exit(1)，报告具体位置 |
| 目标文件不存在 (refine/update) | Fatal exit(1) |
| 无 AI_EDIT 标记 | Fatal exit(1) |

---

## 重试 & 进度策略

### 泛型重试 (with_retry)

```rust
pub async fn with_retry<F, Fut>(
    max_retries: u32,
    f: F,
    label: &str,
) -> Result<T, AppError>
```

- 所有 LLM 调用点统一使用，消除重复代码
- 3 次重试、2 秒间隔、每次失败打印 `[warn] {label} 第 {n} 次失败`
- 最终失败返回含上下文的 `AppError`

### 进度指示器 (new_spinner)

```rust
pub fn new_spinner(msg: &str) -> ProgressBar
```

- 基于 `indicatif` 的 Braille spinner（⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏）
- 80ms tick、绿色动画、消息文本动态更新
- 覆盖所有 LLM 调用点: generate (Pass 1 + 2)、learn、refine (逐标记)、update

---

## 测试

36 个单元测试，分布如下：

| 模块 | 测试数 | 覆盖函数 |
|------|--------|----------|
| `src/io.rs` | 16 | `extract_idea_slug` (10) + `next_version` (6) |
| `src/refine.rs` | 8 | `parse_ai_edits` (8) |
| `src/clipboard.rs` | 5 | `build_cf_html` (5) |
| `src/main.rs` | 7 | `strip_html_tags` (7) |

运行: `cargo test`

---

## 扩展点

- **新增风格**: 在 `styles/` 下添加 `.md` 文件，或使用 `learn` 子命令从 URL 自动生成
- **切换模型**: 修改 `.env` 中 `MODEL`
- **切换 API**: 修改 `.env` 中 `BASE_URL`（任何 OpenAI 兼容端点）
- **多素材**: 使用 `generate --input` 指定不同素材文件
- **批量 update**: 结合 shell 脚本对多个文件执行 update
- **CI 测试**: `cargo test` 已就绪，可直接接入 GitHub Actions
