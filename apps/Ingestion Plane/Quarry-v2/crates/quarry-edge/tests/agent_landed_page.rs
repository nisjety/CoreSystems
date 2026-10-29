//! T4b-1: the page a step landed on, checked after the action ran.
//!
//! A run's first navigation commits its tab on `about:blank` and then navigates it (SC4 T4b).
//! When that navigation never commits, the tab stays on `about:blank`, and the next
//! observation of it is a tab with no page yet: it reached no host, so it is answered (200),
//! the run's checkpoint keeps no URL for it, and a resume of such a run makes no recovery
//! navigation. SC4 D1: such a resumed run, like a run that never navigated, has no tab on
//! chromium until something gives it one; an observing step (a wait) asks the driver for a
//! page (`ensure_page`, a tab on `about:blank` that goes nowhere) and answers 200 with the
//! blank observation, where it answered `DRIVER_FAILED` `no_current_page` (502).
//! `about:blank` is accepted only as that observation. A page on a host outside the run's
//! allowed domains is still refused, with the same `SECURITY_BLOCKED` on the REST step and
//! on the browser socket, and the refused step is recorded as `failed` with no observation
//! and no checkpoint of that page.
//!
//! The browser is the recording driver: `set_page_url` is the URL the page reports to the
//! observation (`window.location.href`), and `time_out_next_goto` is the navigation deadline.

#![cfg(feature = "browser-agent")]

#[path = "support/agent_harness.rs"]
mod agent_harness;
mod support;

use agent_harness::*;
use quarry_edge::agent_routes::sweep_once;
use quarry_edge::run_limits::RunLimits;
use quarry_runtime::step_receipts::AgentRunCheckpoint;
use serde_json::{json, Value};

/// The org of the standard-org member (`org-standard-golden`).
const MEMBER_ORG: &str = "org-standard-golden";
/// The refusal's message: the gateway maps a 403 that carries it to its typed code.
const OUTSIDE: &str = "outside the run's allowed domains";

fn run_on_example_test() -> Value {
    json!({ "constraints": { "max_steps": 0, "allowed_domains": ["example.test"] } })
}

fn navigate(url: &str) -> Value {
    json!({ "type": "navigate", "url": url })
}

/// Caps off, and both idle limits `idle_s`, so only the sweeper ends the run.
fn idle_limits(idle_s: u64) -> RunLimits {
    RunLimits {
        interactive_idle_ttl_s: idle_s,
        agent_idle_ttl_s: idle_s,
        max_concurrent_runs: 0,
        max_runs_per_org: 0,
        max_interactive_runs_per_org: 0,
        ..RunLimits::default()
    }
}

async fn checkpoint(edge: &Edge, run: &str) -> AgentRunCheckpoint {
    edge.state
        .receipts
        .load_run_checkpoint(MEMBER_ORG, &kind(run))
        .await
        .expect("checkpoint read")
        .expect("a non-ZDR run keeps its checkpoint")
}

/// The outcome status of each receipt, in step order.
async fn outcomes(edge: &Edge, person: &str, run: &str) -> Vec<(String, String)> {
    edge.receipts(person, run)
        .await
        .iter()
        .map(|receipt| {
            (
                receipt["action"]["type"].as_str().unwrap_or("").to_owned(),
                receipt["outcome"]["status"].as_str().unwrap_or("").to_owned(),
            )
        })
        .collect()
}

/// A run whose first navigation timed out with the tab left on `about:blank`.
async fn run_left_on_a_blank_tab(edge: &Edge, person: &str) -> String {
    let run = edge.start_run(person, run_on_example_test()).await;
    edge.driver.set_page_url(Some("about:blank"));
    edge.driver.time_out_next_goto();
    let timed_out = edge
        .step(person, &run, navigate("https://example.test/slow"), Some("agent"))
        .await;
    assert_eq!(timed_out.status, 504, "{}", timed_out.body);
    assert_eq!(timed_out.code(), "TIMEOUT", "{}", timed_out.body);
    run
}

/// The live T4b-1 sequence: first-step TIMEOUT, then a wait answers the blank tab (it was
/// a 403 "outside the run's allowed domains" with a `completed` receipt), then a navigate
/// works. The checkpoint keeps no URL for the blank tab.
#[tokio::test]
async fn after_a_first_step_timeout_the_blank_tab_is_observed_and_a_navigate_works() {
    let edge = Edge::start(RunLimits::default()).await;
    let person = member_token();
    let run = run_left_on_a_blank_tab(&edge, &person).await;

    let wait = edge.step(&person, &run, wait_action(), Some("agent")).await;
    assert_eq!(wait.status, 200, "the blank tab is observed: {}", wait.body);
    assert_eq!(wait.data()["url"], "about:blank", "{}", wait.body);
    assert_eq!(
        checkpoint(&edge, &run).await.current_url,
        "",
        "a tab with no page yet is checkpointed as no page"
    );

    edge.driver.set_page_url(Some("https://example.test/"));
    let next = edge
        .step(&person, &run, navigate("https://example.test/"), Some("agent"))
        .await;
    assert_eq!(next.status, 200, "{}", next.body);
    assert_eq!(checkpoint(&edge, &run).await.current_url, "https://example.test/");

    assert_eq!(
        outcomes(&edge, &person, &run).await,
        vec![
            ("navigate".to_owned(), "failed".to_owned()),
            ("wait".to_owned(), "completed".to_owned()),
            ("navigate".to_owned(), "completed".to_owned()),
        ]
    );
}

/// The blank tab is an observation only: a navigation to it is still refused before it
/// runs, as any other non-http(s) target is.
#[tokio::test]
async fn about_blank_is_never_a_navigation_target() {
    let edge = Edge::start(RunLimits::default()).await;
    let person = member_token();
    let run = edge.start_run(&person, run_on_example_test()).await;

    let refused = edge
        .step(&person, &run, navigate("about:blank"), Some("agent"))
        .await;
    assert_eq!(refused.status, 403, "{}", refused.body);
    assert_eq!(edge.driver.goto_entered(), 0, "the navigation never reached the browser");
}

/// A step that lands outside the run's allowed domains is refused, and only that is: the
/// code is `SECURITY_BLOCKED` (the REST step said `BAD_REQUEST`, the socket
/// `SECURITY_BLOCKED`), the message still carries the phrase the gateway maps, the receipt
/// is `failed` with that code and no observation, and the checkpoint never holds the page.
/// `about:srcdoc` is not a tab with no page yet and is refused the same way.
#[tokio::test]
async fn a_step_that_lands_outside_the_run_is_a_failed_security_blocked_step() {
    let edge = Edge::start(RunLimits::default()).await;
    let person = member_token();
    let run = edge.start_run(&person, run_on_example_test()).await;
    edge.driver.set_page_url(Some("https://example.test/a"));
    let first = edge.step(&person, &run, wait_action(), Some("agent")).await;
    assert_eq!(first.status, 200, "{}", first.body);
    // The refused steps did run: they read this egress, and the driver verified this cost.
    edge.driver.record_egress(1);
    edge.driver.set_action_cost(Some(250_000));

    for landed in ["https://evil.test/", "about:srcdoc"] {
        edge.driver.set_page_url(Some(landed));
        let refused = edge.step(&person, &run, wait_action(), Some("agent")).await;
        assert_eq!(refused.status, 403, "{landed}: {}", refused.body);
        assert_eq!(refused.code(), "SECURITY_BLOCKED", "{landed}: {}", refused.body);
        assert!(refused.message().contains(OUTSIDE), "{landed}: {}", refused.body);
        assert_eq!(
            checkpoint(&edge, &run).await.current_url,
            "https://example.test/a",
            "{landed}: the refused page never reaches the checkpoint"
        );
    }

    let receipts = edge.receipts(&person, &run).await;
    assert_eq!(receipts.len(), 3, "{receipts:?}");
    assert_eq!(receipts[0]["outcome"]["status"], "completed", "{receipts:?}");
    for refused in &receipts[1..] {
        assert_eq!(refused["outcome"]["status"], "failed", "{refused}");
        assert_eq!(refused["outcome"]["error_code"], "SECURITY_BLOCKED", "{refused}");
        assert!(refused["observation"].is_null(), "no observation of a refused page: {refused}");
        assert!(!refused.to_string().contains("evil.test"), "{refused}");
        assert_eq!(refused["cost_micro_usd"], 250_000, "the driver's cost: {refused}");
        assert_eq!(refused["cost_source"], "provider_billed", "{refused}");
    }

    // The egress the refused steps read is not lost: the next observation carries it.
    edge.driver.set_page_url(Some("https://example.test/a"));
    let next = edge.step(&person, &run, wait_action(), Some("agent")).await;
    assert_eq!(next.status, 200, "{}", next.body);
    assert_eq!(next.data()["egress_receipts"][0]["sequence"], 1, "{}", next.body);
}

/// The browser socket's step (the shared step path) gives the same two answers.
#[tokio::test]
async fn the_browser_socket_observes_a_blank_tab_and_refuses_a_foreign_page() {
    let edge = Edge::start(RunLimits::default()).await;
    let person = member_token();
    let run = run_left_on_a_blank_tab(&edge, &person).await;
    let mut socket =
        TestSocket::connect(&edge, &person, &format!("/v1/agent/runs/{run}/frames/ws")).await;
    let answer = |message: &Value| message["type"] == "observation" || message["type"] == "error";

    socket
        .send_text(&json!({ "type": "action", "action": wait_action(), "initiated_by": "agent" }).to_string())
        .await;
    let blank = socket.next_matching(answer).await;
    assert_eq!(blank["type"], "observation", "{blank}");
    assert_eq!(blank["observation"]["url"], "about:blank", "{blank}");

    edge.driver.set_page_url(Some("https://evil.test/"));
    socket
        .send_text(&json!({ "type": "action", "action": wait_action(), "initiated_by": "agent" }).to_string())
        .await;
    let refused = socket.next_matching(answer).await;
    assert_eq!(refused["type"], "error", "{refused}");
    assert_eq!(refused["code"], "SECURITY_BLOCKED", "{refused}");
    drop(socket);

    assert_eq!(
        outcomes(&edge, &person, &run).await,
        vec![
            ("navigate".to_owned(), "failed".to_owned()),
            ("wait".to_owned(), "completed".to_owned()),
            ("wait".to_owned(), "failed".to_owned()),
        ]
    );
}

/// A run released while its tab had no page yet resumes, both from the checkpoint this edge
/// writes (`""`) and from one written before the fix (`about:blank`), and the resume makes
/// no recovery navigation (there was a 403 "durable checkpoint URL is outside the persisted
/// allowed domains" for the second); a navigate then works. The recording driver always has
/// a page here; what a first observe after the resume answers is checked under
/// `model_pages` below.
#[tokio::test]
async fn a_run_checkpointed_on_a_blank_tab_resumes_without_a_recovery_navigation() {
    let edge = Edge::start(idle_limits(60)).await;
    let person = member_token();
    let run = run_left_on_a_blank_tab(&edge, &person).await;
    let wait = edge.step(&person, &run, wait_action(), Some("agent")).await;
    assert_eq!(wait.status, 200, "{}", wait.body);

    for stored in ["", "about:blank"] {
        assert_eq!(sweep_once(&edge.state, later(120_000)).await.expired, 1, "{stored:?}");
        let mut saved = checkpoint(&edge, &run).await;
        saved.current_url = stored.to_owned();
        edge.state
            .receipts
            .save_run_checkpoint(saved)
            .await
            .expect("checkpoint write");
        let gotos = edge.driver.goto_entered();

        let resumed = edge
            .post(&person, "/v1/agent/runs", json!({ "resume_run_id": run }))
            .await;
        assert_eq!(resumed.status, 200, "{stored:?}: {}", resumed.body);
        assert_eq!(
            edge.driver.goto_entered(),
            gotos,
            "{stored:?}: a resume never navigates to a blank tab"
        );
        edge.driver.set_page_url(Some("https://example.test/"));
        let after = edge
            .step(&person, &run, navigate("https://example.test/"), Some("agent"))
            .await;
        assert_eq!(after.status, 200, "{stored:?}: {}", after.body);
        assert_eq!(
            checkpoint(&edge, &run).await.current_url,
            "https://example.test/",
            "{stored:?}"
        );
        edge.driver.set_page_url(Some("about:blank"));
    }
}

/// SC4 D1, the live RS sequence: a run checkpointed on a blank tab expires and is resumed
/// from a checkpoint with no URL (this edge's `""`, and a legacy `about:blank`). The resume
/// makes no recovery navigation, so the new session has no page; the next wait asks the
/// driver for one and answers 200 with the blank observation (it was 502 `DRIVER_FAILED`
/// `no_current_page`, not retryable), its receipt is `completed`, the checkpoint keeps no
/// URL, and a navigate after it works.
#[tokio::test]
async fn a_run_resumed_with_no_url_observes_a_blank_tab_and_then_navigates() {
    let edge = Edge::start(idle_limits(60)).await;
    edge.driver.model_pages();
    let person = member_token();
    let run = run_left_on_a_blank_tab(&edge, &person).await;

    for stored in ["", "about:blank"] {
        assert_eq!(
            sweep_once(&edge.state, later(120_000)).await.expired,
            1,
            "{stored:?}"
        );
        let mut saved = checkpoint(&edge, &run).await;
        saved.current_url = stored.to_owned();
        edge.state
            .receipts
            .save_run_checkpoint(saved)
            .await
            .expect("checkpoint write");
        let resumed = edge
            .post(&person, "/v1/agent/runs", json!({ "resume_run_id": run }))
            .await;
        assert_eq!(resumed.status, 200, "{stored:?}: {}", resumed.body);
        let (gotos, ensured) = (edge.driver.goto_entered(), edge.driver.ensure_page_calls());

        edge.driver.set_page_url(Some("about:blank"));
        let wait = edge.step(&person, &run, wait_action(), Some("agent")).await;
        assert_eq!(
            wait.status, 200,
            "{stored:?}: the blank tab is observed: {}",
            wait.body
        );
        assert_eq!(
            wait.data()["url"],
            "about:blank",
            "{stored:?}: {}",
            wait.body
        );
        assert_eq!(edge.driver.ensure_page_calls(), ensured + 1, "{stored:?}");
        assert_eq!(
            edge.driver.goto_entered(),
            gotos,
            "{stored:?}: the wait navigated nowhere"
        );
        assert_eq!(checkpoint(&edge, &run).await.current_url, "", "{stored:?}");

        edge.driver.set_page_url(Some("https://example.test/"));
        let next = edge
            .step(
                &person,
                &run,
                navigate("https://example.test/"),
                Some("agent"),
            )
            .await;
        assert_eq!(next.status, 200, "{stored:?}: {}", next.body);
        assert_eq!(
            checkpoint(&edge, &run).await.current_url,
            "https://example.test/",
            "{stored:?}"
        );
        // The next round starts from a run whose page is blank again.
        edge.driver.set_page_url(Some("about:blank"));
        let blank = edge.step(&person, &run, wait_action(), Some("agent")).await;
        assert_eq!(blank.status, 200, "{stored:?}: {}", blank.body);
    }

    let receipts = edge.receipts(&person, &run).await;
    let waits_after_resume: Vec<&str> = receipts
        .iter()
        .filter(|receipt| receipt["action"]["type"] == "wait")
        .map(|receipt| receipt["outcome"]["status"].as_str().unwrap_or(""))
        .collect();
    assert_eq!(waits_after_resume, vec!["completed"; 4], "{receipts:?}");
}

/// SC4 D1, the decision for a run that never navigated: its first step may be a wait, which
/// observes a blank tab (200, `about:blank`) instead of failing `no_current_page`; nothing is
/// navigated, the checkpoint keeps no URL, and the first navigate then works on that tab.
#[tokio::test]
async fn a_fresh_runs_first_wait_observes_a_blank_tab() {
    let edge = Edge::start(RunLimits::default()).await;
    edge.driver.model_pages();
    let person = member_token();
    let run = edge.start_run(&person, run_on_example_test()).await;

    edge.driver.set_page_url(Some("about:blank"));
    let wait = edge.step(&person, &run, wait_action(), Some("agent")).await;
    assert_eq!(wait.status, 200, "{}", wait.body);
    assert_eq!(wait.data()["url"], "about:blank", "{}", wait.body);
    assert_eq!(edge.driver.ensure_page_calls(), 1);
    assert_eq!(edge.driver.goto_entered(), 0, "a wait navigates nowhere");
    assert_eq!(checkpoint(&edge, &run).await.current_url, "");

    edge.driver.set_page_url(Some("https://example.test/"));
    let next = edge
        .step(
            &person,
            &run,
            navigate("https://example.test/"),
            Some("agent"),
        )
        .await;
    assert_eq!(next.status, 200, "{}", next.body);
    assert_eq!(
        checkpoint(&edge, &run).await.current_url,
        "https://example.test/"
    );
    assert_eq!(
        outcomes(&edge, &person, &run).await,
        vec![
            ("wait".to_owned(), "completed".to_owned()),
            ("navigate".to_owned(), "completed".to_owned()),
        ]
    );
}

/// SC4 D1 review: a `wait_for` is a read like a wait (the edge's passive reads), so a fresh
/// run's first `wait_for` waits on the blank tab (`body` is there on `about:blank`) instead
/// of failing `no_current_page`; nothing is navigated and the checkpoint keeps no URL.
#[tokio::test]
async fn a_fresh_runs_first_wait_for_waits_on_a_blank_tab() {
    let edge = Edge::start(RunLimits::default()).await;
    edge.driver.model_pages();
    let person = member_token();
    let run = edge.start_run(&person, run_on_example_test()).await;

    edge.driver.set_page_url(Some("about:blank"));
    let wait_for = edge
        .step(
            &person,
            &run,
            json!({ "type": "wait_for", "selector": "body", "timeout_ms": 1000 }),
            Some("agent"),
        )
        .await;
    assert_eq!(wait_for.status, 200, "{}", wait_for.body);
    assert_eq!(wait_for.data()["url"], "about:blank", "{}", wait_for.body);
    assert_eq!(edge.driver.ensure_page_calls(), 1);
    assert_eq!(
        edge.driver.goto_entered(),
        0,
        "a wait_for navigates nowhere"
    );
    assert_eq!(checkpoint(&edge, &run).await.current_url, "");
    assert_eq!(
        outcomes(&edge, &person, &run).await,
        vec![("wait_for".to_owned(), "completed".to_owned())]
    );
}
