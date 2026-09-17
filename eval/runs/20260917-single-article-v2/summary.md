# 单篇风格学习对照

模型：deepseek-flash；Rubric B v2。新旧提示词均在当前模型重跑，样文/主题/生成包装/Judge一致。每项一次采样，五种风格各迁移到同一个图书馆主题；不足以证明多主题泛化。模型兼任生成与评审，分数无人评背书。

| 样本 | 方法 | 字数 | Spec均分 | 非长度均分 | 去污染 | 换题均分 | 提取请求数 | 提取耗时秒 |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| 1 | [single_stage](s01_single_stage_spec.md) | 488 | 4.40 | 4.25 | 5.0 | [4.75](s01_single_stage_body.md) | 1 | 33.7 |
| 1 | [two_stage](s01_two_stage_spec.md) | 534 | 4.60 | 4.50 | 5.0 | [4.00](s01_two_stage_body.md) | 2 | 114.2 |
| 2 | [single_stage](s02_single_stage_spec.md) | 513 | 4.40 | 4.25 | 5.0 | [4.25](s02_single_stage_body.md) | 1 | 30.8 |
| 2 | [two_stage](s02_two_stage_spec.md) | 531 | 4.80 | 4.75 | 5.0 | [4.00](s02_two_stage_body.md) | 2 | 81.0 |
| 3 | [single_stage](s03_single_stage_spec.md) | 385 | 4.20 | 4.25 | 5.0 | [4.25](s03_single_stage_body.md) | 1 | 2.5 |
| 3 | [two_stage](s03_two_stage_spec.md) | 539 | 4.80 | 4.75 | 5.0 | [4.50](s03_two_stage_body.md) | 2 | 122.9 |
| 4 | [single_stage](s04_single_stage_spec.md) | 490 | 4.40 | 4.25 | 5.0 | [4.50](s04_single_stage_body.md) | 1 | 66.3 |
| 4 | [two_stage](s04_two_stage_spec.md) | 539 | 4.40 | 4.25 | 5.0 | [4.50](s04_two_stage_body.md) | 2 | 139.7 |
| 5 | [single_stage](s05_single_stage_spec.md) | 523 | 4.80 | 4.75 | 5.0 | [4.50](s05_single_stage_body.md) | 1 | 66.7 |
| 5 | [two_stage](s05_two_stage_spec.md) | 535 | 4.80 | 4.75 | 5.0 | [4.00](s05_two_stage_body.md) | 2 | 126.9 |

预先约定的采用条件计算：
```json
{
  "adopt_two_stage": false,
  "new_severe_contamination": false,
  "semantic_mean": {
    "single_stage": 4.35,
    "two_stage": 4.6
  },
  "spec_cleanliness": {
    "single_stage": 5.0,
    "two_stage": 5.0
  },
  "transfer_mean": {
    "single_stage": 4.45,
    "two_stage": 4.2
  }
}
```

提取耗时含并发排队与运输重试；中断后续跑可能仅反映完成该链的运行片段，不作为严格性能基准。请求数指提取逻辑请求，运输重试另查日志。字数分档未变，coverage语义已变，不与旧人评算误差。阶段JSON为请求响应检查点，完整成功以summary.status为准；语义是否可靠仍需查看证据和诊断。
