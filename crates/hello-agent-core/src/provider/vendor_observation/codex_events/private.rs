//! Validate non-public echo/reasoning shapes without retaining or publishing text.
use super::{decode, invalid, ProviderError};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
enum Item {
    #[serde(rename = "userMessage")]
    User {
        #[serde(rename = "id")]
        _id: String,
        #[serde(rename = "clientId")]
        _client_id: Option<String>,
        #[serde(rename = "content")]
        _content: Vec<Text>,
    },
    #[serde(rename = "reasoning")]
    Reasoning {
        #[serde(rename = "id")]
        _id: String,
        #[serde(rename = "summary")]
        _summary: Vec<String>,
        #[serde(rename = "content")]
        _content: Vec<String>,
    },
}
#[derive(Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
enum Text {
    #[serde(rename = "text")]
    Text {
        #[serde(rename = "text")]
        _text: String,
        #[serde(rename = "text_elements")]
        _elements: Vec<serde::de::IgnoredAny>,
    },
}
pub(super) fn validate_item(raw: &str) -> Result<(), ProviderError> {
    let _: Item = decode(raw)?;
    Ok(())
}
pub(super) fn validate_delta(method: &str, raw: &str) -> Result<(), ProviderError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Delta {
        #[serde(rename = "threadId")]
        _thread: String,
        #[serde(rename = "turnId")]
        _turn: String,
        #[serde(rename = "itemId")]
        _item: String,
        delta: Option<String>,
        content_index: Option<u32>,
        summary_index: Option<u32>,
    }
    let value: Delta = decode(raw)?;
    let valid = match method {
        "item/reasoning/textDelta" => {
            value.delta.is_some() && value.content_index.is_some() && value.summary_index.is_none()
        }
        "item/reasoning/summaryTextDelta" => {
            value.delta.is_some() && value.summary_index.is_some() && value.content_index.is_none()
        }
        "item/reasoning/summaryPartAdded" => {
            value.delta.is_none() && value.summary_index.is_some() && value.content_index.is_none()
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(invalid())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn incomplete_duplicate_or_tool_shaped_items_are_not_discardable() {
        for raw in [
            r#"{"type":"reasoning","id":"i","content":[]}"#,
            r#"{"type":"reasoning","id":"i","summary":[],"content":[],"content":["PRIVATE"]}"#,
            r#"{"type":"reasoning","id":"i","summary":[],"content":[{}]}"#,
            r#"{"type":"userMessage","id":"i","content":[{"type":"localImage","path":"PRIVATE"}]}"#,
            r#"{"type":"userMessage","id":"i","content":[{"type":"text","text":"PRIVATE"}]}"#,
            r#"{"type":"reasoning","id":"i","summary":[],"content":[],"command":"PRIVATE"}"#,
        ] {
            let error = validate_item(raw).unwrap_err();
            assert!(!error.to_string().contains("PRIVATE"));
        }
    }
}
