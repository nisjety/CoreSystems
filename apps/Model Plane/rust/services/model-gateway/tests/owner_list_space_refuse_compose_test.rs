//! The Space listing refusal (C-05, D2-769, D2-774) and the two switches it is ordered against, as the deployment
//! states them.
//!
//! The code side is pinned where each switch is read: model-gateway's `owner_list_space_refuse_enabled` (strict, and
//! not decided by the retrieval switch), execution-core's Space listing gate, and the admission table that
//! capability-core and execution-core read. This is the compose side. Each switch is set on exactly the services that
//! read it, interpolated with its off default, and no compose file pins it. The production overlay must not pin any of
//! them: a deployment that turns on `MODEL_ACTION_ADMISSION=table` with Spaces has to be able to turn the refusal on
//! through its runtime environment, without editing the overlay.

/// One interpolated switch: its name, the services that read it, and the text of its off default.
const SWITCHES: [(&str, &[&str], &str); 3] = [
    // The one switch for the listing refusal; both paths read it (C-05).
    (
        "OWNER_LIST_SPACE_REFUSE",
        &["model-gateway", "execution-core"],
        "false",
    ),
    // The Space retrieval decision for a Do run. execution-core reads no switch of its own.
    (
        "AGENTIC_SPACE_RETRIEVAL_ENABLED",
        &["model-gateway"],
        "false",
    ),
    // The admission table that offers the owner listing. Unset is the tickets.create-only view.
    (
        "MODEL_ACTION_ADMISSION",
        &["capability-core", "execution-core"],
        "",
    ),
];
const DEPLOY: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../deploy");

fn compose(name: &str) -> Option<String> {
    std::fs::read_to_string(format!("{DEPLOY}/{name}"))
        .ok()
        .map(|text| text.replace("\r\n", "\n"))
}

/// The lines of one top-level service of a compose file: from `  <service>:` to the next service key (two-space
/// indent) or the next top-level key.
fn service_block(compose: &str, service: &str) -> Vec<String> {
    let header = format!("  {service}:");
    let mut inside = false;
    let mut block = Vec::new();
    for line in compose.lines() {
        if line == header {
            inside = true;
            continue;
        }
        if inside {
            let top_level = !line.starts_with(' ') && !line.trim().is_empty();
            let next_service = line.starts_with("  ")
                && !line.starts_with("   ")
                && line.trim_end().ends_with(':');
            if top_level || next_service {
                break;
            }
            block.push(line.to_owned());
        }
    }
    block
}

/// The trimmed environment entries of a service block that set `variable`.
fn entries(block: &[String], variable: &str) -> Vec<String> {
    block
        .iter()
        .filter(|line| line.trim_start().starts_with(&format!("{variable}:")))
        .map(|line| line.trim().to_owned())
        .collect()
}

/// The entry of an off, interpolated declaration: `VARIABLE: ${VARIABLE:-<off>}`.
fn off_entry(variable: &str, off: &str) -> String {
    format!("{variable}: ${{{variable}:-{off}}}")
}

/// The services (two-space keys under `services:`) whose block sets `variable`, sorted.
fn services_setting(compose: &str, variable: &str) -> Vec<String> {
    let mut in_services = false;
    let mut service = String::new();
    let mut found = Vec::new();
    for line in compose.lines() {
        if line.trim_start().starts_with('#') {
            continue;
        }
        if !line.starts_with(' ') && !line.trim().is_empty() {
            in_services = line == "services:";
            continue;
        }
        if !in_services {
            continue;
        }
        if let Some(name) = line
            .strip_prefix("  ")
            .and_then(|rest| rest.strip_suffix(':'))
        {
            if !name.starts_with(' ') {
                service = name.to_owned();
                continue;
            }
        }
        if line.trim_start().starts_with(&format!("{variable}:")) && !found.contains(&service) {
            found.push(service.clone());
        }
    }
    found.sort();
    found
}

#[test]
fn the_base_file_sets_each_switch_once_on_each_reader_and_defaults_it_off() {
    let base = compose("docker-compose.yml").expect("the base compose file is readable");
    for (variable, readers, off) in SWITCHES {
        let mut want = readers.to_vec();
        want.sort();
        assert_eq!(
            services_setting(&base, variable),
            want,
            "{variable} is set on exactly its readers"
        );
        for service in readers {
            let block = service_block(&base, service);
            assert!(!block.is_empty(), "the {service} block is in the base file");
            assert_eq!(
                entries(&block, variable),
                vec![off_entry(variable, off)],
                "{service} sets {variable} once, and it defaults to off"
            );
        }
    }
}

#[test]
fn the_production_overlay_sets_each_switch_on_each_reader_without_pinning_it() {
    let production =
        compose("docker-compose.production.yml").expect("the production overlay is readable");
    for (variable, readers, off) in SWITCHES {
        let mut want = readers.to_vec();
        want.sort();
        assert_eq!(
            services_setting(&production, variable),
            want,
            "the production overlay sets {variable} on exactly its readers"
        );
        for service in readers {
            let block = service_block(&production, service);
            assert!(
                !block.is_empty(),
                "the {service} block is in the production overlay"
            );
            assert_eq!(
                entries(&block, variable),
                vec![off_entry(variable, off)],
                "the production {service} declares {variable} once, interpolated, not pinned"
            );
        }
    }
}

#[test]
fn no_compose_file_turns_a_switch_on_or_pins_it_to_another_value() {
    for name in [
        "docker-compose.yml",
        "docker-compose.override.yml",
        "docker-compose.production.yml",
        "docker-compose.release.yml",
        "docker-compose.bridges.yml",
        "docker-compose.replicas.yml",
    ] {
        let Some(text) = compose(name) else { continue };
        for (variable, _, off) in SWITCHES {
            for line in text
                .lines()
                .filter(|line| line.trim_start().starts_with(&format!("{variable}:")))
            {
                assert!(
                    line.trim().ends_with(&format!("${{{variable}:-{off}}}")),
                    "{name} sets {variable} to something other than the off default: {line}"
                );
            }
        }
    }
}
