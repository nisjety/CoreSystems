//! How the `USAGE_EMIT_ENABLED` switch reads, defined once for every producer.
//!
//! The deployment passes the switch as `0` or `1` (compose: `${USAGE_EMIT_ENABLED:-0}`), but the
//! environment parser the services use for their configuration (`envy`) accepts only `true` and
//! `false` for a boolean and fails the whole configuration on anything else, so a plain `bool`
//! field would stop a service at startup on the compose default. This reads both spellings, and
//! an empty value (a variable compose passes through empty) as off. Anything else is an error that
//! says what is accepted: a typo must not silently leave metering off, or on.

use serde::de::Error as _;
use serde::{Deserialize, Deserializer};

/// For `#[serde(default, deserialize_with = "usage_ledger_rs::deserialize_flag")]`.
pub fn deserialize_flag<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" => Ok(true),
        "0" | "false" | "" => Ok(false),
        _ => Err(D::Error::custom(
            "a usage ledger switch is 1 or true to turn it on, 0 or false (or empty) to leave it off",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::deserialize_flag;
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    struct Switch {
        #[serde(default, deserialize_with = "deserialize_flag")]
        on: bool,
    }

    fn read(value: &str) -> Result<bool, serde_json::Error> {
        serde_json::from_value::<Switch>(serde_json::json!({ "on": value })).map(|s| s.on)
    }

    #[test]
    fn both_spellings_of_a_boolean_and_an_empty_value_read_right() {
        for (value, expected) in [
            ("1", true),
            ("true", true),
            ("TRUE", true),
            (" 1 ", true),
            ("0", false),
            ("false", false),
            ("False", false),
            ("", false),
            ("  ", false),
        ] {
            assert_eq!(read(value).unwrap(), expected, "{value:?}");
        }
    }

    #[test]
    fn a_typo_is_an_error_not_a_silent_off() {
        for value in ["yes", "on", "2", "enabled", "tru", "-1"] {
            let error = read(value).expect_err(value);
            assert!(error.to_string().contains("1 or true"), "{error}");
        }
    }

    #[test]
    fn an_absent_variable_is_off() {
        assert!(
            !serde_json::from_value::<Switch>(serde_json::json!({}))
                .unwrap()
                .on
        );
    }
}
