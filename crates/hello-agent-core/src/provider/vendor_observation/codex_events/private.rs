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
        // ThreadItem flattens optional lifecycle metadata into every item.
        // These timestamps are discarded, never used as completion evidence.
        #[serde(rename = "startedAtMs")]
        _started_at_ms: Option<i64>,
        #[serde(rename = "completedAtMs")]
        _completed_at_ms: Option<i64>,
        #[serde(rename = "clientId")]
        _client_id: Option<String>,
        #[serde(rename = "content")]
        _content: Vec<Text>,
    },
    #[serde(rename = "reasoning")]
    Reasoning {
        #[serde(rename = "id")]
        _id: String,
        // ThreadItem flattens optional lifecycle metadata into every item.
        // These timestamps are discarded, never used as completion evidence.
        #[serde(rename = "startedAtMs")]
        _started_at_ms: Option<i64>,
        #[serde(rename = "completedAtMs")]
        _completed_at_ms: Option<i64>,
        #[serde(default, rename = "summary")]
        _summary: Vec<String>,
        #[serde(default, rename = "content")]
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
        // Vendor UserInput.Text defaults omitted display spans to an empty array.
        #[serde(default, rename = "text_elements")]
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
    fn user_text_elements_default_to_empty_as_in_vendor_schema() {
        for raw in [
            r#"{"type":"userMessage","id":"i","content":[{"type":"text","text":"PRIVATE"}]}"#,
            r#"{"type":"userMessage","id":"i","clientId":null,"content":[{"type":"text","text":"PRIVATE","text_elements":[]}]}"#,
        ] {
            validate_item(raw).unwrap();
        }
    }
    #[test]
    fn private_items_accept_only_schema_typed_lifecycle_times() {
        for kind in ["userMessage", "reasoning"] {
            let base = if kind == "userMessage" {
                serde_json::json!({"type":kind,"id":"i","content":[{"type":"text","text":"PRIVATE"}]})
            } else {
                serde_json::json!({"type":kind,"id":"i","content":["PRIVATE"]})
            };
            for field in ["startedAtMs", "completedAtMs"] {
                for value in [serde_json::Value::Null, serde_json::json!(1793343295000i64)] {
                    let mut item = base.clone();
                    item[field] = value;
                    validate_item(&item.to_string()).unwrap();
                }
                for value in [
                    serde_json::json!("PRIVATE"),
                    serde_json::json!(true),
                    serde_json::json!(1.5),
                    serde_json::json!([]),
                    serde_json::json!(u64::MAX),
                ] {
                    let mut item = base.clone();
                    item[field] = value;
                    let error = validate_item(&item.to_string()).unwrap_err();
                    assert!(!error.to_string().contains("PRIVATE"));
                }
                let prefix = base.to_string();
                let duplicate = format!(
                    "{},\"{}\":1,\"{}\":2}}",
                    &prefix[..prefix.len() - 1],
                    field,
                    field
                );
                assert!(validate_item(&duplicate).is_err());
            }
        }
    }
    #[test]
    fn reasoning_arrays_default_to_empty_as_in_vendor_schema() {
        for raw in [
            r#"{"type":"reasoning","id":"i"}"#,
            r#"{"type":"reasoning","id":"i","content":[]}"#,
            r#"{"type":"reasoning","id":"i","summary":[]}"#,
        ] {
            validate_item(raw).unwrap();
        }
    }
    #[test]
    fn incomplete_duplicate_or_tool_shaped_items_are_not_discardable() {
        for raw in [
            r#"{"type":"reasoning","id":"i","summary":null,"content":[]}"#,
            r#"{"type":"reasoning","id":"i","summary":[],"content":[],"content":["PRIVATE"]}"#,
            r#"{"type":"reasoning","id":"i","summary":[],"content":[{}]}"#,
            r#"{"type":"userMessage","id":"i","content":[{"type":"localImage","path":"PRIVATE"}]}"#,
            r#"{"type":"userMessage","id":"i","content":[{"type":"text","text":"PRIVATE","text_elements":null}]}"#,
            r#"{"type":"userMessage","id":"i","content":[{"type":"text","text":"PRIVATE","text_elements":{}}]}"#,
            r#"{"type":"userMessage","id":"i","content":[{"type":"text","text":"PRIVATE","text_elements":[],"text_elements":[]}]}"#,
            r#"{"type":"reasoning","id":"i","summary":[],"content":[],"command":"PRIVATE"}"#,
        ] {
            let error = validate_item(raw).unwrap_err();
            assert!(!error.to_string().contains("PRIVATE"));
        }
    }
}
