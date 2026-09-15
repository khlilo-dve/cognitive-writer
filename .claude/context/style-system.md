# 风格系统与逆向学习

风格保存为 `styles/*.md` 的 Markdown 文本，运行时直接注入生成提示词；不存在 StyleSpec 解析结构体。十节提取规范由 `prompts/learn_style.md` 定义，历史风格文件不强制回写为同一格式。

## 学习与保存
`learn::extract_style(url)` 抓取单篇 URL（Jina Reader，失败降级直接抓取去标签），读取提取提示词并调用 LLM，只返回文本，不读 stdin。
- CLI `run_learn` 包装交互命名，/cancel 取消。
- REPL 收到提取结果后进入 WaitingForStyleName，保存待命名文本，再由主循环读取名称。重名或非法名称返回错误并保留提取结果。
- `styles::save_new_style` 使用 create_new 防覆盖；中途写入失败清理本次新建半成品。
- 删除风格由 REPL 先保存待删除名称，下次明确确认后调用 `delete_style`。

## 规范与校准
提取要求跨主题可迁移；既禁止源句引用，也禁止把源文论点、事件或动作链换词写成规则。保留语域、视角、语气与节奏特征，避免过度抽象成通用套话。目标为含标题、标点及 Markdown 的非空白 Unicode 字符 400–600；长度分数由代码计算，详见 evaluation.md。

## 代码职责
- `styles.rs`：文件列表、查找、读取、创建与删除。
- `style_names.rs`：名称路径校验、拼音归一化、摘要提取。
- `learn.rs`：抓取及提取阶段、CLI 命名包装。
- `generate.rs`：将风格 Markdown 注入大纲与正文。
