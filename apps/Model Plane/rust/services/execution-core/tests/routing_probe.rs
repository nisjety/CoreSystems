//! WS11.S6 live routing probe: does the connector-first guidance move the model's FIRST
//! non-discovery tool choice toward a connected-account tool?
//!
//! `#[ignore]`d: it needs a live inference-core and spends real model calls. Run it from the
//! stack session, once with the guidance off and once on, and compare the two tables:
//!
//! ```bash
//! INFERENCE_CORE_URL=http://inference-core:9092 \
//! E2E_INFERENCE_BEARER=<short-lived-aud-inference-core-user-token> \
//! E2E_ORG_ID=org_real \
//! ROUTING_PROBE_MODEL=<model id> ROUTING_PROBE_PROVIDER=<provider hint> \
//! ROUTING_PROBE_SAMPLES=5 \
//! cargo test -p execution-core --test routing_probe -- --ignored --nocapture
//! ```
//!
//! The probe sets the guidance explicitly through `routing_probe_inputs`, so one process
//! produces both columns. The 90 percent gate on connector-solvable prompts is the plan's
//! PLACEHOLDER: it is enforced only when `ROUTING_PROBE_MIN_PCT` is set, otherwise the table is
//! printed and the run passes.
//!
//! The always-on test below pins the probe table itself, so a prompt cannot be edited into a
//! case the offered set cannot answer.

use execution_core::runtime_loop::agent::routing_probe_inputs;
use mp_contracts::model_plane::v1::inference_core_client::InferenceCoreClient;
use mp_contracts::model_plane::v1::{ChatMessage, InferRequest};

/// The offered set of the probe: a connector pair next to the web and browser tools.
const OFFERED: &[&str] = &[
    "list_provider_actions",
    "execute_provider_action",
    "web_search",
    "web_fetch",
    // `browser_agent` is not a chat-offered tool today (it stays dormant, spec D6); the parent
    // loop's browser tool stands in for the browser route.
    "browser.observe",
];

/// Tool name to the counter's class label, the same mapping as `execution_routing_first_tool_total`.
fn class_of(tool: &str) -> &'static str {
    match tool {
        "execute_provider_action" | "read_provider_action" => "connector",
        "browser_agent" | "browser.observe" | "browser.act" => "browser",
        "web_search" | "web_fetch" | "fetch_url" | "web.search" | "web.read" => "web",
        _ => "other",
    }
}

struct Probe {
    prompt: &'static str,
    /// The class a correct first non-discovery choice has.
    expect: &'static str,
}

/// Connector-solvable prompts (the user's own connected accounts) and public-web prompts (where
/// the web tool is the right answer, so the guidance must not over-route to a connector).
fn probes() -> Vec<Probe> {
    vec![
        Probe { prompt: "Post 'We open at 9 tomorrow' on our Facebook Page.", expect: "connector" },
        Probe { prompt: "List my latest ad campaigns and their spend.", expect: "connector" },
        Probe { prompt: "Send a WhatsApp message to my last customer saying the order shipped.", expect: "connector" },
        Probe { prompt: "Create a GitHub issue titled 'Login page 500s' in our repo.", expect: "connector" },
        Probe { prompt: "What did Equinor announce in their latest quarterly report?", expect: "web" },
        Probe { prompt: "Find the opening hours of Oslo Central Station on the web.", expect: "web" },
    ]
}

#[test]
fn the_probe_table_only_asks_what_the_offered_set_can_answer() {
    assert!(OFFERED.contains(&"list_provider_actions") && OFFERED.contains(&"web_search"));
    let probes = probes();
    assert!(probes.iter().any(|probe| probe.expect == "connector"));
    assert!(probes.iter().any(|probe| probe.expect == "web"));
    for probe in &probes {
        assert!(["connector", "web", "browser"].contains(&probe.expect));
        assert!(!probe.prompt.is_empty());
    }
    // The guidance text only differs when it is on, and names only offered tools.
    let (off, tools) = routing_probe_inputs(OFFERED, false);
    let (on, _) = routing_probe_inputs(OFFERED, true);
    assert!(on.len() > off.len() && on.starts_with(&off));
    assert_eq!(tools.len(), OFFERED.len(), "every probe tool has a definition");
}

#[tokio::test]
#[ignore = "requires a live inference-core and spends real model calls; see file header"]
async fn connector_first_guidance_moves_the_first_tool_choice() {
    let url = std::env::var("INFERENCE_CORE_URL").expect("INFERENCE_CORE_URL");
    let bearer = std::env::var("E2E_INFERENCE_BEARER").expect("E2E_INFERENCE_BEARER");
    let org_id = std::env::var("E2E_ORG_ID").expect("E2E_ORG_ID");
    let model = std::env::var("ROUTING_PROBE_MODEL").unwrap_or_default();
    let provider_hint = std::env::var("ROUTING_PROBE_PROVIDER").unwrap_or_default();
    let samples: usize = std::env::var("ROUTING_PROBE_SAMPLES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);
    let min_pct: Option<f64> = std::env::var("ROUTING_PROBE_MIN_PCT")
        .ok()
        .and_then(|v| v.parse().ok());

    let mut client = InferenceCoreClient::connect(url)
        .await
        .expect("inference-core is reachable at INFERENCE_CORE_URL");

    // [guidance off, guidance on] -> (correct, total) over the connector-solvable prompts.
    let mut score = [(0usize, 0usize); 2];
    for (column, guidance) in [false, true].into_iter().enumerate() {
        let (system, tools) = routing_probe_inputs(OFFERED, guidance);
        for probe in probes() {
            let mut picked = Vec::new();
            for sample in 0..samples {
                let mut request = tonic::Request::new(InferRequest {
                    request_id: format!("routing-probe-{column}-{sample}"),
                    org_id: org_id.clone(),
                    model: model.clone(),
                    provider_hint: provider_hint.clone(),
                    messages: vec![
                        ChatMessage {
                            role: "system".to_owned(),
                            content: system.clone(),
                            ..Default::default()
                        },
                        ChatMessage {
                            role: "user".to_owned(),
                            content: probe.prompt.to_owned(),
                            ..Default::default()
                        },
                    ],
                    temperature: 0.7,
                    max_tokens: 512,
                    tools: tools.clone(),
                    tool_choice: "auto".to_owned(),
                    ..Default::default()
                });
                request.metadata_mut().insert(
                    "authorization",
                    format!("Bearer {bearer}").parse().expect("valid bearer"),
                );
                let reply = client.infer(request).await.expect("inference round").into_inner();
                // The first non-discovery call of the round, as the loop's counter would see it.
                let first = reply
                    .tool_calls
                    .iter()
                    .map(|call| call.name.as_str())
                    .find(|name| *name != "list_provider_actions")
                    .map_or("none", class_of);
                picked.push(first);
            }
            let correct = picked.iter().filter(|class| **class == probe.expect).count();
            eprintln!(
                "guidance={guidance:<5} expect={:<9} {correct}/{samples}  {:?}  | {}",
                probe.expect, picked, probe.prompt
            );
            if probe.expect == "connector" {
                score[column].0 += correct;
                score[column].1 += samples;
            }
        }
    }
    let pct = |(correct, total): (usize, usize)| 100.0 * correct as f64 / total.max(1) as f64;
    eprintln!(
        "connector-solvable first-choice compliance: guidance off {:.0}% | guidance on {:.0}%",
        pct(score[0]),
        pct(score[1])
    );
    if let Some(min) = min_pct {
        assert!(
            pct(score[1]) >= min,
            "guidance-on compliance {:.0}% is below the {min}% gate",
            pct(score[1])
        );
    }
}
