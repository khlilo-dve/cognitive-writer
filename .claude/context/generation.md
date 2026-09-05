# 文章生成与意图调度

## 当前生成流程
`Generate → generate_outline → render_fulltext → WaitingForFulltext`。
大纲与正文分别调用模型，使用同一份 Markdown 风格规范；当前自动从大纲衔接正文，不要求用户确认大纲。CLI 也复用这两组底层函数，并保留 spinner。

`WaitingForOutline` 仍存在于枚举、标签和历史资料中，但常规生成流程不进入，当前 dispatch 也没有该状态确认分支。不要将其描述为已可用的大纲修改能力；历史恢复的处置纳入状态机改造方案。

## 意图解析
- 入口位于 `src/intent.rs`，具体枚举以 `Intent` 为准；不存在 `IntentParser` 类型。
- REPL 对明确确认/取消做快速预检，其余输入调用 `classify_intent`；分类 prompt 携带状态与有效意图。
- 当前 `parse_intent` 仅解析确认/取消，其余返回 Unknown。分类调用失败时明确提示服务暂不可用；不宣称复杂意图可离线降级，用户仍可使用 /help、/styles 等本地命令。
- `ModifySection` / `ChangeStyle` 由 REPL 内部处理正文修改；文件版局部/全文重写分别在 `refine.rs` / `update.rs`。

## 源码指针
- 两阶段 Prompt 与调用：`src/generate.rs`。
- 状态分派与审阅：`src/repl.rs::dispatch`。
- 意图分类及解析：`src/intent.rs`。
