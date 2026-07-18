# Cognitive Writer

> AI 写作 + 风格逆向工具 — 面向微信公众号的 Rust CLI

**版本**: v2.1.0

---

## 这是什么

Cognitive Writer 是一个命令行工具，用 AI 帮你写微信公众号文章。它的核心思路是：

1. **从 URL 逆向学风格** — 给一篇文章链接，自动提取写作风格模板
2. **骨架-渲染双通道生成** — 先规划结构再填充正文，避免 AI 长篇输出时的逻辑坍缩
3. **局部重绘** — 在文中插入 `<AI_EDIT>` 标记，精准改写指定段落
4. **整文重写** — 给一条修改指令，LLM 重写全文
5. **一键粘贴到微信** — 自动注入剪贴板富文本，Ctrl+V 直接发

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
```

支持任何 OpenAI 兼容端点（OpenRouter / OpenAI / Ollama / DeepSeek 等）。

---

## 使用

### 生成文章

```bash
# 使用默认素材文件 inputs/idea_01.md
cargo run -- generate

# 指定素材文件
cargo run -- generate --input inputs/my_topic.md

# 跳过剪贴板注入（仅输出文件）
cargo run -- generate --no-clipboard
```

流程：选择风格 → AI 生成大纲骨架 → AI 按大纲渲染正文 → 输出 `.md` + `.html` + 剪贴板富文本。

### 逆向学习风格

```bash
cargo run -- learn https://example.com/some-article
```

自动抓取文章正文 → AI 分析 10 个维度的写作风格 → 保存为 `styles/{name}.md`，后续生成时可直接选用。

### 局部重绘

在生成的 Markdown 中插入标记：

```markdown
<AI_EDIT instruction="把这段改成更口语化">
原文需要重写的段落内容。
</AI_EDIT>
```

然后运行：

```bash
cargo run -- refine outputs/my_article_v1.md
```

AI 会按标记逐个重写，输出新版本 `.md` + `.html` + 剪贴板。

### 整文重写

```bash
# 命令行指定指令
cargo run -- update outputs/my_article_v1.md -i "把开头改得更直接，删掉铺垫段落"

# 交互式输入指令
cargo run -- update outputs/my_article_v1.md

# 跳过剪贴板
cargo run -- update outputs/my_article_v1.md -i "缩短到1500字" --no-clipboard
```

---

## 项目结构

```
src/
├── main.rs         # CLI 路由 + 5 个子命令入口
├── error.rs        # AppError 枚举 (thiserror)
├── llm.rs          # LLM 调用 + spinner + 重试逻辑
├── clipboard.rs    # CF_HTML 构建 + 多平台剪贴板注入
├── io.rs           # 文件 I/O + 风格选择 + 版本管理
└── refine.rs       # AI_EDIT 标记解析器
```

详细架构见 [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)。

---

## 命令参考

| 命令 | 说明 | 主要参数 |
|------|------|----------|
| `generate` | 生成文章（默认） | `-i, --input` `--no-clipboard` |
| `learn <URL>` | 从 URL 提取风格 | — |
| `refine <FILE>` | 局部重绘 | `--no-clipboard` |
| `update <FILE>` | 整文重写 | `-i, --instruction` `--no-clipboard` |

---

## 更新日志

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
