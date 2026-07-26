# Cognitive Writer

> AI 写作 + 风格逆向工具 — 面向微信公众号 + 个人网站的 AI 写作 Agent，Rust 实现

**版本**: v3.1.0

---

## 这是什么

Cognitive Writer 是一个对话式写作 Agent。它的核心思路是：

1. **对话式 REPL 交互** — 自然语言输入意图，状态机引导完成写作全流程
2. **从 URL 逆向学风格** — 给一篇文章链接，自动提取写作风格模板
3. **骨架-渲染双通道生成** — 先规划结构再填充正文，避免 AI 长篇输出时的逻辑坍缩
4. **局部重绘** — 在文中插入 `<AI_EDIT>` 标记，精准改写指定段落
5. **整文重写** — 给一条修改指令，LLM 重写全文
6. **一键粘贴到微信** — 自动注入剪贴板富文本，Ctrl+V 直接发
7. **个人网站发布** — MDX 生成 + git push → Vercel 自动部署

---

## 安装

```bash
git clone https://github.com/khlilo/cognitive-writer.git
cd cognitive-writer
cargo build --release
```

需要 Rust 工具链（1.80+）。

---

## 配置

复制 `.env.example` 为 `.env`（或直接创建 `.env`）：

```env
API_KEY=your-api-key-here
BASE_URL=https://api.deepseek.com
MODEL=deepseek-v4-pro
WEBSITE_PATH=/path/to/your/nextjs/website   # 新增：个人网站项目路径
```

支持任何 OpenAI 兼容端点（OpenRouter / OpenAI / Ollama / DeepSeek 等）。

---

## 使用

### REPL 模式（主要方式）

```bash
# 启动 REPL
cargo run

# 进入对话后：
> 写一篇关于 AI Agent 的文章，用轻辩风格
[大纲展示...]
[大纲确认] > OK
[全文展示...]
[全文确认] > OK
[草稿就绪 + 剪贴板已注入]
[发布确认] > 发布
```

REPL 内置 4 状态对话状态机：Idle → WaitingForOutline → WaitingForFulltext → WaitingForPublish。自然语言输入由意图解析器（20+ 种模式，纯关键词匹配，零 NLP 依赖）自动路由到 generate / learn / refine / update / website 等模块。

### CLI 快捷模式

```bash
cargo run -- generate          # 生成文章
cargo run -- learn <URL>       # 从 URL 提取风格
cargo run -- refine <FILE>     # 局部重绘
cargo run -- update <FILE>     # 整文重写
```

CLI 模式直接从参数执行，跳过 REPL 对话循环。适合脚本集成和快速操作。

---

## 项目结构

```
src/
├── main.rs         # REPL/CLI 分流器 (126 行)
├── repl.rs         # REPL 循环 + 状态机 (1116 行)
├── intent.rs       # 意图解析器 (894 行)
├── generate.rs     # 骨架→渲染双通道
├── learn.rs        # 风格逆向学习
├── update.rs       # 全文重写
├── refine.rs       # 局部重绘
├── website.rs      # MDX 生成 + git push
├── styles.rs       # 风格库管理
├── llm.rs          # LLM 客户端
├── clipboard.rs    # 剪贴板注入
├── io.rs           # 文件 I/O
├── error.rs        # AppError 枚举
└── lib.rs          # 库入口
```

详细架构见 [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)。

---

## 命令参考

| 功能 | 触发方式 | 行为 |
|------|---------|------|
| 生成文章 | `生成/写一篇/草拟/新文章` | AI 骨架→渲染双通道生成 |
| 逆向学风格 | `学习/逆向/分析风格 <URL>` | 抓取正文 → AI 分析 10 维度 → 保存为风格 |
| 局部重绘 | `修改/重写某段/打磨/润色` | 解析 AI_EDIT 标记 → LLM 逐段重写 |
| 全文重写 | `改全文/翻新/换个角度` | LLM 基于修改指令重写全文 |
| 发布网站 | `发布/部署/推送到网站` | MDX 生成 + git push → Vercel |
| 管理风格 | `查看风格/列出风格/删除风格` | 模糊匹配、列表摘要、删除 |
| 查看帮助 | `帮助/help/?/h` | 显示使用说明 |

---

## 更新日志

### v3.1.0 (2026-07-26)

**异步事件循环 + 会话持久化**

- REPL 循环升级为 tokio 异步 stdin + tokio::select! 事件循环
- 新增会话持久化：每次操作后自动保存到 ~/.cognitive-writer/current.json
- 新增优雅关闭：Ctrl+C/SIGTERM 自动保存状态后退出
- 启动时检测未完成会话并提示恢复
- 新增 dirs 依赖用于跨平台用户目录
- 新增 Repl::save() / Repl::restore() / Repl::clear_session() 方法
- 132 个单元测试覆盖全部模块

### v3.0.0

**CLI → 对话式写作 Agent**

- 新增 REPL 自然语言交互模式，4 状态对话状态机（Idle → WaitingForOutline → WaitingForFulltext → WaitingForPublish）
- 新增 `intent.rs` 意图解析器：20+ 种自然语言输入 → 结构化 Intent（纯关键词匹配，零 NLP 依赖）
- 新增 `website.rs` 个人网站发布模块：MDX frontmatter 生成 + git push → Vercel 自动部署
- 新增 `styles.rs` 风格库管理：模糊匹配、列表摘要、删除
- 主模块拆分：main.rs (667→100行) → generate / learn / update / refine / repl 5 个独立模块
- 新增 `lib.rs` 库入口，支持 `cargo test --lib`
- 新增 `delete_file` 到 io.rs
- error.rs 新增 4 个变体（Website / Git / Intent / InvalidState）
- 新增 `chrono` 依赖用于 MDX 日期格式化
- 132 个单元测试覆盖全部模块（v2.1: 36 tests）
- 公众号保持剪贴板注入方案，不做 API 集成（个人订阅号无 API 权限）

### v2.1.0 (2026-07-18)

**模块化重构 + 新增功能**

- 将 893 行单文件 `main.rs` 拆分为 6 个模块（error / llm / clipboard / io / refine / main）
- 引入 `thiserror` 强类型 `AppError` 枚举替代原始 String 错误
- 泛型 `with_retry()` 统一所有 LLM 调用点的重试逻辑，消除三处重复代码
- 修复 `dotenvy::dotenv()` 在 `run_refine` 中的重复调用
- `--no-clipboard` flag 支持跳过剪贴板注入（generate / refine / update）
- `generate --input` 参数支持指定自定义素材文件
- `inject_clipboard` 每个工具失败时打印具体错误详情
- 新增 `update` 子命令：基于修改指令 LLM 重写全文
- 引入 `indicatif` spinner 替代静态日志，所有 LLM 调用点实时动画反馈
- 36 个单元测试覆盖 `extract_idea_slug` / `next_version` / `parse_ai_edits` / `build_cf_html` / `strip_html_tags`
- `pipe_to_cmd` 完善 stderr 捕获 + 退出码检查
- `docs/ARCHITECTURE.md` 全面更新

### v2.0.2

- `refine` 子命令：局部重绘（AI_EDIT 标记解析 + LLM 重写 + 三轨输出）
- `learn` 接入 Jina Reader API
- `generate` 双通道骨架-渲染架构
- 提取 `extract_idea_slug` 兼容 `# 文章主题：` 格式

### v2.0.1

- 修复剪贴板注入中文乱码（CF_HTML 字节偏移编码链路）
- 隐藏富文本中 `<hr>` 分割线
- 双轨输出 + 风格选择 + Markdown→微信 HTML 转换

### v2.0.0

- 初始版本：`generate` / `learn` 子命令
- OpenAI 兼容 API 支持
- `dialoguer` 交互式风格选择
- `pulldown-cmark` Markdown 渲染

---

## License

MIT
