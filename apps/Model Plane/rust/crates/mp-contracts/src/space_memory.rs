//! Space memory containment flag (build plan WS17-S1).
//!
//! `MEMORY_SPACE_CONTAINMENT` is a kill switch, not an authorization input: containment is
//! ON unless the variable is set to exactly `0`, `false` or `off` (case-insensitive,
//! trimmed). An unset, empty or unrecognised value keeps containment on, so a typo can never
//! widen what a shared room may learn or recall.

/// Environment variable read by session-core.
pub const MEMORY_SPACE_CONTAINMENT_ENV: &str = "MEMORY_SPACE_CONTAINMENT";

/// Pure form of [`containment_enabled`]: `value` is the raw variable, `None` when unset.
#[must_use]
pub fn containment_enabled_from(value: Option<&str>) -> bool {
    !matches!(
        value.map(|raw| raw.trim().to_ascii_lowercase()).as_deref(),
        Some("0" | "false" | "off")
    )
}

/// Whether Space memory containment is on for this process.
#[must_use]
pub fn containment_enabled() -> bool {
    containment_enabled_from(std::env::var(MEMORY_SPACE_CONTAINMENT_ENV).ok().as_deref())
}

/// Environment variable of the audience label switch (WS17-S3, `MEMORY_AUDIENCE_LABELS`).
pub const MEMORY_AUDIENCE_LABELS_ENV: &str = "MEMORY_AUDIENCE_LABELS";

/// Pure form of [`audience_labels_enabled`]: `value` is the raw variable, `None` when unset.
///
/// The opposite default to containment: OFF unless the variable is exactly `1`, `true` or `on`
/// (case-insensitive, trimmed). It gates the label stamps and the label predicate only; it is a
/// rollout control, never an authorization input.
#[must_use]
pub fn audience_labels_enabled_from(value: Option<&str>) -> bool {
    matches!(
        value.map(|raw| raw.trim().to_ascii_lowercase()).as_deref(),
        Some("1" | "true" | "on")
    )
}

/// Whether memory audience labels are switched on for this process.
#[must_use]
pub fn audience_labels_enabled() -> bool {
    audience_labels_enabled_from(std::env::var(MEMORY_AUDIENCE_LABELS_ENV).ok().as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn containment_is_off_only_for_exactly_zero_false_or_off() {
        for off in ["0", "false", "off"] {
            assert!(!containment_enabled_from(Some(off)), "{off} switches it off");
        }
    }

    #[test]
    fn the_off_words_are_case_and_whitespace_insensitive() {
        for off in ["FALSE", "False", "OFF", "Off", " 0 ", "\tfalse\n", "  OFF  "] {
            assert!(!containment_enabled_from(Some(off)), "{off:?} switches it off");
        }
    }

    #[test]
    fn everything_else_keeps_containment_on() {
        assert!(containment_enabled_from(None), "unset is on");
        for on in [
            "", " ", "1", "true", "on", "yes", "enabled", "no", "disabled", "00", "0.0", "falsey",
            "of", "offf", "off off", "-0",
        ] {
            assert!(containment_enabled_from(Some(on)), "{on:?} must keep containment on");
        }
    }

    #[test]
    fn the_variable_name_is_the_documented_one() {
        assert_eq!(MEMORY_SPACE_CONTAINMENT_ENV, "MEMORY_SPACE_CONTAINMENT");
    }

    #[test]
    fn audience_labels_are_on_only_for_1_true_or_on() {
        for on in ["1", "true", "on"] {
            assert!(audience_labels_enabled_from(Some(on)), "{on:?} switches labels on");
        }
    }

    #[test]
    fn the_audience_label_on_words_are_case_and_whitespace_insensitive() {
        for on in ["TRUE", "True", "ON", "On", " 1 ", "\ttrue\n", "  ON  "] {
            assert!(audience_labels_enabled_from(Some(on)), "{on:?} switches labels on");
        }
    }

    #[test]
    fn audience_labels_are_off_when_unset_or_any_other_value() {
        assert!(!audience_labels_enabled_from(None), "unset is off");
        for off in [
            "", " ", "0", "false", "off", "yes", "enabled", "2", "tru", "oon", "1 2", "-1",
        ] {
            assert!(!audience_labels_enabled_from(Some(off)), "{off:?} keeps labels off");
        }
    }

    #[test]
    fn the_audience_label_variable_name_is_the_documented_one() {
        assert_eq!(MEMORY_AUDIENCE_LABELS_ENV, "MEMORY_AUDIENCE_LABELS");
    }
}
