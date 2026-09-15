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
