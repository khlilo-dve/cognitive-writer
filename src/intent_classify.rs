//! LLM 意图分类提示词与响应校验。

use crate::error::AppError;
use crate::intent::{Intent, SessionState};
use crate::llm::call_llm;
use reqwest::Client;

/// LLM 意图分类的 system prompt。
/// `{state}` 与 `{valid_intents}` 占位符在运行时注入。
const INTENT_CLASSIFY_PROMPT: &str = r#"你是「对话式写作 Agent」的意图分类器。把用户输入映射到一个意图，只输出 JSON。

当前对话状态：{state}
{valid_intents}

## 全部意图及字段
- Generate：想写/生成文章。字段 topic（主题及篇幅、受众等写作要求）、style_name（用户明确指定的风格名，必须摘自原话；省略或不确定则留空）
- Learn：想从某个链接学写作风格。字段 url（http/https 链接）
- ListStyles：想看有哪些风格
- ShowStyle：想看某风格详情/摘要。字段 name
- DeleteStyle：想删除某风格。字段 name
- RefineFile：想局部修改某个 .md 文件。字段 path、instruction
- UpdateFile：想整体重写某个 .md 文件。字段 path、instruction
- ModifySection：想修改当前文章某部分。字段 instruction
- ChangeStyle：想换成另一风格重写。字段 style_name
- Publish：确认发布
- PublishWebsiteOnly：只想发布到网站
- Hold：暂缓/先不发布
- Confirm：确认当前操作
- Undo：明确要求撤销上一次局部修改，恢复修改前的正文；无字段
- Cancel：取消当前操作
- Unknown：与写作无关、或无法理解的输入。字段 reason（一句话说明原因）

## 识别要点
- 识别口语、同义、省略表达："整一篇/来一篇/帮我写/写个"=Generate；"拉倒/算了算了/不搞了"=Cancel；"行/好/OK/对"=Confirm
- 输入与写作完全无关（闲聊、查天气等）→ Unknown，reason 写「与写作无关」
- 输入像指令但含糊不清 → Unknown，reason 写「无法理解意图」
- 已明确想写文章但未说明风格，仍返回 Generate，style_name 留空，不得返回 Unknown。
- 不得猜测、补全默认风格；“犀利点/轻松点”等只保留在 topic 中，不当作具体风格名。被否定的风格（如“不要轻辩”）不算选择。
- 正文审阅中，“刚才改得不好，还是恢复之前的那版”等明确回退请求 → Undo，不能映射为 Cancel 或 ModifySection。仅询问能否撤销、否定撤销、多步撤销或同时撤销又提出新修改时 → Unknown，reason 请用户明确当前要执行的操作。
- 只输出一行 JSON，不要解释、不要 Markdown 代码块

## few-shot 示例（输入 → 输出）
"写一篇关于 AI 的文章，用轻辩风格" → {"intent_type":"Generate","topic":"关于 AI 的文章","style_name":"轻辩"}
"帮我整一篇认知偏差的，犀利点" → {"intent_type":"Generate","topic":"认知偏差，犀利点","style_name":""}
"学一下这个风格 https://example.com/a" → {"intent_type":"Learn","url":"https://example.com/a"}
"风格库里有什么" → {"intent_type":"ListStyles"}
"今天天气怎么样" → {"intent_type":"Unknown","reason":"与写作无关"}
"恢复到刚才修改之前吧" → {"intent_type":"Undo"}
"不要撤销，保留现在这版" → {"intent_type":"Unknown","reason":"保留当前内容，不执行撤销"}
"行" → {"intent_type":"Confirm"}
"拉倒吧" → {"intent_type":"Cancel"}"#;

/// 每个状态下的有效意图，注入分类提示词，帮助模型在状态机下正确分类。
fn state_valid_intents(state: &SessionState) -> &'static str {
    match state {
        SessionState::Idle => {
            "有效意图：Generate（写文章）、Learn（学风格）、ListStyles、ShowStyle、DeleteStyle、RefineFile、UpdateFile、Cancel。"
        }
        SessionState::WaitingForOutline => "有效意图：Confirm（确认大纲）、Cancel。",
        SessionState::WaitingForFulltext => {
            "有效意图：Confirm（确认全文）、ModifySection（改某段）、ChangeStyle（换风格）、Undo（撤销上次局部修改）、Cancel。"
        }
        SessionState::WaitingForPublish => {
            "有效意图：Publish（发布）、PublishWebsiteOnly（只发网站）、Hold（暂缓）、Cancel。"
        }
        SessionState::WaitingForEditTarget => "由 REPL 本地处理局部修改目标选择。",
        SessionState::WaitingForGenerationStyle => "由 REPL 本地处理风格选择输入。",
        SessionState::WaitingForStyleName => "由 REPL 本地处理风格名称输入。",
        SessionState::WaitingForDeleteStyleConfirmation => "由 REPL 本地处理删除确认。",
    }
}

/// 使用 LLM 将自然语言输入分类为 Intent。
///
/// 发送极小的 classification prompt，期望 50 token 内的 JSON 响应。
/// 分类失败时返回 Err，由调用方提示服务不可用；本地交互状态不调用模型。
pub async fn classify_intent(
    client: &Client,
    base_url: &str,
    api_key: &str,
    model: &str,
    input: &str,
    state: &SessionState,
) -> Result<Intent, AppError> {
    let state_str = match state {
        SessionState::Idle => "空闲，等待用户发起操作",
        SessionState::WaitingForOutline => "等待用户确认大纲",
        SessionState::WaitingForFulltext => "等待用户确认全文",
        SessionState::WaitingForPublish => "等待用户确认发布",
        SessionState::WaitingForEditTarget
        | SessionState::WaitingForGenerationStyle
        | SessionState::WaitingForStyleName
        | SessionState::WaitingForDeleteStyleConfirmation => {
            return Err(AppError::Intent(
                "当前状态的输入必须由 REPL 本地处理".into(),
            ));
        }
    };

    let system_prompt = INTENT_CLASSIFY_PROMPT
        .replace("{state}", state_str)
        .replace("{valid_intents}", state_valid_intents(state));
    let user_prompt = format!("用户输入：{}", input);

    let raw = call_llm(
        client,
        base_url,
        api_key,
        model,
        &system_prompt,
        &user_prompt,
    )
    .await?;

    parse_classify_response(&raw, input)
}

/// 解析 LLM 返回的 JSON → Intent。
fn parse_classify_response(raw: &str, original_input: &str) -> Result<Intent, AppError> {
    let json_str = raw
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    let v: serde_json::Value = serde_json::from_str(json_str).map_err(|e| {
        AppError::Parse(format!(
            "LLM 分类 JSON 解析失败: {} — raw: {}",
            e,
            raw.chars().take(200).collect::<String>()
        ))
    })?;

    let intent_type = v["intent_type"].as_str().unwrap_or("Unknown");

    match intent_type {
        "Generate" => {
            let topic = v["topic"].as_str().unwrap_or("").trim().to_string();
            let proposed = v["style_name"].as_str().unwrap_or("").trim();
            // 分类器返回的名称必须能在原话中找到；凭空补出的风格进入追问。
            let style_name = if !proposed.is_empty()
                && original_input
                    .to_lowercase()
                    .contains(&proposed.to_lowercase())
            {
                proposed.to_string()
            } else {
                String::new()
            };
            if topic.is_empty() {
                return Err(AppError::Intent("LLM 分类 Generate 缺少 topic".into()));
            }
            Ok(Intent::Generate { topic, style_name })
        }
        "Learn" => {
            let url = v["url"].as_str().unwrap_or("").to_string();
            if url.is_empty() {
                return Err(AppError::Intent("LLM 分类 Learn 缺少 url".into()));
            }
            Ok(Intent::Learn { url })
        }
        "ListStyles" => Ok(Intent::ListStyles),
        "ShowStyle" => {
            let name = v["name"].as_str().unwrap_or("").to_string();
            Ok(Intent::ShowStyle { name })
        }
        "DeleteStyle" => {
            let name = v["name"].as_str().unwrap_or("").to_string();
            Ok(Intent::DeleteStyle { name })
        }
        "RefineFile" => {
            let path = v["path"].as_str().unwrap_or("").to_string();
            let instruction = v["instruction"].as_str().unwrap_or("").to_string();
            Ok(Intent::RefineFile { path, instruction })
        }
        "UpdateFile" => {
            let path = v["path"].as_str().unwrap_or("").to_string();
            let instruction = v["instruction"].as_str().unwrap_or("").to_string();
            Ok(Intent::UpdateFile { path, instruction })
        }
        "ModifySection" => {
            let instruction = v["instruction"].as_str().unwrap_or("").to_string();
            Ok(Intent::ModifySection { instruction })
        }
        "ChangeStyle" => {
            let style_name = v["style_name"].as_str().unwrap_or("").to_string();
            Ok(Intent::ChangeStyle { style_name })
        }
        "Publish" => Ok(Intent::Publish),
        "PublishWebsiteOnly" => Ok(Intent::PublishWebsiteOnly),
        "Hold" => Ok(Intent::Hold),
        "Confirm" => Ok(Intent::Confirm),
        "Cancel" => Ok(Intent::Cancel),
        "Undo" => Ok(Intent::Undo),
        _ => {
            // LLM 无法分类 → 携带原因，供上层准确报错
            let reason = v["reason"].as_str().unwrap_or("无法理解意图").to_string();
            Ok(Intent::Unknown { reason })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_classify_rejects_local_interaction_states_without_network() {
        let client = Client::new();
        for state in [
            SessionState::WaitingForEditTarget,
            SessionState::WaitingForGenerationStyle,
            SessionState::WaitingForStyleName,
            SessionState::WaitingForDeleteStyleConfirmation,
        ] {
            let error = classify_intent(&client, "invalid-url", "", "", "用户输入", &state)
                .await
                .unwrap_err();
            assert!(matches!(error, AppError::Intent(_)));
            assert!(error.to_string().contains("REPL 本地处理"));
        }
    }

    #[test]
    fn test_parse_classify_response_invalid_unicode_returns_error() {
        let raw = "中".repeat(201);
        let error = parse_classify_response(&raw, "test input").unwrap_err();
        assert!(matches!(error, AppError::Parse(_)));
        assert!(error.to_string().ends_with(&"中".repeat(200)));
        assert!(!error.to_string().contains(&"中".repeat(201)));
    }

    #[test]
    fn test_parse_classify_response_generate() {
        let raw = r#"{"intent_type": "Generate", "topic": "AI", "style_name": "轻辩"}"#;
        let result = parse_classify_response(raw, "写 AI，用轻辩风格").unwrap();
        match result {
            Intent::Generate { topic, style_name } => {
                assert_eq!(topic, "AI");
                assert_eq!(style_name, "轻辩");
            }
            other => panic!("Expected Generate, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_classify_response_confirm() {
        let raw = r#"{"intent_type": "Confirm"}"#;
        let result = parse_classify_response(raw, "test input").unwrap();
        assert!(matches!(result, Intent::Confirm));
    }

    #[test]
    fn test_parse_classify_response_unknown() {
        let raw = r#"{"intent_type": "Unknown"}"#;
        let result = parse_classify_response(raw, "test input").unwrap();
        assert!(matches!(result, Intent::Unknown { .. }));
    }

    #[test]
    fn test_parse_classify_response_unknown_with_reason() {
        let raw = r#"{"intent_type": "Unknown", "reason": "与写作无关"}"#;
        let result = parse_classify_response(raw, "test input").unwrap();
        match result {
            Intent::Unknown { reason } => assert_eq!(reason, "与写作无关"),
            other => panic!("Expected Unknown, got {:?}", other),
        }
    }

    #[test]
    fn test_state_valid_intents_covers_all_states() {
        for state in [
            SessionState::Idle,
            SessionState::WaitingForOutline,
            SessionState::WaitingForFulltext,
            SessionState::WaitingForPublish,
            SessionState::WaitingForEditTarget,
            SessionState::WaitingForGenerationStyle,
            SessionState::WaitingForStyleName,
            SessionState::WaitingForDeleteStyleConfirmation,
        ] {
            assert!(!state_valid_intents(&state).is_empty());
        }
    }

    #[test]
    fn test_parse_classify_response_code_fenced() {
        // LLM 可能用 ```json 包裹输出
        let raw = r#"```json
{"intent_type": "Confirm"}
```"#;
        let result = parse_classify_response(raw, "test input").unwrap();
        assert!(matches!(result, Intent::Confirm));
    }

    #[test]
    fn generate_without_a_supported_style_keeps_topic_for_clarification() {
        for raw in [
            r#"{"intent_type":"Generate","topic":"AI，800字"}"#,
            r#"{"intent_type":"Generate","topic":"AI，800字","style_name":"   "}"#,
            r#"{"intent_type":"Generate","topic":"AI，800字","style_name":"模型编造的风格"}"#,
        ] {
            match parse_classify_response(raw, "写 AI，800字").unwrap() {
                Intent::Generate { topic, style_name } => {
                    assert_eq!(topic, "AI，800字");
                    assert!(style_name.is_empty());
                }
                other => panic!("unexpected {other:?}"),
            }
        }
        assert!(
            parse_classify_response(r#"{"intent_type":"Generate","topic":" "}"#, "写文章").is_err()
        );
    }

    #[test]
    fn parses_model_undo_intent() {
        assert!(matches!(
            parse_classify_response(
                r#"{"intent_type":"Undo"}"#,
                "刚才改得不好，还是恢复之前的那版"
            )
            .unwrap(),
            Intent::Undo
        ));
    }
}
