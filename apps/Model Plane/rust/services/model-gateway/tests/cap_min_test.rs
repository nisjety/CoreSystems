//! P0-D2-698: the per-run cap rule behind `MODEL_GATEWAY_CAP_MIN`. The case `run_ceiling.rs` pins near its line
//! 247 (a request cap of 50 USD above an org ceiling of 5 USD) keeps the request cap with the switch off, byte for
//! byte as before, and takes the org ceiling with it on: a request can only tighten the org limit. Only the literal
//! `true` turns the switch on.

use model_gateway::budget::{cap_min_enabled, min_cap, CAP_MIN_ENV};
use model_gateway::sse_events::run_ceiling::RunCeiling;

fn switch(value: Option<&'static str>) -> bool {
    cap_min_enabled(&move |key| {
        assert_eq!(key, CAP_MIN_ENV);
        value.map(str::to_owned)
    })
}

#[test]
fn only_the_literal_true_turns_cap_min_on() {
    assert_eq!(CAP_MIN_ENV, "MODEL_GATEWAY_CAP_MIN");
    for off in [
        None,
        Some(""),
        Some("1"),
        Some("TRUE"),
        Some("True"),
        Some("yes"),
        Some(" true"),
        Some("on"),
    ] {
        assert!(!switch(off), "{off:?}");
    }
    assert!(switch(Some("true")));
}

#[test]
fn a_request_cap_above_the_org_ceiling_wins_off_and_loses_on() {
    // Off: the request cap replaces the org ceiling, as `resolve` always did.
    let off = RunCeiling::resolve_with(Some(50.0), None, Some(5.0), None, false);
    assert_eq!(off, RunCeiling::resolve(Some(50.0), None, Some(5.0), None));
    assert_eq!(off.usd, Some(50.0));
    // On: the smaller applies.
    let on = RunCeiling::resolve_with(Some(50.0), None, Some(5.0), None, true);
    assert_eq!(on.usd, Some(5.0));
    // A request tighter than the org keeps its own cap either way.
    assert_eq!(
        RunCeiling::resolve_with(Some(0.5), None, Some(5.0), None, true).usd,
        Some(0.5)
    );
    assert_eq!(
        RunCeiling::resolve_with(Some(0.5), None, Some(5.0), None, false).usd,
        Some(0.5)
    );
}

#[test]
fn tokens_follow_the_same_rule_and_an_unset_side_takes_the_other() {
    assert_eq!(
        RunCeiling::resolve_with(None, Some(5_000), None, Some(1_000), true).tokens,
        Some(1_000)
    );
    assert_eq!(
        RunCeiling::resolve_with(None, Some(5_000), None, Some(1_000), false).tokens,
        Some(5_000)
    );
    // Only one side set: that one, in both modes.
    for cap_min in [false, true] {
        assert_eq!(
            RunCeiling::resolve_with(None, None, Some(5.0), Some(1_000), cap_min),
            RunCeiling {
                usd: Some(5.0),
                tokens: Some(1_000)
            },
            "cap_min={cap_min}"
        );
        assert_eq!(
            RunCeiling::resolve_with(Some(2.0), Some(300), None, None, cap_min),
            RunCeiling {
                usd: Some(2.0),
                tokens: Some(300)
            },
            "cap_min={cap_min}"
        );
        // A cap that is not positive is no cap on either side.
        assert_eq!(
            RunCeiling::resolve_with(Some(0.0), Some(0), Some(-1.0), Some(-5), cap_min),
            RunCeiling::default(),
            "cap_min={cap_min}"
        );
    }
    // An org ceiling beyond u32 still binds a request under it, and does not wrap.
    assert_eq!(
        RunCeiling::resolve_with(None, Some(10), None, Some(i64::MAX), true).tokens,
        Some(10)
    );
}

#[test]
fn the_budget_gate_and_the_run_ceiling_agree_under_cap_min() {
    // The gate's own rule (`min_cap`) and the ceiling's agree on the same inputs.
    assert_eq!(min_cap(Some(50.0), Some(5.0)), Some(5.0));
    assert_eq!(min_cap(Some(0.5), Some(5.0)), Some(0.5));
    assert_eq!(min_cap(None, Some(5.0)), Some(5.0));
    assert_eq!(min_cap(Some(5.0), None), Some(5.0));
    assert_eq!(min_cap::<f64>(None, None), None);
    // An unreadable request cap cannot loosen the org ceiling.
    assert_eq!(min_cap(Some(f64::NAN), Some(5.0)), Some(5.0));
}
