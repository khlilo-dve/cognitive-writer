# P1 实施契约（预算已批准）

2026-09-26 用户接受100元总预算，前置试跑最多5元包含其中；记录于approval.json。P1先完成离线可靠性，再发起真实请求。

## 研究入口

计划实现 `cargo run -- research validate <manifest>` 与 `cargo run -- research extract <manifest> <run_id> [pilot|full]`。验证不读密钥；运行读取现有环境但不修改.env，研究角色显式指定模型。pilot仅每作者前两篇discovery，full只读discovery；reference/confirmation不进入提取。

数据manifest在目录内引用正文相对路径和SHA256；拒绝越界、重复文章、同文异名重复和内容漂移。保存完整来源元数据；标题、作者与split不进入LLM提取Context。

## 请求和费用

复用现有HTTP传输构造和eval_store::write_json；研究检查点单独使用含role、模型、Prompt、JSON模式、max_completion_tokens、thinking、端点指纹的请求身份。旧评测接口与检查点格式不改变。

研究账本用整数微元，Pro输入/输出单价3/6微元每token，Flash为1/2；输入按UTF-8字节数加协议余量保守预留，输出按显式上限预留。默认Pro开启thinking、详细版输出上限16384；Flash关闭thinking、输出上限4096。仅首次参数核验通过后批量执行。官方参数依据：https://mimo.mi.com/docs/zh-CN/quick-start/usage-guide/text-generation/deep-thinking 。

整个研究根使用OS文件锁，防止两个进程覆盖共享账本；单进程顺序运行P1，之后并发必须继续共享同一账本锁。每次请求前先持久化预留，返回usage后结算；异常或中断保留不确定预留，不自动当零费用重试。请求成功但解析失败也计费并保留响应，不混为完成产物。

研究检查点包含正文、返回模型、finish_reason和usage，不保存reasoning_content、密钥、原始端点或原始HTTP错误正文。usage缺失、返回模型不匹配、超输出上限都停止链并保留费用。无有效响应的重试单独记录attempt，旧损坏检查点不能静默重跑。

SHA256用于内容完整性，不用短非加密指纹假冒正文哈希。新依赖只限必要哈希实现；文件锁用当前Rust标准库。真实请求前验证当前端点属于官方价格渠道，仅记录布尔结果/指纹，避免泄漏完整配置。

## 本阶段验收

数据可重复加载且只提取discovery；临时目录验证账本跨运行累计、超额拒绝、失败保留、检查点漂移拒绝；本地mock验证usage与参数。四篇真实提取成功后复跑零新请求，报告实际费用、长度警告和仍未解决的数据缺口。飞书创建、数据回读和当前用户访问分开验收，不能用bot成功代替用户可读。
