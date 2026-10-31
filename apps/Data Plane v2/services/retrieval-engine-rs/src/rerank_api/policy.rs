//! The decision order for `POST /v1/rerank` (QB-T05a). Pure: no I/O, no clock, no state.
//!
//! The first failing check decides the skip reason. Every skip is an HTTP 200 with
//! `status = skipped`, the input order and zero provider calls; only a `Proceed` makes
//! the one provider call.
//!
//!   1. the route is switched on (`WEB_RERANK_ENABLED`)              -> `disabled`
//!   2. the endpoint passes the route-level Azure rule and a key exists -> `misconfigured`
//!   3. the processor is approved (`WEB_RERANK_PROCESSOR_APPROVED`)     -> `processor_unapproved`
//!   4. a signed metering path exists, so the call can be recorded      -> `metering_unavailable`
//!   5. effective ZDR (signed token `zdr` OR a restrictive body `zdr_mode`) and no
//!      attestation of the provider for ZDR content                    -> `zdr`
//!   6. `sovereign_required` (absent means strict; a signed `sovereign=true` floors it)  -> `sovereign`
//!   7. the provider circuit is closed                                  -> `circuit_open`

/// Every input the decision reads, as plain booleans.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PolicyInput {
    pub enabled: bool,
    pub endpoint_ok: bool,
    pub processor_approved: bool,
    pub metering_available: bool,
    /// The verified token's `zdr` claim (org-level posture as minted, never a body value).
    pub token_zdr: bool,
    /// The body's `zdr_mode` restricts egress (`ephemeral` or `reject`).
    pub body_zdr_restricts: bool,
    /// The provider is attested for ZDR content (Z-41 wires the real answer; `false` today).
    pub attested_for_zdr: bool,
    pub sovereign_required: bool,
    pub circuit_open: bool,
}

/// Why a call did not run. The wire strings are a closed, stable list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    Disabled,
    Misconfigured,
    ProcessorUnapproved,
    MeteringUnavailable,
    Zdr,
    Sovereign,
    CircuitOpen,
}

impl SkipReason {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Misconfigured => "misconfigured",
            Self::ProcessorUnapproved => "processor_unapproved",
            Self::MeteringUnavailable => "metering_unavailable",
            Self::Zdr => "zdr",
            Self::Sovereign => "sovereign",
            Self::CircuitOpen => "circuit_open",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Proceed,
    Skip(SkipReason),
}

/// Effective ZDR for this call: the signed token OR the body's restrictive mode.
#[must_use]
pub const fn effective_zdr(input: &PolicyInput) -> bool {
    input.token_zdr || input.body_zdr_restricts
}

/// The whole decision. See the module comment for the order.
#[must_use]
pub fn decide(input: &PolicyInput) -> Verdict {
    if !input.enabled {
        return Verdict::Skip(SkipReason::Disabled);
    }
    if !input.endpoint_ok {
        return Verdict::Skip(SkipReason::Misconfigured);
    }
    if !input.processor_approved {
        return Verdict::Skip(SkipReason::ProcessorUnapproved);
    }
    if !input.metering_available {
        return Verdict::Skip(SkipReason::MeteringUnavailable);
    }
    if effective_zdr(input) && !input.attested_for_zdr {
        return Verdict::Skip(SkipReason::Zdr);
    }
    if input.sovereign_required {
        return Verdict::Skip(SkipReason::Sovereign);
    }
    if input.circuit_open {
        return Verdict::Skip(SkipReason::CircuitOpen);
    }
    Verdict::Proceed
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An independent statement of the same order, written as a list of conditions so the
    /// matrix below checks `decide` against something other than itself.
    fn oracle(input: &PolicyInput) -> Verdict {
        let checks: [(bool, SkipReason); 7] = [
            (input.enabled, SkipReason::Disabled),
            (input.endpoint_ok, SkipReason::Misconfigured),
            (input.processor_approved, SkipReason::ProcessorUnapproved),
            (input.metering_available, SkipReason::MeteringUnavailable),
            (
                !(effective_zdr(input) && !input.attested_for_zdr),
                SkipReason::Zdr,
            ),
            (!input.sovereign_required, SkipReason::Sovereign),
            (!input.circuit_open, SkipReason::CircuitOpen),
        ];
        for (passes, reason) in checks {
            if !passes {
                return Verdict::Skip(reason);
            }
        }
        Verdict::Proceed
    }

    fn all_inputs() -> Vec<PolicyInput> {
        let mut out = Vec::with_capacity(512);
        for bits in 0u16..512 {
            let bit = |n: u16| bits & (1 << n) != 0;
            out.push(PolicyInput {
                enabled: bit(0),
                endpoint_ok: bit(1),
                processor_approved: bit(2),
                metering_available: bit(3),
                token_zdr: bit(4),
                body_zdr_restricts: bit(5),
                attested_for_zdr: bit(6),
                sovereign_required: bit(7),
                circuit_open: bit(8),
            });
        }
        out
    }

    #[test]
    fn decide_matches_the_independent_oracle_on_every_one_of_512_cells() {
        for input in all_inputs() {
            assert_eq!(decide(&input), oracle(&input), "{input:?}");
        }
    }

    #[test]
    fn a_proceed_is_only_ever_reached_through_every_gate() {
        for input in all_inputs() {
            if decide(&input) == Verdict::Proceed {
                assert!(input.enabled && input.endpoint_ok && input.processor_approved);
                assert!(input.metering_available);
                assert!(!effective_zdr(&input) || input.attested_for_zdr);
                assert!(!input.sovereign_required && !input.circuit_open);
            }
        }
    }

    #[test]
    fn a_restrictive_posture_never_proceeds_unless_the_provider_is_attested() {
        for input in all_inputs() {
            let restricted = input.token_zdr || input.body_zdr_restricts;
            if restricted && !input.attested_for_zdr {
                assert_ne!(decide(&input), Verdict::Proceed, "{input:?}");
            }
        }
    }

    #[test]
    fn the_first_failing_gate_names_the_reason() {
        let open = PolicyInput {
            enabled: true,
            endpoint_ok: true,
            processor_approved: true,
            metering_available: true,
            token_zdr: false,
            body_zdr_restricts: false,
            attested_for_zdr: false,
            sovereign_required: false,
            circuit_open: false,
        };
        assert_eq!(decide(&open), Verdict::Proceed);
        assert_eq!(
            decide(&PolicyInput {
                enabled: false,
                sovereign_required: true,
                ..open
            }),
            Verdict::Skip(SkipReason::Disabled)
        );
        assert_eq!(
            decide(&PolicyInput {
                endpoint_ok: false,
                circuit_open: true,
                ..open
            }),
            Verdict::Skip(SkipReason::Misconfigured)
        );
        assert_eq!(
            decide(&PolicyInput {
                processor_approved: false,
                ..open
            }),
            Verdict::Skip(SkipReason::ProcessorUnapproved)
        );
        assert_eq!(
            decide(&PolicyInput {
                metering_available: false,
                body_zdr_restricts: true,
                ..open
            }),
            Verdict::Skip(SkipReason::MeteringUnavailable)
        );
        assert_eq!(
            decide(&PolicyInput {
                body_zdr_restricts: true,
                sovereign_required: true,
                ..open
            }),
            Verdict::Skip(SkipReason::Zdr),
            "ZDR is decided before sovereignty"
        );
        assert_eq!(
            decide(&PolicyInput {
                token_zdr: true,
                ..open
            }),
            Verdict::Skip(SkipReason::Zdr),
            "a signed zdr token restricts even when the body says disabled"
        );
        assert_eq!(
            decide(&PolicyInput {
                sovereign_required: true,
                circuit_open: true,
                ..open
            }),
            Verdict::Skip(SkipReason::Sovereign)
        );
        assert_eq!(
            decide(&PolicyInput {
                circuit_open: true,
                ..open
            }),
            Verdict::Skip(SkipReason::CircuitOpen)
        );
    }

    #[test]
    fn an_attested_provider_admits_zdr_content_only_when_every_other_gate_passes() {
        let attested = PolicyInput {
            enabled: true,
            endpoint_ok: true,
            processor_approved: true,
            metering_available: true,
            token_zdr: true,
            body_zdr_restricts: true,
            attested_for_zdr: true,
            sovereign_required: false,
            circuit_open: false,
        };
        assert_eq!(decide(&attested), Verdict::Proceed);
        assert_eq!(
            decide(&PolicyInput {
                sovereign_required: true,
                ..attested
            }),
            Verdict::Skip(SkipReason::Sovereign)
        );
    }

    #[test]
    fn skip_reasons_are_the_closed_wire_list() {
        let all = [
            SkipReason::Disabled,
            SkipReason::Misconfigured,
            SkipReason::ProcessorUnapproved,
            SkipReason::MeteringUnavailable,
            SkipReason::Zdr,
            SkipReason::Sovereign,
            SkipReason::CircuitOpen,
        ];
        let names: Vec<&str> = all.iter().map(|r| r.as_str()).collect();
        assert_eq!(
            names,
            [
                "disabled",
                "misconfigured",
                "processor_unapproved",
                "metering_unavailable",
                "zdr",
                "sovereign",
                "circuit_open"
            ]
        );
    }
}
