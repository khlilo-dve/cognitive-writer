# 学习候选的预览、命名保存与版本回看

本阶段承接2026-10-29已经确定的产品路线，把正文学习核心的输出变成可恢复的本地草稿流程。范围是候选导入、预览、命名保存、版本历史、追加来源后的差异和显式导出；不修改现有learn/REPL默认入口，不新增网页抓取、文档解析或付费模型实验。

## 本阶段合同

- 接收学习核心的正文快照与VoiceDraft，核对1–10篇、去重、正文哈希、执行说明格式和来源一致性。候选包明确记录模型输出/实现者修订/用户修订、审阅文本和变化理由。
- 候选先保存到独立工作区，退出后可重新预览；保存前不改变已有风格。命名保存产生不可覆盖的版本，保存不等于质量验收。
- 更新候选必须引用准确的旧版本，保留原文快照。追加学习只能保留原有来源再加入新来源，不能悄悄删文或替换原文。允许相同来源上的说明修订。已有人保存更新时，旧候选不能覆盖最新版本。
- 差异同时展示来源变化、说明文字变化和调用方提供的变化理由；程序不根据文字差异编造风格变化的语义解释。
- Writer只接收用户显式导出的spec，分析、审阅和原文只在工作区留存。导出使用指定版本和新文件名，拒绝覆盖旧文件，不自动写入或切换仓库styles。
- 文件写入采用进程锁及原子、不可覆盖发布；单次保存只发布一个完整版本文件，中断临时文件不计入历史。损坏、断链、来源身份不符、路径逃逸和过期候选明确失败。

## 命令行开发入口

`cognitive-writer style-draft [--root <workspace>] import <bundle.json> [--base <name> <version>]`接收已有核心产出的数据包，不负责从用户文章文件/URL获取正文。包中有articles正文数组、draft、origin、note，可选review/base。后续提供preview、save、versions、show、diff和export命令。默认工作区为用户目录下`.cognitive-writer/learning`；验证使用独立临时目录。

导入与保存都不会发模型请求。已有learn_voice_with/revise_voice_with的调用方可直接使用工作区API，未来的文件/URL层只需取得正文、调用核心并交付同样的候选，不另造版本机制。

工作区核对的是候选与正文快照的一致性，不鉴定作者归属；origin与note是调用方明确提供的来源声明，不等于模型质量认证。追加时，调用方把旧版本的articles与新正文一起交给学习核心重新综合，再以该旧版本作为base导入新候选；本命令不根据旧说明推测新文章风格。

## 用已有候选体验流程

本轮六份示例包位于`research/style-aggregation/runs/learning-workspace-local-01/examples/`，来自既有真实模型产物，质量仍待审。以下用独立目录演示，不覆盖正式风格：

```bash
target/debug/cognitive-writer style-draft --root /tmp/cw-style-demo import research/style-aggregation/runs/learning-workspace-local-01/examples/wxb-k1.json
# 将输出中的候选ID代入后续命令
target/debug/cognitive-writer style-draft --root /tmp/cw-style-demo preview <candidate-id>
target/debug/cognitive-writer style-draft --root /tmp/cw-style-demo save <candidate-id> 示例声音
target/debug/cognitive-writer style-draft --root /tmp/cw-style-demo import research/style-aggregation/runs/learning-workspace-local-01/examples/wxb-k3.json --base 示例声音 1
target/debug/cognitive-writer style-draft --root /tmp/cw-style-demo diff <new-candidate-id>
target/debug/cognitive-writer style-draft --root /tmp/cw-style-demo save <new-candidate-id> 示例声音
target/debug/cognitive-writer style-draft --root /tmp/cw-style-demo versions 示例声音
target/debug/cognitive-writer style-draft --root /tmp/cw-style-demo export 示例声音 2 /tmp/cw-style-demo-writer-v2.md
```

需要退出时直接结束命令即可，候选会保留；用`candidates`找回候选，用`list`找回已命名风格，用`show <name> <version>`回看旧版本。重复导入同一候选不会重复创建记录，重复保存到已有名称或旧基准会明确失败。目标导出文件已存在时也会失败，不自动覆盖。

## 数据与实现

`LearningWorkspace::stage(CandidateInput)`接收学习核心输出；`save`发布包含正文、执行说明、内部分析、审阅与来源声明的完整JSON版本。候选与版本均有内容身份检查；版本形成连续父链，旧候选与已存版本是独立快照。只有`export`把指定版本的spec写入新Markdown文件。

工作区内的`candidates/*.json`保留可恢复候选，`versions/<名称身份>/v000001.json`等保留版本；名称身份用于安全定位，不改变显示名称。进程锁串行化写入，文件先写完并同步，再以不可覆盖的原子发布方式进入正式路径。未发布的`.pending`文件不计入历史。差异使用有资源上限的逐行比对；很长的说明退回完整变化区段展示，不截断内容，也不自动判断哪条风格更好。

初次CLI验证的夹具共享了可变正文数组，负例修改影响了后面的夹具，产品因此正确拒绝了重复文章；修正夹具隔离后全流程通过。该过程记录在本轮`verification-attempts.json`，不改产品校验、不重抽模型产物。

## 验收

以既有真实1→3→10篇候选做命令行演示，核对重启后的预览、三次保存和历史/差异；另以临时目录验证重复来源、来源减少、旧候选冲突、同名新建、文件损坏、写入失败与导出不覆盖。验证导出字节等于spec且无分析/审阅泄漏。全库测试、构建和既有终端回归通过后检查差异并本地提交。研究数据、报告与飞书记录仍统一在research/style-aggregation；本轮新增API费用应为0，保存前后核对账本哈希。

实际结果：两作者六份历史真实候选的CLI验证通过，含两条1→3→10版本链与并发/过期/损坏/失败恢复检查；275项库测试通过、10项忽略，构建与17项既有真实终端回归通过（HTTP使用本地模拟，不调用付费模型）。CLI工作区已清理，费用账本SHA保持不变。完整结果见本轮研究报告；真实学习质量和Writer事实遵守没有因这些工程验证自动变成通过，文章采集与自然语言多篇入口仍未交付。
