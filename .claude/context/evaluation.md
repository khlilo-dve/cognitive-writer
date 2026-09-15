# 评测体系与可续跑评测

## 两套独立评测
- Rubric A：生成文章与输入风格 Spec 的匹配度；破题、论证、节奏、人称、用词、结尾六维 1–5 分。只评最终正文，素材与 Spec 单独提供。
- Rubric B：样文提取出的 Spec 质量；结构覆盖、可执行性、精简度、特征捕捉、去素材污染五维 1–5 分。不是文章通用质量评测。
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
- 字数、标题覆盖、连续 12 字原文重合单独作为机械检查线索，不伪装成人工判断。
- 人工已评 Spec 01–03，不要求补评 04–05；Spec 02 候选是用户接受旧模型分数，不是完全独立的人评。新候选暂无人评，模型实验不等于人工验收。

## 三组校准与长度政策
```bash
MODEL=deepseek-v4-flash EVAL_RUN_ID=20260912-calibration-v3-fixed EVAL_CONCURRENCY=3 EVAL_TIMEOUT_SECONDS=180 cargo test --lib eval_calibration_suite -- --ignored --nocapture
```
该入口重评6个旧产物、提取3个新Spec并评分，共12个请求检查点；不重跑25篇交叉生成。并发可选1–3（默认3），单请求超时30–900秒（默认180）。运输设置不改变模型/提示词请求快照，可续跑同一run。

精简度5分400–600；4分300–399或601–700；3分200–299或701–850；2分100–199或851–1000；1分不足100或超过1000。含标题、标点、Markdown，不含空白；低于400也扣分。模型长度估分单独保存，均分采用代码评分重算；原始人评不改。

最终三组结果见 docs/2026-09-12-completion.md；首轮v2未改善，最终v3-fixed实现长度达标，Judge与人评的误差仍未改善。2026-09-15用户已将.env模型名修正为deepseek-flash；此前HTTP400问题已关闭。
