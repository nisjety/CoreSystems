//! The Space listing refusal (C-05, D2-769, D2-774) as the deployment states it.
//!
//! The code side is pinned where it is read: model-gateway's `owner_list_space_refuse_enabled` (strict, and not
//! decided by the retrieval switch) and execution-core's Space listing gate. This is the compose side. One switch,
//! `OWNER_LIST_SPACE_REFUSE`, is set on both services that read it, interpolated with an off default. The production
//! overlay must not pin it: a deployment that turns on `MODEL_ACTION_ADMISSION=table` with Spaces has to be able to
//! turn the refusal on through its runtime environment, without editing the overlay.

const VARIABLE: &str = "OWNER_LIST_SPACE_REFUSE";
const READERS: [&str; 2] = ["model-gateway", "execution-core"];
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

/// The trimmed environment entries of a service block that set the listing switch.
fn entries(block: &[String]) -> Vec<String> {
    block
        .iter()
        .filter(|line| line.trim_start().starts_with(&format!("{VARIABLE}:")))
        .map(|line| line.trim().to_owned())
        .collect()
}

#[test]
fn the_base_file_sets_the_listing_switch_once_on_each_reader_and_defaults_it_off() {
    let base = compose("docker-compose.yml").expect("the base compose file is readable");
    for service in READERS {
        let block = service_block(&base, service);
        assert!(!block.is_empty(), "the {service} block is in the base file");
        assert_eq!(
            entries(&block),
            vec![format!("{VARIABLE}: ${{{VARIABLE}:-false}}")],
            "{service} sets the listing switch once, and it defaults to off"
        );
    }
}

#[test]
fn the_production_overlay_declares_the_listing_switch_on_each_reader_without_pinning_it() {
    let production =
        compose("docker-compose.production.yml").expect("the production overlay is readable");
    for service in READERS {
        let block = service_block(&production, service);
        assert!(
            !block.is_empty(),
            "the {service} block is in the production overlay"
        );
        assert_eq!(
            entries(&block),
            vec![format!("{VARIABLE}: ${{{VARIABLE}:-false}}")],
            "the production {service} declares the listing switch once, interpolated, not pinned"
        );
    }
}

#[test]
fn no_compose_file_turns_the_listing_switch_on_or_pins_it_to_another_value() {
    for name in [
        "docker-compose.yml",
        "docker-compose.override.yml",
        "docker-compose.production.yml",
        "docker-compose.release.yml",
        "docker-compose.bridges.yml",
        "docker-compose.replicas.yml",
    ] {
        let Some(text) = compose(name) else { continue };
        for line in text
            .lines()
            .filter(|line| line.trim_start().starts_with(&format!("{VARIABLE}:")))
        {
            assert!(
                line.trim().ends_with(&format!("${{{VARIABLE}:-false}}")),
                "{name} sets the listing switch to something other than the off default: {line}"
            );
        }
    }
}
