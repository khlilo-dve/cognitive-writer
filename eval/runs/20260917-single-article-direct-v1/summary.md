# 单篇风格学习对照

模型：deepseek-flash；Rubric B v2。候选按本轮提示词在当前模型生成；未变化的旧请求可从同模型前轮精确复用，样文/主题/生成包装/Judge一致。每项一次采样，五种风格各迁移到同一个图书馆主题；不足以证明多主题泛化。模型兼任生成与评审，分数无人评背书。

| 样本 | 方法 | 字数 | Spec均分 | 非长度均分 | 去污染 | 换题均分 | 提取请求数 | 提取耗时秒 |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| 1 | [single_stage](s01_single_stage_spec.md) | 488 | 4.40 | 4.25 | 5.0 | [4.75](s01_single_stage_body.md) | 1 | 33.7 |
| 1 | [direct_free](s01_direct_free_spec.md) | 499 | 4.60 | 4.50 | 5.0 | [4.25](s01_direct_free_body.md) | 1 | 20.0 |
| 2 | [single_stage](s02_single_stage_spec.md) | 513 | 4.40 | 4.25 | 5.0 | [4.25](s02_single_stage_body.md) | 1 | 30.8 |
| 2 | [direct_free](s02_direct_free_spec.md) | 506 | 5.00 | 5.00 | 5.0 | [3.75](s02_direct_free_body.md) | 1 | 19.5 |
| 3 | [single_stage](s03_single_stage_spec.md) | 385 | 4.20 | 4.25 | 5.0 | [4.25](s03_single_stage_body.md) | 1 | 2.5 |
| 3 | [direct_free](s03_direct_free_spec.md) | 513 | 5.00 | 5.00 | 5.0 | [4.75](s03_direct_free_body.md) | 1 | 34.4 |
| 4 | [single_stage](s04_single_stage_spec.md) | 490 | 4.40 | 4.25 | 5.0 | [4.50](s04_single_stage_body.md) | 1 | 66.3 |
| 4 | [direct_free](s04_direct_free_spec.md) | 481 | 4.80 | 4.75 | 5.0 | [4.75](s04_direct_free_body.md) | 1 | 50.7 |
| 5 | [single_stage](s05_single_stage_spec.md) | 523 | 5.00 | 5.00 | 5.0 | [4.50](s05_single_stage_body.md) | 1 | 66.7 |
| 5 | [direct_free](s05_direct_free_spec.md) | 429 | 4.60 | 4.50 | 4.0 | [3.25](s05_direct_free_body.md) | 1 | 39.7 |

预先约定的采用条件计算：
```json
{
  "adopt_candidate": false,
  "new_severe_contamination": false,
  "semantic_mean": {
    "candidate": 4.75,
    "single_stage": 4.4
  },
  "spec_cleanliness": {
    "candidate": 4.8,
    "single_stage": 5.0
  },
  "transfer_cleanliness": {
    "candidate": 4.0,
    "single_stage": 4.8
  },
  "transfer_mean": {
    "candidate": 4.15,
    "single_stage": 4.45
  },
  "transfer_style_fidelity": {
    "candidate": 4.6,
    "single_stage": 4.4
  },
  "transfer_task_fulfillment": {
    "candidate": 4.0,
    "single_stage": 4.8
  }
}
```

提取耗时含并发排队与运输重试；中断后续跑可能仅反映完成该链的运行片段，不作为严格性能基准。请求数指提取逻辑请求，运输重试另查日志。字数分档未变，coverage语义已变，不与旧人评算误差。阶段JSON为请求响应检查点，完整成功以summary.status为准；语义是否可靠仍需查看证据和诊断。
