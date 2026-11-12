//! Advisory usage metadata, never a completion receipt or tool authorization.
//! Wire shape: app-server ThreadTokenUsageUpdatedNotification JSON schema.
use super::*;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Usage {
    total: Breakdown,
    last: Breakdown,
    model_context_window: Option<u64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Breakdown {
    total_tokens: u64,
    input_tokens: u64,
    cached_input_tokens: u64,
    #[serde(default)]
    cache_write_input_tokens: u64,
    output_tokens: u64,
    reasoning_output_tokens: u64,
}

impl Breakdown {
    fn valid(&self) -> bool {
        [
            self.total_tokens,
            self.input_tokens,
            self.cached_input_tokens,
            self.cache_write_input_tokens,
            self.output_tokens,
            self.reasoning_output_tokens,
        ]
        .into_iter()
        .all(|value| value <= i64::MAX as u64)
    }
}

pub(super) fn validate(raw: &str) -> Result<(), ProviderError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Params {
        token_usage: Usage,
    }
    let params: Params = decode(raw)?;
    let usage = params.token_usage;
    if !usage.total.valid()
        || !usage.last.valid()
        || usage
            .model_context_window
            .is_some_and(|value| value > i64::MAX as u64)
    {
        return Err(invalid());
    }
    Ok(())
}
