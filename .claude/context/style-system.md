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

2026-11-02正文任务接入（10-31启动）：`style-draft learn/append/jobs/job/resume`接真实Pro学习与复读，旧import等命令仍离线。`learning_job.rs`冻结来源/提示词/方法/基准，`learning_transport.rs`保留逐阶段意图/响应及usage，未知请求仅显式最多三次；成功仅返回候选，不自动保存版本。append取回旧原文整体重学。研究适配器`research_learning_jobs.rs`共用产品编排和留证，真实四任务/16响应完成、无密钥恢复和0新请求复跑通过；278库测试通过/10忽略。`voice-core-v2`只增加完整Markdown外包装兼容，旧run不能覆盖。说明仍有身份泛化、条件路线变硬禁令等问题；保存不等于质量通过，文件URL和默认learn/REPL替换仍后置。详见docs/2026-10-31-learning-jobs.md与research/style-aggregation/reports/learning-jobs-2026-11-02.md。

2026-11-02方法28：`voice-reviewed-v3`将规范排序完整原文加入修订输入，仍隔离旧analysis；四份学习提示词增强当篇缘由/表达关系/自检一致性。完整旧任务先找回不可变候选，未完成旧方法仍拒绝静默切换。279库测试通过/10忽略，本地HTTP与17终端回归通过。真实新任务因TLS失败0响应，10稿依赖暂缓；四份旧稿的21处实现者有据派生不冒充自动质量验证。新研究入口`research learning-quality-probe <run> <source-run>`来源未完整则拒绝发请求；资料与恢复顺序见docs/2026-11-02-source-grounded-revision.md。

2026-11-02方法28后续真实恢复：原冻结四任务16份Pro响应及10篇Flash用稿已完整，来源/版本和纯spec输入审计与零请求复跑通过。说明部分条件化改善，但单篇第一人称功能/身体细节范围误判仍在；10稿有具体事实越界且中性也有，不能据此宣布Writer就绪。见research/style-aggregation/reports/learning-quality-recovered-2026-11-02.md；不把前一条TLS失败当当前结果，也不改写历史失败。

2026-11-02方法29：voice-core-v6分析内部使用原生JSON承载analysis和evidence（article/quote），逐条验证来源/连续原字及逐篇覆盖。最多一次analysis-repair，附具体错误与真实原文候选片段；提示不放宽逐字门槛，失败不编译/保存。编译仅接analysis正文与篇数，证据/原文不进入Writer，详见docs/2026-11-02-grounded-style-analysis.md。四任务17Pro响应和四Flash边界稿已完成、来源/版本/纯spec与零请求恢复已核验，282库测试/10忽略；目标表达边界能执行，但四稿仍有事实/推理问题，候选仍待审。原文直入编译、错文缓存和两双篇失败分支完整保留。后续不追加方法分支，围绕style-draft接文件/URL输入与1–10篇澄清，Writer事实独立修正；未替换旧默认入口。

2026-11-03产品设计更新（未实现）：当前PRD为docs/PRD.md，行动计划为docs/plans/2026-11-03-style-learning-agent-plan.md。主Agent仅在学习／追加意图分派风格学习子Agent，子Agent处理输入、工具与有边界澄清；已有风格的使用另走写作。默认澄清1轮／最多2轮和决策预算为待P1走查的设计值，不能描述为当前代码。正文核心、候选与版本继续复用；先更新产品与技术设计再按授权编码，本轮仅文档。

2026-11-11 P5开发入口：style-draft chat在P4材料Agent之后调用learning_flow，绑定task/revision→冻结learning_job→候选/版本，复用原Pro学习核心。保存同候选幂等；无名先交候选，明确组合需求可直接Flash写作；/preview、/diff、/save、/versions、/show、/use-version、/use-candidate支持复看与控制。Writer只接纯spec与独立新写作要求，逐阶段回执与未知请求显式重试共用learning_transport，输出进入旧全文审阅态。首尾粘贴可界定正文；DOCX/PDF仍P6。327库测试/10忽略及11真实响应/零请求恢复通过，但新稿有事实补写，P7/P8未完成；见PRD、设计第11节与learning-flow-2026-11-11报告。旧默认入口及styles/.env未切换。

2026-11-12 P6开发入口：learning_document分流DOCX/文字PDF，learning_docx保留可见正文与标题，learning_pdf逐页提取并适配明确UCS2编码；扫描/缺字映射/混合扫描不交部分正文。source_binary保留原文件blob与哈希，派生纯文本仍由P3处理；旧集合可缺省新字段，恢复仍可核验。learning_input_limits在选文和job准备时约束总正文256KiB，采集仍保存完整8MiB内来源；不是输出长度或质量指标。10本地场景/333库测试通过，0新增API费用；详见PRD/设计12及learning-formats报告，P7语义/事实、P8正式采用仍待办。


2026-11-12 P7/P8现行状态：显式`style-draft chat`已从混合TXT/MD/DOCX/文字PDF/正文/URL接到1–10篇学习、待审候选、显式保存/追加版本、指定旧版Writer和中断恢复。新Writer用Flash大纲/正文、Pro独立事实首审，最多一次Flash修订后Pro复核；首审有问题且原段不变时额外阻断。`/writer-review`在学习等待态和全文审阅态只读查看，不发模型请求。历史已完成Writer原回执可读但未经新事实核对；未完成旧Writer不静默升级。P7两作者10篇真实候选和15条重要概括核对完成，旧单/双篇有必要化/否定反转/穷尽结尾问题，Writer用稿仍有事实漏检与任务检查口吻。P8全链工程已验收，当前仅限定范围采用显式工作区；默认旧`learn`/REPL及`styles/`不自动切换。需求/采用决定见`docs/PRD.md`的D08，使用见`docs/style-learning-workspace-guide.md`，证据见`research/style-aggregation/reports/product-acceptance-p8-2026-11-12.md`。本段为现行事实，上方历史段保留原阶段语境。
