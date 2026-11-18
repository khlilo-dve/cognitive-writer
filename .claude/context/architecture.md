# 架构与系统分层

`main.rs` 加载环境与参数：有子命令参数进入 CLI，否则进入 REPL（stdin 可为终端或管道）。业务模块仅在 `lib.rs` 注册；main 使用库，避免编译两套模块。

## 状态与输入
- `Idle`：生成、学习、管理风格、编辑文件。
- `WaitingForGenerationStyle`：保留首次写作需求，等待明确风格；支持名称/编号选择、查询列表、取消与保存恢复。
- `WaitingForOutline`：生成中的大纲检查点。常规流程自动衔接正文；旧会话或中断恢复后可确认已有大纲继续，缺少前置内容则报错并可取消。
- `WaitingForFulltext`：审阅正文、修改、换风格或确认。
- `WaitingForEditTarget`：保留局部修改要求与原文，等待明确修改块编号；取消只退出本次修改，返回正文审阅。
- `WaitingForPublish`：本地 Markdown、HTML 及已配置的网站 MDX 写入成功后，才允许发布、仅网站发布、暂缓或取消。
- `WaitingForStyleName`：保存提取内容后等待命名；名称直接处理，不调用分类模型。
- `WaitingForDeleteStyleConfirmation`：只接受 y / yes / 确认删除，其他自然语言不会触发删除。

`repl_input.rs` 的一个独立线程读取 stdin，通过通道交给 REPL。恢复提示与主循环共用读取器；模型请求与信号同时被轮询，Ctrl+C 可取消正在等待的请求。线程不占 Tokio blocking pool，退出不会等待被 stdin 阻塞的任务。

## 保存与错误
等待态的 Ctrl+C、EOF、/quit 保存会话；/cancel 放弃当前操作。保存失败时 /quit、Ctrl+C 保持交互，允许另存或修复后再退出。EOF 时会报告失败；已存在的会话文件不会被半次写入破坏。

会话先写临时文件再替换 current.json；待命名文本与待删除名称通过 serde default 兼容旧 JSON。首次生成的原始需求与候选名称顺序也随会话保存；新增字段带 serde default，旧会话仍兼容。密钥/客户端/路径配置不持久化，恢复时从环境重建。换风格的正文与风格元数据只在模型成功后一起更新。

草稿文件采用原子替换；任一必需写入失败都保留正文确认态，可修复目录后输入「确认」重试。已经成功写出的归档保留，重试创建下一版本；剪贴板失败只影响复制，不影响已落盘草稿。

恢复只将文件不存在视为无会话。读取、解析或配置失败会带原因报错并停止启动，原文件保持原样；不会进入新会话覆盖或清除它。用户可修复文件，或将其移走备份后重新启动。

局部修改请求始终包含完整文章；替换只发生在允许的原文范围。多块响应须全部校验通过才更新正文与必要的标题元数据；错误或中断保留旧稿。待定位指令和候选编号、上次成功修改前的正文均随会话保存；/undo 只在正文仍等于该次修改结果时可用。

源码入口：`src/repl.rs`、`src/repl_input.rs`、`src/intent.rs`、`src/io.rs::write_file_atomic`。

2026-11-03 P1/P2：docs/PRD.md v0.2及docs/designs/style-learning-agent.md先定主子Agent与工具合同。新增learning_sources（不可变集合／范围判定／导出）、learning_source_fetch（有界读取）、learning_source_parse（结构正文候选）、learning_source_cli（collect／collection／export-texts），挂在style-draft开发入口。没有新增stdin、没有接新Agent模型或更改旧learn/REPL；needs_inspection是内部领域检查，不自动追问用户。来源保存于工作区source-collections，恢复读快照不抓取；只有范围满足才导出articles/note。P3动态Agent及主会话交接仍待做。294库测试／10忽略、构建、本地HTTP和17终端回归通过；真实网页质量未验收。

2026-11-03 P3：独立材料域任务learning_agent及types/store/scope/tools/model/cli已接style-draft agent-*开发命令，主会话仍旧REPL。短状态CAS锁与执行锁分离，模型等待不阻止取消；父身份状态链、输入修订、冻结决策、collection步骤缓存及既有请求日志可恢复。明确材料0模型，复杂Flash／单次Pro、12决策／2无进展／2主动澄清；完整候选已在观察可判断，片段先inspect。终点ready_for_learning而非候选／保存，ready_texts才返回完整正文。309库测试／10忽略、本地协议与少量真实Flash决策留证，P4/P5和质量P7待做。入口和失败说明见docs/designs/style-learning-agent.md第8/9节及研究reports/learning-agent-2026-11-03.md。

2026-11-09 P4：style-draft chat复用Repl与唯一stdin；WaitingForLearningAgent只保存ChatRef，父会话learning_chat及parse/reply/store/types拥有不可变输入／状态链及任务引用，P3仍拥有材料／问题与预算。复杂解释Flash经既有请求／费用适配，简单输入不加模型；缺材料及失败材料的问题均计入原领域两轮。支持原话、保存／写作要求保留、同任务回答／修订、暂停／继续／取消／明确替换，恢复不重新问或隐式重发未知请求。321库测试／10忽略、真实四小例与终端回归留证，P5下游交接未接，旧默认未切换。详见技术设计第10节及研究reports/learning-chat-2026-11-09.md。

2026-11-18 默认入口开放。无参数启动继续读取原 `current.json`，明确学习请求才在同一个主会话创建 `ChatRef`。学习数据与 `style-draft chat` 共用默认工作区，两个入口各有独立的会话文件。普通写作仍走主意图与已有生成流程。已命名工作区风格可在主会话选用，同名时显式选择。旧命名态和未完成稿件可恢复。采用决定见 `docs/PRD.md` D09。
