# 自由格式提示词迭代与换模型交接（2026-09-22）

本地实现和验证已完成，最终采用验证尚未完成。当前候选为v8，默认学习方法仍为legacy；未覆盖或新增正式风格。DS接口在第八轮正文生成时持续返回HTTP 402，最小请求确认包含余额/额度不足信号。用户计划后续换MiMo，已停止DS线上请求，不修改.env，不再自动重试额度错误。

## 已完成的改进

自由组织执行Markdown，不强制十个维度；400–600非空白字符目标与最低长度扣分不变。保留声音、句法、信息安排与修辞自由，禁止迁移样文剧情和结论。依赖事实的技法按素材条件执行；完稿同时检查新增事实和遗漏的必写数量、身份、地点与结果。v8进一步区分第一人称判断与亲历身份，保留“我认为”和自嘲，限制无依据的亲见、回忆和现场对白。

发布评测实现已完成：同模型新旧提取、strict/creative迁移、原始请求检查点、分项均值与配对胜平负、固定门槛、开发通过后冻结Spec在预登记新主题确认。没有更改Rubric B v2、Transfer v2或生产生成包装来提高候选分数。Judge仅对两个诊断字段的单字符串/数组做无损兼容，原始分数与引用不改。

## 开发结果

下表全部为模型评分，不是新增人评。每轮候选各5篇strict、5篇creative；旧版相同模型请求精确复用42个，SHA与来源保存在各run。旧版strict均值为4.2/4.8/4.6/4.0/4.6，creative为4.0/5.0/5.0/4.0/5.0；依次为风格、原文独立、事实、自然度、素材覆盖。不合成总分。

| 候选 | 场景 | 风格 | 原文独立 | 事实 | 自然度 | 素材覆盖 | 字数达标 |
|---|---|---:|---:|---:|---:|---:|---:|
| [v2](../eval/runs/20260921-free-dev-v2/summary.md) | strict | 3.8 | 5.0 | 4.6 | 3.8 | 5.0 | 4/5 |
| [v2](../eval/runs/20260921-free-dev-v2/summary.md) | creative | 4.2 | 5.0 | 5.0 | 4.2 | 5.0 | 5/5 |
| [v3](../eval/runs/20260921-free-dev-v3/summary.md) | strict | 3.4 | 5.0 | 4.4 | 4.0 | 4.8 | 4/5 |
| [v3](../eval/runs/20260921-free-dev-v3/summary.md) | creative | 4.2 | 5.0 | 5.0 | 4.2 | 4.8 | 5/5 |
| [v4](../eval/runs/20260921-free-dev-v4/summary.md) | strict | 3.8 | 5.0 | 4.4 | 4.0 | 5.0 | 5/5 |
| [v4](../eval/runs/20260921-free-dev-v4/summary.md) | creative | 4.2 | 5.0 | 5.0 | 4.2 | 4.8 | 5/5 |
| [v5](../eval/runs/20260922-free-dev-v5/summary.md) | strict | 4.2 | 5.0 | 4.0 | 4.0 | 5.0 | 5/5 |
| [v5](../eval/runs/20260922-free-dev-v5/summary.md) | creative | 4.0 | 5.0 | 5.0 | 4.2 | 5.0 | 5/5 |
| [v6](../eval/runs/20260922-free-dev-v6/summary.md) | strict | 4.2 | 5.0 | 4.4 | 3.8 | 4.6 | 5/5 |
| [v6](../eval/runs/20260922-free-dev-v6/summary.md) | creative | 4.0 | 5.0 | 5.0 | 4.2 | 5.0 | 5/5 |
| [v7](../eval/runs/20260922-free-dev-v7/summary.md) | strict | 3.8 | 5.0 | 4.6 | 3.8 | 5.0 | 5/5 |
| [v7](../eval/runs/20260922-free-dev-v7/summary.md) | creative | 4.2 | 5.0 | 5.0 | 4.4 | 5.0 | 5/5 |

六轮完整开发对照均保留未过线证据：v2/v3/v4/v7主要为strict风格，v5为strict事实，v6为strict素材覆盖。尚未有候选同时通过全部先登记条件，因此没有启动确认题材，也不能宣称本阶段已经验收。

v8已完成五组新旧提取与Spec评分；保留54个成功请求检查点，但候选正文与迁移评分未完成。partial.json中的10条成功行均为复用的旧版文章，不能当作候选成功。此轮不是新提示词评分失败，是接口额度中断。

## 继续方式

不需要重做研发，也不需要重跑v2–v7。提示词、代码、机械校验和失败诊断都可继续使用。

### 若继续使用相同DS接口与模型

恢复可用额度后，从原ID补齐未完成请求，已有54个成功检查点复用：

```bash
EVAL_RUN_ID=20260922-free-dev-v8 EVAL_RELEASE_PHASE=development EVAL_TIMEOUT_SECONDS=180 cargo test --lib eval_free_style_release_suite -- --ignored --nocapture
```

至少还缺29个生成/评分请求，不含可能的格式修复与运输重试。通过开发门槛后另需20篇确认文章，至少60个请求；充值金额无法由这些数量推定，不承诺这一轮一定通过。

### 换MiMo或其他模型

用户自行更新.env里的可用接口与准确模型ID。以当前v8提示词新建开发run，重新做一次当前候选与旧提示词的同模型对照；不重跑历史候选，不把DS分数或检查点混入新模型：

```bash
EVAL_RUN_ID=mimo-free-v8-dev-01 EVAL_RELEASE_PHASE=development EVAL_TIMEOUT_SECONDS=180 cargo test --lib eval_free_style_release_suite -- --ignored --nocapture
python3 scripts/verify_free_style_release.py eval/runs/mimo-free-v8-dev-01 --current
```

新模型无需重做此前工程和诊断；重新提取与评分是模型适配验证。不要使用DS原run_id，也不要只改检查点中的模型名。同一个模型与请求才可以续用缓存。

### 开发过线后

候选提示词和Spec冻结，在尚未使用的共享雨伞任务上确认：

```bash
EVAL_RUN_ID=mimo-free-v8-confirm-01 EVAL_RELEASE_PHASE=confirmation EVAL_RELEASE_SOURCE_RUN=mimo-free-v8-dev-01 EVAL_TIMEOUT_SECONDS=180 cargo test --lib eval_free_style_release_suite -- --ignored --nocapture
python3 scripts/verify_free_style_release.py eval/runs/mimo-free-v8-confirm-01 --current
```

确认同样通过后，才切默认direct_free，并将五份候选以独立名称加入风格库，保留旧风格。若确认失败，保留结果、继续优化，下次确认换新主题。以上名字只是run标识，不是API模型ID。

## 本地验证与边界

- cargo test --lib：218通过、0失败、10个真实API入口按设计忽略；二进制构建通过。
- 真实终端回归：17项会话/草稿检查、11项风格选择、14项局部修改/自然语言撤销，全通过；均为本地mock，不产生线上费用。
- 六轮完整产物核验通过；v8已接收产物与中断状态单独核验。历史Transfer v2的45篇/4控制/109检查点核验通过，当前提取提示词不同，因此历史核验不用--current。
- 23个冻结输入、原人评、旧风格、裁判与生成包装文件与原HEAD逐字节一致。原自由提示词保存在eval/fixtures/learn_style_direct_v1.md，迭代快照随各run保存。
- 验证索引：eval/free-style-release-validation.json；终端报告为eval/free-style-{repl,selection,local-edit}-validation.json。

旧版仍为默认；用户已授权最终达标后采用新版，不需再次审批。当前等待的是可用模型接口，不是新增人工评分。单篇范围保持不变，多文章过拟合/欠拟合实验未开展。
