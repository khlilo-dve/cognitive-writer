# Cognitive Writer — 开发规范与核心规则

每次完成版本变动或代码修改后，都必须进行 `cargo test --lib` 验证，检查 `git diff`，并完成 `git add` 和 `git commit`。

## 项目概述
- Rust 对话式写作 Agent (v3.3.2)，面向微信公众号 + 个人网站 (khlilo.xyz)
- 源文件与测试数量以 `src/` 和 `cargo test --lib` 为准；REPL 自然语言交互 + CLI 快捷模式
- 常规状态流程: `Idle` → `WaitingForOutline`（内部检查点）→ `WaitingForFulltext` → `WaitingForPublish`；大纲自动衔接正文，中断后可确认已有大纲继续；未明确风格时先进入选择等待态，学习命名、删除确认另有等待态

## 运行方式
- REPL 交互模式: `cargo run`
- CLI 快捷指令: `cargo run -- generate|learn|refine|update [...]`
- 全量库测试: `cargo test --lib`
- 单模块测试: `cargo test --lib <module_name>`
- 真实终端回归（Linux/WSL，无真实 API）：`cargo build --bin cognitive-writer && python3 scripts/verify_repl_terminal.py`
- 首次生成风格追问回归：`python3 scripts/verify_style_selection.py`（先构建二进制）。
- 局部修改范围回归：`python3 scripts/verify_local_edit.py`（先构建二进制）。

## 核心开发规范
- **模块入口**: 新模块必须在 `src/lib.rs` 注册；`src/main.rs` 仅作为启动分流器（无参数进入 REPL，有子命令参数进入 CLI；stdin 可为终端或管道）
- **错误处理**: 公开函数必须返回 `crate::error::AppError`，禁止私自使用裸 `String` 作为错误类型
- **测试约束**: 单元测试必须内嵌在模块底部的 `#[cfg(test)] mod tests` 中；文件 I/O 测试一律使用 `tempfile`，严禁污染工程真实目录
- **单一职责**: 每个模块控制在 500 行以内，`src/repl.rs` 作为状态机与交互编排层可除外
- **代码变动**: 严禁注释报错或添加未解决的 bypass 标记；必须解决根本问题

## 环境变量 (.env)
- `API_KEY` (必填), `BASE_URL` (默认 openai), `MODEL` (默认 gpt-4o)
- `WEBSITE_PATH` (可选，个人网站本地工程根目录，配置后支持自动推送到 Astro 博客)

## Context Engineering 规则（防 Token 膨胀）
本仓库实行分层知识架构，严禁盲目读取整个项目或将长篇业务知识硬塞入此文档：
1. **稳定规则** → 本文件 (`AGENTS.md`)，负责核心纪律与开发铁律。
2. **领域与架构知识** → `.claude/context/`。Codex 与 Claude 共用此目录，不建立重复的上下文树。必须**按需加载**，严禁一次性全量读取全部 context：
   - 寻找上下文前先读 `.claude/context/manifest.md`（索引与场景路由表）。
   - 根据当前具体任务，仅读取对应的一份 context 文件。
3. **短期任务状态** → `.claude/tasks/current.md`（当前状态）与 `plan.md`（实施计划）。
4. **历史操作经验** → 由 `Codex-mem` 自动记录与检索。
5. **代码实现事实** → `src/` 下的具体代码，Context 仅是指针，真实逻辑以代码为准。

## 当前实现与后续观察
- REPL 单一 stdin 读取器位于 `repl_input.rs`；学习命名、删除确认由主循环处理，等待态支持取消/保存恢复。
- CLI/REPL 生成已统一；公开 I/O/解析/环境错误已迁移为 AppError，intent/styles 职责已拆分。
- 三组人评与确定性字数分档见 `eval/human_calibration.json`、`eval/AGENTS.md`；新提示词仍需以实际评测结果判断效果，不能把模型分数当人评。
- 首次 Generate 风格缺失、未知或有歧义时进入 WaitingForGenerationStyle；保留原始需求，按名称/编号选择后生成，支持取消与保存恢复。不自动选择唯一/最近风格；描述性匹配仅供候选参考。

- REPL 局部修改使用完整文章作为模型上下文，只接收目标块替换内容；程序按原文偏移应用并校验范围外内容。位置不明确进入 WaitingForEditTarget，/cancel 仅取消本次修改；/blocks 查看块编号，可直接说“撤销刚才的修改”“恢复修改前”“改回去”撤销上次局部修改，/undo 保留兼容。

- 单篇学习默认 `direct_free`（自由格式，不强制十维）；`STYLE_LEARNING_METHOD` 可显式选择 legacy / direct_free / two_stage。MiMo v10同模型开发与新主题确认均通过既定实用门槛，五份已验证风格独立入库，原风格保留。校验与Rubric B v2仍按400–600非空白字符满分、低于最低扣分；阶段留证在 outputs/style-learning/。结果见 docs/2026-09-22-mimo-style-release.md。模型评分不等于人评或普遍优胜；DS历史分数未用于此次采用。

- 当前迁移评测使用 Transfer v2：风格保真、原文独立性、事实合规、自然度、主题完成度独立报告；字数程序统计，不合成采用总分。入口 `eval_transfer_audit_suite`，方案见 docs/superpowers/specs/2026-09-18-transfer-eval-correction.md。

## 目录与产物约定
- `src/`：扁平 Rust 模块；新模块在 `src/lib.rs` 注册，测试内嵌模块底部。
- `eval/`：冻结样本、候选产物和评测报告；具体命名、版本与续跑规则见 `eval/AGENTS.md`。不得用候选生成覆盖冻结基线。
- `.claude/tasks/`：`current.md` 保存事实状态，`plan.md` 保存实施步骤及需人工确认项，任务结束后更新为实际结果。
- `docs/`：长期设计与演进记录；历史内容需标记历史状态，不能把已完成事项继续列为当前待办。

## Agent 工作纪律与 Git 闭环
1. **先看再改**: 修改前必须先阅读目标文件及调用方，严禁无事实依据地猜测代码。
2. **最小阅读原则**: 只读取当前任务紧密相关的代码行/模块；绝不无节制 grep/read 整个仓库。
3. **完成标准 (Definition of Done)**:
   - 代码编写符合 Rust 规范并通过 `cargo test --lib`
   - 检查 `git diff` 确保无临时文件、误修改或无关代码
   - 执行 `git add` 与 `git commit`（严禁添加 `Co-Authored-By` 等 AI 协同签名）
   - 不能以“代码已修改但未提交”作为任务终态。
4. **自主红线（不可擅自操作）**:
   - 严禁执行 `git reset --hard`、`git push`、强制推送、删除分支或改写 git 历史
   - 严禁擅自修改 `.env`、密钥、以及生产配置文件
