use std::path::Path;

use serde_json::Value;

pub fn transport_label(value: &str) -> String {
    match value {
        "direct_nostr_relay" => "direct nostr relay",
        "radrootsd_proxy" => "radrootsd proxy",
        "local" => "local",
        "preview" => "preview",
        other => other,
    }
    .to_owned()
}

pub fn relay_summary(success: usize, failed: usize, success_label: &str) -> String {
    format!("{success} {success_label} · {failed} failed")
}

pub fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

pub fn ready_blocked(value: bool) -> &'static str {
    if value { "ready" } else { "blocked" }
}

pub fn plural(count: usize, singular: &str, plural: &str) -> String {
    if count == 1 {
        format!("1 {singular}")
    } else {
        format!("{count} {plural}")
    }
}

pub fn quantity_label(amount: impl ToString, unit: Option<&str>) -> String {
    match unit.filter(|unit| !unit.trim().is_empty()) {
        Some(unit) => format!("{} {unit}", amount.to_string()),
        None => format!("quantity {}", amount.to_string()),
    }
}

pub fn price_label(amount: impl ToString, currency: &str, unit: Option<&str>) -> String {
    match unit.filter(|unit| !unit.trim().is_empty()) {
        Some(unit) => format!("{} {currency} / {unit}", amount.to_string()),
        None => format!("{} {currency}", amount.to_string()),
    }
}

pub fn compact_id(value: &str, head: usize, tail: usize) -> String {
    let chars = value.chars().collect::<Vec<_>>();
    if chars.len() <= head + tail + 1 {
        return value.to_owned();
    }
    let start = chars.iter().take(head).collect::<String>();
    let end = chars
        .iter()
        .skip(chars.len().saturating_sub(tail))
        .collect::<String>();
    format!("{start}…{end}")
}

pub fn path_label(path: &Path) -> String {
    path.display().to_string()
}

pub fn string_path<'a>(value: &'a Value, path: &[&str]) -> Option<&'a str> {
    let mut current = value;
    for segment in path {
        current = current.get(*segment)?;
    }
    current.as_str().filter(|value| !value.trim().is_empty())
}

pub fn bool_path(value: &Value, path: &[&str]) -> Option<bool> {
    let mut current = value;
    for segment in path {
        current = current.get(*segment)?;
    }
    current.as_bool()
}

pub fn proof_summary(value: &Value) -> Option<String> {
    if bool_path(
        value,
        &["proof_verification", "cryptographic_proof_verified"],
    ) == Some(true)
    {
        let system = string_path(value, &["proof_verification", "proof_system"])
            .or_else(|| string_path(value, &["receipt", "proof", "system"]))
            .unwrap_or("cryptographic proof");
        return Some(format!("verified · {system}"));
    }
    if string_path(value, &["proof_verification", "state"]).is_some() {
        return Some("available".to_owned());
    }
    None
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn formats_domain_values() {
        assert_eq!(transport_label("direct_nostr_relay"), "direct nostr relay");
        assert_eq!(
            relay_summary(2, 0, "acknowledged"),
            "2 acknowledged · 0 failed"
        );
        assert_eq!(quantity_label("12", Some("lb")), "12 lb");
        assert_eq!(quantity_label("3", None), "quantity 3");
        assert_eq!(price_label("6.00", "CAD", Some("1 lb")), "6.00 CAD / 1 lb");
        assert_eq!(plural(1, "relay", "relays"), "1 relay");
        assert_eq!(plural(2, "relay", "relays"), "2 relays");
    }

    #[test]
    fn compacts_long_ids() {
        assert_eq!(compact_id("1234567890abcdef", 4, 3), "1234…def");
        assert_eq!(compact_id("short", 4, 3), "short");
    }

    #[test]
    fn proof_summary_requires_verified_true_for_verified_label() {
        assert_eq!(
            proof_summary(&json!({
                "proof_verification": {
                    "cryptographic_proof_verified": true,
                    "proof_system": "SP1"
                }
            }))
            .as_deref(),
            Some("verified · SP1")
        );
        assert_eq!(
            proof_summary(&json!({
                "proof_verification": {
                    "state": "valid",
                    "cryptographic_proof_verified": false,
                    "proof_system": "SP1"
                }
            }))
            .as_deref(),
            Some("available")
        );
        assert_eq!(proof_summary(&json!({})), None);
    }
}
