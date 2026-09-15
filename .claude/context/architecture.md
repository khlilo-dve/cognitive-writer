# 架构与系统分层

`main.rs` 加载环境与参数：有子命令参数进入 CLI，否则进入 REPL（stdin 可为终端或管道）。业务模块仅在 `lib.rs` 注册；main 使用库，避免编译两套模块。

## 状态与输入
- `Idle`：生成、学习、管理风格、编辑文件。
- `WaitingForOutline`：生成中的大纲检查点。常规流程自动衔接正文；旧会话或中断恢复后可确认已有大纲继续，缺少前置内容则报错并可取消。
- `WaitingForFulltext`：审阅正文、修改、换风格或确认。
- `WaitingForPublish`：发布、仅网站发布、暂缓或取消。
- `WaitingForStyleName`：保存提取内容后等待命名；名称直接处理，不调用分类模型。
- `WaitingForDeleteStyleConfirmation`：只接受 y / yes / 确认删除，其他自然语言不会触发删除。

`repl_input.rs` 的一个独立线程读取 stdin，通过通道交给 REPL。恢复提示与主循环共用读取器；模型请求与信号同时被轮询，Ctrl+C 可取消正在等待的请求。线程不占 Tokio blocking pool，退出不会等待被 stdin 阻塞的任务。

## 保存与错误
等待态的 Ctrl+C、EOF、/quit 保存会话；/cancel 放弃当前操作。保存失败时 /quit、Ctrl+C 保持交互，允许另存或修复后再退出。EOF 时会报告失败；已存在的会话文件不会被半次写入破坏。

会话先写临时文件再替换 current.json；待命名文本与待删除名称通过 serde default 兼容旧 JSON。密钥/客户端/路径配置不持久化，恢复时从环境重建。换风格的正文与风格元数据只在模型成功后一起更新。

源码入口：`src/repl.rs`、`src/repl_input.rs`、`src/intent.rs`、`src/io.rs::write_file_atomic`。
