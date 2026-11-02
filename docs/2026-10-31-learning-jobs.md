# 正文学习任务接入草稿工作区

承接候选工作区，把已有正文直接交给学习核心，再进入候选预览、差异和显式保存。新建与追加共用“完整原文分析→执行说明→原文复读→范围修订”链；困难学习阶段固定MiMo Pro，Flash生成文章的既有职责不改。本阶段不引入文件/URL抓取和新的REPL状态机，不修改.env、旧learn默认入口或正式styles。

## 交付与验收

- `style-draft learn <texts.json>`创建并运行任务；包为articles正文数组和note变化理由。
- `style-draft append <name> <version> <texts.json>`读取旧版正文，加入新正文，校验重复和总数上限10，再完整重新学习；旧Style不作为递归压缩证据。
- 任务冻结正文、基准、四份提示词、方法、模型与端点身份。`jobs`/`job`可查看状态，`resume`使用冻结输入恢复；退出或失败不改变旧版本，学习成功只产生待审候选，不自动命名保存。
- 每个请求先记录意图，收到后保存带哈希的响应和usage；相同请求复用缓存，配置或响应身份不符则失败。仅完整Markdown外包装可规范化，内部仍走现有校验。
- 已发出但未落盘响应的请求保留不确定状态，不自动重发；显式`resume --retry-uncertain`可最多再试到该阶段三次，总记录不覆盖。不把未知用量写成0，不把模型自评当质量通过。
- 通过本地HTTP模拟验证新建、追加、恢复、源文件/提示词变化隔离、错误状态、缓存损坏、配置变化和纯spec导出。真实开发小试最多两作者各1→2篇四任务，费用最多¥3，计入既定¥100总额；真实调用由研究账本适配器统一预留/结算。

## 边界

本轮取得的是核心到工作区的开发命令链。文章获取、目标文章数量选择、作者/声音归属和不可访问/非文章/多文章歧义追问仍由后续输入层处理。模型接入、缓存复跑和保存成功不会改变既有语义与Writer事实失败的质量结论。研究目录保留全部成功、失败和阶段报告，飞书只作阅读投影。

## 使用方式

正文包是已经取得的完整正文，不是网址或风格候选：

```json
{"articles":["第一篇完整正文","第二篇完整正文"],"note":"学习这两篇可见的声音与写法"}
```

```bash
target/debug/cognitive-writer style-draft --root /tmp/cw-learning learn /tmp/texts.json
target/debug/cognitive-writer style-draft --root /tmp/cw-learning jobs
target/debug/cognitive-writer style-draft --root /tmp/cw-learning job <job-id>
target/debug/cognitive-writer style-draft --root /tmp/cw-learning resume <job-id>
# 完成后用candidate_id预览，再明确命名保存
target/debug/cognitive-writer style-draft --root /tmp/cw-learning preview <candidate-id>
target/debug/cognitive-writer style-draft --root /tmp/cw-learning save <candidate-id> 示例声音
# 新正文包只放新增文章；系统从第1版恢复旧正文并整体重新学习
target/debug/cognitive-writer style-draft --root /tmp/cw-learning append 示例声音 1 /tmp/new-texts.json
target/debug/cognitive-writer style-draft --root /tmp/cw-learning diff <new-candidate-id>
target/debug/cognitive-writer style-draft --root /tmp/cw-learning save <new-candidate-id> 示例声音
```

新建和追加都会进行真实模型调用，固定使用Pro。旧的离线import/preview/save等命令仍不调用模型。任务ID在请求前打印；Ctrl-C可停止等待，已收到的阶段会保留。某阶段请求已发出却未落盘响应时，默认resume会报告不确定状态；需要再次发送时使用`resume <job-id> --retry-uncertain`。该阶段最多3次，每次意图记录都保留，旧的不确定用量不会被清零。

## 记录与恢复

任务位于工作区`jobs/<job-id>/`：plan.json冻结输入和方法，state.json保存状态快照，requests目录保存逐阶段请求意图及带哈希的响应。日志不存API凭证或原始HTTP错误正文；usage缺失或请求中断按未知处理，不能报成0费用。任务页展示的是已收到响应的token用量及仍不确定的请求数量，不是供应商账单。

重新开始相同输入、理由、基准与配置会定位同一任务；输入顺序在创建任务前规范化。resume不重新读取正文包或当前提示词文件，使用plan内的快照。方法实现已变更或端点/模型身份不符时不静默复用。完成的候选可在后来保存新版本后仍找回，不自动保存或再次采用。

本次`voice-core-v2`只增加与范围修订共用的完整Markdown包装规范化；旧core-01仍保留voice-core-v1原产物，研究入口冻结方法身份，不能原地覆盖旧run。包装内正文仍走原格式和原文内容隔离校验，不放宽语义要求。

## 2026-11-02 完成核对

真实小试learning-jobs-live-02两作者四任务全部完成，共16次Pro响应、无不确定请求；1→2篇来源保留和四份候选/版本身份均通过审计。产品CLI在无API_KEY时恢复四个完整候选通过，研究入口完整复跑0新请求、账本SHA及旧版本均不变。278项库测试通过、10忽略；17项既有终端回归通过，本地HTTP新任务链10次模拟请求及历史工作区回归均通过。

第一次开发启动因工作区锁竞争失败，0模型请求，原learning-jobs-live-01保留；工作区回归曾与真实付费运行并发而碰到全局账本变动，保持原SHA断言，付费运行结束后完整复验通过，均记入verification-attempts.json。

本轮账本估算费用0.352719元，全研究占用22.108035元，100元内余额77.891965元，历史未知预留仍在，非供应商实付核账。四份风格说明仍为待审稿，汪曾祺双篇稿仍将当篇序言场景泛化为说话人身份，并将可选路线写成段落禁令/自检要求；没有因为流程完成而自动采用。详细报告见research/style-aggregation/reports/learning-jobs-2026-11-02.md。
