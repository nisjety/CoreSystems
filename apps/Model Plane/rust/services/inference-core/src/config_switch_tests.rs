//! The strict switch reader behind `INFERENCE_BYO_GATEWAY_ENABLED`,
//! `INFERENCE_PROVIDER_BENCHING` and `INFERENCE_SAME_TIER_FALLBACK`: only the literal
//! `true` turns a switch on. The residency opt-ins (`truthy_env`) follow the same rule.
//! The legacy ZDR boolean (`resolve_zdr`) asserts ZDR only on the literal `true`, reads the
//! explicit off words (`false`, `0`, `off` and `no`, in any case and padded) as no claim, and
//! refuses any other value rather than reading it as off.

use super::*;

#[test]
fn the_routing_switches_are_on_only_for_the_literal_true() {
    assert!(byo_gateway_switch_on(Some("true")));
    for raw in [
        None,
        Some(""),
        Some("1"),
        Some("TRUE"),
        Some("True"),
        Some("on"),
        Some("yes"),
        Some("false"),
        Some("true1"),
        Some("t rue"),
    ] {
        assert!(
            !byo_gateway_switch_on(raw),
            "{raw:?} must not enable the switch"
        );
    }
}

#[test]
fn default_off_boolean_flags_are_on_only_for_the_exact_literal_true() {
    // `truthy_env` reads MODEL_PLANE_ALLOW_GLOBAL_DEPLOYMENT, MODEL_PLANE_ALLOW_GLOBAL_RESIDENCY
    // and MODEL_PLANE_ALLOW_NON_EU_EMBEDDING. A probe name keeps the real switches out of it.
    const PROBE: &str = "INFERENCE_CORE_SWITCH_PROBE_EXACT_LITERAL";
    assert!(!truthy_env(PROBE), "an unset variable is off");
    for raw in [
        "1", " true", "true ", "TRUE", "True", "yes", "on", "false", "0", "",
    ] {
        std::env::set_var(PROBE, raw);
        assert!(!truthy_env(PROBE), "{raw:?} must not enable the flag");
    }
    std::env::set_var(PROBE, "true");
    assert!(truthy_env(PROBE), "the exact literal true enables the flag");
    std::env::remove_var(PROBE);
}

/// The legacy boolean asserts ZDR only on the exact literal `true`. Unset and the explicit off
/// words make no claim, padded and in any case: they are the same words `parse_switch` reads as
/// off, because both take `mp_contracts::loop_fold::EXPLICIT_OFF_WORDS`. Every other spelling is
/// refused, never read as off: a `1` or `TRUE` read as off would boot without the claim its
/// operator meant, which is the silent drop this reader exists to prevent.
#[test]
fn legacy_zdr_boolean_reads_only_its_known_spellings() {
    const LEGACY: &str = "AZURE_OPENAI_ZDR_CONFIRMED";
    let read = |raw: Option<&str>| legacy_zdr_asserted(LEGACY, raw.map(OsStr::new));
    assert_eq!(read(None), Ok(false), "unset makes no claim");
    for &off in mp_contracts::loop_fold::EXPLICIT_OFF_WORDS {
        assert_eq!(read(Some(off)), Ok(false), "{off:?} makes no claim");
    }
    for off in [" false ", "FALSE", "Off", " NO ", "\tno\n", "   "] {
        assert_eq!(read(Some(off)), Ok(false), "{off:?} makes no claim");
    }
    assert_eq!(
        read(Some("true")),
        Ok(true),
        "the exact literal asserts ZDR"
    );
    for other in [
        "1", "TRUE", "True", " true", "true ", "yes", "on", "ture", "onn", "offf", "enforce",
    ] {
        assert_eq!(
            read(Some(other)),
            Err(ZdrAttestationError::UnreadableLegacyBoolean { legacy_var: LEGACY }),
            "{other:?} must be refused, not read as off"
        );
    }
}

/// A value the OS holds as bytes that are not UTF-8 is not a spelling this reader knows, so it is
/// refused in the same way rather than read as off.
#[cfg(unix)]
#[test]
fn legacy_zdr_boolean_that_is_not_utf8_is_refused() {
    use std::os::unix::ffi::OsStrExt;
    const LEGACY: &str = "AZURE_OPENAI_ZDR_CONFIRMED";
    assert_eq!(
        legacy_zdr_asserted(LEGACY, Some(OsStr::from_bytes(b"\xff"))),
        Err(ZdrAttestationError::UnreadableLegacyBoolean { legacy_var: LEGACY })
    );
}

/// As above, on Windows, where an unpaired UTF-16 surrogate is not valid Unicode.
#[cfg(windows)]
#[test]
fn legacy_zdr_boolean_that_is_not_utf8_is_refused() {
    use std::os::windows::ffi::OsStringExt;
    const LEGACY: &str = "AZURE_OPENAI_ZDR_CONFIRMED";
    let raw = std::ffi::OsString::from_wide(&[0xD800]);
    assert_eq!(
        legacy_zdr_asserted(LEGACY, Some(raw.as_os_str())),
        Err(ZdrAttestationError::UnreadableLegacyBoolean { legacy_var: LEGACY })
    );
}

/// The boot decision for each spelling. Off spellings boot without ZDR. Any other spelling stops
/// the boot as unreadable, and the exact literal `true` stops it as an unbacked assertion.
#[test]
fn legacy_zdr_boolean_boots_without_zdr_only_for_its_off_spellings() {
    const PREFIX: &str = "INFERENCE_CORE_SWITCH_PROBE";
    const LEGACY: &str = "INFERENCE_CORE_SWITCH_PROBE_ZDR_CONFIRMED";
    let today = chrono::NaiveDate::from_ymd_opt(2026, 11, 17).expect("static date");
    std::env::remove_var(LEGACY);
    assert!(
        matches!(resolve_zdr("test", PREFIX, LEGACY, today), Ok(None)),
        "unset must not assert ZDR"
    );
    for &raw in mp_contracts::loop_fold::EXPLICIT_OFF_WORDS {
        std::env::set_var(LEGACY, raw);
        assert!(
            matches!(resolve_zdr("test", PREFIX, LEGACY, today), Ok(None)),
            "{raw:?} must not assert ZDR"
        );
    }
    for raw in [" false ", "FALSE", "Off", " NO ", "\tno\n"] {
        std::env::set_var(LEGACY, raw);
        assert!(
            matches!(resolve_zdr("test", PREFIX, LEGACY, today), Ok(None)),
            "{raw:?} must not assert ZDR"
        );
    }
    for raw in ["1", " true", "TRUE", "yes", "on", "ture"] {
        std::env::set_var(LEGACY, raw);
        let error = resolve_zdr("test", PREFIX, LEGACY, today)
            .expect_err("a spelling this reader does not know must stop the boot");
        assert!(
            matches!(
                error.downcast_ref::<ZdrAttestationError>(),
                Some(ZdrAttestationError::UnreadableLegacyBoolean { .. })
            ),
            "{raw:?} must be refused as unreadable, not read as off: {error:#}"
        );
    }
    std::env::set_var(LEGACY, "true");
    assert!(
        resolve_zdr("test", PREFIX, LEGACY, today).is_err(),
        "the exact literal true is an unbacked assertion and fails boot"
    );
    std::env::remove_var(LEGACY);
}
