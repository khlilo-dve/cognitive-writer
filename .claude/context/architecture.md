# 架构与系统分层 (architecture.md)

## 总体系统拓扑

```
[Terminal Stdin / CLI Args]
           │
           ▼
     src/main.rs (分流器: TTY 进 REPL，参数/管道走 CLI 子命令)
           │
           ▼
     src/repl.rs (异步事件循环 + 4状态机 + 意图分发)
           │
     ┌─────┼─────────────┬──────────────┬─────────────┐
     ▼     ▼             ▼              ▼             ▼
generate  learn        refine         update        eval
 (生成)   (风格学习)   (局部重绘)     (全文重写)    (评测引擎)
     │     │             │              │             │
     └─────┴─────────────┼──────────────┴─────────────┘
                         ▼
             [底层通用支撑服务层]
       llm.rs        styles.rs      clipboard.rs
       website.rs    io.rs          error.rs
```

## 核心状态机 (src/repl.rs)

REPL 会话由 4 个核心状态驱动：

1. **`Idle` (空闲态)**: 等待输入主题/灵感，触发生成大纲并自动渲染正文。
2. **`WaitingForOutline` (历史枚举)**: 常规流程不进入；dispatch 未实现该状态确认分支，旧会话兼容处置待方案确认。
3. **`WaitingForFulltext` (正文审阅态)**: 正文已生成，可要求局部修改 (`refine`)、重写 (`update`) 或确认通过。
4. **`WaitingForPublish` (发布确认态)**: 正文通过，询问是否推送至个人网站博客并复制到剪贴板。

## 关键文件与职责边界

- **`src/main.rs`**: 保持极致精简，仅做环境判断与分流，不包含业务逻辑。
- **`src/lib.rs`**: 库的唯一导出入口，暴露所有公共模块供 `main.rs` 和单元测试调用。
- **`src/repl.rs`**: 最核心的编排层，管理 `SessionState` 和用户交互。
- **`src/error.rs`**: 全局 `AppError` 错误枚举。任何跨模块暴露的函数必须使用 `Result<T, AppError>`。

## 何时需要查阅代码
- 需要修改或增加新的交互状态：查阅 `src/repl.rs` 中的 `SessionState` 枚举与状态流转分支。
- 需要暴露新的系统服务：查阅 `src/lib.rs` 与 `src/error.rs`。
