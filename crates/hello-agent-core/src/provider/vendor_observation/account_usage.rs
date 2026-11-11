//! Display-only quota windows, never credentials or execution authorization.
use super::*;
use serde_json::{json, Value};
pub fn codex(input: &BoundedObservation) -> Result<Value, ProviderError> {
    let value = input.json()?;
    let snapshot = value
        .get("rateLimits")
        .and_then(Value::as_object)
        .ok_or_else(|| ProviderError("usage unavailable".into()))?;
    let mut windows = Vec::new();
    for key in ["primary", "secondary"] {
        let Some(window) = snapshot.get(key).filter(|v| !v.is_null()) else {
            continue;
        };
        let percent = window["usedPercent"]
            .as_f64()
            .filter(|v| v.is_finite() && *v >= 0.0 && *v <= 100.0)
            .ok_or_else(|| ProviderError("invalid usage percentage".into()))?;
        let minutes = window
            .get("windowDurationMins")
            .filter(|v| !v.is_null())
            .map(|v| {
                v.as_u64()
                    .filter(|v| *v > 0 && *v <= 525600)
                    .ok_or_else(|| ProviderError("invalid usage window".into()))
            })
            .transpose()?;
        let reset = window
            .get("resetsAt")
            .filter(|v| !v.is_null())
            .map(|v| {
                v.as_u64()
                    .filter(|v| *v <= 253402300799)
                    .ok_or_else(|| ProviderError("invalid usage reset".into()))
            })
            .transpose()?;
        windows
            .push(json!({"id":key,"usedPercent":percent,"windowMinutes":minutes,"resetsAt":reset}));
    }
    Ok(json!({"windows": windows}))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn usage_is_bounded_and_does_not_expose_account_fields() {
        let read =
            |v: Value| codex(&BoundedObservation::new(serde_json::to_vec(&v).unwrap()).unwrap());
        let out = read(json!({"rateLimits":{"planType":"private","credits":{"balance":"secret"},"primary":{"usedPercent":31,"windowDurationMins":300,"resetsAt":123}}})).unwrap();
        assert_eq!(
            out,
            json!({"windows":[{"id":"primary","usedPercent":31.0,"windowMinutes":300,"resetsAt":123}]})
        );
        assert!(read(json!({"rateLimits":{"primary":{"usedPercent":-1}}})).is_err());
        assert!(read(json!({"rateLimits":{"primary":{"usedPercent":101}}})).is_err());
        assert!(read(json!({"rateLimits":{"primary":{"usedPercent":"1"}}})).is_err());
        assert_eq!(
            read(json!({"rateLimits":{}})).unwrap(),
            json!({"windows":[]})
        );
    }
}
