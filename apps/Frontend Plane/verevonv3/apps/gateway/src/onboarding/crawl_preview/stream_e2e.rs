//! End-to-end SSE contract test for `POST /api/v1/onboarding/crawl-preview`.
//!
//! The two fixtures below are BYTE-FOR-BYTE captures taken from the live
//! Ingestion Plane on 2026-09-04 against `https://aquatiq.com`:
//!
//! * `SEED_STREAM` — the `POST /v1/scrape/stream` response (full quarry-core
//!   `Event` envelopes: `page_fetched`, `change_detected`,
//!   `branding_extracted`, three `artifact_written`, and the new
//!   `page_extracted`).
//! * `JOB_EVENTS` — the `GET /v1/jobs/{id}/events` drain for a 4-page crawl
//!   of the same site (`run_started`, then `page_fetched` +
//!   `branding_extracted` + `page_extracted` per page, then `run_completed`).
//!
//! Replaying real upstream bytes through the real handler is what pins the
//! wire contract the onboarding wizard consumes: that `page_extracted` (not
//! just the text-less `page_fetched`) becomes a snippet carrying a genuine
//! title and excerpt, that the ledger collapses the seed/live duplicates of
//! one page into a single card instead of emitting an empty second one, and
//! that `done.pages` counts unique pages. A stub upstream keeps it hermetic;
//! only the browser session and the network hop are simulated.

use axum::{
    body::to_bytes, extract::State, http::HeaderMap, response::IntoResponse, Extension, Json,
};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::audience_tokens::new_audience_token_cache;
use crate::cache::ResultCache;
use crate::config::AppState;
use crate::contracts::CrawlPreviewRequest;
use crate::middleware::AuthenticatedUser;

const JOB_ID: &str = "job_01M1P07TCD0H216B5VJ8G9SGZS";

const SEED_STREAM: &str = r##"event: page_fetched
data: {"event_id":"evt_01M1P0T7NMEW48TP80Q2N1FZJN","run_id":"run_01M1P0T4F0QQVRC6SJMQWJ2TKG","type":"page_fetched","ts":"2026-09-04T10:53:14.292537955Z","seq":0,"payload":{"content_type":"text/html; charset=utf-8","duration_ms":92,"status":200,"url":"https://www.aquatiq.com/no"},"idempotency_key":"stream-e2e-idempotency-key-not-real-01"}

event: change_detected
data: {"event_id":"evt_01M1P0T7PCN5AKMXCE7QG6W84Y","run_id":"run_01M1P0T4F0QQVRC6SJMQWJ2TKG","type":"change_detected","ts":"2026-09-04T10:53:14.316033348Z","seq":1,"payload":{"fingerprint":"blake3:34f8dee750095ca67dcb81abe364af2c5f10868dc6a0940ee7e751d4d7add388","prev":null,"text_fingerprint":"blake3:53d65f4330bb8f1ba8231e11d0e2ae38e7087120e8667dfb82eb700e29aec0e7","url":"https://www.aquatiq.com/no"},"idempotency_key":"stream-e2e-idempotency-key-not-real-02"}

event: branding_extracted
data: {"event_id":"evt_01M1P0T7PJKYVHY8CXE5SBXNZZ","run_id":"run_01M1P0T4F0QQVRC6SJMQWJ2TKG","type":"branding_extracted","ts":"2026-09-04T10:53:14.322647565Z","seq":2,"payload":{"branding":{"body_background":null,"font_family":null,"logo_candidate":null,"palette":[],"static_signals":{"favicon":"https://www.aquatiq.com/favicon.ico","site_name":"Aquatiq"}},"url":"https://www.aquatiq.com/no"},"idempotency_key":"stream-e2e-idempotency-key-not-real-03"}

event: artifact_written
data: {"event_id":"evt_01M1P0T7PJXEGTDH1KGTATZCGR","run_id":"run_01M1P0T4F0QQVRC6SJMQWJ2TKG","type":"artifact_written","ts":"2026-09-04T10:53:14.322781588Z","seq":3,"payload":{"artifact_id":"art_01M1P0T7PJ2A0ZRAXKSYTCWH0H","bytes":219793,"format":"html","url":"https://www.aquatiq.com/no"},"idempotency_key":"stream-e2e-idempotency-key-not-real-04"}

event: artifact_written
data: {"event_id":"evt_01M1P0T7PJRRWBYE0Q4RSR7NQ7","run_id":"run_01M1P0T4F0QQVRC6SJMQWJ2TKG","type":"artifact_written","ts":"2026-09-04T10:53:14.322787609Z","seq":4,"payload":{"artifact_id":"art_01M1P0T7PJYDW1X0NYNKRTTQJZ","bytes":6203,"format":"markdown","url":"https://www.aquatiq.com/no"},"idempotency_key":"stream-e2e-idempotency-key-not-real-05"}

event: artifact_written
data: {"event_id":"evt_01M1P0T7PJSTDYKKQA0RDMJS2E","run_id":"run_01M1P0T4F0QQVRC6SJMQWJ2TKG","type":"artifact_written","ts":"2026-09-04T10:53:14.322804884Z","seq":5,"payload":{"artifact_id":"art_01M1P0T7PJ9SJ1NDJ8WHY3KSEW","bytes":130,"format":"meta","url":"https://www.aquatiq.com/no"},"idempotency_key":"stream-e2e-idempotency-key-not-real-06"}

event: page_extracted
data: {"event_id":"evt_01M1P0T7PKW31WXWGJ3KPF4E82","run_id":"run_01M1P0T4F0QQVRC6SJMQWJ2TKG","type":"page_extracted","ts":"2026-09-04T10:53:14.323590353Z","seq":6,"payload":{"content_type":"text/html; charset=utf-8","driver":"static","excerpt":"Food Safety Experts Leverandør av kompetanse, rengjøringssystemer, kjemi og hygieniske prosessløsninger til den globale næringsmiddelindustrien. Hvordan kan vi hjelpe deg? Aquatiq tilbyr profesjonelle tjenester og produkter innen følgende kategorier: Chemistry Spesialisert kjemi for næringsmiddel…","fingerprint":"blake3:34f8dee750095ca67dcb81abe364af2c5f10868dc6a0940ee7e751d4d7add388","lang":"nb-NO","title":"Aquatiq - Global leder på Trygg Mat ekspertise og…","title_source":"html","url":"https://www.aquatiq.com/no","word_count":651},"idempotency_key":"stream-e2e-idempotency-key-not-real-07"}

data: done

"##;

const JOB_EVENTS: &str = r##"event: run_started
data: {"event_id":"evt_45d0bf2ce44e9207a799528c39","idempotency_key":"stream-e2e-idempotency-key-not-real-08","job_id":"job_01M1P07TCD0H216B5VJ8G9SGZS","payload":{"kind":"crawl","max_depth":1,"max_pages":4,"seeds":1},"run_id":"run_01M1P07VCJ7KQ92PT8PE2JF79J","seq":1,"ts":"2026-09-04T10:43:11.9276Z","type":"run_started"}

event: page_fetched
data: {"event_id":"evt_63b1e417b72535d9a9a3ae31f0","idempotency_key":"stream-e2e-idempotency-key-not-real-09","job_id":"job_01M1P07TCD0H216B5VJ8G9SGZS","payload":{"content_type":"text/html; charset=utf-8","fingerprint":"blake3:34f8dee750095ca67dcb81abe364af2c5f10868dc6a0940ee7e751d4d7add388","links":70,"status":200,"title":"Aquatiq - Global leder på Trygg Mat ekspertise og løsninger | Aquatiq","url":"https://aquatiq.com"},"run_id":"run_01M1P07VCJ7KQ92PT8PE2JF79J","seq":2,"ts":"2026-09-04T10:43:14.696468Z","type":"page_fetched"}

event: branding_extracted
data: {"event_id":"evt_4c59e970ffebcdeff4d7d54c1b","idempotency_key":"stream-e2e-idempotency-key-not-real-10","job_id":"job_01M1P07TCD0H216B5VJ8G9SGZS","payload":{"branding":{"body_background":null,"font_family":null,"logo_candidate":null,"palette":[],"static_signals":{"favicon":"https://www.aquatiq.com/favicon.ico","site_name":"Aquatiq"}},"url":"https://aquatiq.com"},"run_id":"run_01M1P07VCJ7KQ92PT8PE2JF79J","seq":3,"ts":"2026-09-04T10:43:14.769116Z","type":"branding_extracted"}

event: page_extracted
data: {"event_id":"evt_d04d0e0dbb5ab3df42b0ffdbcb","idempotency_key":"stream-e2e-idempotency-key-not-real-11","job_id":"job_01M1P07TCD0H216B5VJ8G9SGZS","payload":{"content_type":"text/html; charset=utf-8","driver":"static","excerpt":"Food Safety Experts Leverandør av kompetanse, rengjøringssystemer, kjemi og hygieniske prosessløsninger til den globale næringsmiddelindustrien. Hvordan kan vi hjelpe deg? ### Aquatiq tilbyr profesjonelle tjenester og produkter innen følgende kategorier: [ Chemistry ### Spesialisert kjemi for…","fingerprint":"blake3:34f8dee750095ca67dcb81abe364af2c5f10868dc6a0940ee7e751d4d7add388","lang":"nb-NO","title":"Aquatiq - Global leder på Trygg Mat ekspertise og…","title_source":"html","url":"https://aquatiq.com","word_count":676},"run_id":"run_01M1P07VCJ7KQ92PT8PE2JF79J","seq":4,"ts":"2026-09-04T10:43:14.84493Z","type":"page_extracted"}

event: page_fetched
data: {"event_id":"evt_c7d933c81dba4e2808f6244d6f","idempotency_key":"stream-e2e-idempotency-key-not-real-12","job_id":"job_01M1P07TCD0H216B5VJ8G9SGZS","payload":{"content_type":"text/html; charset=utf-8","fingerprint":"blake3:34f8dee750095ca67dcb81abe364af2c5f10868dc6a0940ee7e751d4d7add388","links":70,"status":200,"title":"Aquatiq - Global leder på Trygg Mat ekspertise og løsninger | Aquatiq","url":"https://www.aquatiq.com/no"},"run_id":"run_01M1P07VCJ7KQ92PT8PE2JF79J","seq":5,"ts":"2026-09-04T10:43:23.390707Z","type":"page_fetched"}

event: branding_extracted
data: {"event_id":"evt_32a09602b9cc8a6e3832bdf116","idempotency_key":"stream-e2e-idempotency-key-not-real-13","job_id":"job_01M1P07TCD0H216B5VJ8G9SGZS","payload":{"branding":{"body_background":null,"font_family":null,"logo_candidate":null,"palette":[],"static_signals":{"favicon":"https://www.aquatiq.com/favicon.ico","site_name":"Aquatiq"}},"url":"https://www.aquatiq.com/no"},"run_id":"run_01M1P07VCJ7KQ92PT8PE2JF79J","seq":6,"ts":"2026-09-04T10:43:23.437619Z","type":"branding_extracted"}

event: page_extracted
data: {"event_id":"evt_478d077c88db65d9f56d2afe57","idempotency_key":"stream-e2e-idempotency-key-not-real-14","job_id":"job_01M1P07TCD0H216B5VJ8G9SGZS","payload":{"content_type":"text/html; charset=utf-8","driver":"static","excerpt":"Food Safety Experts Leverandør av kompetanse, rengjøringssystemer, kjemi og hygieniske prosessløsninger til den globale næringsmiddelindustrien. Hvordan kan vi hjelpe deg? ### Aquatiq tilbyr profesjonelle tjenester og produkter innen følgende kategorier: [ Chemistry ### Spesialisert kjemi for…","fingerprint":"blake3:34f8dee750095ca67dcb81abe364af2c5f10868dc6a0940ee7e751d4d7add388","lang":"nb-NO","title":"Aquatiq - Global leder på Trygg Mat ekspertise og…","title_source":"html","url":"https://www.aquatiq.com/no","word_count":676},"run_id":"run_01M1P07VCJ7KQ92PT8PE2JF79J","seq":7,"ts":"2026-09-04T10:43:23.491743Z","type":"page_extracted"}

event: page_fetched
data: {"event_id":"evt_e2c032d18053fdd3a0a8515137","idempotency_key":"stream-e2e-idempotency-key-not-real-15","job_id":"job_01M1P07TCD0H216B5VJ8G9SGZS","payload":{"content_type":"text/html; charset=utf-8","fingerprint":"blake3:0eb853566bac514c178b43c55a9d68ac0eb5e67e79c34ad145905f9b4266da82","links":84,"status":200,"title":"Kjemiske løsninger for mat, havbruk og industri | Aquatiq","url":"https://www.aquatiq.com/no/chemistry"},"run_id":"run_01M1P07VCJ7KQ92PT8PE2JF79J","seq":8,"ts":"2026-09-04T10:43:26.333162Z","type":"page_fetched"}

event: branding_extracted
data: {"event_id":"evt_26b114e13e603d53cde85cf68c","idempotency_key":"stream-e2e-idempotency-key-not-real-16","job_id":"job_01M1P07TCD0H216B5VJ8G9SGZS","payload":{"branding":{"body_background":null,"font_family":null,"logo_candidate":null,"palette":[],"static_signals":{"favicon":"https://www.aquatiq.com/favicon.ico","site_name":"Aquatiq"}},"url":"https://www.aquatiq.com/no/chemistry"},"run_id":"run_01M1P07VCJ7KQ92PT8PE2JF79J","seq":9,"ts":"2026-09-04T10:43:26.386451Z","type":"branding_extracted"}

event: page_extracted
data: {"event_id":"evt_ddd3d621e85880acecd4e00bf1","idempotency_key":"stream-e2e-idempotency-key-not-real-17","job_id":"job_01M1P07TCD0H216B5VJ8G9SGZS","payload":{"content_type":"text/html; charset=utf-8","driver":"static","excerpt":"Chemistry Services Spesialisert kjemi for matindustrien, transportsektoren og tungindustrien. [Næringsmiddel ](/no/naeringsmiddelindustri)[Akvakultur ](/en/aquaculture)[Industri ](/no/mekanisk-industri)[Transport ](/no/transportindustrien)[Helse, miljø og sikkerhet ](/no/hms)[Miljøsikring og…","fingerprint":"blake3:0eb853566bac514c178b43c55a9d68ac0eb5e67e79c34ad145905f9b4266da82","lang":"nb-NO","title":"Kjemiske løsninger for mat, havbruk og industri | Aquatiq","title_source":"html","url":"https://www.aquatiq.com/no/chemistry","word_count":1025},"run_id":"run_01M1P07VCJ7KQ92PT8PE2JF79J","seq":10,"ts":"2026-09-04T10:43:26.439251Z","type":"page_extracted"}

event: page_fetched
data: {"event_id":"evt_e5c5d713eee6952c2a84edda07","idempotency_key":"stream-e2e-idempotency-key-not-real-18","job_id":"job_01M1P07TCD0H216B5VJ8G9SGZS","payload":{"content_type":"text/html; charset=utf-8","fingerprint":"blake3:af2b5a652d7422879e57219e2814516a3a99d3ebd0e18449ee8c0a43bf52082f","links":85,"status":200,"title":"Kurs og revisjon innen mattrygghet og kvalitet | Aquatiq","url":"https://www.aquatiq.com/no/kurs-and-revisjon"},"run_id":"run_01M1P07VCJ7KQ92PT8PE2JF79J","seq":11,"ts":"2026-09-04T10:43:29.04034Z","type":"page_fetched"}

event: branding_extracted
data: {"event_id":"evt_3de15d0d3c11ec11af0bf6c310","idempotency_key":"stream-e2e-idempotency-key-not-real-19","job_id":"job_01M1P07TCD0H216B5VJ8G9SGZS","payload":{"branding":{"body_background":null,"font_family":null,"logo_candidate":null,"palette":[],"static_signals":{"favicon":"https://www.aquatiq.com/favicon.ico","site_name":"Aquatiq"}},"url":"https://www.aquatiq.com/no/kurs-and-revisjon"},"run_id":"run_01M1P07VCJ7KQ92PT8PE2JF79J","seq":12,"ts":"2026-09-04T10:43:29.090695Z","type":"branding_extracted"}

event: page_extracted
data: {"event_id":"evt_163c292c7d0eaf6851452bff88","idempotency_key":"stream-e2e-idempotency-key-not-real-20","job_id":"job_01M1P07TCD0H216B5VJ8G9SGZS","payload":{"content_type":"text/html; charset=utf-8","driver":"static","excerpt":"Kurs og revisjon Tjenester Vi hjelper matindustrien med å forebygge og minimere risiko knyttet til mattrygghet. [Åpne kurs ](/no/kurs-og-opplaering)[E-læring ](/no/e-learning)[Aquatiq Food Forum ](https://www.aquatiq.com/no/food-forum)[Bedriftstilpassede kurs…","fingerprint":"blake3:af2b5a652d7422879e57219e2814516a3a99d3ebd0e18449ee8c0a43bf52082f","lang":"nb-NO","title":"Kurs og revisjon innen mattrygghet og kvalitet | Aquatiq","title_source":"html","url":"https://www.aquatiq.com/no/kurs-and-revisjon","word_count":427},"run_id":"run_01M1P07VCJ7KQ92PT8PE2JF79J","seq":13,"ts":"2026-09-04T10:43:29.153851Z","type":"page_extracted"}

event: run_completed
data: {"event_id":"evt_52e9582dad69265324b97ef070","idempotency_key":"stream-e2e-idempotency-key-not-real-21","job_id":"job_01M1P07TCD0H216B5VJ8G9SGZS","payload":{"pages_failed":0,"pages_visited":4},"run_id":"run_01M1P07VCJ7KQ92PT8PE2JF79J","seq":14,"ts":"2026-09-04T10:43:29.267843Z","type":"run_completed"}

data: done

"##;

fn test_state(quarry_edge_url: String) -> AppState {
    AppState {
        client: reqwest::Client::new(),
        streaming_client: reqwest::Client::new(),
        internal_api_key: "test-key".into(),
        enforcement_mode: "off".into(),
        // Unroutable: the onboarding preview-token mint fails, so the
        // upstream calls go out unauthenticated — which is exactly the
        // degraded path we want covered here, and the stub does not check.
        auth_core_url: "http://127.0.0.1:1".into(),
        verevon_public_origin: "http://localhost:5173".into(),
        session_core_url: "http://127.0.0.1:1".into(),
        session_core_service_token: "stream-e2e-not-a-real-session-core-service-token".into(),
        user_core_service_token: "stream-e2e-not-a-real-user-core-service-token".into(),
        billing_core_url: "http://127.0.0.1:1".into(),
        billing_core_service_token: "stream-e2e-not-a-real-billing-core-service-token".into(),
        org_core_url: "http://127.0.0.1:1".into(),
        org_core_service_token: "stream-e2e-not-a-real-org-core-service-token".into(),
        integration_core_url: "http://127.0.0.1:1".into(),
        audit_core_url: "http://127.0.0.1:1".into(),
        audit_core_service_token: "stream-e2e-not-a-real-audit-core-service-token".into(),
        insight_core_url: "http://127.0.0.1:1".into(),
        leads_core_url: "http://127.0.0.1:1".into(),
        shipping_core_url: "http://127.0.0.1:1".into(),
        user_core_url: "http://127.0.0.1:1".into(),
        application_convex_url: String::new(),
        application_convex_service_key: String::new(),
        graph_index_url: "http://127.0.0.1:1".into(),
        quarry_edge_url,
        model_recommend_url: "http://127.0.0.1:1".into(),
        model_gateway_url: "http://127.0.0.1:1".into(),
        cost_core_url: "http://127.0.0.1:1".into(),
        model_gateway_dev_bearer: String::new(),
        inference_core_url: "http://127.0.0.1:1".into(),
        documents_api_url: "http://127.0.0.1:1".into(),
        retrieval_engine_url: "http://127.0.0.1:1".into(),
        wiki_store_url: "http://127.0.0.1:1".into(),
        embedding_engine_url: "http://127.0.0.1:1".into(),
        quickwit_adapter_url: "http://127.0.0.1:1".into(),
        finspo_core_url: "http://127.0.0.1:1".into(),
        imports_api_url: "http://127.0.0.1:1".into(),
        notification_core_url: "http://127.0.0.1:1".into(),
        notification_core_service_token: "stream-e2e-not-a-real-notification-core-service-token".into(),
        information_core_url: "http://127.0.0.1:1".into(),
        conversation_core_url: "http://127.0.0.1:1".into(),
        conversation_core_service_token: "stream-e2e-not-a-real-conversation-core-service-token".into(),
        social_core_url: "http://127.0.0.1:1".into(),
        searxng_url: "http://127.0.0.1:1".into(),
        autocomplete_core_url: "http://127.0.0.1:1".into(),
        autocomplete_token: String::new(),
        zammad_api_url: "http://127.0.0.1:1".into(),
        zammad_api_token: String::new(),
        remote_support_rendezvous_url: String::new(),
        remote_support_relay_url: String::new(),
        remote_support_server_public_key: String::new(),
        audience_token_cache: new_audience_token_cache(),
        browser_run_store: crate::domains::browser::new_browser_run_store(),
        rate_limiter: crate::rate_limit::RateLimiter::from_cache(&ResultCache::disabled()),
        cache: ResultCache::disabled(),
        studio_store: crate::domains::studio::StudioStore::new(),
        allow_dev_actor_headers: false,
        allow_dev_auth_bypass: false,
    }
}

fn test_user() -> AuthenticatedUser {
    AuthenticatedUser {
        user_id: "user-onboarding-test".to_owned(),
        user_email: "user@example.test".to_owned(),
        user_name: "User Test".to_owned(),
        user_image: None,
        email_verified: true,
        auth_role: Some("member".to_owned()),
        // The website step runs before an org exists.
        active_org_id: None,
        authorized_membership: None,
    }
}

/// One `(event_name, payload)` pair per SSE frame, in stream order.
fn parse_sse(body: &str) -> Vec<(String, Value)> {
    let mut out = Vec::new();
    for frame in body.split("\n\n") {
        let mut name = String::new();
        let mut data = String::new();
        for line in frame.lines() {
            if let Some(v) = line.strip_prefix("event:") {
                name = v.trim().to_owned();
            }
            if let Some(v) = line.strip_prefix("data:") {
                data.push_str(v.trim());
            }
        }
        if name.is_empty() || data.is_empty() {
            continue;
        }
        if let Ok(value) = serde_json::from_str::<Value>(&data) {
            out.push((name, value));
        }
    }
    out
}

fn page_key(url: &str) -> String {
    url.split('#')
        .next()
        .unwrap_or(url)
        .trim_end_matches('/')
        .to_lowercase()
}

async fn run_preview() -> Vec<(String, Value)> {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/crawl"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": { "job_id": JOB_ID },
            "meta": { "request_id": "req_test" },
            "error": null,
        })))
        .mount(&upstream)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/scrape/stream"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(SEED_STREAM)
                .insert_header("content-type", "text/event-stream"),
        )
        .mount(&upstream)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/v1/jobs/{JOB_ID}/events")))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(JOB_EVENTS)
                .insert_header("content-type", "text/event-stream"),
        )
        .mount(&upstream)
        .await;

    let response = crate::onboarding::crawl_preview::crawl_preview(
        State(test_state(upstream.uri())),
        Extension(test_user()),
        HeaderMap::new(),
        Json(CrawlPreviewRequest {
            url: "https://aquatiq.com".to_owned(),
            _brief: None,
            max_pages: Some(4),
        }),
    )
    .await
    .into_response();

    let bytes = to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .expect("collect sse body");
    parse_sse(&String::from_utf8_lossy(&bytes))
}

#[tokio::test]
async fn crawl_preview_streams_real_titles_and_excerpts_end_to_end() {
    let events = run_preview().await;
    let names: Vec<&str> = events.iter().map(|(n, _)| n.as_str()).collect();

    // Lifecycle frames the wizard drives its phase list from.
    assert_eq!(names.first(), Some(&"started"), "names: {names:?}");
    assert_eq!(names.last(), Some(&"done"), "names: {names:?}");
    assert!(names.contains(&"progress"), "names: {names:?}");
    assert!(names.contains(&"branding"), "names: {names:?}");
    // A crawl that collected content must never report the empty-crawl warning.
    let warnings: Vec<&Value> = events
        .iter()
        .filter(|(n, _)| n == "warning")
        .map(|(_, v)| v)
        .collect();
    assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");

    let snippets: Vec<&Value> = events
        .iter()
        .filter(|(n, _)| n == "snippet")
        .map(|(_, v)| v)
        .collect();
    assert!(!snippets.is_empty(), "no snippets: {names:?}");

    // Quarry emits TWO frames per page: the pre-transform `page_fetched`
    // (no text — and on the seed path not even a title, since the runtime's
    // payload is only url/status/duration/content_type) and the
    // post-transform `page_extracted`. The gateway forwards the first as an
    // immediate "fetching" placeholder and the second as a richer UPDATE
    // under the same id, so one page may appear twice in the stream. What
    // must hold is that the LAST snippet for every page is the rich one —
    // that is the state the wizard renders after its own merge.
    let mut last_per_page: BTreeMap<String, &Value> = BTreeMap::new();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for snippet in &snippets {
        let key = page_key(snippet["url"].as_str().unwrap_or_default());
        *counts.entry(key.clone()).or_default() += 1;
        last_per_page.insert(key, snippet);
        // A blank title would fail the wizard's `min(1)` schema and drop the
        // card silently, and a redirect stub in the excerpt was the original
        // empty-text bug.
        let title = snippet["title"].as_str().unwrap_or_default();
        assert!(!title.trim().is_empty(), "blank title: {snippet:?}");
        assert!(
            !snippet["excerpt"]
                .as_str()
                .unwrap_or_default()
                .contains("Redirecting"),
            "redirect stub leaked into an excerpt: {snippet:?}"
        );
    }
    assert!(
        counts.values().all(|count| *count <= 2),
        "a page was streamed more than twice (pre/post transform): {counts:?}"
    );
    // The ledger's update path must actually have run on this real capture,
    // otherwise the dedupe is untested here.
    assert!(
        counts.values().any(|count| *count == 2),
        "no page was updated by a richer snippet: {counts:?}"
    );

    // The regression this whole change exists for: the card the wizard ends
    // up rendering for each page carries a real title and real text, never a
    // host label over an empty excerpt.
    for (key, snippet) in &last_per_page {
        let title = snippet["title"].as_str().expect("snippet title");
        let excerpt = snippet["excerpt"].as_str().unwrap_or_default();
        assert_ne!(title, "aquatiq.com", "host-label title survived for {key}");
        assert_ne!(
            title, "www.aquatiq.com",
            "host-label title survived for {key}"
        );
        assert!(
            excerpt.chars().count() > 40,
            "excerpt too short for {key}: {excerpt:?}"
        );
        assert_eq!(snippet["titleSource"], "html", "for {key}");
        assert_eq!(snippet["driver"], "static", "for {key}");
        assert_eq!(snippet["kind"], "text", "for {key}");
        assert!(
            snippet["wordCount"].as_u64().unwrap_or(0) > 100,
            "word count for {key}"
        );
    }

    // Real page identities from the live capture, proving actual site content
    // (not a fixture placeholder) travelled the whole way through.
    let titles: BTreeSet<&str> = last_per_page
        .values()
        .map(|s| s["title"].as_str().unwrap_or_default())
        .collect();
    assert!(
        titles
            .iter()
            .any(|t| t.contains("Kjemiske løsninger for mat")),
        "titles: {titles:?}"
    );
    assert!(
        titles
            .iter()
            .any(|t| t.contains("Kurs og revisjon innen mattrygghet")),
        "titles: {titles:?}"
    );

    // `done` counts unique pages, seed included.
    let done = events
        .iter()
        .rev()
        .find(|(n, _)| n == "done")
        .map(|(_, v)| v)
        .expect("done frame");
    assert_eq!(done["status"], "completed");
    assert_eq!(
        done["pages"].as_u64().unwrap_or(0) as usize,
        last_per_page.len(),
        "done.pages must equal the unique pages streamed: {done:?}"
    );
}

/// Prints the exact SSE the wizard receives for the captured aquatiq crawl.
/// Not an assertion — a diagnostic for when the onboarding step misbehaves:
/// `cargo test --bin verevon-gateway-rs stream_e2e -- --ignored --nocapture`.
#[tokio::test]
#[ignore = "diagnostic dump, not an assertion"]
async fn dump_crawl_preview_stream() {
    for (name, payload) in run_preview().await {
        match name.as_str() {
            "snippet" => println!(
                "snippet  source={:<5} titleSource={:<5} driver={:<7} words={:<5} title={:?}
         url={} excerpt={:?}",
                payload["source"].as_str().unwrap_or("-"),
                payload["titleSource"].as_str().unwrap_or("-"),
                payload["driver"].as_str().unwrap_or("-"),
                payload["wordCount"].as_u64().unwrap_or(0),
                payload["title"].as_str().unwrap_or("-"),
                payload["url"].as_str().unwrap_or("-"),
                payload["excerpt"]
                    .as_str()
                    .map(|e| e.chars().take(80).collect::<String>())
                    .unwrap_or_default(),
            ),
            "branding" => println!(
                "branding siteName={:?} themeColor={:?} favicon={:?}",
                payload["siteName"].as_str(),
                payload["themeColor"].as_str(),
                payload["favicon"].as_str()
            ),
            other => println!("{other:<8} {payload}"),
        }
    }
}

#[tokio::test]
async fn crawl_preview_branding_carries_real_site_signals() {
    let events = run_preview().await;
    let branding = events
        .iter()
        .find(|(n, _)| n == "branding")
        .map(|(_, v)| v)
        .expect("branding frame");
    // Non-null signals from the real capture; the wizard paints its brand
    // strip from these.
    assert!(
        branding["themeColor"].is_string()
            || branding["favicon"].is_string()
            || branding["logoCandidate"].is_string()
            || branding["siteName"].is_string(),
        "branding had no usable signal: {branding:?}"
    );
}
