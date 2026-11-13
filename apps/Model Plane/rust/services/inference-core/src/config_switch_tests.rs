//! The strict switch reader behind `INFERENCE_BYO_GATEWAY_ENABLED`,
//! `INFERENCE_PROVIDER_BENCHING` and `INFERENCE_SAME_TIER_FALLBACK`: only the literal
//! `true` turns a switch on.

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
