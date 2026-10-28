# 多文章作者风格研究

**2026-10-28 当前进展：** [长篇声音分析](reports/voice-prose-2026-10-28.md)四份完成，开始正确保留整体声音和题材变化，但单篇产物仍含源文案例，状态`requires_content_separation_before_writer`。接下来独立剥离内容，再用新题材验证单篇与两篇路径。累计账本占用¥20.605440含未知预留，不能当实付；正式风格库尚未采用候选。

**2026-10-26 最新断点：** [通俗阶段报告](https://www.feishu.cn/docx/ZhK0dW1GfoW61Sxxt9lc7bgxnFe)已发布并回读。两作者各两篇的 [v3学习竖切](reports/voice-learning-v3-2026-10-26.md) 已在工程上完整，但 Writer 说明仍逐条机械展开；[上位声音编辑实验](reports/voice-editor-2026-10-26.md) 把部分规则写得更连贯，却是3份结构完成、1份修复后失败。[直接回原文的语义核查](reports/voice-learning-semantic-audit-2026-10-26.md)又发现关键概括与两篇的题材变体冲突，未过产品门槛。接下来先找反例并收窄规则，再做新题材执行。上传文件、URL、1–10篇及异常追问继续后置。实际费用以最新run、进程和账本为准。

**2026-10-24 当前研究优先级：先把学习方法做稳，再接文章输入。** 用户明确：上传文件、文章网址、1–10篇选择及异常追问是下一阶段的输入工作，可以独立交给子 Agent；当前不以 URL/REPL 接入阻塞学习算法。旧 12 稿试写已完成并在[读稿报告](reports/detailed-transfer-pilot-01-readout.md)中记录事实扩编。直接把1/2/3篇全文交给一次综合的 [v1小试](reports/voice-learning-v1-2026-10-24.md) 因原文泄漏和增量去留不明被拒绝；当前在 [v2逐篇观察与增量综合](method-revision-19.md)中另起 run 验证。完成请求不等于方法已通过。

**当前目标（2026-10-23）：交付稳定的单篇与多篇风格学习功能。** 请先读 [产品目标与验收](product-goal-2026-10-23.md)，[飞书阅读版](https://www.feishu.cn/docx/DjKVdwu91onhHRxQ0tEc3lAhnCg)已回读。单篇、多篇各有独立的可用性要求；同题比较用于定位增量学习的帮助与误伤，不用于选出“赢家”。下方早期“多篇能否胜过单篇”的中心问题及 P0–P6 是历史实验设计，不再决定产品是否完成。现有单篇学习已进入 CLI/REPL，多篇仍在研究阶段，尚非用户可用功能。

**2026-10-23 方向纠偏：先读 [direction-correction-2026-10-23.md](direction-correction-2026-10-23.md)，[飞书通俗报告](https://www.feishu.cn/docx/Aa22d75Q8oxUG7xs61EcSm9fnvg)已发布并回读。** 风格文件的用途是给 Writer 一份详尽、可信、能迁移到新题材的写法说明。字数、句长、规则条数仅作诊断，不是质量目标或阶段通过条件。原 P2/P3 严格语义核验未过，探索性两份聚合文件尚不能作为可靠 Style；基于它们的 60 篇生成稿只证明流程运行并提供失败案例。旧版匿名问卷暂停分发。接下来先补好、审好两份详细风格文件，再做少量直接迁移验证；暂不新增 API 花费。

方向纠偏后的两份**开发候选**已在 `runs/detailed-executable-style-01/profiles/`，逐篇依据与待审问题在同run的`evidence-review.md`。新题材Flash试写冻结于`runs/detailed-transfer-pilot-01/inputs.json`，首次全部因网络/TLS失败，0篇正文；本run未知费用预留0.196609元。候选还没有经过独立审阅或有效Writer迁移验证，不进入生产风格库。此前“暂不新增API花费”指纠偏文档/编写阶段；本次小试已按method-revision-17启动并在网络故障后停止。

[详细候选阶段报告](https://www.feishu.cn/docx/IuP6d5X7oo8Ojix6FYDcD8zcnph)已发布并回读；内含两份可阅读的候选文档和本次传输失败说明。
原文依据、题材变体和仍待修正的问题另见[飞书证据清单](https://www.feishu.cn/docx/D1UrdxE3xogLeoxsUjIc0qKonke)。

状态（2026-10-21）：100元预算已批准。P1及P2单篇提取已完成，P2严格语义测量/P3严格证据聚合仍未通过。探索性P3候选已用于完成P5 60篇Flash生成；P4有20篇原文表层统计，语义收敛未证明。旧评分方法停止时全研究占用18.033171元含未知预留；新compact评分费用以账本为准。来源、失败、图表及飞书报告保留。

历史实验问题（已降为诊断）：多篇文章相对单篇的表示和迁移会发生什么变化？当前主问题见上方产品目标文件。

## 从这里开始

| 文件 | 用途 |
|---|---|
| [product-goal-2026-10-23.md](product-goal-2026-10-23.md) | 当前产品目标、单篇/多篇合同、稳定性定义和 S1–S4 验收 |
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
