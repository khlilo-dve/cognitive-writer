# 文章生成与意图调度

## 当前生成流程
`Generate → 明确风格或 WaitingForGenerationStyle 追问 → generate_outline → render_fulltext → WaitingForFulltext`。
首次输入的主题与原始写作要求同时保留并传入大纲和正文调用；追问只补充风格，不丢弃篇幅、受众等要求。
大纲与正文分别调用模型，使用同一份 Markdown 风格规范；当前自动从大纲衔接正文，不要求用户确认大纲。CLI 也复用这两组底层函数，并保留 spinner。

`WaitingForOutline` 现在是内部检查点。常规生成仍自动衔接正文；恢复时可确认已有大纲继续，缺失内容时明确报错且允许取消。换风格请求成功后才一起更新正文和风格元数据。

## 意图解析
- 兼容入口位于 `src/intent.rs`；LLM 分类在 `intent_classify.rs`，确认/取消在 `intent_quick.rs`，历史关键词提取仅用于测试；不存在 `IntentParser` 类型。
- REPL 对明确确认/取消做快速预检，其余输入调用 `classify_intent`；分类 prompt 携带状态与有效意图。
- 当前 `parse_intent` 仅解析确认/取消，其余返回 Unknown。分类调用失败时明确提示服务暂不可用；不宣称复杂意图可离线降级，用户仍可使用 /help、/styles 等本地命令。
- 风格选择、局部修改位置选择、命名与删除确认在 REPL 本地处理，四个等待态的分类入口会直接拒绝请求。
- `ModifySection` / `ChangeStyle` 由 REPL 内部处理正文修改；文件版局部/全文重写分别在 `refine.rs` / `update.rs`。

## 源码指针
- 两阶段 Prompt 与调用：`src/generate.rs`。
- 状态分派与审阅：`src/repl.rs::dispatch`。
- 意图分类及解析：`src/intent.rs`。

## 首次生成风格追问（2026-09-15）
- Generate 的 topic 必需，style_name 允许为空。分类器不得猜测风格，名称必须来自原输入；缺失风格不再作为缺参错误。
- `style_selection.rs` 区分准确名称（含中文/拼音归一化唯一匹配）与描述性候选。后者、未知名称、多候选、仅提及最近风格都会追问；即使只有一个候选也不自动选择。
- `WaitingForGenerationStyle` 接收名称、编号、“用某某风格”、列表查询与取消；确认词不会代替风格选择。/styles 刷新全部候选；空库提示先学习或导入，文件读取失败保留实际错误。
- 候选名称列表随会话保存，恢复时编号沿用原列表；选择时按确切文件名重新读取，不受目录新增文件重排影响。/quit、Ctrl+C、EOF 保存；/cancel 清除待生成需求。
- 选择后直接生成，不追加确认。不扩展 CLI 选择器或正文换风格流程。
- 验证：`scripts/verify_style_selection.py`，结果记录见 `docs/2026-09-15-style-clarification.md`。

## 局部修改范围保护（2026-09-16）
- `edit_blocks.rs` 按 Markdown 原文偏移索引顶层内容块。正文自然段单独计数，标题、列表、引用、代码和表格另列 B 编号；不重新渲染原文。
- 明确正文段落序号、标题下段落、唯一原文引用可直接定位；重复原文、模糊范围、包含排除/参考关系的复杂指令进入 `WaitingForEditTarget`，本地选择 B 编号。/blocks 展示位置，/cancel 只取消本次修改而保留文章。
- `local_edit.rs` 将完整原文、索引、允许编号、风格与用户原始修改指令交给模型，仅接受 edits 数组；程序按偏移拼接原文。额外/重复/缺失编号、空替换、无效 JSON、Markdown 边界外溢均拒绝，多个目标要全部有效才应用。
- 替换内容使用 Markdown；原始 HTML 替换会被拒绝，避免跨块影响。空替换不作为删除整块命令处理。
- 成功后保留一份修改前正文，/undo 可撤销且支持重启恢复。标题未改时文章 slug 保持原样；错误或中断不提交正文变更。
- `scripts/verify_local_edit.py` 使用完整原文断言及字节拼接比较，验证流程与边界，不评价真实模型改写质量。
