# 多文章作者风格研究

状态：计划已形成，预算待用户确认；尚未启动付费实验。创建日期：2026-09-26。

本项目要回答：多篇文章能否比单篇文章更可靠地表示作者风格？增加样本后是否趋于稳定？这种稳定是否仍保留作者差异，并改善新主题写作？

## 从这里开始

| 文件 | 用途 |
|---|---|
| [plan.md](plan.md) | 目标、阶段、模型分工、边界、验收与续做入口 |
| [protocol.md](protocol.md) | 作者选择、实验组合、指标定义、盲测和负面结果处理 |
| [budget.md](budget.md) | 官方价格、请求与 token 假设、费用范围、预算停止规则 |
| [budget.csv](budget.csv) | 可重新计算的预算明细；这是估算，不是实际消费 |
| [lark-design.md](lark-design.md) | 飞书表格、图表、问卷和同步设计 |
| [journal.md](journal.md) | 每次突破、失败、方法变更与阶段结论的连续记录 |
| [templates/stage-report.md](templates/stage-report.md) | 面向用户的阶段报告模板 |

计划由用户给出的方向约束，具体方法由执行者负责。预算确认后按阶段自主推进；不为例行步骤反复确认。追加费用、改变研究问题或公开分享等实质变化另行处理。文档中所有“将”“拟”“计划”均不表示功能已经实现。

## 统一存放

本轮新增数据与报告统一归入 `research/style-aggregation/`，不散落到旧评测或正式风格库。下列目录由需要它的阶段创建，不提前生成假数据或空报告：

```text
research/style-aggregation/
  README.md / plan.md / protocol.md / budget.md / budget.csv
  lark-design.md / journal.md
  templates/stage-report.md
  datasets/<dataset_id>/
    manifest.json              # 来源、作者、主题、划分、哈希、清洗记录
    sources/                   # 正文快照；署名等元数据与模型输入分开
    splits.json                # discovery / reference / confirmation
  runs/<run_id>/
    inputs.json / state.json   # 不可变运行输入、可恢复任务状态
    requests/                  # 请求检查点、usage、失败与修复响应
    single/ / comparisons/ / profiles/ / convergence/
    generations/ / evaluations/ / usage.jsonl
    summary.json / summary.md
  reports/<stage>-<run_id>.md
  charts/<run_id>/             # CSV、PNG/SVG、生成参数与数据哈希
  lark/                       # 远端 ID、字段映射、导入批次和回读校验
  blind-test/                 # 问卷版本、匿名题包、受限映射、反馈导出
```

源代码仍在 `src/`，可复用脚本仍在 `scripts/`；其输出必须进入此目录。现有 `eval/` 冻结数据与历史输出不搬家、不覆盖。旧 `Store` 的默认目录保持兼容，新研究入口显式传入本轮运行目录。

本地冻结产物是研究事实源；飞书是阅读、筛选、图表与人工反馈入口。飞书问卷的原始提交是反馈事实源，导出后按批次冻结。本地路径不能充当飞书网页链接；给远端记录写入真实上传附件或已验证可访问链接。

## 当前下一步

用户审阅预算。随后执行 P1：收集并核验两位作者各 12 篇文章、冻结划分、实现最小研究入口与费用记录、建立飞书看板。付费试跑前先落实已批准额度。当前没有任何完成的多文章实验结论，也没有已创建的飞书 Base。
