# 文章生成与意图调度 (generation.md)

## 两阶段生成流水线 (Two-Stage Pipeline)

为了彻底避免“一步到位直接写正文导致跑题或无法掌控结构”的问题，系统强制使用两阶段流水线：

```
[用户输入主题/选题]
        │
        ▼ 阶段一：大纲推导 (Outline Stage)
   generate_outline()
        │
        ▼
   [大纲确认态: WaitingForOutline] ──(用户提出大纲修改要求)──┐
        │                                                    │
        ▼ (用户明确确认大纲)                                  ▼
   阶段二：正文渲染 (Fulltext Stage)                    重新调整大纲
   render_fulltext() (结合所选 StyleSpec 展开各章节论据与案例)
        │
        ▼
   [正文审阅态: WaitingForFulltext]
```

## 意图解析系统 (`src/intent.rs`)

用户在 REPL 中的自然语言输入，不直接发送给大模型，而是先经过本地快速规则与意图提取器：

- **`Intent` 枚举**: 包括 `Generate`, `Confirm`, `Modify`, `Refine`, `Update`, `Learn`, `SelectStyle`, `Help`, `Exit` 等。
- **意图判定逻辑**:
  - `IntentParser::parse(input, current_state)`: 结合**当前状态机状态**来解析相同字词的不同含义。例如在 `WaitingForOutline` 状态下输入“好的/可以”，解析为 `Confirm`（触发渲染正文）；而在 `Idle` 状态下可能作为普通闲聊或提示无效。
  - 49 个单元测试保障了各种口语化中文意图的鲁棒性。

## 文章演进与重写模块

- **局部微调 (`src/refine.rs`)**:
  - 针对正文中特定段落/论点的重写，只替换局部内容，保持全文其他上下文不变。
- **全文重写 (`src/update.rs`)**:
  - 保持现有主题与大纲基本框架，依据新的调优提示进行全篇深度重绘。

## 何时需要查阅代码
- 扩展或调整中文自然语言匹配规则：查阅 `src/intent.rs`。
- 调整大纲与正文生成提示词工程及参数：查阅 `src/generate.rs` 与 `src/llm.rs`。
- 修改局部替换或全文更新逻辑：查阅 `src/refine.rs` 与 `src/update.rs`。
