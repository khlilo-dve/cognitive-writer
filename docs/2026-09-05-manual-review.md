# 人工复核材料

自动工作已完成：110 个请求、45 项评分、25/25 交叉生成与评分。完整续跑验证复用了 110 个检查点，新模型调用为 0。

## 已完成的判断

本轮提取 Prompt 候选不采纳：模型均分由基线 4.44 降到 4.16，胜/平/负为 1/1/3；5 份候选全部超过 600 字。现行 `prompts/learn_style.md` 和风格文件保持不变。候选作为失败实验保留，避免再次重复。

[自动评测总表](../eval/runs/20260905-auto-v1/summary.md) · [完整评分与机械检查](../eval/runs/20260905-auto-v1/summary.json)

## 需要你的人工校准

逐篇对照原文和冻结 Spec，填写 Rubric B 五维评分：结构覆盖 / 可执行性 / 精简度 / 特征捕捉 / 去素材污染（各 1–5），并标记漏抓、误抓或素材耦合。无需重新生成或运行命令。

| 样本 | 原文 | 冻结 Spec | 字数 | 模型分 | 候选字数 / 分数 | 人工五维分与诊断 |
|---|---|---|---:|---:|---|---|
| 01 | [样文](../eval/samples/sample_01.md) | [Spec](../eval/baseline_specs/spec_01.md) | 824 | 4.40 | [927 / 3.80](../eval/runs/20260905-auto-v1/candidate_b_01.md) | 待填 |
| 02 | [样文](../eval/samples/sample_02.md) | [Spec](../eval/baseline_specs/spec_02.md) | 599 | 5.00 | [954 / 4.20](../eval/runs/20260905-auto-v1/candidate_b_02.md) | 待填 |
| 03 | [样文](../eval/samples/sample_03.md) | [Spec](../eval/baseline_specs/spec_03.md) | 910 | 4.00 | [998 / 4.00](../eval/runs/20260905-auto-v1/candidate_b_03.md) | 待填 |
| 04 | [样文](../eval/samples/sample_04.md) | [Spec](../eval/baseline_specs/spec_04.md) | 652 | 4.40 | [1212 / 3.80](../eval/runs/20260905-auto-v1/candidate_b_04.md) | 待填 |
| 05 | [样文](../eval/samples/sample_05.md) | [Spec](../eval/baseline_specs/spec_05.md) | 727 | 4.40 | [818 / 5.00](../eval/runs/20260905-auto-v1/candidate_b_05.md) | 待填 |

### 优先核对的矛盾

- 基线 Spec 03 为 910 字、Spec 04 为 652 字，模型精简度均给了 4 分；候选 Spec 05 为 818 字却得到精简度 5 分。Rubric B 对 4–5 分要求 ≤600 字，因此这些分数与客观计数冲突。
- 所有字数按“包含标题和标点，不含空白”计数；这一定义已同时应用于基线和候选。
- 基线 Spec 01 含连续 12 字原文重合线索；候选消除了这种长串重合，但篇幅普遍增加。重合检查不自动等于抄袭判定。
- 生成端候选均分 4.57，历史基线 3.63，单次 5/5 胜；历史生成元数据不完整，不能据此断言本轮工程修改改善了风格。
- 交叉评测平均 4.64；其中 Spec 02 五篇均为满分，适合人工抽查 Judge 是否区分度不足。

## 需要确认的工程方案

具体方案见 [.claude/tasks/plan.md](../.claude/tasks/plan.md) 的 A–C：

1. REPL 单一输入通道：学习后等命名、删除前等确认，支持取消与会话恢复；修复旧大纲状态的继续路径。
2. 公开错误类型迁移：io/refine/utils 及调用方统一 AppError，保留 CLI 与 REPL 各自错误处理边界。
3. 模块规模治理：拆分 intent/styles 职责，保持公开调用兼容和内嵌测试规则。

这些较大改造尚未实施。按你提供的“ 大改动前先在 Plan Mode 出方案，我确认后再动手 ”规则，确认后进入实施；D 的交互提示最小修复已完成。
