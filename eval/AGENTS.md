# Eval 目录约定

- `baseline/`、`baseline_specs/`、`samples/` 为冻结输入，日常评测只读；建立新基线必须使用新的目录并记录来源。
- `spec_runs/` 是历史交叉生成产物；缺少生成元数据时不得当作可复现的新版本产物，也不得覆盖。
- `runs/{run_id}/` 保存本次自动评测：`run_id` 只允许字母、数字、下划线与连字符。设置 `EVAL_RUN_ID` 续跑同一轮，默认使用 `YYYYMMDDTHHMMSSffffffZ` 时间戳。
- 每个任务保存一个 JSON 检查点，包含请求快照、模型、输出与时间；只保存端点指纹，不保存 API_KEY、Authorization 或原始端点。请求变化时拒绝复用同名检查点，使用新 run_id。
- 文章评分只输入正文，主题与 Spec 独立提供；历史包裹格式必须先校验并提取正文。
- `summary.json` 保存机器可读结果，`summary.md` 保存分差、胜率和人工复核入口。模型评分不能标记为人工校准完成。
- 生成或评分失败终止该依赖链并保留检查点；其他独立链可继续。最多 3 个请求并发，不同时运行同名任务。不得把缺失样本、失败调用或解析失败记作通过。
- `rejected-{task}.json` 保存收到但未通过结构验证的模型输出及原因，不属于成功检查点；`failure-{mode}.json` 保存失败时间。检查完成状态时比较报告时间，不把历史失败或旧汇总当作新结果。
- 正文/Spec 的 `.md` 为检查点的阅读副本，`inputs.json` 冻结本轮输入；候选提示词只在运行目录落盘。
- 本地离线测试只用 tempfile。真实模型任务使用 ignored 测试单独运行，不使用泛化的 `--ignored eval` 批量触发旧基线生成入口。
- 不自动删除历史产物；清理或重新冻结基线需单独确认。

## 人工校准与精简度（本轮授权）
- `human_calibration.json` 保存用户对 Spec 01–03 的原始评分及评价；04–05 不再要求补评，未评数据不伪造为人评。
- 用户原始精简度分数保留；后续运行使用确定性的长度分档，两者分别记录，不回写历史报告。
- 字数包含标题、标点和 Markdown 标记，排除所有空白字符。400–600 字为 5 分；300–399 / 601–700 为 4 分；200–299 / 701–850 为 3 分；100–199 / 851–1000 为 2 分；少于 100 / 超过 1000 为 1 分。满分最低字数为 400，低于即扣分。
- 新校准后实验新建 run_id；不得与 20260905-auto-v1 混用。用户授权修改提取提示词与评分规范，保留冻结输入和原始人评。

三组在线校准命令：`MODEL=deepseek-v4-flash EVAL_RUN_ID=20260912-calibration-v3-fixed EVAL_CONCURRENCY=3 EVAL_TIMEOUT_SECONDS=180 cargo test --lib eval_calibration_suite -- --ignored --nocapture`。请求并发1–3、超时30–900秒可配置，默认分别3和180；这些运输设置不改变请求快照。每个成功checkpoint复用，首次失败记录与后续完成结果并存以便审计。
终端回归产物 `eval/repl-terminal-validation.json` 来自 `scripts/verify_repl_terminal.py`，使用临时目录与本地mock API，不属于模型评分或冻结业务样本。

## 单篇自由格式实验（2026-09-17）
- `fixtures/learn_style_single_v1.md`、`fixtures/eval_judge_learn_v1.md` 为本次变更前的提示词快照，只读；历史人评校准入口继续使用 v1。当前 Rubric B v2 的 coverage 改为证据机制覆盖，不能与 v1 人评分数直接计算校准误差。
- 新入口：`EVAL_RUN_ID=20260917-single-article-v2 EVAL_CONCURRENCY=3 EVAL_TIMEOUT_SECONDS=180 cargo test --lib eval_single_article_learning_suite -- --ignored --nocapture`。同模型重跑旧单阶段与新两阶段，并做五组同主题迁移；题材/生成提示词/Judge固定。
- 两阶段分析/编写原始响应检查点可以包含随后被语义/结构校验拒绝的输出；修复为独立命名请求。只有最终完整 summary 才表示该实验完成，不能把阶段响应视为合格 Spec。最多一次诊断修复；运输失败仍由 Store 有界重试。
- 自由格式不得按十标题得分。最低字数和确定性分档保持不变；未达目标不强行填充，无效执行文本不能入正式库。

- 单阶段自由格式对照：`EVAL_LEARNING_METHOD=direct_free EVAL_RUN_ID=20260917-single-article-direct-v1 cargo test --lib eval_single_article_learning_suite -- --ignored --nocapture`；省略方法变量仍为两阶段。每种候选各用新run_id。
- 产物核验：`python3 scripts/verify_learning_run.py eval/runs/<run_id> --current`（历史不同提示词省略--current）。默认方案的选择以完整结果报告为依据，不因Spec分数上升而忽略换题退化。

## Transfer v2 迁移评测修正（2026-09-18）
- 当前入口：`EVAL_RUN_ID=20260918-transfer-audit-v2 EVAL_CONCURRENCY=3 EVAL_TIMEOUT_SECONDS=180 cargo test --lib eval_transfer_audit_suite -- --ignored --nocapture`。复用旧正文重评15项，同一新题材下strict/creative权限补测30项，四个独立控制；不重新提取Spec。
- 五维分别报告：style_fidelity / source_independence / factual_compliance / naturalness / task_completion。事实与字数不能重复扣主题/风格分；原文污染必须有原文和正文两处可定位证据。程序单独统计600–900非空白字符达标率。
- 不再合成自动采用总分；按任务、维度、配对差异解释。模型控制失败或非事实维度漂移必须如实呈现，不调裁判直到获得期望结论。
- 旧提示词、Spec、文章、历史summary及原人评只读；新评分保存新run_id。阶段评分响应检查点仅表示收到，若证据校验失败会有独立repair请求。只有最终完整summary标记完成。
- 离线检查：`python3 scripts/verify_transfer_audit.py eval/runs/20260918-transfer-audit-v2 --current`；续跑应缓存命中且0新请求。

评分的结构/引用修复耗尽后，错误响应检查点仍保留；不能反复同ID重放同一无效输出。使用新run_id，仅复用已验证请求，排除失败评分请求并记录复用来源SHA；不删除历史失败，也不手工补分。运输失败且未形成有效响应的请求可继续用原ID续跑。本轮最终完整记录为 `20260918-transfer-audit-v2-retry1`，原 `20260918-transfer-audit-v2` 保留44成功/1格式失败的证据。
