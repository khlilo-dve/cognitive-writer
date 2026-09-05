# 自动评测结果

模型：deepseek-v4-flash

模型评分尚未经人工校准。历史 Baseline 的生成模型和提示词元数据不全，分差不能单独归因于本轮提示词改动。

| 组别 | 样本 | 得分 | 基线 | Δ | 产物 |
|---|---|---:|---:|---:|---|
| compare_a | topic_01 | 4.50 | 4.00 | 0.50 | [查看](candidate_a_01.md) |
| compare_a | topic_02 | 5.00 | 4.33 | 0.67 | [查看](candidate_a_02.md) |
| compare_a | topic_03 | 4.83 | 2.33 | 2.50 | [查看](candidate_a_03.md) |
| compare_a | topic_04 | 4.67 | 3.83 | 0.83 | [查看](candidate_a_04.md) |
| compare_a | topic_05 | 3.83 | 3.67 | 0.17 | [查看](candidate_a_05.md) |
| baseline_a | topic_01 | 4.00 | — | — | [查看](baseline_a_01.json) |
| baseline_a | topic_02 | 4.33 | — | — | [查看](baseline_a_02.json) |
| baseline_a | topic_03 | 2.33 | — | — | [查看](baseline_a_03.json) |
| baseline_a | topic_04 | 3.83 | — | — | [查看](baseline_a_04.json) |
| baseline_a | topic_05 | 3.67 | — | — | [查看](baseline_a_05.json) |
| compare_b | spec_01 | 3.80 | 4.40 | -0.60 | [查看](candidate_b_01.md) |
| compare_b | spec_02 | 4.20 | 5.00 | -0.80 | [查看](candidate_b_02.md) |
| compare_b | spec_03 | 4.00 | 4.00 | 0.00 | [查看](candidate_b_03.md) |
| compare_b | spec_04 | 3.80 | 4.40 | -0.60 | [查看](candidate_b_04.md) |
| compare_b | spec_05 | 5.00 | 4.40 | 0.60 | [查看](candidate_b_05.md) |
| baseline_b | spec_01 | 4.40 | — | — | [查看](baseline_b_01.json) |
| baseline_b | spec_02 | 5.00 | — | — | [查看](baseline_b_02.json) |
| baseline_b | spec_03 | 4.00 | — | — | [查看](baseline_b_03.json) |
| baseline_b | spec_04 | 4.40 | — | — | [查看](baseline_b_04.json) |
| baseline_b | spec_05 | 4.40 | — | — | [查看](baseline_b_05.json) |
| cross | cross_s01_t01 | 4.83 | — | — | [查看](cross_s01_t01.md) |
| cross | cross_s01_t02 | 4.67 | — | — | [查看](cross_s01_t02.md) |
| cross | cross_s01_t03 | 4.50 | — | — | [查看](cross_s01_t03.md) |
| cross | cross_s01_t04 | 4.50 | — | — | [查看](cross_s01_t04.md) |
| cross | cross_s01_t05 | 4.83 | — | — | [查看](cross_s01_t05.md) |
| cross | cross_s02_t01 | 5.00 | — | — | [查看](cross_s02_t01.md) |
| cross | cross_s02_t02 | 5.00 | — | — | [查看](cross_s02_t02.md) |
| cross | cross_s02_t03 | 5.00 | — | — | [查看](cross_s02_t03.md) |
| cross | cross_s02_t04 | 5.00 | — | — | [查看](cross_s02_t04.md) |
| cross | cross_s02_t05 | 5.00 | — | — | [查看](cross_s02_t05.md) |
| cross | cross_s03_t01 | 4.83 | — | — | [查看](cross_s03_t01.md) |
| cross | cross_s03_t02 | 4.67 | — | — | [查看](cross_s03_t02.md) |
| cross | cross_s03_t03 | 4.50 | — | — | [查看](cross_s03_t03.md) |
| cross | cross_s03_t04 | 4.67 | — | — | [查看](cross_s03_t04.md) |
| cross | cross_s03_t05 | 3.67 | — | — | [查看](cross_s03_t05.md) |
| cross | cross_s04_t01 | 4.00 | — | — | [查看](cross_s04_t01.md) |
| cross | cross_s04_t02 | 4.67 | — | — | [查看](cross_s04_t02.md) |
| cross | cross_s04_t03 | 3.67 | — | — | [查看](cross_s04_t03.md) |
| cross | cross_s04_t04 | 4.83 | — | — | [查看](cross_s04_t04.md) |
| cross | cross_s04_t05 | 4.83 | — | — | [查看](cross_s04_t05.md) |
| cross | cross_s05_t01 | 4.83 | — | — | [查看](cross_s05_t01.md) |
| cross | cross_s05_t02 | 4.83 | — | — | [查看](cross_s05_t02.md) |
| cross | cross_s05_t03 | 4.33 | — | — | [查看](cross_s05_t03.md) |
| cross | cross_s05_t04 | 5.00 | — | — | [查看](cross_s05_t04.md) |
| cross | cross_s05_t05 | 4.33 | — | — | [查看](cross_s05_t05.md) |

## 分组统计

- baseline_a：5 项，均分 3.63
- baseline_b：5 项，均分 4.44
- compare_a：5 项，均分 4.57，胜/平/负 5/0/0，胜率 100%（单次评测，阈值 ±0.05）
- compare_b：5 项，均分 4.16，胜/平/负 1/1/3，胜率 20%（单次评测，阈值 ±0.05）
- cross：25 项，均分 4.64

## 人工复核

按 baseline_b / compare_b 的 JSON 中 mechanical_checks、negative_hits 与诊断核对原文和 Spec，逐篇标记漏抓、误抓、素材污染与是否接受候选。无需重新生成。
