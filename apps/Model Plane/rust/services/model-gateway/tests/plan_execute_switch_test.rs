//! The execute-turn kill switch (WS14 S2a, "run the approved plan") as the deployment states it.
//!
//! The code side (one strict parser, read once by the plan store, nothing else reads the
//! variable) is pinned by the unit tests in `coordinator.rs`; this is the compose side: the
//! switch is set on the model-gateway service, once, defaulting OFF, and on no other service,
//! because no other service reads it. The behaviour with the switch off and on is driven end
//! to end in `e2e_invoke_chain_test.rs`.

const VARIABLE: &str = "MODEL_GATEWAY_PLAN_EXECUTE_ENABLED";
const DEPLOY: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../deploy");

fn compose(name: &str) -> Option<String> {
    std::fs::read_to_string(format!("{DEPLOY}/{name}"))
        .ok()
        .map(|text| text.replace("\r\n", "\n"))
}

/// The lines of one top-level service of a compose file: from `  <service>:` to the next
/// service key (two-space indent) or the next top-level key.
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
            let next_service = line.starts_with("  ") && !line.starts_with("   ") && line.trim_end().ends_with(':');
            if top_level || next_service {
                break;
            }
            block.push(line.to_owned());
        }
    }
    block
}

fn entries(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .filter(|line| line.trim_start().starts_with(&format!("{VARIABLE}:")))
        .map(|line| line.trim().to_owned())
        .collect()
}

#[test]
fn compose_sets_the_execute_switch_on_the_gateway_only_and_defaults_it_off() {
    let main = compose("docker-compose.yml").expect("the main compose file is readable");
    let gateway = service_block(&main, "model-gateway");
    assert!(gateway.len() > 50, "the model-gateway block was found ({} lines)", gateway.len());
    assert_eq!(
        entries(&gateway),
        vec![format!("{VARIABLE}: ${{{VARIABLE}:-false}}")],
        "the gateway sets the switch once, and it defaults to off"
    );
    // No other service reads it, so none sets it (a second setter would be a second opinion).
    let mut elsewhere = 0;
    for line in main.lines() {
        if line.contains(&format!("{VARIABLE}:")) {
            elsewhere += 1;
        }
    }
    assert_eq!(elsewhere, 1, "the compose file names the switch as an entry once");
}

#[test]
fn no_sibling_compose_file_turns_the_execute_switch_on_by_default() {
    for name in [
        "docker-compose.override.yml",
        "docker-compose.production.yml",
        "docker-compose.release.yml",
        "docker-compose.bridges.yml",
    ] {
        let Some(text) = compose(name) else { continue };
        for line in text.lines().filter(|line| line.contains(&format!("{VARIABLE}:"))) {
            assert!(
                line.trim().ends_with(&format!("${{{VARIABLE}:-false}}")),
                "{name} sets the execute switch to something other than the off default: {line}"
            );
        }
    }
}

/// The execute switch is coupled to the approval-resume switch (`/continue`): the gateway
/// reads the latter too, so the gateway's own environment must carry it (off by default), and
/// the entry of the execute switch must say so for the person who flips it.
#[test]
fn compose_states_the_coupling_of_the_execute_switch_to_the_approval_resume_switch() {
    let main = compose("docker-compose.yml").expect("the main compose file is readable");
    let gateway = service_block(&main, "model-gateway");
    let resume = "AGENT_CONTINUE_AFTER_APPROVAL_ENABLED";
    let entries: Vec<&String> = gateway
        .iter()
        .filter(|line| line.trim_start().starts_with(&format!("{resume}:")))
        .collect();
    assert_eq!(entries.len(), 1, "the gateway reads the resume switch, so it sets it once");
    assert!(entries[0].trim().ends_with(&format!("${{{resume}:-false}}")), "{}", entries[0]);
    let at = gateway
        .iter()
        .position(|line| line.trim_start().starts_with(&format!("{VARIABLE}:")))
        .expect("the execute switch entry");
    let comment: String = gateway[..at]
        .iter()
        .rev()
        .take_while(|line| line.trim_start().starts_with('#'))
        .map(|line| line.trim())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        comment.contains(resume) && comment.contains("COUPLED"),
        "the entry's comment names the coupling: {comment}"
    );
}
