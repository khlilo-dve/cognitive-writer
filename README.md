# Cognitive Writer

> 一个想法进去，两边文章出来。用 Rust 写的 AI 写作 Agent。

<p align="center">
  <strong>说人话 → 自动写文章 → 公众号 + 个人网站 双平台发布</strong>
</p>

**版本**: v3.1.0 | **License**: MIT

---

## 它做什么

你对着终端说一句话，剩下的全自动：

```
> 写一篇关于认知偏差在投资中的应用，轻辩风格

Agent:  匹配风格 → 生成大纲 → 渲染全文 → 展示
[全文确认] > 第三段加个具体案例
Agent:  局部重绘，展示新版本
[全文确认] > OK
Agent:  富文本已注入剪贴板（微信 Ctrl+V 即发）
        网站 MDX 已写入，确认后说「发布」
[发布确认] > 发布
Agent:  git push → Vercel 自动部署
        done. https://khlilo.xyz/signal/cognitive-bias-investing
```

**三个关键设计：**

1. **端到端** — 从选题到两边上线，全程不切换工具
2. **自然语言** — 不用记命令，LLM 理解你的意图
3. **渐进确认** — 全文和发布两步确认，中间随时反悔或修改

---

## 安装

需要 Rust 1.80+。

```bash
git clone https://github.com/khlilo/cognitive-writer.git
cd cognitive-writer
cargo build --release
```

## 配置

创建 `.env`：

```env
API_KEY=your-api-key-here
BASE_URL=https://api.deepseek.com        # 任何 OpenAI 兼容端点
MODEL=deepseek-v4-pro
WEBSITE_PATH=/path/to/your/website        # 可选，个人网站项目路径
```

## 使用

### REPL 模式（主要方式）

```bash
cargo run
# 进入对话，说人话就行
```

### CLI 快捷模式

```bash
cargo run -- generate          # 生成文章
cargo run -- learn <URL>       # 学风格
cargo run -- refine <FILE>     # 局部重绘
cargo run -- update <FILE>     # 全文重写
```

---

## 能力边界

| ✅ 能做 | ❌ 不做（by design） |
|---------|---------------------|
| 自然语言 → 结构化意图（LLM 分类） | 选题推荐 |
| 10 维度逆向学习写作风格 | 多轮闲聊 |
| 骨架→渲染双通道 LLM 生成 | 数据统计 / 定时发布 |
| 微信富文本剪贴板注入 | Web UI |
| 个人网站 MDX + git push 发布 | 多平台分发 |
| 风格库模糊匹配 + 管理 | 英文版同步生成 |
| 会话持久化（进程中断可恢复） | 公众号 API 集成 |

---

## 架构

14 个源文件，132 个单元测试。main.rs 从 667 行降到 126 行。

```
src/
├── main.rs         # REPL/CLI 分流器 (126 行)
├── repl.rs         # 异步 REPL 循环 + 状态机 + 会话持久化 (1116 行)
├── intent.rs       # LLM 意图分类 + 关键词 fallback (894 行)
├── generate.rs     # 骨架→渲染双通道
├── learn.rs        # Jina Reader + 10 维度风格逆向
├── website.rs      # MDX 生成 + git push
├── styles.rs       # 风格库管理
├── refine.rs       # 局部重绘
├── update.rs       # 全文重写
├── llm.rs          # OpenAI 兼容 API 客户端
├── clipboard.rs    # 跨平台 CF_HTML 剪贴板
├── io.rs           # 文件 I/O + 版本管理
├── error.rs        # AppError 枚举
└── lib.rs          # 库入口
```

完整架构文档：[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)
演进史与设计理念：[docs/EVOLUTION.md](docs/EVOLUTION.md)

---

## 📮 Hosted Version — Coming Soon

在做一个托管版：不用装 Rust、不用配环境、不用管 API key。浏览器打开，输入选题，点发布。预计 2026 年 Q3 内测。

开源版永远免费。托管版感兴趣的话直接联系我：**ferkasybilla312@gmail.com**

---

## 更新日志

### v3.1.0 (2026-07-26) — 异步事件循环 + 会话持久化

- REPL 升级 tokio 异步 stdin + `select!` 事件循环，Ctrl+C 自动保存
- 会话持久化：每次操作后自动写 `~/.cognitive-writer/current.json`
- 启动恢复：检测未完成会话并提示恢复
- 132 个单元测试

### v3.0.0 (2026-07-26) — CLI → 对话式 Agent

- REPL 自然语言交互 + 4 状态对话状态机
- LLM 意图分类替代关键词匹配
- 个人网站发布模块（MDX + git push）
- 风格库管理、模块拆分（main.rs 667→126 行）

### v2.1.0 (2026-07-18) — 模块化重构

6 模块拆分、thiserror 强类型错误、泛型重试、36 单测、spinner 进度

### v2.0.0 — 初始版本

generate / learn 子命令、骨架-渲染双通道、跨平台剪贴板

---

## Star History

如果这个项目对你有用，给个 ⭐ 就是对我最大的支持。

[![Star History Chart](https://api.star-history.com/svg?repos=khlilo/cognitive-writer&type=Date)](https://star-history.com/#khlilo/cognitive-writer&Date)

---

## License

MIT
