# 完整原文直达修订者与范围表达改进

2026-11-02，承接docs/2026-10-31-learning-jobs.md，方法预登记为research/style-aggregation/method-revision-28.md。

## 改动

`voice_revision::revise_voice_with`在范围修订输入中加入规范排序的完整原文，与复读阶段使用同一来源身份。原先修订者只有候选和source_review，无法直接核对复读者是否误解；现在仍不接收旧analysis，保留审阅独立输入。方法升voice-reviewed-v3，指纹和任务计划随方法/提示词变化，不复用旧请求。

四份开发提示词区分当篇缘由和可迁移的表达关系；关系可以穿插，风格禁令也需证据；末尾自检不得重新把正文的可选写法设为必过路线。不增加标题配额、字数指标或模型评分。

旧方法完整任务可以直接找回已有不可变候选；未完成的旧方法仍拒绝静默用新代码继续。新测试和四个历史真实任务的无密钥读取都通过，旧数据不变。

新增研究入口`research learning-quality-probe <run> <source-run>`，从四个完整学习任务中按候选身份取得纯spec，结合固定两新题材和同一事实政策生成10篇Flash稿；上限1元，调用前冻结身份，依赖未完成时拒绝发送。正式默认learn/REPL、styles和.env不改。

## 实际验证与限制

279库测试通过、0失败、10在线入口忽略；构建通过。本地HTTP完整链验证10次本地请求、0付费，覆盖原文直达修订者/分析隔离、冻结恢复、追加、缓存/配置损坏和Ctrl-C。

真实learning-jobs-quality-01在两个Pro首阶段网络中断，0响应；Windows和WSL的无密钥TLS握手均失败，直连TLS1.2也失败。停止付费重试，保留0.251340元未知预留，累计占用22.359375元；10篇Flash因依赖缺失暂缓，不能称新方法质量通过。

另存learning-quality-reviewed-01：四份旧自动候选上的21处实现者修订，每处有原文摘引和偏移。临时工作区导入、0新增来源差异、旧版保护和纯spec导出通过，测试工作区已清理。该四稿不是新自动方法输出，没有Writer执行或独立人评结论。

## 恢复

先对官方端点进行不带密钥的TLS探针。只有可达后，使用同一冻结run的`research learning-jobs learning-jobs-quality-01 retry-uncertain`，已有请求意图/未知预留继续保留，不覆盖原记录；每阶段最多三次。任务完整后使用`research learning-quality-probe learning-quality-probe-01 learning-jobs-quality-01`完成冻结10稿，再质性读稿。若传输仍失败，保持缺失，不改模型或拿人工稿充当自动结果。

## 同日后续：真实恢复已完成

此前失败为历史状态，失败/暂缓和验证摘要都另存。连接恢复后，同冻结四任务实际完成16份Pro响应，10篇Flash用稿也全部完成；来源/版本/纯spec及响应审计、无密钥找回和两轮零请求复跑通过。新增usage估算0.376723元，旧未知预留0.251340元保留，累计占用22.736098元。新方法条件表达局部改善但仍有原文可证的概括错误，10稿事实越界单列，未通过全面质量验收。详情见research/style-aggregation/reports/learning-quality-recovered-2026-11-02.md。
