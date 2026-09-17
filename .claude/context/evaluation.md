# 评测体系与可续跑评测

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

默认提取保留 legacy：两阶段v2/v3与自由格式单阶段均未通过预定采用条件。结果见 `docs/2026-09-17-style-learning-results.md`。增加 `EVAL_LEARNING_METHOD=direct_free` 可运行单阶段自由格式对照；省略或设为 two_stage 则运行两阶段。各方法独立run_id；直接复用相同请求时必须保留来源记录。
离线产物核验：`python3 scripts/verify_learning_run.py eval/runs/<run_id> --current`，核对十行结果、请求模型、来源和候选提示词、执行文本/正文、长度、阶段调用数和复用SHA。历史提示词不同的run省略--current。
