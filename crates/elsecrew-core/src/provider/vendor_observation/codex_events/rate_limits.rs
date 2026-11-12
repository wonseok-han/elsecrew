//! Account-wide advisory metadata. Never changes authentication or execution authority.
//! Types follow AccountRateLimitsUpdatedNotification; sparse/null fields are valid.
use super::*;

// Deserialize for validation only; no account metadata is retained or published.
#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    limit_id: Option<String>,
    limit_name: Option<String>,
    normal_model_slug: Option<String>,
    plan_type: Option<String>,
    rate_limit_reached_type: Option<String>,
    primary: Option<Window>,
    secondary: Option<Window>,
    credits: Option<Credits>,
    individual_limit: Option<SpendLimit>,
    spend_control_reached: Option<bool>,
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Window {
    used_percent: i32,
    window_duration_mins: Option<i64>,
    resets_at: Option<i64>,
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Credits {
    has_credits: bool,
    unlimited: bool,
    balance: Option<String>,
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpendLimit {
    limit: String,
    used: String,
    remaining_percent: i32,
    resets_at: i64,
}

pub(super) fn validate(raw: &str) -> Result<(), ProviderError> {
    #[derive(Deserialize)]
    struct Params {
        #[serde(rename = "rateLimits")]
        _rate_limits: Snapshot,
    }
    // Additive fields and new display enum values are harmless here: this
    // bounded notification only becomes Private, never a policy decision.
    let _: Params = decode(raw)?;
    Ok(())
}
