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
//! # with a real Chrome/Chromium available to it. execution-core must have a
//! # registered Auth Core service principal for the `quarry` audience.
//! QUARRY_BROWSER_AGENT_ENABLED=1 \
//! QUARRY_EDGE_URL=https://quarry-edge.internal \
//! AUTH_CORE_URL=http://auth-core:3011 \
//! EXECUTION_CORE_SERVICE_API_KEY=<credential> \
//! QUARRY_BROWSER_AGENT_LLM=1 \
//! INFERENCE_CORE_URL=http://inference-core:9092 \
//! E2E_INFERENCE_BEARER=<short-lived-aud-inference-core-user-token> \
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
        .expect("Auth Core service-principal configuration must be valid")
        .expect("set QUARRY_BROWSER_AGENT_ENABLED=1 and QUARRY_EDGE_URL to run the E2E test");

    // Optional real LLM planner. When unset, the loop uses the deterministic
    // planner (repeated Observe) — still a real Quarry round-trip per step.
    let inference_bearer = std::env::var("E2E_INFERENCE_BEARER").ok();
    let planner = LlmPlanner::from_env(inference_bearer.as_deref());

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
        profile_id: None,
        // Phase 2, found live: a freshly `start_run`'d Quarry lease has no
        // page loaded, so the first action needs an explicit destination.
        start_url: Some("https://example.com".to_owned()),
        postcondition: String::new(),
    };

    let result = run_browser_agent_loop(config, Some(&client), planner.as_ref(), None, None).await;

    eprintln!(
        "E2E result: status={} observations={} summary={}",
        result.status.as_str(),
        result.observations.len(),
        result.summary
    );

    assert_ne!(
        result.status,
        PlanStatus::Failed,
        "browser-agent loop failed: {}",
        result.summary
    );
    assert!(
        !result.observations.is_empty(),
        "expected at least one observation from the real Quarry browser"
    );
}
