# Cognitive Writer — 系统架构文档

> 面向微信公众号的 AI 文章生成 CLI 工具，Rust 实现。

---

## 项目结构

```
cognitive-writer/
├── Cargo.toml              # 依赖声明
├── .env                    # API_KEY / BASE_URL / MODEL
├── src/main.rs             # 全部业务逻辑（~720 行）
├── inputs/
│   └── idea_01.md          # 用户素材输入（主题+内容）
├── styles/
│   └── wechat_base.md      # 风格模板（LLM system prompt）
└── outputs/
    ├── {slug}_v{N}.md      # Markdown 归档
    └── {slug}_v{N}.html    # 微信兼容 HTML
```

## 依赖栈

| Crate | 用途 |
|-------|------|
| `clap` (derive) | 子命令路由 |
| `tokio` | 异步运行时 |
| `reqwest` | HTTP 客户端（LLM API + URL 抓取） |
| `serde` / `serde_json` | JSON 序列化 |
| `dotenvy` | 加载 `.env` |
| `pulldown-cmark` | Markdown → HTML |
| `dialoguer` | 交互式 CLI 选择 |

## CLI 子命令

```
cognitive-writer generate    # 生成文章（默认，无参数时触发）
cognitive-writer learn <URL> # 从 URL 逆向分析写作风格
```

---

## 核心流程一：`generate` — 文章生成

### Pipeline 总览

```
                         Pass 1 (骨架)              Pass 2 (渲染)
inputs/idea_01.md ──┬─→ OUTLINE_SYSTEM_PROMPT ─→ outline ──┐
                    │                                       ├─→ style prompt ─→ Markdown ─┬─→ outputs/{slug}_v{N}.md
styles/*.md ────────┘───────────────────────────────────────┘                              ├─→ outputs/{slug}_v{N}.html（微信格式）
                                                                                          └─→ 系统剪贴板（富文本）
```

### Phase 1: 初始化 (run_generate)

1. **加载配置** — 从 `.env` 读取：
   - `API_KEY`（必填，缺失则 fatal exit）
   - `BASE_URL`（默认 `https://api.openai.com/v1`）
   - `MODEL`（默认 `gpt-4o`）

2. **风格选择** — 扫描 `styles/` 目录下所有 `.md` 文件：
   - 多个文件 → `dialoguer::Select` 交互选择
   - 单个文件 → 自动选用
   - 读取选中文件内容作为 system prompt

3. **读取素材** — 加载 `inputs/idea_01.md`，空文件则 fatal exit

### Phase 2: 双通道 LLM 调用 (generate_with_outline)

采用 **骨架-渲染（CoT）** 双通道架构，解决单次长文生成的逻辑坍缩/重复问题。

#### Pass 1 — 骨架生成

```
POST {base_url}/chat/completions

system: OUTLINE_SYSTEM_PROMPT（结构设计专家，要求输出 300-500 字 Markdown 大纲）
user:   inputs/idea_01.md 原始素材
→ outline（结构化大纲，含核心论点 + 支撑素材方向 + 段落衔接关系）
```

- 大纲仅内部流转，不输出文件
- 独立重试 3 次（2s 间隔）

#### Pass 2 — 正文渲染

```
POST {base_url}/chat/completions

system: styles/*.md 风格 prompt
user:   拼接模板（大纲 + 原始素材 + "严格按大纲展开"指令）
→ markdown（最终完整正文）
```

- 独立重试 3 次（2s 间隔）
- 支持任何 OpenAI 兼容端点（OpenRouter / OpenAI / Ollama 等）

### Phase 3: 版本命名

- **Slug 提取** (`extract_idea_slug`)：从素材中提取 `文章主题：` 或 `# ` 后的文字，过滤为合法文件名字符
- **版本号** (`next_version`)：扫描 `outputs/` 已有同名文件，取最大版本号 +1

### Phase 4: 三轨输出

#### Track A — Markdown 归档
- 路径：`outputs/{slug}_v{N}.md`
- 内容：LLM 原始 Markdown 输出，无任何变换

#### Track B — 微信 HTML (`md_to_wechat_html`)
- 路径：`outputs/{slug}_v{N}.html`
- 转换链路：
  1. `pulldown-cmark` 解析 Markdown（启用 strikethrough + tables）
  2. 生成原始 HTML
  3. **剥离 `<hr>` 标签**（字符串替换，因微信编辑器会剥离 `<style>` 导致分割线暴露）
  4. 外包 `<section style="font-size:15px;line-height:2;color:#333;">` 内联样式
  5. 包装为完整 HTML 文档（含 `<meta charset="utf-8">`）

#### Track C — 剪贴板注入 (`inject_clipboard`)

跨平台富文本注入，按优先级尝试：

| 优先级 | 环境 | 方法 | 关键细节 |
|--------|------|------|----------|
| 1 | WSL2 | PowerShell CF_HTML | 构建 CF_HTML 字节偏移格式 → 写入 `/tmp/cw_clipboard.html` → PowerShell 读取原始 UTF-8 字节 → MemoryStream 绕过 .NET UTF-16 编码，解决中文乱码 |
| 2 | Linux X11 | `xclip -selection clipboard -t text/html` | 管道传入 HTML |
| 3 | Linux Wayland | `wl-copy --type text/html` | 管道传入 HTML |

失败时退化为提示用户手动打开 HTML 文件。

---

## 核心流程二：`learn` — 风格逆向分析

### Pipeline 总览

```
URL ─→ Jina Reader (r.jina.ai) ─→ Markdown 正文 ─→ LLM 风格分析 ─→ styles/{name}.md
         │ (失败时降级)
         └→ 直接 GET + strip_html_tags() ─→ 纯文本 ──┘
```

### Step 1: 文章抓取 (`fetch_readable_text`)

- **首选路径**：通过 Jina Reader API (`https://r.jina.ai/{url}`)
  - Header: `Accept: text/markdown`, `User-Agent: cognitive-writer/0.3`
  - 超时 30s，响应即为清洗后的 Markdown 正文，无需 DOM 解析
- **降级路径**（Jina 返回非 2xx 或网络错误时）：
  - 直接 GET 原 URL 获取 HTML
  - `strip_html_tags()` 移除所有 HTML 标签，压缩连续空行，提取纯文本

### Step 2: LLM 风格分析

使用专用 system prompt (`LEARN_SYSTEM_PROMPT`)，指示 LLM 从 10 个维度分析：

1. 整体风格定位（如"逻辑+隐喻"、"口语化+犀利"）
2. 标题策略（长度/疑问/数字等）
3. 开头模式（故事钩子/金句/直接观点）
4. 段落节奏（长短交替/密集短段）
5. 句式特征（长短偏好/排比/反问频率）
6. 论证方法（类比/举例/数据/反直觉）
7. 情绪基调（冷静/激昂/反讽/温暖）
8. 结尾策略（升华/行动号召/开放问题）
9. 用词偏好（口语/书面/中英混用/领域术语）
10. 读者互动方式

最终输出"风格复刻提示词"段落，可直接作为 system prompt 使用。

### Step 3: 保存

- 交互式输入风格名称（dialoguer）
- 保存至 `styles/{name}.md`
- 后续 `generate` 可选用此风格

---

## 风格模板系统 (styles/*.md)

每个 `.md` 文件是一个完整的 LLM system prompt，控制文章生成的风格。

**`wechat_base.md` 要点**：

- **格式**：H1/H2 层级、段间空行、`**加粗**`、`> 引用`、1500-3000 字
- **人称**：第一人称"我"为主，"我们"≤1-2 次
- **反 AI 清单**：
  - 禁用套话："众所周知"、"不可否认"、"值得注意的是"等
  - 禁止"三段并列+升华"公式
  - 禁止每段末尾金句
  - 句长以短为主（10-20 字），偶尔长句调节节奏
- **风格标杆**：
  - 李笑来：概念先行，精确定义，短段落
  - 万维钢：工程思维，用数据说话，冷峻克制
  - 王小波：真诚幽默，粗糙例子讲大道理
- **结构**：开头直给洞察 → 核心概念一句话定义 → 2-3 论点+具体案例 → 最小可行动作或长期思考

---

## 错误处理

| 类型 | 行为 |
|------|------|
| API_KEY 缺失 | Fatal exit(1) |
| 素材文件空 | Fatal exit(1) |
| 无风格文件 | Fatal exit(1) |
| LLM 调用失败 | 重试 3 次（2s 间隔），全部失败后 fatal |
| 文件写入失败 | Fatal exit(1) |
| .env 不存在 | Warning，降级到系统环境变量 |
| 剪贴板工具不可用 | Warning，提示手动打开 HTML |

---

## 扩展点

- **新增风格**：在 `styles/` 下添加 `.md` 文件，或使用 `learn` 子命令从 URL 自动生成
- **切换模型**：修改 `.env` 中 `MODEL`（支持 OpenRouter 所有模型）
- **切换 API**：修改 `.env` 中 `BASE_URL`（任何 OpenAI 兼容端点）
- **多素材**：当前硬编码 `inputs/idea_01.md`，可扩展为目录扫描+选择
