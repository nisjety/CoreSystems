//! Tests for the web-policy lattice (WS7.B). Written in the build, run in the
//! wave's final test phase. The golden test regenerates the catalogue with
//! `UPDATE_CONTRACT_GOLDEN=1 cargo test -p quarry-core --test web_policy`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use proptest::prelude::*;
use quarry_core::contracts::AgentAction;
use quarry_core::error::{ErrorCode, QuarryError};
use quarry_core::web_policy::{
    action_class, action_tag, catalogue_json, zdr_forces_disabled, ActionClass, CapKey,
    DecisionSource, EgressNarrowing, LayerError, Mode, OrgInput, PolicyDenial, PolicyInputs,
    PolicyLayer, Principal, ProxyMax, RequestAsk, RobotsMax, CATALOGUE,
};
use serde_json::json;
use sha2::{Digest, Sha256};

// ── helpers ─────────────────────────────────────────────────────────────────

fn layer(modes: &[(CapKey, Mode)]) -> PolicyLayer {
    PolicyLayer::new(
        modes.iter().copied().collect::<BTreeMap<_, _>>(),
        None,
        None,
        None,
        Vec::new(),
    )
    .expect("test layer is valid")
}

fn layer_domains(allow: Option<Vec<&str>>, deny: Vec<&str>) -> PolicyLayer {
    PolicyLayer::new(
        BTreeMap::new(),
        None,
        None,
        allow.map(|list| list.into_iter().map(str::to_string).collect()),
        deny.into_iter().map(str::to_string).collect(),
    )
    .expect("test domain layer is valid")
}

fn inputs(principal: Principal) -> PolicyInputs {
    PolicyInputs {
        principal,
        zdr: false,
        baseline: PolicyLayer::default(),
        org: OrgInput::Absent,
        grant: None,
        request: None,
    }
}

fn arb_host() -> impl Strategy<Value = String> {
    prop::sample::select(vec![
        "example.com",
        "sub.example.com",
        "bad.example",
        "docs.example.org",
    ])
    .prop_map(str::to_string)
}

fn arb_layer() -> impl Strategy<Value = PolicyLayer> {
    let keys = prop::sample::select(vec![
        CapKey::ActInput,
        CapKey::ActInteract,
        CapKey::ActUpload,
        CapKey::ActDownload,
        CapKey::ProfilePersistent,
        CapKey::ConsentReject,
        CapKey::SearchPaidProvider,
        CapKey::Recording,
        CapKey::RequestCredentials,
    ]);
    let modes = prop::collection::btree_map(
        keys,
        prop_oneof![Just(Mode::Allowed), Just(Mode::Disabled)],
        0..4,
    );
    let robots = prop::option::of(prop_oneof![
        Just(RobotsMax::Strict),
        Just(RobotsMax::Respect),
        Just(RobotsMax::Ignore),
    ]);
    let proxy = prop::option::of(prop_oneof![Just(ProxyMax::None), Just(ProxyMax::Pool)]);
    let allow = prop::option::of(prop::collection::vec(arb_host(), 0..3));
    let deny = prop::collection::vec(arb_host(), 0..3);
    (modes, robots, proxy, allow, deny).prop_map(|(modes, robots, proxy, allow, deny)| {
        PolicyLayer::new(modes, robots, proxy, allow, deny).expect("generated layer is valid")
    })
}

// ── the lattice ─────────────────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(4096))]

    #[test]
    fn meet_is_commutative_associative_idempotent(
        a in arb_layer(),
        b in arb_layer(),
        c in arb_layer(),
    ) {
        prop_assert_eq!(a.meet(&b), b.meet(&a));
        prop_assert_eq!(a.meet(&b).meet(&c), a.meet(&b.meet(&c)));
        prop_assert_eq!(a.meet(&a), a.clone());
    }

    #[test]
    fn resolved_is_never_more_permissive_than_any_input_layer(
        org in arb_layer(),
        baseline in arb_layer(),
        request in arb_layer(),
    ) {
        let input = PolicyInputs {
            principal: Principal::Autonomous,
            zdr: false,
            baseline: baseline.clone(),
            org: OrgInput::Present { layer: org.clone(), rev: 1 },
            grant: None,
            request: Some(request.clone()),
        };
        let resolved = quarry_core::web_policy::resolve(&input).expect("resolves");
        for source in [&org, &baseline, &request] {
            for key in CapKey::ALL {
                if source.mode(key) == Some(Mode::Disabled) {
                    prop_assert!(!resolved.allows(key), "{key:?} re-enabled");
                }
            }
            if let Some(max) = source.robots_max() {
                prop_assert!(resolved.robots_max().0 <= max);
            }
            if let Some(max) = source.proxy_max() {
                prop_assert!(resolved.proxy_max().0 <= max);
            }
            for host in source.deny() {
                prop_assert!(resolved.check_host(host).is_err(), "deny {host} leaked");
            }
        }
    }
}

#[test]
fn ask_cannot_express_a_loosening() {
    // Unknown fields are refused, so an ask has no way to say "allowed" or "forced".
    assert!(serde_json::from_value::<RequestAsk>(json!({
        "modes": { "browser.act.input": "allowed" }
    }))
    .is_err());
    // A ceiling cannot be disabled by an ask.
    let ask: RequestAsk =
        serde_json::from_value(json!({ "disable": ["robots.max_mode"] })).expect("parses");
    assert!(ask.into_layer().is_err());
    // An org disable survives any ask.
    let mut input = inputs(Principal::Autonomous);
    input.org = OrgInput::Present {
        layer: layer(&[(CapKey::ActInput, Mode::Disabled)]),
        rev: 3,
    };
    input.request = Some(RequestAsk::default().into_layer().expect("empty ask"));
    let resolved = quarry_core::web_policy::resolve(&input).expect("resolves");
    assert!(!resolved.allows(CapKey::ActInput));
    // A disable in an ask tightens only the key it names.
    let ask: RequestAsk =
        serde_json::from_value(json!({ "disable": ["browser.act.interact"] })).expect("parses");
    let mut input = inputs(Principal::Autonomous);
    input.request = Some(ask.into_layer().expect("valid ask"));
    let resolved = quarry_core::web_policy::resolve(&input).expect("resolves");
    assert!(!resolved.allows(CapKey::ActInteract));
    assert!(resolved.allows(CapKey::ActInput));
}

#[test]
fn forced_on_an_ability_is_rejected_at_construction() {
    assert!(serde_json::from_value::<PolicyLayer>(json!({
        "modes": { "browser.act.input": "forced" }
    }))
    .is_err());
    assert_eq!(
        PolicyLayer::new(
            BTreeMap::from([(CapKey::ActInput, Mode::Forced)]),
            None,
            None,
            None,
            Vec::new(),
        ),
        Err(LayerError::ForcedOnAbility(CapKey::ActInput))
    );
}

#[test]
fn unknown_capability_keys_are_refused_in_a_layer() {
    assert!(serde_json::from_value::<PolicyLayer>(json!({
        "modes": { "browser.act.teleport": "disabled" }
    }))
    .is_err());
}

// ── decision sources ────────────────────────────────────────────────────────

#[test]
fn decision_source_prefers_invariant_then_zdr_then_org_then_grant_then_baseline_then_request() {
    // invariant: act.script is off whatever the org says.
    let mut input = inputs(Principal::Autonomous);
    input.org = OrgInput::Present {
        layer: layer(&[(CapKey::ActScript, Mode::Allowed)]),
        rev: 1,
    };
    let resolved = quarry_core::web_policy::resolve(&input).expect("resolves");
    assert_eq!(
        resolved.mode(CapKey::ActScript),
        Some((Mode::Disabled, DecisionSource::Invariant))
    );

    // zdr beats org when both disable the profile.
    let mut input = inputs(Principal::Autonomous);
    input.zdr = true;
    input.org = OrgInput::Present {
        layer: layer(&[(CapKey::ProfilePersistent, Mode::Disabled)]),
        rev: 2,
    };
    let resolved = quarry_core::web_policy::resolve(&input).expect("resolves");
    assert_eq!(
        resolved.mode(CapKey::ProfilePersistent),
        Some((Mode::Disabled, DecisionSource::Zdr))
    );

    // org beats grant, grant beats request.
    let mut input = inputs(Principal::Autonomous);
    input.org = OrgInput::Present {
        layer: layer(&[(CapKey::ActUpload, Mode::Disabled)]),
        rev: 5,
    };
    input.grant = Some(layer(&[(CapKey::ActUpload, Mode::Disabled)]));
    input.request = Some(layer(&[(CapKey::ActUpload, Mode::Disabled)]));
    let resolved = quarry_core::web_policy::resolve(&input).expect("resolves");
    assert_eq!(
        resolved.mode(CapKey::ActUpload),
        Some((Mode::Disabled, DecisionSource::OrgPolicy))
    );
    let denied = resolved.require(CapKey::ActUpload, "upload_ref");
    assert_eq!(
        denied,
        Err(PolicyDenial::PolicyDenied {
            capability: CapKey::ActUpload,
            decision_source: DecisionSource::OrgPolicy,
            policy_rev: Some(5),
            denied_action: "upload_ref".to_string(),
        })
    );

    let mut input = inputs(Principal::Autonomous);
    input.grant = Some(layer(&[(CapKey::ActDownload, Mode::Disabled)]));
    input.request = Some(layer(&[(CapKey::ActDownload, Mode::Disabled)]));
    let resolved = quarry_core::web_policy::resolve(&input).expect("resolves");
    assert_eq!(
        resolved.mode(CapKey::ActDownload),
        Some((Mode::Disabled, DecisionSource::Grant))
    );

    // baseline beats request.
    let mut input = inputs(Principal::Autonomous);
    input.baseline = layer(&[(CapKey::ActInteract, Mode::Disabled)]);
    input.request = Some(layer(&[(CapKey::ActInteract, Mode::Disabled)]));
    let resolved = quarry_core::web_policy::resolve(&input).expect("resolves");
    assert_eq!(
        resolved.mode(CapKey::ActInteract),
        Some((Mode::Disabled, DecisionSource::Baseline))
    );

    // request alone.
    let mut input = inputs(Principal::Autonomous);
    input.request = Some(layer(&[(CapKey::ActInteract, Mode::Disabled)]));
    let resolved = quarry_core::web_policy::resolve(&input).expect("resolves");
    assert_eq!(
        resolved.mode(CapKey::ActInteract),
        Some((Mode::Disabled, DecisionSource::Request))
    );
}

#[test]
fn unreadable_org_policy_refuses_the_request() {
    let mut input = inputs(Principal::Autonomous);
    input.org = OrgInput::Unreadable;
    let denied = quarry_core::web_policy::resolve(&input).expect_err("refused");
    assert_eq!(denied, PolicyDenial::PolicyUnreadable);
}

// ── actions ─────────────────────────────────────────────────────────────────

#[test]
fn action_class_is_total() {
    // The compile-time guarantee is the wildcard-free match in action_class.rs.
    // This checks that the wire tag and the serde tag agree on a sample.
    let samples = vec![
        AgentAction::Navigate {
            url: "https://example.com".to_string(),
        },
        AgentAction::Click {
            selector: "a".to_string(),
        },
        AgentAction::ClickPoint { x: 1.0, y: 2.0 },
        AgentAction::Wait { ms: 1 },
        AgentAction::Pdf,
        AgentAction::GetContent,
        AgentAction::Back,
        AgentAction::Evaluate {
            script: "1".to_string(),
        },
    ];
    for action in &samples {
        let wire = serde_json::to_value(action).expect("serialises");
        assert_eq!(wire["type"], action_tag(action));
    }
}

#[test]
fn evaluate_maps_to_script_and_script_is_never_unlockable() {
    let evaluate = AgentAction::Evaluate {
        script: "document.cookie".to_string(),
    };
    assert_eq!(action_class(&evaluate), ActionClass::Script);
    assert_eq!(ActionClass::Script.cap_key(), Some(CapKey::ActScript));
    let unlockable_false: Vec<CapKey> = CATALOGUE
        .iter()
        .filter(|spec| !spec.unlockable)
        .map(|spec| spec.key)
        .collect();
    assert_eq!(unlockable_false, vec![CapKey::ActScript]);

    let mut input = inputs(Principal::Autonomous);
    input.org = OrgInput::Present {
        layer: layer(&[(CapKey::ActScript, Mode::Allowed)]),
        rev: 9,
    };
    input.grant = Some(layer(&[(CapKey::ActScript, Mode::Allowed)]));
    let resolved = quarry_core::web_policy::resolve(&input).expect("resolves");
    assert_eq!(
        resolved.mode(CapKey::ActScript),
        Some((Mode::Disabled, DecisionSource::Invariant))
    );
    assert_eq!(
        resolved.gate_action(&evaluate),
        Err(PolicyDenial::RawScriptDenied)
    );
}

#[test]
fn autonomous_only_keys_are_skipped_for_a_human() {
    let org = || OrgInput::Present {
        layer: layer(&[(CapKey::ActInput, Mode::Disabled)]),
        rev: 1,
    };
    let mut human = inputs(Principal::Human);
    human.org = org();
    assert!(quarry_core::web_policy::resolve(&human)
        .expect("resolves")
        .allows(CapKey::ActInput));

    let mut agent = inputs(Principal::Autonomous);
    agent.org = org();
    assert!(!quarry_core::web_policy::resolve(&agent)
        .expect("resolves")
        .allows(CapKey::ActInput));

    // The allow list is autonomous-only too.
    let mut human = inputs(Principal::Human);
    human.request = Some(layer_domains(Some(vec!["a.example.com"]), Vec::new()));
    assert!(quarry_core::web_policy::resolve(&human)
        .expect("resolves")
        .check_host("b.example.org")
        .is_ok());
}

#[test]
fn upload_applies_to_humans_too() {
    let mut human = inputs(Principal::Human);
    human.org = OrgInput::Present {
        layer: layer(&[(CapKey::ActUpload, Mode::Disabled)]),
        rev: 4,
    };
    let resolved = quarry_core::web_policy::resolve(&human).expect("resolves");
    assert!(!resolved.allows(CapKey::ActUpload));
    assert!(resolved.require(CapKey::ActUpload, "upload_ref").is_err());
}

#[test]
fn zdr_is_taken_from_the_floor_and_cannot_be_lowered_by_any_layer() {
    let mut input = inputs(Principal::Autonomous);
    input.zdr = true;
    input.org = OrgInput::Present {
        layer: layer(&[(CapKey::ProfilePersistent, Mode::Allowed)]),
        rev: 2,
    };
    let resolved = quarry_core::web_policy::resolve(&input).expect("resolves");
    assert!(resolved.zdr());
    assert_eq!(
        resolved.mode(CapKey::ProfilePersistent),
        Some((Mode::Disabled, DecisionSource::Zdr))
    );
}

#[test]
fn zdr_forces_disabled_covers_profile_paid_search_and_recording() {
    for key in CapKey::ALL {
        let expected = matches!(
            key,
            CapKey::ProfilePersistent | CapKey::SearchPaidProvider | CapKey::Recording
        );
        assert_eq!(zdr_forces_disabled(key), expected, "{key:?}");
    }
}

#[test]
fn ceiling_is_the_minimum() {
    let default = quarry_core::web_policy::resolve(&inputs(Principal::Autonomous))
        .expect("resolves");
    assert_eq!(
        default.robots_max(),
        (RobotsMax::Respect, DecisionSource::Baseline)
    );
    assert!(default.require_robots(RobotsMax::Respect).is_ok());
    assert!(default.require_robots(RobotsMax::Ignore).is_err());
    assert!(default.require_proxy(ProxyMax::None).is_ok());
    assert!(default.require_proxy(ProxyMax::Pool).is_err());

    let mut input = inputs(Principal::Autonomous);
    input.org = OrgInput::Present {
        layer: PolicyLayer::new(BTreeMap::new(), Some(RobotsMax::Strict), None, None, Vec::new())
            .expect("valid"),
        rev: 7,
    };
    let strict = quarry_core::web_policy::resolve(&input).expect("resolves");
    assert_eq!(
        strict.robots_max(),
        (RobotsMax::Strict, DecisionSource::OrgPolicy)
    );
    assert!(matches!(
        strict.require_robots(RobotsMax::Respect),
        Err(PolicyDenial::CeilingExceeded { .. })
    ));
}

// ── domains ─────────────────────────────────────────────────────────────────

#[test]
fn deny_wins_over_allow() {
    let mut input = inputs(Principal::Autonomous);
    input.request = Some(layer_domains(
        Some(vec!["example.com"]),
        vec!["example.com"],
    ));
    let resolved = quarry_core::web_policy::resolve(&input).expect("resolves");
    assert!(matches!(
        resolved.check_host("example.com"),
        Err(PolicyDenial::DomainDenied { .. })
    ));
}

#[test]
fn suffix_match_requires_a_dot_boundary() {
    let mut input = inputs(Principal::Autonomous);
    input.request = Some(layer_domains(Some(vec!["example.com"]), Vec::new()));
    let resolved = quarry_core::web_policy::resolve(&input).expect("resolves");
    assert!(resolved.check_host("example.com").is_ok());
    assert!(resolved.check_host("a.example.com").is_ok());
    assert_eq!(
        resolved.check_host("evilexample.com"),
        Err(PolicyDenial::DomainNotAllowed {
            host: Some("evilexample.com".to_string()),
            decision_source: DecisionSource::Request,
        })
    );
}

#[test]
fn empty_intersection_is_deny_all_not_unconstrained() {
    let mut input = inputs(Principal::Autonomous);
    input.org = OrgInput::Present {
        layer: layer_domains(Some(vec!["a.com"]), Vec::new()),
        rev: 1,
    };
    input.request = Some(layer_domains(Some(vec!["b.com"]), Vec::new()));
    let resolved = quarry_core::web_policy::resolve(&input).expect("resolves");
    assert!(resolved.check_host("a.com").is_err());
    assert!(resolved.check_host("b.com").is_err());

    // An empty allow list is deny-all, not "no list".
    let mut empty = inputs(Principal::Autonomous);
    empty.request = Some(layer_domains(Some(Vec::new()), Vec::new()));
    assert!(quarry_core::web_policy::resolve(&empty)
        .expect("resolves")
        .check_host("a.com")
        .is_err());

    // No allow list at all is unconstrained.
    assert!(quarry_core::web_policy::resolve(&inputs(Principal::Autonomous))
        .expect("resolves")
        .check_host("a.com")
        .is_ok());
}

#[test]
fn ip_literals_and_wildcards_are_rejected_in_a_layer() {
    for bad in [
        "1.2.3.4",
        "*.example.com",
        "[::1]",
        "Example.com",
        ".example.com",
        "example.com.",
        "a..b.com",
        "under_score.com",
    ] {
        assert_eq!(
            PolicyLayer::new(BTreeMap::new(), None, None, None, vec![bad.to_string()]),
            Err(LayerError::InvalidHost),
            "{bad} must be refused"
        );
    }
    assert!(
        PolicyLayer::new(BTreeMap::new(), None, None, None, vec!["example.com".to_string()])
            .is_ok()
    );
}

#[test]
fn rules_cap_at_64_entries_and_2_kib() {
    let many: Vec<String> = (0..65).map(|i| format!("h{i}.example.com")).collect();
    assert_eq!(
        PolicyLayer::new(BTreeMap::new(), None, None, None, many),
        Err(LayerError::RulesTooLarge)
    );
    let sixty_four: Vec<String> = (0..64).map(|i| format!("h{i}.example.com")).collect();
    assert!(PolicyLayer::new(BTreeMap::new(), None, None, None, sixty_four).is_ok());
    let long: Vec<String> = (0..30)
        .map(|i| format!("{}{i}.example.com", "a".repeat(60)))
        .collect();
    assert_eq!(
        PolicyLayer::new(BTreeMap::new(), None, None, None, long),
        Err(LayerError::RulesTooLarge)
    );
}

#[test]
fn check_url_never_echoes_the_host_under_zdr() {
    let mut input = inputs(Principal::Autonomous);
    input.zdr = true;
    input.request = Some(layer_domains(None, vec!["bad.example"]));
    let denied = quarry_core::web_policy::resolve(&input)
        .expect("resolves")
        .check_url("https://bad.example/path?q=secret")
        .expect_err("denied");
    assert_eq!(
        denied,
        PolicyDenial::DomainDenied {
            host: None,
            decision_source: DecisionSource::Request,
        }
    );

    let mut plain = inputs(Principal::Autonomous);
    plain.request = Some(layer_domains(None, vec!["bad.example"]));
    let denied = quarry_core::web_policy::resolve(&plain)
        .expect("resolves")
        .check_url("https://bad.example/path")
        .expect_err("denied");
    assert_eq!(
        denied,
        PolicyDenial::DomainDenied {
            host: Some("bad.example".to_string()),
            decision_source: DecisionSource::Request,
        }
    );
}

#[test]
fn narrow_egress_keeps_only_allowed_hosts() {
    let mut input = inputs(Principal::Autonomous);
    input.request = Some(layer_domains(
        Some(vec!["example.com"]),
        vec!["bad.example.com"],
    ));
    let resolved = quarry_core::web_policy::resolve(&input).expect("resolves");
    let hosts = |list: &[&str]| list.iter().map(|h| h.to_string()).collect::<Vec<String>>();
    assert_eq!(
        resolved.narrow_egress(&hosts(&["a.example.com", "bad.example.com", "other.org"])),
        EgressNarrowing::Narrowed(hosts(&["a.example.com"]))
    );
    assert_eq!(
        resolved.narrow_egress(&hosts(&["other.org"])),
        EgressNarrowing::DenyAll
    );
    assert_eq!(
        resolved.narrow_egress(&hosts(&["a.example.com"])),
        EgressNarrowing::Unchanged
    );
    assert_eq!(resolved.narrow_egress(&[]), EgressNarrowing::Unchanged);
}

#[test]
fn digest_is_stable_and_follows_the_policy() {
    let first = quarry_core::web_policy::resolve(&inputs(Principal::Autonomous))
        .expect("resolves")
        .digest();
    let again = quarry_core::web_policy::resolve(&inputs(Principal::Autonomous))
        .expect("resolves")
        .digest();
    assert_eq!(first, again);
    let mut tighter = inputs(Principal::Autonomous);
    tighter.request = Some(layer(&[(CapKey::ActInteract, Mode::Disabled)]));
    let changed = quarry_core::web_policy::resolve(&tighter)
        .expect("resolves")
        .digest();
    assert_ne!(first, changed);
}

// ── denials ─────────────────────────────────────────────────────────────────

#[test]
fn denial_serializes_to_the_documented_details() {
    let denial = PolicyDenial::PolicyDenied {
        capability: CapKey::ActUpload,
        decision_source: DecisionSource::OrgPolicy,
        policy_rev: Some(4),
        denied_action: "upload_ref".to_string(),
    };
    let error = QuarryError::from(denial);
    assert_eq!(error.code, ErrorCode::Forbidden);
    assert!(!error.retryable);
    let details = error.details.expect("details");
    assert_eq!(details["reason"], "policy_denied");
    assert_eq!(details["capability"], "browser.act.upload");
    assert_eq!(details["decision_source"], "org_policy");
    assert_eq!(details["denied_action"], "upload_ref");
    assert_eq!(details["policy_rev"], 4);
    assert_eq!(details["retryable"], false);

    let unknown = QuarryError::from(PolicyDenial::UnknownCapability {
        capability: "browser.act.teleport".to_string(),
    });
    assert_eq!(unknown.code, ErrorCode::BadRequest);
    assert_eq!(unknown.details.expect("details")["reason"], "unknown_capability");
}

#[test]
fn codes_are_unique_and_stable() {
    let denials = vec![
        PolicyDenial::PolicyDenied {
            capability: CapKey::ActUpload,
            decision_source: DecisionSource::Baseline,
            policy_rev: None,
            denied_action: "upload_ref".to_string(),
        },
        PolicyDenial::RawScriptDenied,
        PolicyDenial::PolicyUnreadable,
        PolicyDenial::UnknownCapability {
            capability: "x".to_string(),
        },
        PolicyDenial::CeilingExceeded {
            capability: CapKey::RobotsMaxMode,
            max: "respect".to_string(),
            requested: "ignore".to_string(),
        },
        PolicyDenial::DomainDenied {
            host: None,
            decision_source: DecisionSource::Request,
        },
        PolicyDenial::DomainNotAllowed {
            host: None,
            decision_source: DecisionSource::Request,
        },
    ];
    let codes: Vec<&str> = denials.iter().map(PolicyDenial::code).collect();
    assert_eq!(
        codes,
        vec![
            "policy_denied",
            "raw_script_denied",
            "policy_unreadable",
            "unknown_capability",
            "ceiling_exceeded",
            "domain_denied",
            "domain_not_allowed",
        ]
    );
}

#[test]
fn raw_script_denied_is_byte_compatible_with_d2() {
    let legacy = QuarryError::raw_script_denied();
    let current = QuarryError::from(PolicyDenial::RawScriptDenied);
    assert_eq!(current.code, legacy.code);
    assert_eq!(current.message, legacy.message);
    let legacy_details = legacy.details.expect("legacy details");
    let current_details = current.details.expect("current details");
    assert_eq!(current_details["reason"], legacy_details["reason"]);
    assert_eq!(current_details["denied_action"], legacy_details["denied_action"]);
    assert_eq!(current_details["capability"], "browser.act.script");
    assert_eq!(current_details["decision_source"], "invariant");
}

// ── the catalogue ───────────────────────────────────────────────────────────

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn catalogue_json_matches_the_checked_in_file_and_its_pin() {
    let golden_path = crate_dir().join("contracts/web-policy-catalogue.v1.json");
    let pin_path = crate_dir().join("contracts/web-policy-catalogue.v1.json.sha256");
    let text = serde_json::to_string_pretty(&catalogue_json()).expect("serialises") + "\n";
    let pin = format!(
        "{:x}  web-policy-catalogue.v1.json\n",
        Sha256::digest(text.as_bytes())
    );
    if std::env::var("UPDATE_CONTRACT_GOLDEN").as_deref() == Ok("1") {
        std::fs::write(&golden_path, &text).expect("write golden");
        std::fs::write(&pin_path, &pin).expect("write pin");
        return;
    }
    let read = |path: &PathBuf| {
        std::fs::read_to_string(path)
            .unwrap_or_else(|_| panic!("{} is missing: run with UPDATE_CONTRACT_GOLDEN=1", path.display()))
            .replace("\r\n", "\n")
    };
    assert_eq!(read(&golden_path), text, "web-policy-catalogue.v1.json drifted from the table");
    assert_eq!(read(&pin_path), pin, "the sha256 pin drifted from the golden");
}

// ── the RequestAsk golden ───────────────────────────────────────────────────

/// `contracts/request-ask.v1.json`, parsed. Each case has the ask as written (`ask`)
/// and the wire form it must round-trip to (`json`).
fn request_ask_golden() -> serde_json::Value {
    let path = crate_dir().join("contracts/request-ask.v1.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("{} is missing", path.display()));
    serde_json::from_str(&text).expect("request-ask.v1.json is JSON")
}

#[test]
fn request_ask_golden_matches_its_pin() {
    let golden_path = crate_dir().join("contracts/request-ask.v1.json");
    let pin_path = crate_dir().join("contracts/request-ask.v1.json.sha256");
    let bytes = std::fs::read(&golden_path)
        .unwrap_or_else(|_| panic!("{} is missing", golden_path.display()));
    let pin = format!("{:x}  request-ask.v1.json\n", Sha256::digest(&bytes));
    let read_pin = std::fs::read_to_string(&pin_path)
        .unwrap_or_else(|_| panic!("{} is missing", pin_path.display()));
    assert_eq!(
        read_pin, pin,
        "the sha256 pin drifted from request-ask.v1.json"
    );
}

#[test]
fn request_ask_golden_round_trips_byte_for_byte() {
    let golden = request_ask_golden();
    assert_eq!(golden["schema"], "quarry.web_policy.RequestAsk");
    assert_eq!(golden["version"], 1);
    let cases = golden["cases"].as_array().expect("cases is an array");
    assert!(!cases.is_empty(), "the golden has no cases");
    for case in cases {
        let name = case["name"].as_str().expect("every case is named");
        let ask: RequestAsk = serde_json::from_value(case["ask"].clone())
            .unwrap_or_else(|error| panic!("{name}: ask does not parse: {error}"));
        let wire: RequestAsk = serde_json::from_value(case["json"].clone())
            .unwrap_or_else(|error| panic!("{name}: json does not parse: {error}"));
        assert_eq!(ask, wire, "{name}: the ask and its json are different asks");
        // Both sides are written from a serde_json::Value, so each is one canonical text.
        let written = serde_json::to_value(&wire).expect("a RequestAsk serialises");
        assert_eq!(
            serde_json::to_string(&written).expect("serialises"),
            serde_json::to_string(&case["json"]).expect("serialises"),
            "{name}: RequestAsk does not re-serialise to the golden"
        );
    }
}

#[test]
fn request_ask_golden_refuses_the_keys_it_lists() {
    let golden = request_ask_golden();
    let refused = golden["refused_keys"]
        .as_array()
        .expect("refused_keys is an array");
    assert!(!refused.is_empty(), "the golden refuses no key");
    for key in refused {
        let key = key.as_str().expect("a refused key is a string");
        let mut probe = serde_json::Map::new();
        probe.insert(key.to_owned(), json!("x"));
        assert!(
            serde_json::from_value::<RequestAsk>(serde_json::Value::Object(probe)).is_err(),
            "RequestAsk accepts {key}, which the golden lists as refused"
        );
    }
}

#[test]
fn every_key_has_a_site_or_is_reserved() {
    for spec in CATALOGUE {
        assert!(
            spec.site.is_some() || spec.reserved,
            "{:?} has no consumer and is not reserved",
            spec.key
        );
    }
}

#[test]
fn reserved_keys_are_listed_so_a_new_option_must_call_require() {
    let reserved: Vec<CapKey> = CATALOGUE
        .iter()
        .filter(|spec| spec.reserved)
        .map(|spec| spec.key)
        .collect();
    assert_eq!(
        reserved,
        vec![
            CapKey::Recording,
            CapKey::RequestCredentials,
            CapKey::RobotsMaxMode,
            CapKey::ProxyMaxTier,
        ]
    );
}

#[test]
fn denied_hosts_lists_every_layers_deny_once_and_sorted() {
    let mut input = inputs(Principal::Autonomous);
    input.baseline = layer_domains(None, vec!["zz.example", "ads.example.com"]);
    input.request = Some(layer_domains(None, vec!["ads.example.com", "bad.example"]));
    let resolved = quarry_core::web_policy::resolve(&input).expect("resolves");
    assert_eq!(
        resolved.denied_hosts(),
        vec![
            "ads.example.com".to_string(),
            "bad.example".to_string(),
            "zz.example".to_string()
        ]
    );
    let none = quarry_core::web_policy::resolve(&inputs(Principal::Human)).expect("resolves");
    assert!(none.denied_hosts().is_empty(), "no deny rule means an empty list");
}
