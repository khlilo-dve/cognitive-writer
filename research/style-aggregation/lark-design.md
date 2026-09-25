# 飞书研究看板与盲测设计

状态：设计完成，尚未创建远端资源。2026-09-26 本地 `lark-cli version 1.0.0` 已核对；dashboard-block-create、form-create 帮助可用。用户 token 已过期，目前仅 bot 身份可用；帮助可用不等于已验证服务端权限。

## 1. 用户看到什么

创建一个专用 Base：`Cognitive Writer｜多文章风格研究`。默认面向用户自己，首页先显示研究问题、当前阶段、最新结论、未完成/失败数、已用预算和下一步。

用户的阅读顺序：实验总览 → 阶段结论 → 图表 → 代表案例 → 有需要再看原始记录。所有图表标题、注释和报告用中文；技术字段保持稳定英文键，在字段说明和数据字典中解释中文含义。不给用户展示一屏无法解释的模型分数。

## 2. 表结构

| 表/视图展示名 | 主要字段 | 一行代表什么 |
|---|---|---|
| Runs / 实验总览 | run_id, stage, question, status, conclusion, caveat, spent_cny, report, updated_at | 一次可复现实验 |
| Articles / 样本清单 | article_id, author_id, title, topic, split, source_url, length, sha256, exposure | 一篇原文 |
| StyleFeatures / 风格证据 | feature_id, profile_id, dimension, rule, support_count, eligible_count, counterexamples, uncertainty, executable, artifact | 一个 Profile/Style 中的特征 |
| Comparisons / 相似度明细 | pair_id, run_id, dimension, pair_group, overlap, conflict_count, coverage, evidence | 一对 Style 的一个维度 |
| Convergence / 样本量变化 | run_id, author_id, seed, sample_count, dimension, change_rate, support_rate, stable_count, unknown_count, topic_coverage | 一个子集/维度的结果 |
| Generations / 新主题文章 | generation_id, run_id, author_id, method, source_style_id, topic_id, replicate, five_scores, author_match, length, article | 一篇正文及独立评估 |
| Journal / 过程与结果 | event_id, run_id, kind, question, change, observation, meaning, evidence, next_step | 一次突破/失败/变更 |
| ChartData / 绘图数据 | chart_id, run_id, dimension, group, x, y, sample_count, low, high, metric_version | 一个可追溯图形数据点 |
| Feedback / 人工反馈 | response_id, batch_id, participant_code, questionnaire_version, item_id, choice, confidence, reason, submitted_at | 一次配对判断 |

`five_scores` 在实际建表时展开为五个数字字段，单项允许缺失并另列原因，不把 NA 写 0。状态字段使用明确枚举：planned / running / partial / complete / failed / inconclusive，中文说明分别展示。试验未开始时不创建演示数据冒充实测。

Feedback 不向受试者开放表格读取；只开放已验证的问卷提交入口。若同 Base 的分享设置无法隔离实验答案，问卷使用单独收集 Base，本地仍存入统一 blind-test 目录。题目映射不上传到可访问问卷字段。

## 3. 图表及它回答的问题

| 问题 | 飞书原生展示 | 补充 |
|---|---|---|
| 现在进展到哪、花了多少 | 阶段状态视图、指标卡 | 包含失败与缺失数 |
| 同作者是否更像 | 各维作者内/间分组柱状图 | 同步原始点，避免只看均值 |
| 谁的哪些表达更稳定 | 维度条形图 | 标明可判定文章分母 |
| 增加文章是否趋稳 | 按作者/维度的折线图 | 展示不同 seed，范围说明不冒充统计置信区间 |
| 聚合是否改善生成 | 分项分组柱状图 | 风格、事实、自然度分开，无综合总分 |
| 人更偏向哪一组 | 配对选择条形图 | 保留平局、都不像、无法判断 |
| 哪些文章之间相似 | PNG/SVG 热力图附件 | CLI 当前支持列表无 heatmap，不伪称原生支持 |

当前 CLI 图表类型支持 column/bar/line/scatter/statistics 等；未确认的误差棒、箱线图、热力图不作为原生能力承诺。热力图与分布图由冻结数据生成，上传图像附件，阶段报告解释横纵轴、颜色和样本数。每张图记录数据 CSV、脚本版本和哈希，不手工改图里的数字。

报告先保留本地 Markdown 完整版本，并通过记录正文/附件供用户阅读；需要原生飞书文档时按 lark-doc 能力创建真实文档并回读，不伪造链接。研究原文、长请求与密钥不塞到看板；用户需要核验时提供适量证据、来源链接和研究产物。

## 4. 数据与同步

实验的特征判定和确定性指标在本地计算并冻结，然后同步飞书。飞书中的数量/平均值/分组等普通展示统计优先 `+data-query` 或公式；复杂研究算法的结果作为具版本的存储字段导入，不在表中重新实现语义判断。

先读字段结构再写入。同步使用本地 artifact_id → record_id 映射和内容哈希；同一次同步可重跑，不复制出第二套记录。新增/更新前确认 run_id、字段类型和可写性。导入完成后回读记录数量、关键值和图表数据源；创建成功返回 ID 不算展示验收。

字段、表、仪表盘与 form 的真实 ID 存入本目录下 `lark/`。上传附件使用 `+record-upload-attachment`，不能手造附件 token。列表调用串行，单表写入分批串行，遵守 CLI 批量限制与节流。失败批次留日志，仅重试未完成部分。

使用 user 身份优先。若改由 bot 创建，必须确认用户获得管理员权限并可以打开；不把“bot 能访问”当成用户可见。当前过期用户登录在建表前集中处理，不启动不必要的账号重配。

## 5. 问卷设计

目标：8–12 人，每人 4 组，约 32–48 个配对判断；细节见 [protocol.md](protocol.md)。问卷仅在生成与题包冻结后创建，避免每次改实验都使旧反馈失去对应关系。

每个题包显示匿名参考材料和 A/B 两篇文章。固定问题：

1. 哪篇的表达方式更接近参考材料？A / B / 接近 / 两篇都不像 / 无法判断。
2. 你对这次判断多有把握？低 / 中 / 高。
3. 可选：哪些具体表达影响了你的判断？

入口另收随机参与码及阅读说明确认，不要求姓名电话。熟悉原作者不作为参与条件；可选记录是否认出作者，分析时披露。不能让问题措辞暗示“多篇聚合应该更好”。

CLI 支持 text/select/number 问题，description 可包含链接；问题一次最多 10 个，4 组题分批建立。长文阅读体验需要实际预览，必要时题包放到只含匿名材料的阅读页，表单仅收选择。不要链接到含方法标签的 Generations 记录。

原生问卷随机化尚未验证，使用多个预生成平衡版本，A/B 映射固定存档。正式发布前做一份测试提交、核对题目顺序和入表结果，测试数据明确标记并排除统计。公开分享与分发范围由用户决定；本轮不自动联系任何人。

到预登记截止日收集反馈，用 response_id 幂等导出，保留原始提交与剔除原因。先按参与者汇总，避免把一个人四次选择算四个独立人。缺失反馈不补造；用户无需逐条评分才能让其余研究继续。

## 6. 能力证据与后续验证

2026-09-26 已读取 lark-base/shared 技能及 dashboard-block-create、dashboard-block-data-config、form-create、form-questions-create reference，并执行对应 CLI help、auth status。核对结果：本地命令存在；用户授权已过期；尚未创建 Base、图表、表单或验证任何远端提交。P1 与 P5 分别完成真实验收后更新此处。
