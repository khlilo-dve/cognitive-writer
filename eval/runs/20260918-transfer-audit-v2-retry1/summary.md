# 风格迁移评测修正：Transfer v2

模型：deepseek-flash。提取提示词、Spec和生产生成包装保持不变；15篇历史文章重评，另有同一新题材的严格/创作两类任务各15篇。没有重新学习、多文章合并或新增用户人评。方法标签与旧分数不输入裁判。

## 独立维度

风格保真 / 原文独立性 / 事实合规 / 自然度 / 主题完成。事实问题不再计入原文污染或主题完成，长度单独由程序计数；不输出合成总分或自动采用结论。

| 组别 | 方法 | 风格 | 原文独立 | 事实 | 自然度 | 主题完成 | 字数达标 |
|---|---|---:|---:|---:|---:|---:|---:|
| historical | single_stage | 4.40 | 5.00 | 4.00 | 4.00 | 5.00 | 3/5 |
| historical | two_stage | 4.40 | 4.80 | 3.80 | 3.80 | 5.00 | 4/5 |
| historical | direct_free | 5.00 | 5.00 | 3.00 | 4.20 | 4.80 | 3/5 |
| strict | single_stage | 4.20 | 5.00 | 4.40 | 4.00 | 5.00 | 4/5 |
| strict | two_stage | 4.00 | 5.00 | 4.60 | 3.80 | 4.80 | 4/5 |
| strict | direct_free | 4.40 | 4.80 | 3.60 | 4.00 | 4.60 | 5/5 |
| creative | single_stage | 4.20 | 5.00 | 5.00 | 4.00 | 5.00 | 4/5 |
| creative | two_stage | 4.40 | 4.80 | 5.00 | 4.20 | 4.60 | 4/5 |
| creative | direct_free | 4.60 | 5.00 | 5.00 | 4.20 | 4.60 | 5/5 |

## 裁判机制控制

以下控制不纳入方法均分。四个控制中，同一新增场景在严格任务应扣事实分、明确创作任务不应扣；同一源文内容搬运在两种任务下都应识别。短文故意未达600字，长度不得影响上述分项。

| 控制 | 通过 | 五维分数 |
|---|---|---|
| extra_strict | true | [2.0, 5.0, 3.0, 4.0, 5.0] |
| extra_creative | true | [2.0, 5.0, 5.0, 4.0, 5.0] |
| leak_strict | true | [1.0, 2.0, 3.0, 3.0, 5.0] |
| leak_creative | true | [2.0, 3.0, 5.0, 4.0, 5.0] |

控制通过仅代表预设方向正确，不等于评分稳定或人工校准。同文只改变事实权限时的分差（creative减strict）如下；事实分预期变化，其他维度的变化作为敏感性诊断保留，不据此调整裁判：
```json
{
  "extra": {
    "creative_minus_strict": {
      "factual_compliance": 2.0,
      "naturalness": 0.0,
      "source_independence": 0.0,
      "style_fidelity": 0.0,
      "task_completion": 0.0
    },
    "non_fact_drift_observed": false
  },
  "leak": {
    "creative_minus_strict": {
      "factual_compliance": 2.0,
      "naturalness": 1.0,
      "source_independence": 1.0,
      "style_fidelity": 1.0,
      "task_completion": 0.0
    },
    "non_fact_drift_observed": true
  }
}
```

## 逐篇证据

| ID | 组别 | 样文 | 方法 | 字数 | 五维分数 | 确定污染/事实问题 |
|---|---|---:|---|---:|---|---|
| [item_01](item_01.md) | historical | 1 | two_stage | 1103 | [4.0, 5.0, 5.0, 3.0, 5.0] | 0/0（[诊断](item_01_result.json)） |
| [item_02](item_02.md) | historical | 1 | direct_free | 848 | [5.0, 5.0, 3.0, 4.0, 5.0] | 0/4（[诊断](item_02_result.json)） |
| [item_03](item_03.md) | historical | 1 | single_stage | 744 | [5.0, 5.0, 5.0, 4.0, 5.0] | 0/0（[诊断](item_03_result.json)） |
| [item_04](item_04.md) | historical | 2 | direct_free | 560 | [5.0, 5.0, 2.0, 4.0, 4.0] | 0/12（[诊断](item_04_result.json)） |
| [item_05](item_05.md) | historical | 2 | single_stage | 806 | [4.0, 5.0, 3.0, 4.0, 5.0] | 0/4（[诊断](item_05_result.json)） |
| [item_06](item_06.md) | historical | 2 | two_stage | 894 | [4.0, 5.0, 3.0, 4.0, 5.0] | 0/1（[诊断](item_06_result.json)） |
| [item_07](item_07.md) | historical | 3 | single_stage | 1023 | [4.0, 5.0, 4.0, 4.0, 5.0] | 0/1（[诊断](item_07_result.json)） |
| [item_08](item_08.md) | historical | 3 | two_stage | 849 | [5.0, 5.0, 4.0, 4.0, 5.0] | 0/1（[诊断](item_08_result.json)） |
| [item_09](item_09.md) | historical | 3 | direct_free | 818 | [5.0, 5.0, 5.0, 4.0, 5.0] | 0/0（[诊断](item_09_result.json)） |
| [item_10](item_10.md) | historical | 4 | two_stage | 813 | [5.0, 4.0, 5.0, 4.0, 5.0] | 1/0（[诊断](item_10_result.json)） |
| [item_11](item_11.md) | historical | 4 | direct_free | 968 | [5.0, 5.0, 3.0, 4.0, 5.0] | 0/2（[诊断](item_11_result.json)） |
| [item_12](item_12.md) | historical | 4 | single_stage | 982 | [4.0, 5.0, 5.0, 4.0, 5.0] | 0/0（[诊断](item_12_result.json)） |
| [item_13](item_13.md) | historical | 5 | direct_free | 755 | [5.0, 5.0, 2.0, 5.0, 5.0] | 0/6（[诊断](item_13_result.json)） |
| [item_14](item_14.md) | historical | 5 | single_stage | 733 | [5.0, 5.0, 3.0, 4.0, 5.0] | 0/6（[诊断](item_14_result.json)） |
| [item_15](item_15.md) | historical | 5 | two_stage | 674 | [4.0, 5.0, 2.0, 4.0, 5.0] | 0/6（[诊断](item_15_result.json)） |
| [item_16](item_16.md) | strict | 1 | two_stage | 673 | [5.0, 5.0, 5.0, 4.0, 4.0] | 0/0（[诊断](item_16_result.json)） |
| [item_17](item_17.md) | strict | 1 | direct_free | 698 | [4.0, 5.0, 4.0, 4.0, 5.0] | 0/1（[诊断](item_17_result.json)） |
| [item_18](item_18.md) | strict | 1 | single_stage | 800 | [4.0, 5.0, 5.0, 4.0, 5.0] | 0/0（[诊断](item_18_result.json)） |
| [item_19](item_19.md) | strict | 2 | direct_free | 658 | [5.0, 5.0, 3.0, 4.0, 5.0] | 0/5（[诊断](item_19_result.json)） |
| [item_20](item_20.md) | strict | 2 | single_stage | 592 | [4.0, 5.0, 3.0, 4.0, 5.0] | 0/5（[诊断](item_20_result.json)） |
| [item_21](item_21.md) | strict | 2 | two_stage | 614 | [4.0, 5.0, 3.0, 4.0, 5.0] | 0/3（[诊断](item_21_result.json)） |
| [item_22](item_22.md) | strict | 3 | single_stage | 720 | [5.0, 5.0, 5.0, 4.0, 5.0] | 0/0（[诊断](item_22_result.json)） |
| [item_23](item_23.md) | strict | 3 | two_stage | 923 | [5.0, 5.0, 5.0, 4.0, 5.0] | 0/0（[诊断](item_23_result.json)） |
| [item_24](item_24.md) | strict | 3 | direct_free | 692 | [4.0, 5.0, 5.0, 4.0, 5.0] | 0/0（[诊断](item_24_result.json)） |
| [item_25](item_25.md) | strict | 4 | two_stage | 707 | [5.0, 5.0, 5.0, 3.0, 5.0] | 0/0（[诊断](item_25_result.json)） |
| [item_26](item_26.md) | strict | 4 | direct_free | 850 | [4.0, 4.0, 4.0, 4.0, 4.0] | 1/1（[诊断](item_26_result.json)） |
| [item_27](item_27.md) | strict | 4 | single_stage | 737 | [4.0, 5.0, 5.0, 4.0, 5.0] | 0/0（[诊断](item_27_result.json)） |
| [item_28](item_28.md) | strict | 5 | direct_free | 660 | [5.0, 5.0, 2.0, 4.0, 4.0] | 0/4（[诊断](item_28_result.json)） |
| [item_29](item_29.md) | strict | 5 | single_stage | 700 | [4.0, 5.0, 4.0, 4.0, 5.0] | 0/1（[诊断](item_29_result.json)） |
| [item_30](item_30.md) | strict | 5 | two_stage | 720 | [1.0, 5.0, 5.0, 4.0, 5.0] | 0/0（[诊断](item_30_result.json)） |
| [item_31](item_31.md) | creative | 1 | two_stage | 768 | [4.0, 5.0, 5.0, 4.0, 4.0] | 0/0（[诊断](item_31_result.json)） |
| [item_32](item_32.md) | creative | 1 | direct_free | 711 | [4.0, 5.0, 5.0, 4.0, 4.0] | 0/0（[诊断](item_32_result.json)） |
| [item_33](item_33.md) | creative | 1 | single_stage | 719 | [5.0, 5.0, 5.0, 5.0, 5.0] | 0/0（[诊断](item_33_result.json)） |
| [item_34](item_34.md) | creative | 2 | direct_free | 697 | [5.0, 5.0, 5.0, 5.0, 5.0] | 0/0（[诊断](item_34_result.json)） |
| [item_35](item_35.md) | creative | 2 | single_stage | 806 | [4.0, 5.0, 5.0, 4.0, 5.0] | 0/0（[诊断](item_35_result.json)） |
| [item_36](item_36.md) | creative | 2 | two_stage | 484 | [4.0, 5.0, 5.0, 5.0, 4.0] | 0/0（[诊断](item_36_result.json)） |
| [item_37](item_37.md) | creative | 3 | single_stage | 739 | [4.0, 5.0, 5.0, 3.0, 5.0] | 0/0（[诊断](item_37_result.json)） |
| [item_38](item_38.md) | creative | 3 | two_stage | 710 | [5.0, 5.0, 5.0, 4.0, 5.0] | 0/0（[诊断](item_38_result.json)） |
| [item_39](item_39.md) | creative | 3 | direct_free | 666 | [5.0, 5.0, 5.0, 4.0, 4.0] | 0/0（[诊断](item_39_result.json)） |
| [item_40](item_40.md) | creative | 4 | two_stage | 772 | [5.0, 4.0, 5.0, 4.0, 5.0] | 1/0（[诊断](item_40_result.json)） |
| [item_41](item_41.md) | creative | 4 | direct_free | 758 | [5.0, 5.0, 5.0, 4.0, 5.0] | 0/0（[诊断](item_41_result.json)） |
| [item_42](item_42.md) | creative | 4 | single_stage | 669 | [4.0, 5.0, 5.0, 4.0, 5.0] | 0/0（[诊断](item_42_result.json)） |
| [item_43](item_43.md) | creative | 5 | direct_free | 631 | [4.0, 5.0, 5.0, 4.0, 5.0] | 0/0（[诊断](item_43_result.json)） |
| [item_44](item_44.md) | creative | 5 | single_stage | 554 | [4.0, 5.0, 5.0, 4.0, 5.0] | 0/0（[诊断](item_44_result.json)） |
| [item_45](item_45.md) | creative | 5 | two_stage | 783 | [4.0, 5.0, 5.0, 4.0, 5.0] | 0/0（[诊断](item_45_result.json)） |

## 解释限制

历史文章按原任务权限重评；其原字数口径没有说明，当前非空白字符统计只作统一诊断，不倒推原评分。strict/creative是同一修理日素材包的两种权限，不能当作两个独立题材。生成与裁判同模型，每项一次采样，任务/体裁仍有限，没有显著性或泛化结论。事实严格场景与创作场景分别解释，不平均成单一胜者。原文独立分高也不证明完全无语义污染；必须保留引用证据、uncertainties和控制失败。旧综合分量尺已废弃，不能与新分数相减声称提升。默认方法本轮不自动切换。
