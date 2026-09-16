# Context 索引与加载路由表 (manifest.md)

本文件是 `.claude/context/` 的导航目录。**严禁在任务开始时一次性加载全部 Context**。
请先根据任务类型，找到匹配的 Trigger，然后**仅加载单份目标 Context 文件**。

---

## 路由地图

| 任务场景 / 关键词 | 对应 Context 文件 | 核心覆盖范围 | 源码指针建议 |
|---|---|---|---|
| 架构设计、状态机扭转、新增模块、异步循环 | [architecture.md](architecture.md) | 总体拓扑、8 状态机模型、异步架构与入口分流 | `src/repl.rs`, `src/lib.rs`, `src/main.rs` |
| 公众号格式、剪贴板输出、个人网站 MDX 发布 | [domain.md](domain.md) | 业务背景、双发布通道（微信富文本 / Astro 博客） | `src/website.rs`, `src/clipboard.rs` |
| 文章撰写、大纲生成、局部微调、全文重写、意图识别 | [generation.md](generation.md) | 大纲与正文两阶段渲染流水线、意图解析器机制 | `src/generate.rs`, `src/intent.rs`, `src/refine.rs` |
| 写作风格库、逆向风格学习、执行 Spec 注入 | [style-system.md](style-system.md) | Markdown 风格规范、风格逆向学习流程、提示词模板 | `src/styles.rs`, `src/learn.rs`, `prompts/learn_style.md` |
| 评测指标、LLM-Judge、Baseline 样本集、自动化评分 | [evaluation.md](evaluation.md) | Rubric A/B 评测体系、自动化评分引擎、基线比对 | `src/eval.rs`, `eval/`, `prompts/eval_judge_*.md` |
| 架构决策背景、为什么这样做、技术选型历史、技术债详情 | [decisions.md](decisions.md) | 历史架构决策 (ADR)、已知技术债剖析、演进路线 | `docs/ARCHITECTURE.md`, `docs/EVOLUTION.md` |

---

## 读取原则与最佳实践

1. **按需命中**: 只有当修改或排查涉及具体子系统时，才读取对应的单个 Context 文件。
2. **指针先导**: Context 文件只提供架构心智模型、约束和核心函数位置，**绝不包含实现代码全文**。
3. **真相源在代码**: Context 仅负责指路，具体参数签名、结构体字段、返回值请直接使用 `Read` 工具定位具体行号。
4. **同步维护**: Context 文档应与项目当前实现保持一致；架构、业务规则或重要技术决策发生变化时，同步更新对应 Context 文档。
