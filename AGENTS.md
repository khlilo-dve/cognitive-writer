# Cognitive Writer — 开发规范与核心规则

每次完成版本变动或代码修改后，都必须进行 `cargo test --lib` 验证，检查 `git diff`，并完成 `git add` 和 `git commit`。

## 项目概述
- Rust 对话式写作 Agent (v3.3.2)，面向微信公众号 + 个人网站 (khlilo.xyz)
- 源文件与测试数量以 `src/` 和 `cargo test --lib` 为准；REPL 自然语言交互 + CLI 快捷模式
- 常规状态流程: `Idle` → `WaitingForFulltext` → `WaitingForPublish`；`WaitingForOutline` 枚举仍保留，当前生成自动衔接大纲与正文；旧状态恢复待完善

## 运行方式
- REPL 交互模式: `cargo run`
- CLI 快捷指令: `cargo run -- generate|learn|refine|update [...]`
- 全量库测试: `cargo test --lib`
- 单模块测试: `cargo test -- <module_name>`

## 核心开发规范
- **模块入口**: 新模块必须在 `src/lib.rs` 注册；`src/main.rs` 仅作为启动分流器（终端进入 REPL，参数/管道进入 CLI）
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

## 技术债简要
- `run_learn` 在 REPL 下阻塞读 stdin 与 REPL 异步循环冲突，待两阶段对话改造
- `handle_delete_style` 仍有同步 `stdin.read_line()`，待异步化
- CLI/REPL 底层生成已统一；剩余：公开 String 错误类型迁移、intent/styles 模块规模治理，详见当前计划。

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
