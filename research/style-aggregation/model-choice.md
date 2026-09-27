# 模型选择记录（2026-09-26）

当前决定：继续MiMo 2.6 Pro做困难分析、MiMo 2.6 Flash做文章生成。DS 4.1 Flash是候选，不在正式聚合A/B中途换Writer。现阶段没有相同样本、提示词和生成条件的跨厂商写作对照，不能宣称谁的作者保真最好。

| 每百万token，国内人民币 | 未缓存输入 | 输出 |
|---|---:|---:|
| MiMo 2.6 Flash | 1 | 2 |
| MiMo 2.6 Pro | 3 | 6 |
| DS 4.1 Flash 空闲 | 1 | 4 |
| DS 4.1 Flash 高峰 | 2 | 8 |

来源：[MiMo官方](https://mimo.mi.com/docs/en-US/pricing)、[DeepSeek官方](https://api-docs.deepseek.com/zh-cn/quick_start/pricing/)，2026-09-26核对。DeepSeek当前API名称为deepseek-flash，版本为V4.1-Flash；按官方定义，工作日非中国法定假日9–12点和14–18点为高峰，其余含周末为空闲。旧V4 Flash分数不能当V4.1 Flash实测。

价格只是选择依据之一。MiMo Flash的输出单价低于DS Flash，因此同等token量的写作更便宜；实际性价比应按可用文章计算，包含思考token、重试、失败和人工返工。Pro贵于Flash，但本项目困难提取/判断若做错，会放大到全部后续实验。

用户询问其他模型时，当前不扩大为多厂商排行榜研究。先完成本轮作者风格问题；需要选日常Writer时，另做少量固定Style、新主题、匿名的配对验证。不得为调用DS搜索其他项目密钥或擅改.env。

Lite核对见budget.md：仅Coding套餐不能替代本应用的非Coding实验调用。100元是总上限，不是预充值或预支出。

## 同期基线变化

另一个已提交变更fc5c919按用户要求将单篇规则升级为detailed-v1：2000–5000字目标、1500下限，不因过长压缩。多文章真实调用尚未开始，故在第一次提取前同步采用该基线；旧400–600预算与协议作为历史保留，更新研究版本为v2_detailed。不能同时改变Writer再把效果都归因于聚合。
