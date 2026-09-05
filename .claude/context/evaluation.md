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
- 候选提取 Prompt 仅保存在运行目录，针对已发现的原句引用和超长问题增加抽象/长度约束，不替换 `prompts/learn_style.md`。

## 运行
```bash
# 离线
cargo test --lib
# 完整在线运行；断网后原命令续跑
EVAL_RUN_ID=20260905-auto-v1 cargo test --lib eval_automated_suite -- --ignored --nocapture
```
可单独运行：`eval_judge_baseline_suite`、`eval_judge_learn_baseline_suite`、`eval_style_compare_suite`、`eval_learn_compare_suite`、`eval_all_specs_on_eval1_suite`。相同 run_id 共享检查点，不同时运行相同入口或同名任务。

## 解释边界
- 历史 Baseline 缺少完整生成模型/Prompt 元数据，与候选的分差不能单独归因于本轮改动。
- 5 篇单次胜率只用于发现问题，不代表统计显著改善。
- 字数、标题覆盖、连续 12 字原文重合单独作为机械检查线索，不伪装成人工判断。
- 人工校准仍需用户审阅样文、基线与候选 Spec；自动执行完不代表人工任务完成。
