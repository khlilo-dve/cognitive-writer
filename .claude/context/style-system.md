# 风格系统与逆向学习

风格保存为 `styles/*.md` 的 Markdown 文本，运行时直接注入生成提示词；不存在 StyleSpec 解析结构体。执行 Markdown 不限制标题、数量和顺序。默认使用自由格式单阶段 `direct_free`，2026-09-26按用户要求改为详细说明；历史紧凑版MiMo v10通过过开发与新主题确认，不能据此宣称本次详细版已完成对照；冻结单阶段与两阶段保留显式选项。历史风格不回写。

## 学习与保存
`learn::extract_style(url)` 抓取单篇 URL（Jina Reader，失败降级直接抓取去标签），按 `STYLE_LEARNING_METHOD` 提取，最终返回纯 Markdown，不读 stdin。`legacy` 为冻结旧单阶段；默认 `direct_free` 使用 `prompts/learn_style_direct.md`；`two_stage` 先用 `analyze_style.md` 分析可定位摘录，再只传规则/条件/支持程度给 `learn_style.md` 编写。
- CLI `run_learn` 包装交互命名，/cancel 取消。候选每阶段最多一次诊断修复；旧方法对格式错误最多修复一次、长度偏差直接警告；失败不进入命名。
- REPL 收到提取结果后进入 WaitingForStyleName，保存待命名文本，再由主循环读取名称。保存前（含恢复后）校验执行文本。重名或非法名称返回错误并保留提取结果。
- 学习阶段请求与输出保存在 `outputs/style-learning/{id}/`，不含端点和密钥；只有执行文本进入正式风格，分析不注入写作。
- `styles::save_new_style` 使用 create_new 防覆盖；中途写入失败清理本次新建半成品。
- 删除风格由 REPL 先保存待删除名称，下次明确确认后调用 `delete_style`。

## 规范与校准
提取要求跨主题可迁移；既禁止源句引用，也禁止把源文论点、事件或动作链换词写成规则。保留语域、视角、语气与节奏特征，避免过度抽象成通用套话。建议含标题、标点及Markdown的非空白Unicode字符2000–5000，1500为完整度下限；不足时最多一次有证据的补充，仍短则明确提示，不凑字。超过5000不自动压缩、不因长度扣分或拒绝保存。空内容、仅标题、JSON/围栏仍拒绝进入命名；CLI与REPL（含恢复后）共用。新长度分按detailed-v1，旧400–600分档只用于历史校准。详见 evaluation.md。

## 代码职责
- `styles.rs`：文件列表、查找、读取、创建与删除。
- `style_names.rs`：名称路径校验、拼音归一化、摘要提取。
- `learn.rs`：抓取、阶段留证、CLI 命名包装。
- `style_learning.rs`：三个提取入口、证据校验、摘录隔离、编写与有界修复；`spec_quality.rs`：自由格式机械校验与长度评分。
- `generate.rs`：将风格 Markdown 注入大纲与正文；区分核心偏好/条件策略，当前任务明确要求与事实边界优先。

当前采用状态（2026-09-25）：direct_free已成为CLI/REPL共用默认，显式legacy/two_stage继续兼容。五份新Spec以自由直评拆解、自由白描、自由系统解说、自由概念辨析、自由冷面杂文独立入库；旧风格不覆盖。开发与确认均在mimo-v2.6-pro完成，不混用DS分数；结果与局限见docs/2026-09-22-mimo-style-release.md。后续运行模型切换不改写这批评测的模型归属。

MiMo适配：已知MiMo v2.5/v2.6系列的direct_free/two_stage结构化学习启用原生JSON模式，legacy纯Markdown保持普通文本。阶段审计记录response_format。长度修复仅补充过短说明，仍最多一次；不压缩有效长规则。自由格式不代表免除格式、长度、素材与事实边界校验。

详细版需求与实际验证：docs/2026-09-26-detailed-style-learning.md。参考孙割文件的说明颗粒度，不复制其题材或固定栏目。

2026-10-29开发核心（不替换上述默认入口）：`voice_learning::learn_voice_with`接受1–10篇正文，规范排序、去重、完整分析与执行说明分层；`voice_revision::revise_voice_with`核对原文身份，再用原文复读与范围修订输出待审草稿。内部分析和复读不注入Writer；方法不依赖固定栏目、字数或规则数量。模型调用和留证由调用方注入，当时尚无正式多篇CLI/REPL输入和版本管理。接口、外包装兼容、缓存恢复和仍有的语义/事实缺口见`docs/2026-10-29-voice-scope-revision.md`与研究报告；不要把候选生成/恢复成功当作自动采用。

2026-10-31新增`LearningWorkspace`与`style-draft`开发入口，接已有核心的正文/候选包，不发模型请求。支持import、preview、candidates、list、diff、save、versions、show、export；默认用户目录`.cognitive-writer/learning`，可显式--root。候选与命名版本分开，保存状态仍非质量认证；追加须引用准确旧版本并保留旧正文，过期候选不能覆盖新版。只显式export指定版本spec到新.md文件，拒绝覆盖，不把分析、复读或原文注入Writer。存储、来源/版本身份与并发写入分别在`learning_candidate.rs`、`learning_workspace.rs`、`learning_workspace_store.rs`；逐行差异在`learning_diff.rs`，CLI在`learning_workspace_cli.rs`。使用与验收见`docs/2026-10-31-learning-workspace.md`。多篇真实模型交互与文件/URL取得正文仍未接入旧learn/REPL，10-29的质量缺口保留。
