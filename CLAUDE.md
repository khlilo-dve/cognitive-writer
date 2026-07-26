每次完成版本变动后都需要进行git add和commit

## 项目概述
- Rust 对话式写作 Agent (v3.0.0)，面向微信公众号 + 个人网站(khlilo.xyz)
- 14 个源文件，124 个单元测试，REPL 自然语言交互 + CLI 快捷模式
- 4 状态对话状态机: Idle → WaitingForOutline → WaitingForFulltext → WaitingForPublish

## 开发规范
- 新模块在 src/lib.rs 注册（库入口），src/main.rs 只做 REPL/CLI 分流
- 公开函数使用 `crate::error::AppError`（非 String），保持错误类型统一
- 测试放在各模块的 `#[cfg(test)] mod tests` 中，用 tempfile 做文件 I/O 测试
- 模块拆分原则：每个文件职责单一，< 500 行为佳，repl.rs 作为编排层可以是例外

## 运行方式
- REPL 模式: `cargo run`
- CLI 快捷: `cargo run -- generate|learn|refine|update [...]`
- 全量测试: `cargo test --lib`
- 单模块测试: `cargo test -- <module_name>`

## 环境变量 (.env)
- API_KEY (必填), BASE_URL (默认 openai), MODEL (默认 gpt-4o)
- WEBSITE_PATH (可选，个人网站项目路径，为空则跳过网站发布)

## 技术债
- run_learn/run_update 内部 process::exit(1) 会杀死 REPL，待重构为 Result
- 各模块有私有的 env_var/md_to_wechat_html 副本，待提取到 utils.rs
- clap 依赖已无使用方（CLI 用手工参数解析），可移除

## 网站集成
- 个人网站: Next.js 16 + next-mdx-remote, content/signal/ 目录
- 部署: git push → Vercel 自动部署，不需要 Deploy Hook
- 公众号: CF_HTML 注入剪贴板，不做 API 集成