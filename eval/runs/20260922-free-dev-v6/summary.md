# 自由格式学习发布评测

阶段：development；模型：deepseek-flash。固定Rubric B v2和Transfer v2，不合成总分。

门槛通过：false

| 模式 | 方法 | 风格 | 原文独立 | 事实 | 自然度 | 素材完成 | 长度达标 |
|---|---|---:|---:|---:|---:|---:|---:|
| strict | single_stage | 4.20 | 4.80 | 4.60 | 4.00 | 4.60 | 4/5 |
| strict | direct_free | 4.20 | 5.00 | 4.40 | 3.80 | 4.60 | 5/5 |
| creative | single_stage | 4.00 | 5.00 | 5.00 | 4.00 | 5.00 | 5/5 |
| creative | direct_free | 4.00 | 5.00 | 5.00 | 4.20 | 5.00 | 5/5 |

## 分项配对胜/平/负

| 模式 | 维度 | 新版胜/平/负 |
|---|---|---|
| strict | style_fidelity | 2/1/2 |
| strict | source_independence | 1/4/0 |
| strict | factual_compliance | 0/4/1 |
| strict | naturalness | 0/4/1 |
| strict | task_completion | 1/3/1 |
| creative | style_fidelity | 0/5/0 |
| creative | source_independence | 0/5/0 |
| creative | factual_compliance | 0/5/0 |
| creative | naturalness | 1/4/0 |
| creative | task_completion | 0/5/0 |

## 未达标项

- strict task_completion 4.60 < 4.8

## Spec

| 样文 | 方法 | 字数 | 可执行性 | 特征 | 去污染 |
|---|---|---:|---:|---:|---:|
| 1 | [direct_free](spec_direct_free_01.md) | 527 | 5 | 4 | 5 |
| 1 | [single_stage](spec_single_stage_01.md) | 440 | 5 | 4 | 5 |
| 2 | [direct_free](spec_direct_free_02.md) | 596 | 5 | 5 | 5 |
| 2 | [single_stage](spec_single_stage_02.md) | 493 | 5 | 4 | 5 |
| 3 | [direct_free](spec_direct_free_03.md) | 594 | 5 | 4 | 5 |
| 3 | [single_stage](spec_single_stage_03.md) | 534 | 4 | 4 | 5 |
| 4 | [direct_free](spec_direct_free_04.md) | 544 | 4 | 5 | 5 |
| 4 | [single_stage](spec_single_stage_04.md) | 519 | 4 | 4 | 5 |
| 5 | [direct_free](spec_direct_free_05.md) | 537 | 5 | 5 | 5 |
| 5 | [single_stage](spec_single_stage_05.md) | 525 | 4 | 4 | 5 |

## 正文与诊断

- creative 样文1 direct_free：[正文](creative_direct_free_01.md) / [评分](creative_direct_free_01_result.json)
- creative 样文2 direct_free：[正文](creative_direct_free_02.md) / [评分](creative_direct_free_02_result.json)
- creative 样文3 direct_free：[正文](creative_direct_free_03.md) / [评分](creative_direct_free_03_result.json)
- creative 样文4 direct_free：[正文](creative_direct_free_04.md) / [评分](creative_direct_free_04_result.json)
- creative 样文5 direct_free：[正文](creative_direct_free_05.md) / [评分](creative_direct_free_05_result.json)
- creative 样文1 single_stage：[正文](creative_single_stage_01.md) / [评分](creative_single_stage_01_result.json)
- creative 样文2 single_stage：[正文](creative_single_stage_02.md) / [评分](creative_single_stage_02_result.json)
- creative 样文3 single_stage：[正文](creative_single_stage_03.md) / [评分](creative_single_stage_03_result.json)
- creative 样文4 single_stage：[正文](creative_single_stage_04.md) / [评分](creative_single_stage_04_result.json)
- creative 样文5 single_stage：[正文](creative_single_stage_05.md) / [评分](creative_single_stage_05_result.json)
- strict 样文1 direct_free：[正文](strict_direct_free_01.md) / [评分](strict_direct_free_01_result.json)
- strict 样文2 direct_free：[正文](strict_direct_free_02.md) / [评分](strict_direct_free_02_result.json)
- strict 样文3 direct_free：[正文](strict_direct_free_03.md) / [评分](strict_direct_free_03_result.json)
- strict 样文4 direct_free：[正文](strict_direct_free_04.md) / [评分](strict_direct_free_04_result.json)
- strict 样文5 direct_free：[正文](strict_direct_free_05.md) / [评分](strict_direct_free_05_result.json)
- strict 样文1 single_stage：[正文](strict_single_stage_01.md) / [评分](strict_single_stage_01_result.json)
- strict 样文2 single_stage：[正文](strict_single_stage_02.md) / [评分](strict_single_stage_02_result.json)
- strict 样文3 single_stage：[正文](strict_single_stage_03.md) / [评分](strict_single_stage_03_result.json)
- strict 样文4 single_stage：[正文](strict_single_stage_04.md) / [评分](strict_single_stage_04_result.json)
- strict 样文5 single_stage：[正文](strict_single_stage_05.md) / [评分](strict_single_stage_05_result.json)

开发样文为已使用的五篇单篇文章；确认阶段只换题材，不是作者留出。每项一次生成、同模型裁判，有已知评分波动。门槛只表示本阶段实用预期，不宣称所有维度都胜过旧版或统计显著。本次未修改裁判、题材事实权限或生成包装来提高候选成绩。
