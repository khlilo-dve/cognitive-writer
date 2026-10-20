# 多文章作者风格研究

状态（2026-10-20）：100元预算已批准，累计占用14.563149元（含不确定预留）。P1与单篇提取已完成；P2语义度量未通过；P3独立聚合与逐篇核验已实际试验，最新16/20篇有效，但语义复核仍拒绝采用。P4/P5尚无真实结果。24篇样本、费用、失败与飞书报告均已留证。创建日期：2026-09-26；当前详细度协议v2_detailed，方法变更见method-revision文档。

本项目要回答：多篇文章能否比单篇文章更可靠地表示作者风格？增加样本后是否趋于稳定？这种稳定是否仍保留作者差异，并改善新主题写作？

## 从这里开始

| 文件 | 用途 |
|---|---|
| [plan.md](plan.md) | 目标、阶段、模型分工、边界、验收与续做入口 |
| [protocol.md](protocol.md) | 作者选择、实验组合、指标定义、盲测和负面结果处理 |
| [budget.md](budget.md) | 官方价格、请求与 token 假设、费用范围、预算停止规则 |
| [budget.csv](budget.csv) | 可重新计算的预算明细；这是估算，不是实际消费 |
| [budget-detailed.csv](budget-detailed.csv) | 当前详细版预算；旧budget.csv仅作历史 |
| [model-choice.md](model-choice.md) | MiMo/DS价格与选择边界、Lite用途 |
| [implementation.md](implementation.md) | P1入口、费用与恢复实现约束 |
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

## 2026-10-20 最新结果

- p3-ending-anchor-01已完成4/4，程序保证实际末尾引文位置；模型2支持/2反例，语义理由仍有附加条件/行数错误，不进入P5。
- 全研究占用14.563149元（含未知预留3.128379元），100元尚余85.436851元。本run0.035508元；缓存复跑4命中/0新请求，账本不变。无活模型任务。
- 253项库测试与构建通过，方法10、原始响应、位置审计及语义复核均已保存。云报告https://www.feishu.cn/docx/Hbm2d9fs1oTWjFx6ISqcIODunv8 已回读。
- 前批已提交903bad0；当前追加为结尾硬约束及四例验证。整体研究未完成；下一步需以更简单的可观察机制分离位置/计数与语义，不复活旧比较器，不直接用不可靠支持票生成。


## 2026-10-19 最新事实（本批次已结束）

- p3-evidence-audit-01：4/4控制、16/20篇有效、37响应；4失败见summary，16有效共33支持/54未知，23项缺失单列；语义复核拒绝生成，不能把小试完整性或控制通过当作者结论。
- 全研究占用14.527641元，含未知预留3.128379元，100元尚余85.472359元；没有活模型任务。本轮小试不是整个研究完成，P4/P5仍无真实结果。
- 两轮聚合及逐篇核验全部保留。report_profile_evidence_audit.py支持partial报告，不填补失败。prepare_profile_evidence_anchors.py已生成20篇准确偏移的末尾片段，0API。
- 252库测试通过、二进制构建通过；目标模块与脚本已核验。下一步从源头区分可验证的位置/频率与解释性语义，缩小复合规则；保持原文证据、不复活旧比较器、不从失效支持票直接编译生成。

最新图文报告：https://www.feishu.cn/docx/Qi7kdtsFEoXlH1x1NqBcf2AZnOd 。研究Base：https://my.feishu.cn/base/Jkvpbe5DUaBDZSsQJ7KcRuSSnXe 。阶段变更与完整失败史见journal.md。
