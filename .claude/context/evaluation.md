# 评测体系与 Eval 基线 (evaluation.md)

## 双评测维度体系

写作系统的输出主观性极高，本项目建立了一套科学可量化的自动化评测体系：

### 1. Rubric A：风格匹配度评测 (Style Fidelity)
- **目标**: 衡量生成文章是否严格契合目标 StyleSpec 的约束，杜绝模型自我发挥导致的“AI 味”。
- **评分维度**:
  - 结构范式拟合度
  - 核心论证逻辑与认知深度
  - 句式与语言语气习惯
  - 负向约束遵守率（是否误用了禁止词）

### 2. Rubric B：文章质感与独立可读性评测 (Writing Quality)
- **目标**: 脱离特定风格，评估文章作为一篇大众公众号传播内容的综合水准。
- **评分维度**:
  - 抓人程度（Hooking / 黄金前三屏）
  - 论点推进流畅性与内在逻辑连贯性
  - 金句密度与认知价值提炼

## LLM-Judge 自动化评测引擎 (`src/eval.rs`)

为了实现无人工介入的基线监控与批量回归：
- **评测驱动**: `src/eval.rs` 加载预定义的 Baseline 题目 (`eval/baseline/*.md`)。
- **评分提示词**:
  - `prompts/eval_judge_style.md`: 驱动 LLM 按照 Rubric A 规范严格量化打分（1~5 分制），并输出结构化 JSON 理由。
  - `prompts/eval_judge_learn.md`: 评测逆向学习提取的 Spec 本身是否完整精准。
- **评测输出**: 评分与理由自动记录至 `eval/spec_runs/` 下便于对比版本间差异。

## 目录结构映射
- `eval/baseline/`: 冻结的标准选题基线集，用于每次版本变更后的横向对比。
- `eval/samples/`: 人类作者真实样本文章库，用于风格逆向学习的标准输入。
- `eval/baseline_specs/`: 冻结的基准风格 Spec 快照。

## 何时需要查阅代码
- 增加或修改评分维度：查阅 `prompts/eval_judge_style.md` 与 `src/eval.rs`。
- 新增评测集或修改评测运行方式：查阅 `eval/` 目录结构与 `tests/` 中的 eval 用例。
