# 架构与系统分层

`main.rs` 加载环境与参数：有子命令参数进入 CLI，否则进入 REPL（stdin 可为终端或管道）。业务模块仅在 `lib.rs` 注册；main 使用库，避免编译两套模块。

## 状态与输入
- `Idle`：生成、学习、管理风格、编辑文件。
- `WaitingForGenerationStyle`：保留首次写作需求，等待明确风格；支持名称/编号选择、查询列表、取消与保存恢复。
- `WaitingForOutline`：生成中的大纲检查点。常规流程自动衔接正文；旧会话或中断恢复后可确认已有大纲继续，缺少前置内容则报错并可取消。
- `WaitingForFulltext`：审阅正文、修改、换风格或确认。
- `WaitingForPublish`：本地 Markdown、HTML 及已配置的网站 MDX 写入成功后，才允许发布、仅网站发布、暂缓或取消。
- `WaitingForStyleName`：保存提取内容后等待命名；名称直接处理，不调用分类模型。
- `WaitingForDeleteStyleConfirmation`：只接受 y / yes / 确认删除，其他自然语言不会触发删除。

`repl_input.rs` 的一个独立线程读取 stdin，通过通道交给 REPL。恢复提示与主循环共用读取器；模型请求与信号同时被轮询，Ctrl+C 可取消正在等待的请求。线程不占 Tokio blocking pool，退出不会等待被 stdin 阻塞的任务。

## 保存与错误
等待态的 Ctrl+C、EOF、/quit 保存会话；/cancel 放弃当前操作。保存失败时 /quit、Ctrl+C 保持交互，允许另存或修复后再退出。EOF 时会报告失败；已存在的会话文件不会被半次写入破坏。

会话先写临时文件再替换 current.json；待命名文本与待删除名称通过 serde default 兼容旧 JSON。首次生成的原始需求与候选名称顺序也随会话保存；新增字段带 serde default，旧会话仍兼容。密钥/客户端/路径配置不持久化，恢复时从环境重建。换风格的正文与风格元数据只在模型成功后一起更新。

草稿文件采用原子替换；任一必需写入失败都保留正文确认态，可修复目录后输入「确认」重试。已经成功写出的归档保留，重试创建下一版本；剪贴板失败只影响复制，不影响已落盘草稿。

恢复只将文件不存在视为无会话。读取、解析或配置失败会带原因报错并停止启动，原文件保持原样；不会进入新会话覆盖或清除它。用户可修复文件，或将其移走备份后重新启动。

源码入口：`src/repl.rs`、`src/repl_input.rs`、`src/intent.rs`、`src/io.rs::write_file_atomic`。
