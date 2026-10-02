# 评测体系与可续跑评测

## 多文章作者风格研究（2026-09-28）
独立入口在`research/style-aggregation/README.md`，协议/阶段/预算以该目录为准；不把历史单篇评测当作多文章结论。P1已验证24篇网页清单、4篇独立详细Style、费用与零新请求复跑；P2正在补齐单篇、重复提取和逐维比较。

`research_run.rs`提供research validate/extract/views/pairs；`research_dataset.rs`做来源SHA与划分隔离；`research_store.rs`共用原HTTP传输并增加显式模型角色、最多3并发、跨run已验证复用；`research_budget.rs`维护全研究与试跑共享上限，失败预留不清零；`research_receipt.rs`校验响应哈希/usage；`style_compare.rs`保留未知与冲突、计算逐维一对一重合，缺失维度为NA。新数据与图表都在research目录，不覆盖eval冻结产物。飞书是可读投影，不替代本地检查点。

后续研究实现（2026-10-01，尚待实际实验）：`style_profile.rs`校验原文证据并按文章计数；`research_profile.rs`独立构建冻结子集；`research_convergence.rs`做相邻/全量/重复/跨作者比较；`research_generation.rs`固定Flash生成48+12篇；`research_evaluation.rs`复用Transfer v2并分reference/confirmation解封，`research_judgment.rs`验证匿名作者匹配引用。实施细则在research目录P3-P5-implementation.md。实验控制失败即停，不把代码完成当作研究结论。

当前更新（2026-09-26）：Rubric B v3采用detailed-v1长度充分度，至少1500满分、不因更长扣分，建议2000–5000；固定JSON字段conciseness保留兼容。当前自由格式发布入口登记free-release-v2-detailed，旧run与人评继续使用冻结规则。以下400–600与v2评测为历史记录；详细版尚无新的在线质量对照，见docs/2026-09-26-detailed-style-learning.md。


## 两套独立评测
- Rubric A：生成文章与输入风格 Spec 的匹配度；破题、论证、节奏、人称、用词、结尾六维 1–5 分。只评最终正文，素材与 Spec 单独提供。
- Rubric B：样文提取出的 Spec 质量；有证据的重要机制覆盖（v2）、可执行性、精简度、特征捕捉、去素材污染五维 1–5 分。不是文章通用质量评测。
- JSON 分数必须在 1–5 之间，均分由代码重新计算。模型评分与人工黄金标准分开记录。

## 实现与产物
- `src/eval.rs`：冻结主题、Judge 解析与验证、历史归档正文提取、底部 ignored 入口。
- `src/eval_store.rs`：请求快照、检查点、请求漂移检测、错误脱敏。每个成功请求独立保存，中断后可复用；输入变化必须新开 run_id。
- `src/eval_runs.rs`：A/B 基线重评、候选文章与 Spec 对比、5×5 交叉评测、JSON/Markdown 汇总。
- 目录与命名规范见 `eval/AGENTS.md`。历史输入只读，新结果在 `eval/runs/{run_id}/`；JSON 保存输入和结果，Markdown 是便于阅读的正文/Spec 副本。
- 2026-09-05 旧候选仅作实验保留；用户三组校准后已授权修正现行提取/Judge Prompt，新实验使用独立运行目录。
- `spec_quality.rs` 以非空白 Unicode 字符数确定精简度，`eval_calibration.rs` 只处理三组人评；人评、历史模型和新模型原值分别保留。
- `eval_batch.rs` 保留旧批量 API，经 eval.rs 重导出；新任务优先用检查点入口。

## 运行
```bash
# 离线
cargo test --lib
# 完整在线运行；断网后原命令续跑
EVAL_RUN_ID=<新的运行ID> cargo test --lib eval_automated_suite -- --ignored --nocapture
```
可单独运行：`eval_judge_baseline_suite`、`eval_judge_learn_baseline_suite`、`eval_style_compare_suite`、`eval_learn_compare_suite`、`eval_all_specs_on_eval1_suite`。相同 run_id 共享检查点，不同时运行相同入口或同名任务。

## 解释边界
- 历史 Baseline 缺少完整生成模型/Prompt 元数据，与候选的分差不能单独归因于本轮改动。
- 5 篇单次胜率只用于发现问题，不代表统计显著改善。
- 字数、执行文本机械有效性、连续 12 字原文重合单独作为机械检查线索，不伪装成人工判断。
- 人工已评 Spec 01–03，不要求补评 04–05；Spec 02 候选是用户接受旧模型分数，不是完全独立的人评。新候选暂无人评，模型实验不等于人工验收。

## 三组校准与长度政策
```bash
MODEL=deepseek-v4-flash EVAL_RUN_ID=20260912-calibration-v3-fixed EVAL_CONCURRENCY=3 EVAL_TIMEOUT_SECONDS=180 cargo test --lib eval_calibration_suite -- --ignored --nocapture
```
该入口重评6个旧产物、提取3个新Spec并评分，共12个请求检查点；不重跑25篇交叉生成。并发可选1–3（默认3），单请求超时30–900秒（默认180）。运输设置不改变模型/提示词请求快照，可续跑同一run。

精简度5分400–600；4分300–399或601–700；3分200–299或701–850；2分100–199或851–1000；1分不足100或超过1000。含标题、标点、Markdown，不含空白；低于400也扣分。模型长度估分单独保存，均分采用代码评分重算；原始人评不改。

最终三组结果见 docs/2026-09-12-completion.md；首轮v2未改善，最终v3-fixed实现长度达标，Judge与人评的误差仍未改善。2026-09-15用户已将.env模型名修正为deepseek-flash；此前HTTP400问题已关闭。

## 单篇学习对照（2026-09-17）
`EVAL_RUN_ID=<新ID> cargo test --lib eval_single_article_learning_suite -- --ignored --nocapture`。
`eval_learning.rs` 同模型重跑冻结的旧单阶段提示词与新两阶段流程，五份样文各自学习，迁移到同一个新主题；共享生成包装和匿名Judge。没有多篇合并学习，不要求新增人评。报告结构/长度、非长度均分、去污染、换题四维、提取调用次数与耗时，采用条件见需求文档。
Rubric B v2 取消十标题奖惩；旧 coverage 人评语义不再直接适用。`eval_calibration_suite` 专用于历史 v1 校准，固定读取 `eval/fixtures/*_v1.md`，不评测当前学习流程。学习流水线的阶段检查点只表示响应已收到，完整合格以最终 summary 为准；失败诊断与修复请求独立保留。

2026-09-17按当时门槛保留legacy；该混合门槛已被下文Transfer v2修正，旧未通过不代表新提示词整体失败。结果见 `docs/2026-09-17-style-learning-results.md`。增加 `EVAL_LEARNING_METHOD=direct_free` 可运行单阶段自由格式对照；省略或设为 two_stage 则运行两阶段。各方法独立run_id；直接复用相同请求时必须保留来源记录。
离线产物核验：`python3 scripts/verify_learning_run.py eval/runs/<run_id> --current`，核对十行结果、请求模型、来源和候选提示词、执行文本/正文、长度、阶段调用数和复用SHA。历史提示词不同的run省略--current。

## Transfer v2 修正（2026-09-18）
当前风格迁移评测入口为 `eval_transfer_audit_suite`，评分提示词 `prompts/eval_judge_transfer_v2.md`。分开报告风格保真、原文独立性、事实合规、自然度、主题/核心素材完成度；字数按非空白Unicode字符独立计算600–900达标率，不交给模型估算，不重复计入主题或风格。没有合成总分或自动采用判定。
- `transfer_quality.rs`：匿名评分输入、独立分数与可定位引用校验、字数诊断；低分须有对应证据，最多一次诊断修复。
- `eval_transfer_audit.rs`：复用15篇历史正文重评，冻结15份Spec在同一新题材的strict/creative权限下各生成15篇；四个裁判机制控制，所有请求留证/续跑。
- `transfer_report.rs`：分任务五维均分、逐样文配对差异、长度达标率和控制的非目标维度漂移。四项控制通过不代表模型评分稳定或人工校准。
- `scripts/verify_transfer_audit.py eval/runs/<run_id> --current`：离线核验45行/4控制、原正文/Spec未变、请求匿名输入/生成模板、逐项分数和引用、字数及汇总。
旧 `eval_single_article_learning_suite` 保留用于历史复现，其混合cleanliness与综合采用标准不再用于判断方法总体优劣。旧4.45/4.15等综合分不能与新分项相减。当时默认legacy作为稳定基线保留，该轮只修正评测，未调提取提示词或默认学习方法。

评分的结构/引用修复耗尽后，错误响应检查点仍保留；不能反复同ID重放同一无效输出。使用新run_id，仅复用已验证请求，排除失败评分请求并记录复用来源SHA；不删除历史失败，也不手工补分。运输失败且未形成有效响应的请求可继续用原ID续跑。本轮最终完整记录为 `20260918-transfer-audit-v2-retry1`，原 `20260918-transfer-audit-v2` 保留44成功/1格式失败的证据。

完整结果见 `docs/2026-09-18-transfer-eval-results.md`；不能从旧总分判新提示词整体失败，新旧方法需按场景和独立维度解释。

## 自由格式采用验证（2026-09-21起）
- 开发：`EVAL_RUN_ID=<新ID> EVAL_RELEASE_PHASE=development cargo test --lib eval_free_style_release_suite -- --ignored --nocapture`。两方法×五样文分别提取，同一修理日任务分strict/creative生成20篇。旧法请求可精确复用，SHA与来源写入reused-checkpoints.json。
- 确认：`EVAL_RUN_ID=<新ID> EVAL_RELEASE_PHASE=confirmation EVAL_RELEASE_SOURCE_RUN=<通过开发门槛的run> cargo test --lib eval_free_style_release_suite -- --ignored --nocapture`。冻结相同Spec与模型，换事先登记的新题材再生成20篇；不是新作者留出。
- `free_style_gate.rs`检查先登记的实用门槛，各维度独立；不修改裁判、不合成总分。详见docs/superpowers/specs/2026-09-21-free-style-release.md。通过开发与确认才采用，失败证据保留；对已看过的确认主题不能再声称未见。
- 离线核验：`python3 scripts/verify_free_style_release.py eval/runs/<run> --current`。核对原始响应、学习结果、正文、匿名Judge、可定位引用、分项汇总、确定性长度、确认Spec来源与复用SHA。历史提示词变化的run不加--current。
- Judge仅对task_issues/uncertainties的单字符串做无损单元素列表兼容；分数、理由与引用不补造，缺失证据仍拒绝。

2026-09-23：发布评测先检查Spec，门槛未过写preflight.json并停止正文生成；数值门槛不变。MiMo JSON评分显式使用response_format，参数进入检查点身份，模式改变不能冒用旧请求。当前mimo-free-v10-json-dev-01数值和实现者复核通过，mimo-free-v10-json-confirm-01在新主题确认；模型分数仍不冒充人评。最新结果见docs/2026-09-22-mimo-style-release.md。

## 当前采用结果（2026-09-25）
MiMo v10开发mimo-free-v10-json-dev-01与新主题确认mimo-free-v10-json-confirm-01均通过固定门槛；分别84/61请求检查点核验通过，缓存复跑均0新请求。五份Spec经实现者复核（不是新增用户人评），已成为独立风格；默认学习改为direct_free。报告见docs/2026-09-22-mimo-style-release.md。两轮评测模型均为mimo-v2.6-pro；用户随后要求切换Flash运行，Pro评分不作为Flash的同模型验证结果。
