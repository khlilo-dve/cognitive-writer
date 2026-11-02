# Cognitive Writer v3.3.2

**产品文档入口（2026-11-03）：** [产品需求PRD](docs/PRD.md)与[风格学习子Agent行动计划](docs/plans/2026-11-03-style-learning-agent-plan.md)。当前已实现正文学习与版本开发链；主Agent调度、风格学习子Agent及统一文件／URL输入链尚在设计阶段。本轮只更新文档，产品文档先于后续代码。下方更新日志保留当时实现，不代表当前新功能已完成。

> 一个想法进去，两边文章出来。用 Rust 写的 AI 写作 Agent。

<p align="center">
  <strong>说人话 → 智能风格匹配 / 逆向提取 → 自动写文章 → 公众号 + 个人网站 双平台发布</strong>
</p>

**版本**: v3.3.2 | **License**: MIT

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

**核心设计理念：**

1. **端到端交付** — 从选题输入到公众号剪贴板与网站上线，全程不切换工具
2. **自然语言交互** — 结构化意图识别 + 状态机流转，说人话即可调度
3. **详细风格学习** — 自由格式Markdown说明、拼音归一化匹配与单篇URL学习；多篇正文开发链另见产品需求。
4. **渐进确认机制** — 大纲衔接、全文确认、局部重绘与发布确认，过程随时掌控

---

## 核心亮点：风格引擎与逆向分析

风格说明用于告诉Writer一种声音怎样形成、在什么材料条件下怎样使用。当前默认单篇学习使用自由格式详细Markdown，不强制十维标题或固定规则数；旧风格文件继续可读。

### 1. 详细可执行的风格说明

说明围绕输入文章实际展示的视角、材料组织、叙述与判断、句段节奏、情绪及落点展开，保留适用条件和未知。学习分析、原文与证据独立保存；进入Writer的是执行说明。质量以原文依据和新题材执行判断，不以字数或模型总分替代。

多篇正文核心及候选／版本开发入口已实现，统一用户输入与子Agent调度尚未交付。最新状态见[PRD中的能力与缺口](docs/PRD.md)。

### 2. URL 一键风格逆向提取（Learn Pipeline）
看到优质好文想沉淀其写作风格？只需提供文章链接，系统自动完成提取：
```bash
# REPL 中直接输入：
> https://mp.weixin.qq.com/s/xxxxxx 学一下这个风格

# 或 CLI 模式：
cargo run -- learn https://mp.weixin.qq.com/s/xxxxxx
```
- **智能抓取**：基于 Jina Reader API（`r.jina.ai`）深度解析 SPA、反爬与微信公众号排版，自动降级纯文本清洗
- **详细学习**：默认`direct_free`使用`prompts/learn_style_direct.md`形成详细Markdown，候选经命名保存；历史400–600字与十维模板不作为当前产品目标。

### 3. 智能风格选择与双向匹配
- **拼音归一化精确匹配**：基于 `pinyin` 库支持中文名与拼音文件名的双向智能互通（如 `轻辩` ↔ `qingbian`），避免同音异字或大小写错位导致的匹配混乱
- **风格歧义处理**：未指定、未知或有歧义时进入风格选择，保留原写作需求；不自动采用唯一或最近风格。
- **交互式风格选择**：未指定风格时自动列出风格库摘要供选择，支持 `/styles` 或自然语言查看详情与管理

---

## 安装

需要 Rust 1.80+。

```bash
git clone https://github.com/khlilo-dve/cognitive-writer.git
cd cognitive-writer
cargo build --release
```

## 配置

创建 `.env`：

```env
API_KEY=your-api-key-here
BASE_URL=https://api.openai.com/v1       # 任何 OpenAI 兼容端点（如 DeepSeek / OpenRouter）
MODEL=gpt-4o
WEBSITE_PATH=/path/to/your/website       # 可选，个人网站项目路径（为空则跳过网站发布）
```

## 使用方式

### REPL 模式（推荐交互方式）

```bash
cargo run
```

进入交互终端后支持自然语言与多行输入（行末加 `\` 换行）：

```
> 写一篇关于认知偏差在投资中的应用，孙割风格 \
素材：认知偏差是投资中最大的隐性成本。 \
行为金融学研究表明，即使是专业投资者也无法完全避免这些偏差。 \
关键是建立一套系统性的纠正机制。

Agent: 匹配风格 → 生成大纲 → 渲染全文...
```

### CLI 快捷模式

适合脚本集成或单次任务：

```bash
cargo run -- generate          # 交互式选择素材与风格生成文章
cargo run -- learn <URL>       # 逆向学习文章风格并保存为 Spec
cargo run -- refine <FILE>     # 基于 <AI_EDIT> 标记局部重绘
cargo run -- update <FILE>     # 输入修改意见全篇重写
```

---

## 能力边界

| ✅ 能做 | ❌ 不做（by design） |
|---------|---------------------|
| 自然语言 → 结构化意图（LLM 意图分类） | 选题自动化推荐 |
| 单篇URL风格学习；多篇正文学习开发入口 | 无意义多轮闲聊 |
| 骨架（大纲）→ 渲染（正文）双通道生成 | 粉丝数据统计 / 定时发布 |
| 风格库拼音归一化精确匹配与管理 | Web 网页端 UI |
| 微信公众号富文本剪贴板注入（CF_HTML） | 第三方全平台自动矩阵分发 |
| 个人网站 MDX 自动化构建 + Git Push 部署 | 微信公众号官方后台 API 登录对接 |
| 会话持久化与异常中断恢复机制 | 英文版多语言机器翻译 |

---

## 架构设计

核心模块与测试数量以 `src/` 和 `cargo test --lib` 为准，内置生成端与风格提取端两套 LLM-Judge 评测。

```
src/
├── main.rs         # REPL/CLI 路由与启动分流器
├── repl.rs         # 异步 REPL 循环 + 4 状态状态机 + 会话持久化
├── intent.rs       # LLM 意图分类 + 关键词匹配 fallback
├── generate.rs     # 骨架 (Pass 1) → 渲染 (Pass 2) 双通道生成
├── learn.rs        # 单篇URL抓取与风格学习入口
├── styles.rs       # 风格库管理 + 拼音归一化精确匹配
├── eval.rs         # 生成端 (Rubric A) + 提取端 (Rubric B) 双核评测引擎
├── website.rs      # 个人网站 MDX 模板生成 + Git Push 发布
├── refine.rs       # <AI_EDIT> 局部精确重绘
├── update.rs       # 全文指令迭代重写
├── clipboard.rs    # 跨平台标准 CF_HTML 微信剪贴板格式注入
├── llm.rs          # OpenAI 兼容 API 客户端 + 泛型重试
├── io.rs           # 文件 I/O 与版本号递增管理
├── utils.rs        # 环境变量 / HTML 转换等公共工具
├── error.rs        # 强类型 AppError 枚举
└── lib.rs          # 库入口定义
```

完整架构与设计文档：

- [产品需求PRD](docs/PRD.md)
- [下一阶段行动计划](docs/plans/2026-11-03-style-learning-agent-plan.md)
- 系统架构说明：[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)
- 演进史与设计理念：[docs/EVOLUTION.md](docs/EVOLUTION.md)
- 评测体系架构设计：[评测上下文索引](.claude/context/evaluation.md)

---

## 📮 Hosted Version — Coming Soon

托管版正在筹备中：免配置 Rust 环境与 API Key，浏览器开箱即用。

开源版持续维护并保持免费。托管版感兴趣可联系：**ferkasybilla312@gmail.com**

---

## 更新日志

### v3.3.2 (2026-08) — 风格工程演进 + 自动化 Eval 评测体系
- **风格提示词重构**：全面落地 10 维度执行 Spec，消灭感性赏析词，确立负面禁止规则最高优先级
- **风格逆向提取升级**：外置提示词机制 + Jina Reader API 深度解析，支持微信文章 URL 一键逆向
- **拼音归一化精确匹配**：支持中文名与拼音文件名双向精准映射与口语化时间感知查询
- **双核 LLM-Judge 评测体系**：引入生成端（Rubric A 匹配度）与提取端（Rubric B 规范度）自动化评估套件
- **稳定性与交付加固**：Result 强类型错误传播，本地生成产物版本化归档，145 个单测全量通过

### v3.1.0 (2026-07) — 异步事件循环 + 会话持久化
- REPL 升级 tokio 异步 stdin + `select!` 事件循环，支持异常中断自动保存
- 会话持久化机制：每次交互自动写入 `~/.cognitive-writer/current.json`，启动时智能检测恢复
- 交互优化：多行输入（行末 `\` 续行）、终端行编辑能力恢复

### v3.0.0 (2026-07) — CLI → 对话式 Agent
- 自然语言 REPL 交互 + 4 状态对话状态机（Idle → WaitingForOutline → WaitingForFulltext → WaitingForPublish）
- LLM 意图分类替代传统纯关键词硬匹配
- 个人网站发布模块（MDX + Git Push）

### v2.1.0 (2026-07) — 模块化重构
- 6 模块解耦、thiserror 错误类型、泛型 API 重试与进度反馈

---

## License

MIT


## 可续跑自动评测

```bash
cargo test --lib
EVAL_RUN_ID=20260905-auto-v1 cargo test --lib eval_automated_suite -- --ignored --nocapture
```

网络恢复后原命令续跑，已经完成的请求直接复用。输入、Prompt 或模型变化时请换新的 `EVAL_RUN_ID`。本轮结果写入 `eval/runs/{run_id}/summary.md` 和 `summary.json`；历史基线不会被覆盖。目录规则见 [eval/AGENTS.md](eval/AGENTS.md)，人工校准仍须独立完成。
