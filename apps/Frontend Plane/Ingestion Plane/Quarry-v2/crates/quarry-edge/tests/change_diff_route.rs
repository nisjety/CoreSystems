//! INNH-4: `GET /v1/change/diff` through the real router and real RS256 tokens.
//!
//! The handler is gated three ways before it touches the baseline store: the switch
//! (`QUARRY_EDGE__CHANGE_TEXT_DIFF`, 501 when off), the caller's ZDR posture (403) and the query
//! (400). This harness wires no baseline store, so what is proved here is the gate order and the
//! answers; the read logic behind it is covered against an in-memory store in
//! `change_diff.rs`.
//!
//! One test, because it flips a process-wide environment variable.
#![cfg(feature = "postgres-queue")]

mod support;
#[path = "support/zdr_harness.rs"]
mod zdr_harness;

use serde_json::Value;

use zdr_harness::{user_in_standard_org, user_in_zdr_org, Harness};

const FLAG: &str = "QUARRY_EDGE__CHANGE_TEXT_DIFF";
const ROUTE: &str = "/v1/change/diff";

async fn answer(harness: &Harness, bearer: &str, query: &str) -> (u16, Value) {
    let response = harness.get(bearer, &format!("{ROUTE}{query}")).await;
    let status = response.status().as_u16();
    (status, response.json().await.unwrap_or(Value::Null))
}

fn message(payload: &Value) -> String {
    payload["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

#[tokio::test]
async fn the_route_is_off_by_default_then_gates_zdr_and_the_query() {
    let harness = Harness::start().await;
    let standard = user_in_standard_org();
    let zdr = user_in_zdr_org();

    // Off (unset): 501 for everyone, naming the switch; a ZDR caller gets the same answer.
    std::env::remove_var(FLAG);
    for bearer in [&standard, &zdr] {
        let (status, payload) = answer(&harness, bearer, "?url=https://example.com/pricing").await;
        assert_eq!(status, 501, "{payload}");
        assert!(
            message(&payload).contains(FLAG),
            "the 501 names the switch: {payload}"
        );
    }
    std::env::set_var(FLAG, "0");
    let (status, _) = answer(&harness, &standard, "?url=https://example.com/pricing").await;
    assert_eq!(status, 501, "0 is off");

    // On: a ZDR caller is refused before anything else, whatever the query.
    std::env::set_var(FLAG, "1");
    for query in ["?url=https://example.com/pricing", "", "?diff_id=diff_1"] {
        let (status, payload) = answer(&harness, &zdr, query).await;
        assert_eq!(status, 403, "{query}: {payload}");
        assert!(
            message(&payload).contains("Zero Data Retention"),
            "{payload}"
        );
    }

    // On, a standard caller: the query is checked (400) before the store is asked.
    for query in [
        "",
        "?from=bln_1&to=bln_2",
        "?url=https://example.com/pricing&from=bln_1",
        "?diff_id=diff_1&from=bln_1&to=bln_2",
        "?url=https://example.com/pricing&include=html",
        "?url=not-a-url",
    ] {
        let (status, payload) = answer(&harness, &standard, query).await;
        assert_eq!(status, 400, "{query}: {payload}");
    }
    // A valid query reaches the store check, and this harness has none: 501 with its own text.
    let (status, payload) = answer(&harness, &standard, "?url=https://example.com/pricing").await;
    assert_eq!(status, 501, "{payload}");
    assert!(message(&payload).contains("DATABASE_URL"), "{payload}");
    assert!(!message(&payload).contains(FLAG), "{payload}");

    std::env::remove_var(FLAG);
}
