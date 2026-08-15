# v3.3.2 迭代设计：稳定性 + 交付完整性 + 两套 eval

## 状态

已实现（2026-08-15）。

## 范围

### 做

1. **舍弃「不编造素材外事实」约束**：删掉 `style_system_prompt` 和风格 spec 禁止段里的「不添加素材中没有依据的事实」，改为「可适当补充有深度的案例、数据、反例，保持自洽、合理、不矛盾」。
2. **`run_learn`/`run_update` 返回 Result**：REPL 学风格/重写文件失败不再 `process::exit` 杀进程。
3. **本地归档落盘**：`handle_draft_publish`/`handle_publish` 写 `outputs/{slug}_v{N}.md/.html`，公众号文章有本地原件。
4. **提取 `utils.rs`**：收敛 `env_var`/`env_var_or`/`md_to_wechat_html` 三份副本；移除 `clap`、`AppError::InvalidState`、`ModifyOutline` + `handle_modify_outline` 死代码。
5. **intent 分类 prompt 加 few-shot**。
6. **两套 eval**（见下）。

### 不做（明确舍弃 / 暂缓）

- **方案 D（生成后风格审查/重写闭环）——舍弃不改。**
- 双通道合并、日志分级、learn 两阶段 stdin 重构——暂缓。
- `run_generate`/`run_refine`（CLI 专用）保留 `process::exit`——CLI 模式下退出是正确行为。

## 两套 eval 设计

### eval 1：风格提示词（生成端）

目的：验证风格 spec 注入大纲+正文后，生成文章是否贴合风格。

- 输入：同一主题 + 同一风格 spec（qingbian）。
- 指标（具体数据）：
  - 正文字符数 / 大纲字符数
  - 平均段落字数（风格要求短段）
  - 禁用 AI 套话检出数（众所周知/总而言之/值得注意的是…）
  - 反问句数量
  - LLM-judge 风格贴合度（1–5 分）

### eval 2：learn 提取提示词（提取端）

目的：验证提取提示词产出的 spec 是否精简、可执行。

- 输入：一篇风格鲜明的文章。
- 指标（具体数据）：
  - spec 字符数（目标 ≤600）
  - 10 段覆盖率（定位/标题/开头/结构论证/段落句式/语气人称/用词/结尾/排版/禁止）
  - 原文引用检出数（应为 0）
  - 平均每段字数

两者都通过 `cargo test --lib -- --ignored eval --nocapture` 手动运行，输出可对比的数字。
