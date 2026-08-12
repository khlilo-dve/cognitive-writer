# Externalize Learn Prompt Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将逆向风格提取的 system prompt 从 `src/learn.rs` 外置到 `prompts/learn_style.md`，并让 `learn` 运行时读取该文件。

**Architecture:** 新增 `prompts/learn_style.md` 作为唯一提示词源。`src/learn.rs` 增加私有 `load_learn_prompt() -> Result<String, String>`，在调用 LLM 前读取、校验非空内容；读取失败直接返回明确错误，不保留源码硬编码回退。架构文档记录该运行时依赖。

**Tech Stack:** Rust、标准库 `std::fs`、现有 `reqwest`/异步 LLM 调用、Cargo lib tests。

## Global Constraints

- 新模块在 `src/lib.rs` 注册；本任务不新增 Rust 模块。
- 公开函数错误类型保持项目既有约定；加载函数为 `learn.rs` 私有函数，沿用该文件现有 `Result<String, String>` 风格。
- 测试放在 `src/learn.rs` 的 `#[cfg(test)] mod tests` 中。
- 不修改 `.env`、密钥、token、CI/CD 配置。
- 不删除用户已有文件；只新增 `prompts/learn_style.md`，修改 `src/learn.rs` 与已同步的 `docs/ARCHITECTURE.md`。
- 完成后运行 `cargo test --lib`，并执行源码搜索确认 `LEARN_SYSTEM_PROMPT` 不再存在。
- 项目规范要求版本变动完成后执行 `git add` 与 `git commit`。

---

## 文件结构与职责

- Create: `prompts/learn_style.md` — 逆向风格提取的唯一运行时 system prompt，保留原提示词完整内容。
- Modify: `src/learn.rs:124-171` — 删除 `LEARN_SYSTEM_PROMPT`，新增提示词加载与非空校验，并将加载结果传给 `call_llm`。
- Modify: `src/learn.rs:206-257` — 增加提示词加载契约测试。
- Modify: `docs/ARCHITECTURE.md:207-223` — 已记录 `prompts/learn_style.md` 来源、失败策略和调用链；实现后仅在行为发生变化时补充。

---

### Task 1: 建立外置提示词文件

**Files:**
- Create: `prompts/learn_style.md`

**Interfaces:**
- Produces: 文件路径 `prompts/learn_style.md`，供 `src/learn.rs` 的运行时加载函数读取。

- [ ] **Step 1: 创建目录和提示词文件**

将 `src/learn.rs` 原 `LEARN_SYSTEM_PROMPT` 的完整字符串内容逐字迁移为 Markdown 正文，不包含 Rust raw string 定界符 `r#"..."#`。文件内容必须包含：

```markdown
你是一个写作风格分析专家。用户会给你一篇完整的文章正文，你需要逆向分析该文章的写作风格，并输出一份可以直接作为 system prompt 使用的「风格指令文档」。

输出要求：
1. 用 Markdown 格式
2. 涵盖以下维度（如果文章中体现了的话）：
   - 整体风格定位（如"理性 + 隐喻"、"口语化 + 犀利"等）
   - 标题策略（标题长度、是否用问句/反问/数字等）
   - 开头模式（故事切入、金句开头、直接观点等）
   - 段落节奏（长短交替、短段密集等）
   - 句式特征（长句/短句偏好、排比、设问等）
   - 论证手法（类比、举例、数据引用、反直觉等）
   - 情绪基调（冷静、激昂、反讽、温暖等）
   - 结尾策略（升华、行动号召、开放式提问等）
   - 用词偏好（口语/书面、中英混用、领域术语等）
   - 读者互动方式（如果有的话）
3. 每个维度给出具体的示例句子或段落片段作为佐证
4. 最后给出一段可直接作为 system prompt 的「风格复刻指令」

注意：不要评价文章质量，只做风格提取和描述。
```

- [ ] **Step 2: 检查文件内容和工作区状态**

Run: `wc -l prompts/learn_style.md && git status --short prompts/learn_style.md`

Expected: 文件存在、约 20 行，Git 状态显示新增文件。

- [ ] **Step 3: Commit**

```bash
git add prompts/learn_style.md
git commit -m "docs: 外置逆向风格提取提示词"
```

---

### Task 2: 让 `learn` 运行时加载外置提示词

**Files:**
- Modify: `src/learn.rs:124-171`
- Test: `src/learn.rs:206-257`

**Interfaces:**
- Produces: 私有函数 `fn load_learn_prompt() -> Result<String, String>`。
- Behavior: 从工作目录下的 `prompts/learn_style.md` 读取 UTF-8 文本；读取失败返回包含文件路径和底层错误的 `Err`；内容为空白时返回明确 `Err`；成功返回原始文本。

- [ ] **Step 1: 先写加载成功测试**

在现有 `tests` 模块中增加：

```rust
#[test]
fn test_load_learn_prompt_from_external_file() {
    let prompt = load_learn_prompt().expect("external learn prompt should be readable");
    assert!(!prompt.trim().is_empty());
    assert!(prompt.contains("写作风格分析专家"));
    assert!(prompt.contains("不要评价文章质量"));
}
```

- [ ] **Step 2: 运行测试确认当前实现缺失**

Run: `cargo test --lib learn::tests::test_load_learn_prompt_from_external_file`

Expected: FAIL，因为 `load_learn_prompt` 尚未定义。

- [ ] **Step 3: 删除硬编码常量并实现加载函数**

删除 `const LEARN_SYSTEM_PROMPT: &str = ...`，在原提示词区块位置加入：

```rust
const LEARN_PROMPT_PATH: &str = "prompts/learn_style.md";

fn load_learn_prompt() -> Result<String, String> {
    let prompt = std::fs::read_to_string(LEARN_PROMPT_PATH).map_err(|e| {
        format!(
            "无法读取逆向风格提取提示词 `{LEARN_PROMPT_PATH}`: {e}"
        )
    })?;

    if prompt.trim().is_empty() {
        return Err(format!(
            "逆向风格提取提示词 `{LEARN_PROMPT_PATH}` 为空"
        ));
    }

    Ok(prompt)
}
```

在 `run_learn` 调用 LLM 前加载：

```rust
let system_prompt = load_learn_prompt().unwrap_or_else(|e| fatal(&e));
let style_analysis = call_llm(
    &client,
    &base_url,
    &api_key,
    &model,
    &system_prompt,
    &article_text,
)
```

不要保留 `LEARN_SYSTEM_PROMPT` 副本，也不要在读取失败时回退到旧字符串。

- [ ] **Step 4: 运行定向测试确认通过**

Run: `cargo test --lib learn::tests::test_load_learn_prompt_from_external_file`

Expected: PASS。

- [ ] **Step 5: 增加源码引用校验并运行完整库测试**

Run: `grep -RIn "LEARN_SYSTEM_PROMPT" src prompts docs || true && cargo test --lib`

Expected:
- grep 无输出；
- 全部 lib tests 通过，0 failed。

- [ ] **Step 6: Commit**

```bash
git add src/learn.rs
git commit -m "refactor: 运行时加载外置风格提取提示词"
```

---

### Task 3: 文档与最终工作区验收

**Files:**
- Verify: `docs/ARCHITECTURE.md`
- Verify: `prompts/learn_style.md`
- Verify: `src/learn.rs`

**Interfaces:**
- Consumes: Task 1 的外置提示词文件和 Task 2 的加载实现。
- Produces: 文档、代码和运行时文件路径一致的可验证闭环。

- [ ] **Step 1: 检查架构文档描述**

Run: `grep -n -A15 -B3 "prompts/learn_style.md" docs/ARCHITECTURE.md`

Expected: 文档明确说明该文件是唯一运行时 system prompt，读取失败不回退到硬编码提示词。

- [ ] **Step 2: 检查实现路径一致性**

Run: `grep -n "LEARN_PROMPT_PATH\|load_learn_prompt\|call_llm" src/learn.rs`

Expected: `LEARN_PROMPT_PATH` 与文档中的 `prompts/learn_style.md` 一致，`call_llm` 使用加载后的 `system_prompt`。

- [ ] **Step 3: 检查 Git 状态并完成最终提交**

```bash
git status --short
git log -3 --oneline
```

Expected: 本功能涉及的文件已提交；用户原有的 `styles/qingbian.md` 修改和 `docs/tiktok_script.pdf` 未被加入本功能提交。

如 Task 1、Task 2 已分别提交，本步骤不再重复提交；若文档在实现后有额外修订，则只提交文档修订：

```bash
git add docs/ARCHITECTURE.md
git commit -m "docs: 记录外置风格提示词来源"
```

- [ ] **Step 4: 最终验证**

Run: `cargo test --lib && git status --short`

Expected: 测试通过；工作区只保留用户原有未提交文件，或无本功能产生的未提交变更。
