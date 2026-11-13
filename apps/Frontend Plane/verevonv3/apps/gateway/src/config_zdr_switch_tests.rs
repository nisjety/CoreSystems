//! 3.4: the BFF switches added in this phase. Each is on only for the literal `true` (trimmed, as
//! the unit specs say), each is read once into `AppState` by `build_state`, and each is declared in
//! the gateway service's compose block with a `false` default.

use super::{zdr_effective_posture_read_from, zdr_waiver_ui_enabled_from};

#[test]
fn the_zdr_switches_are_on_only_for_the_literal_true() {
    for parse in [zdr_effective_posture_read_from, zdr_waiver_ui_enabled_from] {
        assert!(parse(Some("true")));
        assert!(parse(Some(" true\n")), "trimmed");
        for off in [
            None,
            Some(""),
            Some("1"),
            Some("TRUE"),
            Some("True"),
            Some("yes"),
            Some("on"),
            Some("false"),
        ] {
            assert!(!parse(off), "{off:?}");
        }
    }
}

#[test]
fn the_test_state_has_every_phase_switch_off() {
    let state = crate::tests::test_state(false);
    assert!(!state.zdr_effective_posture_read);
    assert!(!state.zdr_waiver_ui_enabled);
    assert!(state.retention_decision_service_api_key.is_none());
    assert!(!state.space_features.thread_scope_resolve);
}

/// Every BFF switch `build_state` reads for this phase is declared in the gateway's compose block,
/// defaulting to `false`, so an operator can see and set it (project rule: the service's compose
/// block and the flag registry).
#[test]
fn every_phase_switch_is_declared_in_the_gateway_compose_block() {
    let compose = include_str!("../../../docker-compose.yml").replace("\r\n", "\n");
    for switch in [
        "GATEWAY_ZDR_EFFECTIVE_POSTURE_ENABLED",
        "ZDR_WAIVER_UI_ENABLED",
        "SPACE_THREAD_SCOPE_RESOLVE",
    ] {
        let line = format!("{switch}: ${{{switch}:-false}}\n");
        assert_eq!(
            compose.matches(&line).count(),
            1,
            "{switch} is not declared in docker-compose.yml"
        );
    }
}

// 3.4, round 2: every BFF switch this phase added (git log -S on config.rs since 031e7965d), not only
// the three ZDR ones above.

/// Every switch the gateway reads that this phase added.
const PHASE_SWITCHES: [&str; 11] = [
    "GATEWAY_ZDR_EFFECTIVE_POSTURE_ENABLED",
    "ZDR_WAIVER_UI_ENABLED",
    "SPACE_THREAD_SCOPE_RESOLVE",
    "VEREVON_GATEWAY_CONTINUE_QUESTION_ENABLED",
    "AGENT_DEFINITION_VERSIONS",
    "CHAT_PROCESS_OUTPUT_READ_ENABLED",
    "CHAT_SNAPSHOTS_ENABLED",
    "EVENT_TRIGGERS_UI_ENABLED",
    "CREDENTIAL_BINDINGS_UI_ENABLED",
    "CHAT_MEDIA_TRANSCRIPTS_ENABLED",
    "CLIENT_ENV_IDENTITY",
];

#[test]
fn every_strict_phase_switch_is_on_only_for_the_exact_word_true() {
    let parsers: [(&str, fn(Option<&str>) -> bool); 8] = [
        (
            "VEREVON_GATEWAY_CONTINUE_QUESTION_ENABLED",
            super::continue_question_form_from,
        ),
        (
            "AGENT_DEFINITION_VERSIONS",
            super::agent_definition_versions_from,
        ),
        (
            "CHAT_PROCESS_OUTPUT_READ_ENABLED",
            super::chat_process_output_from,
        ),
        ("CHAT_SNAPSHOTS_ENABLED", super::chat_snapshots_from),
        ("EVENT_TRIGGERS_UI_ENABLED", super::event_triggers_ui_from),
        (
            "CREDENTIAL_BINDINGS_UI_ENABLED",
            super::credential_bindings_ui_from,
        ),
        (
            "CHAT_MEDIA_TRANSCRIPTS_ENABLED",
            super::chat_media_transcripts_from,
        ),
        ("CLIENT_ENV_IDENTITY", super::client_env_identity_from),
    ];
    for (name, parse) in parsers {
        assert!(parse(Some("true")), "{name}");
        for off in [
            None,
            Some(""),
            Some(" true"),
            Some("true\n"),
            Some("1"),
            Some("TRUE"),
            Some("True"),
            Some("yes"),
            Some("on"),
            Some("false"),
        ] {
            assert!(!parse(off), "{name}: {off:?}");
        }
    }
}

#[test]
fn the_test_state_has_every_added_phase_switch_off() {
    let state = crate::tests::test_state(false);
    assert!(!state.continue_question_form);
    assert!(!state.agent_definition_versions);
    assert!(!state.chat_process_output_enabled);
    assert!(!state.chat_snapshots_enabled);
    assert!(!state.event_triggers_enabled);
    assert!(!state.credential_bindings_enabled);
    assert!(!state.space_features.chat_media_transcripts);
    assert!(state.space_features.client_environment.is_none());
}

/// The compose block is the only way a value reaches the gateway container (no `env_file`), so a
/// switch that is not declared there can never be turned on in the stack.
#[test]
fn every_added_phase_switch_is_declared_in_the_gateway_compose_block() {
    let compose = include_str!("../../../docker-compose.yml").replace("\r\n", "\n");
    let missing: Vec<&str> = PHASE_SWITCHES
        .iter()
        .copied()
        .filter(|switch| {
            compose
                .matches(&format!("{switch}: ${{{switch}:-false}}\n"))
                .count()
                != 1
        })
        .collect();
    assert!(
        missing.is_empty(),
        "not declared once with a false default in docker-compose.yml: {missing:?}"
    );
}

/// Project rule: a new switch is declared in the flag registry too.
#[test]
fn every_added_phase_switch_has_a_flag_registry_entry() {
    let registry: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../../Control Plane/flag-registry/flag-registry.json"
    ))
    .expect("registry json");
    let names: Vec<&str> = registry["flags"]
        .as_array()
        .expect("flags")
        .iter()
        .filter_map(|flag| flag["name"].as_str())
        .collect();
    let missing: Vec<&str> = PHASE_SWITCHES
        .iter()
        .copied()
        .filter(|switch| !names.contains(switch))
        .collect();
    assert!(
        missing.is_empty(),
        "no entry in flag-registry.json: {missing:?}"
    );
}

/// Upstream config and switches live on `AppState`, read once by `build_state`: no handler reads a
/// phase switch from the environment (a handler's `std::env::var` raced under parallel tests).
#[test]
fn no_phase_switch_is_read_from_the_environment_outside_config() {
    fn rust_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).expect("read src") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                rust_files(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
    }
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    let mut offences = Vec::new();
    for file in files {
        let name = file
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if name == "config.rs" || name.ends_with("_tests.rs") {
            continue;
        }
        let text = std::fs::read_to_string(&file).expect("read");
        for switch in PHASE_SWITCHES {
            for read in ["env::var(\"", "env::var_os(\""] {
                if text.contains(&format!("{read}{switch}\"")) {
                    offences.push(format!("{}: {switch}", file.display()));
                }
            }
        }
    }
    assert!(offences.is_empty(), "{offences:?}");
}
