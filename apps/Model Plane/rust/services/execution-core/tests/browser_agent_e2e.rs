//! P7 · Phase F — REAL end-to-end test for the agentic browser loop.
//!
//! No mocks: this drives the actual Model Plane loop → real `QuarryAgentClient`
//! (HTTP) → quarry-edge `/v1/agent/*` (real chromiumoxide CDP browser) → real
//! `BrowserObservation`, optionally with the real inference-core LLM planner.
//!
//! It is `#[ignore]`d because it needs a live stack. To run it:
//!
//! ```bash
//! # quarry-edge must be built WITH the chromiumoxide driver and reachable,
//! # with a real Chrome/Chromium available to it. QUARRY_EDGE_TOKEN must be a
//! # JWT whose `org_id` claim matches the run.
//! QUARRY_BROWSER_AGENT_ENABLED=1 \
//! QUARRY_EDGE_URL=https://quarry-edge.internal \
//! QUARRY_EDGE_TOKEN=<jwt> \
//! QUARRY_BROWSER_AGENT_LLM=1 \
//! INFERENCE_CORE_URL=http://inference-core:9092 \
//! E2E_ORG_ID=org_real \
//! cargo test -p execution-core --test browser_agent_e2e -- --ignored --nocapture
//! ```

use execution_core::browser_agent::{run_browser_agent_loop, PlanConfig, PlanStatus};
use execution_core::llm_planner::LlmPlanner;
use execution_core::quarry_agent::QuarryAgentClient;

#[tokio::test]
#[ignore = "requires a live quarry-edge (chromiumoxide) + inference-core stack; see file header"]
async fn browser_agent_drives_real_quarry_loop() {
    // Real Quarry agent client from env — panics if the operator forgot to set
    // QUARRY_BROWSER_AGENT_ENABLED=1 + QUARRY_EDGE_URL (the whole point of the
    // test is to exercise the real wire path).
    let client = QuarryAgentClient::from_env()
        .expect("set QUARRY_BROWSER_AGENT_ENABLED=1 and QUARRY_EDGE_URL to run the E2E test");

    // Optional real LLM planner. When unset, the loop uses the deterministic
    // planner (repeated Observe) — still a real Quarry round-trip per step.
    let planner = LlmPlanner::from_env();

    let config = PlanConfig {
        plan_id: "e2e_plan".to_owned(),
        grant_id: "e2e_grant".to_owned(),
        run_id: "e2e_run".to_owned(),
        org_id: std::env::var("E2E_ORG_ID").unwrap_or_else(|_| "org_e2e".to_owned()),
        system_prompt: "Open https://example.com and read the page title.".to_owned(),
        max_steps: 3,
        max_runtime_s: 60,
        allowed_domains: vec!["example.com".to_owned()],
        stop_criteria: String::new(),
        require_approval: false,
        max_cost_usd: Some(0.50),
        zdr: false,
    };

    let (status, observations, summary) =
        run_browser_agent_loop(config, Some(&client), planner.as_ref(), None).await;

    eprintln!(
        "E2E result: status={} observations={} summary={summary}",
        status.as_str(),
        observations.len()
    );

    assert_ne!(status, PlanStatus::Failed, "browser-agent loop failed: {summary}");
    assert!(
        !observations.is_empty(),
        "expected at least one observation from the real Quarry browser"
    );
}
