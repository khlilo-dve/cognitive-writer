# 产品与业务背景 (domain.md)

## 产品定位
Cognitive Writer 是一个专为创作者打造的对话式写作 Agent，核心使命是：**将高价值思想/零散灵感，通过预设或逆向学得的行文风格，转化为结构严谨、可直接排版发表的内容资产。**

## 双通道交付模型

最终内容面向两个主要消费端，二者在格式与排版上有明确区分：

### 1. 微信公众号排版通道 (`src/clipboard.rs`, `outputs/*.html`)
- **诉求**: 微信公众平台不支持直接贴 Markdown，需要将 Markdown 转换为带内联样式的特定 HTML。
- **机制**:
  - 生成 `.html` 文件并借助系统剪贴板直接注入富文本格式。
  - 用户只需在微信编辑器直接 `Ctrl+V`，即可保留标题字号、加粗着色、引用块与代码块格式。

### 2. 个人独立博客发布通道 (`src/website.rs`, Astro + MDX)
- **目标工程**: 宿主机器上的 Astro/Starlight 个人网站项目（由环境变量 `WEBSITE_PATH` 指定）。
- **机制**:
  - 正文确认通过后，自动在 `$WEBSITE_PATH/src/content/blog/` 下写入符合 YAML Frontmatter 规范的 `.mdx` 文件。
  - 触发本地 `git add`, `git commit` 以及远程同步（需用户授权机制保证安全）。

## 输出产物与存储约定
- `inputs/`: 用户放置的原始选题、灵感短句或知识点大纲。
- `outputs/{slug}_v{N}.md`: 每次生成的 Markdown 正文草稿。
- `outputs/{slug}_v{N}.html`: 微信公众号富文本排版 HTML。

## 何时需要查阅代码
- 涉及微信排版样式调整：查阅 `src/clipboard.rs` 中的富文本内联样式渲染逻辑。
- 涉及个人网站 MDX Frontmatter 字段变更或推送逻辑：查阅 `src/website.rs`。
