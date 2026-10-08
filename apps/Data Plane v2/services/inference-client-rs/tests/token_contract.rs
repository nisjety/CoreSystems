//! The token client against what Auth Core's real mint produced.
//!
//! The golden (`tests/fixtures/auth-core/inference-service-tokens.golden.json`)
//! is written by Auth Core's real controller, registry rows, organization binding
//! and tenant posture resolver, and carries the real compact RS256 JWTs and the
//! real error bodies. It is vendored byte for byte with a sha256 pin, and Auth
//! Core's spec `inference-service-token.contract.spec.ts` regenerates it and fails
//! when it drifts. A missing or altered golden FAILS here: nothing in this file
//! skips.
//!
//! The only double is the loopback endpoint that serves those bytes (see
//! `common::Stub`); every rule exercised below is the client's own. Time is a
//! manual clock set relative to the instant the golden was captured, because the
//! captured tokens expired on that day.

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{
    client, forge_claims, forge_header, Case, Golden, ManualClock, Reply, Stub, SERVICE_KEY,
};
use inference_client_rs::{
    Disposition, Failure, InferenceError, InferenceTokenClient, MintError, Reason,
    RetentionPosture, TokenClientConfig, TENANT_MAX_REUSE,
};
use serde_json::{json, Value};

fn org_reason(case: &Case) -> (String, Reason) {
    let reason = Reason::ALL
        .into_iter()
        .find(|reason| reason.as_str() == case.reason)
        .unwrap_or_else(|| {
            panic!(
                "case {} uses reason {} the client does not know",
                case.name, case.reason
            )
        });
    (case.org_id.clone(), reason)
}

async fn mint(case: &Case, posture: RetentionPosture, offset: i64) -> Result<(), MintError> {
    let golden = Golden::load();
    let stub = Stub::always(case).await;
    let clock = ManualClock::at(golden.frozen_now() + offset);
    let (org, reason) = org_reason(case);
    client(&stub.url, &golden, &case.service_id, posture, clock)
        .bearer(&org, reason)
        .await
        .map(|_| ())
}

// ───────────────────────── every captured success is accepted ─────────────────────────

#[tokio::test]
async fn every_captured_success_is_accepted_under_the_posture_it_was_issued_for() {
    let golden = Golden::load();
    let successes = golden.successes();
    assert_eq!(
        successes.len(),
        10,
        "the golden's success cases changed; extend this file"
    );
    for case in successes {
        let stub = Stub::always(&case).await;
        let clock = ManualClock::at(golden.frozen_now() + 1);
        let (org, reason) = org_reason(&case);
        let bearer = client(&stub.url, &golden, &case.service_id, case.posture(), clock)
            .bearer(&org, reason)
            .await
            .unwrap_or_else(|error| panic!("{}: {error}", case.name));

        assert_eq!(bearer.as_str(), case.token(), "{}", case.name);
        assert_eq!(
            bearer.zdr(),
            case.zdr(),
            "{}: the bearer must expose the claim the caller has to honour",
            case.name
        );
        let seen = stub.requests();
        assert_eq!(seen.len(), 1, "{}", case.name);
        assert_eq!(
            seen[0].service_id.as_deref(),
            Some(case.service_id.as_str())
        );
        assert_eq!(seen[0].api_key.as_deref(), Some(SERVICE_KEY));
        assert_eq!(
            seen[0].body,
            json!({"orgId": case.org_id, "scopes": ["inference:invoke"], "reason": case.reason}),
            "{}: the request carries the organization, the one scope and the reason, nothing else",
            case.name
        );
        assert!(
            !seen[0].has_zdr_header,
            "{}: the issuer alone selects the posture; Auth Core answers 400 to a caller-supplied x-zdr",
            case.name
        );
    }
}

#[tokio::test]
async fn a_tenant_client_accepts_either_value_of_the_claim() {
    let golden = Golden::load();
    let mut seen = std::collections::BTreeSet::new();
    for case in golden.successes() {
        mint(&case, RetentionPosture::Tenant, 1)
            .await
            .unwrap_or_else(|error| panic!("{}: {error}", case.name));
        seen.insert(case.zdr());
    }
    assert_eq!(
        seen,
        [false, true].into(),
        "the golden must hold tokens of both values for this test to mean anything"
    );
}

// ───────────────────────── the posture is asserted, in both directions ─────────────────────────

#[tokio::test]
async fn a_persistent_client_refuses_a_zdr_token_and_says_both_sides() {
    let golden = Golden::load();
    // A registry row flipped to `zdr`, and a tenant row answering for a ZDR
    // organization, both put zdr:true on a token a `persistent` client must not use.
    for name in ["embeddingEngineZdrRegistry", "embeddingEngineTenantOrgZdr"] {
        let case = golden.case(name);
        assert!(case.zdr(), "{name}");
        let error = mint(&case, RetentionPosture::Persistent, 1)
            .await
            .expect_err(name);
        assert_eq!(
            error,
            MintError::PostureMismatch {
                observed: "zdr",
                expected: "persistent"
            },
            "{name}"
        );
        let message = error.to_string();
        assert!(
            message.contains("`zdr`") && message.contains("`persistent`"),
            "{message}"
        );
        assert!(
            message.contains("MODEL_PLANE_INFERENCE_RETENTION_POSTURE"),
            "{message}"
        );
        assert_eq!(error.failure(), Failure::Auth);
    }
}

#[tokio::test]
async fn a_zdr_client_refuses_a_persistent_token() {
    let golden = Golden::load();
    let error = mint(
        &golden.case("embeddingEnginePersistent"),
        RetentionPosture::ZeroRetention,
        1,
    )
    .await
    .expect_err("zdr client");
    assert_eq!(
        error,
        MintError::PostureMismatch {
            observed: "persistent",
            expected: "zdr"
        }
    );
}

// ───────────────────────── the token is the one asked for ─────────────────────────

#[tokio::test]
async fn a_token_for_another_reason_organization_or_service_is_refused() {
    let golden = Golden::load();
    let persistent = golden.case("embeddingEnginePersistent");

    // Asked for contextual retrieval, handed the document-embedding token.
    let stub = Stub::always(&persistent).await;
    let service = client(
        &stub.url,
        &golden,
        "embedding-engine",
        RetentionPosture::Persistent,
        ManualClock::at(golden.frozen_now() + 1),
    );
    assert!(matches!(
        service
            .bearer(&persistent.org_id, Reason::ContextualRetrieval)
            .await,
        Err(MintError::Contract(_))
    ));
    // Asked for another organization.
    assert!(matches!(
        service.bearer("org-other", Reason::DocumentEmbedding).await,
        Err(MintError::Contract(_))
    ));
    // The other service's token served to this one.
    let graph = golden.case("graphIndexPersistent");
    let stub = Stub::always(&graph).await;
    let wrong_service = client(
        &stub.url,
        &golden,
        "embedding-engine",
        RetentionPosture::Persistent,
        ManualClock::at(golden.frozen_now() + 1),
    );
    assert!(matches!(
        wrong_service
            .bearer(&graph.org_id, Reason::GraphExtraction)
            .await,
        Err(MintError::Contract(_))
    ));
}

#[tokio::test]
async fn a_token_whose_issuer_is_not_the_configured_one_is_refused() {
    let golden = Golden::load();
    let case = golden.case("embeddingEnginePersistent");
    let stub = Stub::always(&case).await;
    let service = InferenceTokenClient::with_clock(
        TokenClientConfig {
            token_url: stub.url.clone(),
            expected_issuer: "http://auth-core:3011/api/convex-auth".to_owned(),
            service_id: "embedding-engine".to_owned(),
            service_api_key: SERVICE_KEY.to_owned(),
            posture: RetentionPosture::Persistent,
            allow_unconfigured: false,
        },
        ManualClock::at(golden.frozen_now() + 1),
    )
    .expect("client");
    assert!(matches!(
        service
            .bearer(&case.org_id, Reason::DocumentEmbedding)
            .await,
        Err(MintError::Contract(_))
    ));
}

type ClaimEdit = fn(&mut Value, i64);
/// An edit to a header or to the bundle around the token.
type DocumentEdit = fn(&mut Value);

/// One claim at a time edited after the mint. Each must be refused, and a refused
/// token is never cached.
fn forged_claims() -> Vec<(&'static str, ClaimEdit)> {
    vec![
        ("a widened scope", |claims, _| {
            claims["scopes"] = json!(["inference:invoke", "models:invoke"]);
        }),
        ("no scope", |claims, _| claims["scopes"] = json!([])),
        ("another scope", |claims, _| {
            claims["scopes"] = json!(["data:read"])
        }),
        ("another audience", |claims, _| {
            claims["aud"] = json!("model-gateway")
        }),
        ("another issuer", |claims, _| {
            claims["iss"] = json!("https://evil.test")
        }),
        ("a user principal", |claims, _| {
            claims["principal_type"] = json!("user")
        }),
        ("another subject", |claims, _| {
            claims["sub"] = json!("service:graph-index")
        }),
        ("another service id", |claims, _| {
            claims["service_id"] = json!("service:graph-index");
        }),
        ("another organization", |claims, _| {
            claims["org_id"] = json!("org-other")
        }),
        ("another reason", |claims, _| {
            claims["reason"] = json!("anything")
        }),
        ("no zdr claim", |claims, _| {
            claims.as_object_mut().expect("object").remove("zdr");
        }),
        ("no expiry", |claims, _| {
            claims.as_object_mut().expect("object").remove("exp");
        }),
        ("an expired token", |claims, now| {
            claims["exp"] = json!(now - 10)
        }),
        ("a lifetime over five minutes", |claims, now| {
            claims["exp"] = json!(now + 1000);
        }),
        ("not valid yet", |claims, now| {
            claims["nbf"] = json!(now + 3600);
        }),
        ("issued in the future", |claims, now| {
            claims["iat"] = json!(now + 3600);
        }),
    ]
}

#[tokio::test]
async fn a_forged_claim_is_refused_and_not_cached() {
    let golden = Golden::load();
    let case = golden.case("embeddingEnginePersistent");
    let now = golden.frozen_now() + 1;
    for (label, edit) in forged_claims() {
        let mut body = case.body.clone();
        body["token"] = json!(forge_claims(case.token(), |claims| edit(claims, now)));
        let reply = Reply::json(201, &body);
        let stub = Stub::start(move |_| reply.clone()).await;
        let service = client(
            &stub.url,
            &golden,
            "embedding-engine",
            RetentionPosture::Persistent,
            ManualClock::at(now),
        );
        for attempt in 1..=2 {
            let error = service
                .bearer(&case.org_id, Reason::DocumentEmbedding)
                .await
                .expect_err(label);
            assert!(matches!(error, MintError::Contract(_)), "{label}: {error}");
            assert_eq!(
                stub.request_count(),
                attempt,
                "{label}: a refused token must not be cached"
            );
        }
    }
}

#[tokio::test]
async fn a_forged_header_is_refused() {
    let golden = Golden::load();
    let case = golden.case("embeddingEnginePersistent");
    let now = golden.frozen_now() + 1;
    let edits: [(&str, DocumentEdit); 3] = [
        ("an HS256 header", |header| header["alg"] = json!("HS256")),
        ("an unsigned header", |header| header["alg"] = json!("none")),
        ("no typ", |header| {
            header.as_object_mut().expect("object").remove("typ");
        }),
    ];
    for (label, edit) in edits {
        let mut body = case.body.clone();
        body["token"] = json!(forge_header(case.token(), edit));
        let stub = Stub::always(&Case {
            body,
            ..case.clone()
        })
        .await;
        let service = client(
            &stub.url,
            &golden,
            "embedding-engine",
            RetentionPosture::Persistent,
            ManualClock::at(now),
        );
        assert!(
            matches!(
                service
                    .bearer(&case.org_id, Reason::DocumentEmbedding)
                    .await,
                Err(MintError::Contract(_))
            ),
            "{label}"
        );
    }
}

#[tokio::test]
async fn a_bundle_that_breaks_the_bounded_token_contract_is_refused() {
    let golden = Golden::load();
    let case = golden.case("embeddingEnginePersistent");
    let now = golden.frozen_now() + 1;
    let edits: [(&str, DocumentEdit); 10] = [
        ("zero lifetime", |body| body["expiresInSeconds"] = json!(0)),
        ("a negative lifetime", |body| {
            body["expiresInSeconds"] = json!(-5)
        }),
        ("a lifetime over five minutes", |body| {
            body["expiresInSeconds"] = json!(301)
        }),
        ("another audience", |body| {
            body["audience"] = json!("model-gateway")
        }),
        ("another issuer", |body| {
            body["issuer"] = json!("https://evil.test")
        }),
        ("an empty token", |body| body["token"] = json!("")),
        ("a token with whitespace", |body| {
            let token = body["token"]
                .as_str()
                .expect("token")
                .replacen('.', ". ", 1);
            body["token"] = json!(token);
        }),
        ("an oversized token", |body| {
            body["token"] = json!(format!("{}.{}", "a".repeat(9000), "b"));
        }),
        ("a malformed expiry", |body| {
            body["expiresAt"] = json!("tomorrow")
        }),
        ("an expiry that disagrees with the claim", |body| {
            body["expiresAt"] = json!("2026-10-06T12:10:00.000Z");
        }),
    ];
    for (label, edit) in edits {
        let mut body = case.body.clone();
        edit(&mut body);
        let stub = Stub::always(&Case {
            body,
            ..case.clone()
        })
        .await;
        let service = client(
            &stub.url,
            &golden,
            "embedding-engine",
            RetentionPosture::Persistent,
            ManualClock::at(now),
        );
        assert!(
            matches!(
                service
                    .bearer(&case.org_id, Reason::DocumentEmbedding)
                    .await,
                Err(MintError::Contract(_))
            ),
            "{label}"
        );
    }
}

#[tokio::test]
async fn fields_the_control_plane_adds_later_do_not_break_the_mint() {
    // The first copies of this client rejected unknown response fields: one new
    // field in Auth Core's answer would have failed every mint in every worker.
    let golden = Golden::load();
    let case = golden.case("embeddingEnginePersistent");
    let mut body = case.body.clone();
    body["tokenType"] = json!("Bearer");
    body["policyVersion"] = json!({"nested": [1, 2, 3]});
    let stub = Stub::always(&Case {
        body,
        ..case.clone()
    })
    .await;
    let service = client(
        &stub.url,
        &golden,
        "embedding-engine",
        RetentionPosture::Persistent,
        ManualClock::at(golden.frozen_now() + 1),
    );
    assert!(service
        .bearer(&case.org_id, Reason::DocumentEmbedding)
        .await
        .is_ok());
}

// ───────────────────────── lifetimes and the cache ─────────────────────────

/// Serves the same captured exchange and moves the clock, to find the instant a
/// bearer stops being served from memory.
async fn first_refresh_offset(case: &Case, posture: RetentionPosture, minted_at: i64) -> i64 {
    let golden = Golden::load();
    let stub = Stub::always(case).await;
    let clock = ManualClock::at(golden.frozen_now() + minted_at);
    let service = client(&stub.url, &golden, &case.service_id, posture, clock.clone());
    let (org, reason) = org_reason(case);
    service.bearer(&org, reason).await.expect("first mint");
    for offset in minted_at..=case.exp() - golden.frozen_now() {
        clock.set(golden.frozen_now() + offset);
        let before = stub.request_count();
        // A refresh re-mints, and the stand-in serves the same captured token; at
        // or past its expiry the client refuses it, which is also "not served
        // from the cache".
        let _ = service.bearer(&org, reason).await;
        if stub.request_count() > before {
            return offset;
        }
    }
    panic!("{} was served from the cache until it expired", case.name);
}

#[tokio::test]
async fn a_sixty_second_tenant_token_is_reused_for_at_most_45_seconds() {
    let golden = Golden::load();
    let case = golden.case("embeddingEngineTenantOrgPlain");
    assert_eq!(
        case.exp() - case.iat(),
        60,
        "tenant tokens are capped at 60 s"
    );
    // Minted one second after it was issued: the 15 s margin (a quarter of the
    // lifetime) puts the end of reuse at issue + 45, and the 45 s cap would put it
    // at mint + 45 = issue + 46. The earlier one wins.
    assert_eq!(
        first_refresh_offset(&case, RetentionPosture::Tenant, 1).await,
        45
    );
    // A bearer was never refreshed on every call, which a fixed 60 s margin does.
    let stub = Stub::always(&case).await;
    let service = client(
        &stub.url,
        &golden,
        &case.service_id,
        RetentionPosture::Tenant,
        ManualClock::at(golden.frozen_now() + 1),
    );
    let (org, reason) = org_reason(&case);
    for _ in 0..50 {
        service.bearer(&org, reason).await.expect("bearer");
    }
    assert_eq!(stub.request_count(), 1);
    let stats = service.stats();
    assert_eq!((stats.mints, stats.misses, stats.hits), (1, 1, 49));
}

#[tokio::test]
async fn a_five_minute_token_is_reused_until_a_minute_before_it_expires() {
    let golden = Golden::load();
    let case = golden.case("embeddingEnginePersistent");
    assert_eq!(case.exp() - case.iat(), 300);
    assert_eq!(
        first_refresh_offset(&case, RetentionPosture::Persistent, 1).await,
        240
    );
}

#[tokio::test]
async fn a_tenant_bearer_is_never_reused_past_the_staleness_bound_whatever_its_lifetime() {
    // Auth Core caps tenant tokens at 60 s today. If that cap were ever raised the
    // `zdr` snapshot on the token would outlive the figure the staleness is
    // disclosed with, so the client bounds reuse itself.
    let golden = Golden::load();
    let long_lived = golden.case("embeddingEnginePersistent");
    assert_eq!(long_lived.exp() - long_lived.iat(), 300);
    assert_eq!(
        first_refresh_offset(&long_lived, RetentionPosture::Tenant, 1).await,
        1 + i64::try_from(TENANT_MAX_REUSE.as_secs()).expect("seconds")
    );
}

#[tokio::test]
async fn the_cache_is_keyed_by_organization_and_reason() {
    let golden = Golden::load();
    let persistent: Vec<Case> = [
        "embeddingEnginePersistent",
        "embeddingEngineContextualRetrievalPersistent",
        "embeddingEngineVideoCaptionPersistent",
    ]
    .into_iter()
    .map(|name| golden.case(name))
    .collect();
    let stub = Stub::replaying(persistent.clone()).await;
    let service = client(
        &stub.url,
        &golden,
        "embedding-engine",
        RetentionPosture::Persistent,
        ManualClock::at(golden.frozen_now() + 1),
    );
    let org = persistent[0].org_id.clone();

    // Three reasons for one organization: three tokens, each carrying its own reason.
    for case in &persistent {
        let (_, reason) = org_reason(case);
        let bearer = service.bearer(&org, reason).await.expect("bearer");
        assert_eq!(bearer.as_str(), case.token(), "{}", case.name);
    }
    assert_eq!(stub.request_count(), 3);
    // Asking again, in any order, is served from memory and returns the same one.
    for case in persistent.iter().rev() {
        let (_, reason) = org_reason(case);
        let bearer = service.bearer(&org, reason).await.expect("bearer");
        assert_eq!(bearer.as_str(), case.token(), "{}", case.name);
    }
    assert_eq!(stub.request_count(), 3);

    // Two organizations under the tenant posture, same reason: two tokens, each
    // with its own organization's posture. One org's bearer is never served for
    // another.
    let tenant: Vec<Case> = [
        "embeddingEngineTenantOrgPlain",
        "embeddingEngineTenantOrgZdr",
    ]
    .into_iter()
    .map(|name| golden.case(name))
    .collect();
    let stub = Stub::replaying(tenant.clone()).await;
    let service = client(
        &stub.url,
        &golden,
        "embedding-engine",
        RetentionPosture::Tenant,
        ManualClock::at(golden.frozen_now() + 1),
    );
    for case in tenant.iter().chain(tenant.iter()) {
        let (org, reason) = org_reason(case);
        let bearer = service.bearer(&org, reason).await.expect("bearer");
        assert_eq!(bearer.as_str(), case.token(), "{}", case.name);
        assert_eq!(bearer.zdr(), case.zdr(), "{}", case.name);
    }
    assert_eq!(stub.request_count(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_callers_share_one_mint() {
    let golden = Golden::load();
    let case = golden.case("embeddingEngineTenantOrgPlain");
    let stub = Stub::always(&case).await;
    // Long enough that every caller arrives while the mint is still in flight.
    stub.delay(Duration::from_millis(400));
    let service = Arc::new(client(
        &stub.url,
        &golden,
        &case.service_id,
        RetentionPosture::Tenant,
        ManualClock::at(golden.frozen_now() + 1),
    ));
    let (org, reason) = org_reason(&case);

    let calls: Vec<_> = (0..16)
        .map(|_| {
            let service = service.clone();
            let org = org.clone();
            tokio::spawn(async move { service.bearer(&org, reason).await })
        })
        .collect();
    for call in calls {
        let bearer = call.await.expect("task").expect("bearer");
        assert_eq!(bearer.as_str(), case.token());
    }
    assert_eq!(
        stub.request_count(),
        1,
        "sixteen chunks, one request to Auth Core"
    );
    let stats = service.stats();
    assert_eq!(stats.mints, 1);
    assert_eq!(stats.misses, 1);
    assert_eq!(stats.joined + stats.hits, 15);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_callers_share_a_failure_and_the_next_call_tries_again() {
    let golden = Golden::load();
    let down = golden.case("tenantPostureUnavailable");
    let ok = golden.case("embeddingEngineTenantOrgPlain");
    let stub = Stub::always(&down).await;
    stub.delay(Duration::from_millis(300));
    let service = Arc::new(client(
        &stub.url,
        &golden,
        "embedding-engine",
        RetentionPosture::Tenant,
        ManualClock::at(golden.frozen_now() + 1),
    ));
    let (org, reason) = org_reason(&ok);

    let calls: Vec<_> = (0..8)
        .map(|_| {
            let service = service.clone();
            let org = org.clone();
            tokio::spawn(async move { service.bearer(&org, reason).await })
        })
        .collect();
    for call in calls {
        let error = call
            .await
            .expect("task")
            .expect_err("no token while posture is unknown");
        assert_eq!(error, MintError::PostureUnavailable);
    }
    assert_eq!(stub.request_count(), 1, "eight callers, one failed request");

    // A failure poisons nothing: once Auth Core answers again the next call mints.
    stub.delay(Duration::ZERO);
    let reply = Reply::of(&ok);
    stub.respond_with(move |_| reply.clone());
    let bearer = service.bearer(&org, reason).await.expect("recovered");
    assert_eq!(bearer.as_str(), ok.token());
    assert_eq!(stub.request_count(), 2);
    assert_eq!(service.stats().failures, 1);
}

// ───────────────────────── typed refusals, from the real error bodies ─────────────────────────

#[tokio::test]
async fn an_unresolvable_posture_is_a_wait_never_a_token_and_never_a_skip() {
    let golden = Golden::load();
    for (name, expected, kind) in [
        (
            "tenantPostureUnavailable",
            MintError::PostureUnavailable,
            "retention_posture_unavailable",
        ),
        (
            "tenantDirectoryUnavailable",
            MintError::DirectoryUnavailable,
            "organization_directory_unavailable",
        ),
    ] {
        let case = golden.case(name);
        assert_eq!(case.status, 503, "{name}");
        let stub = Stub::always(&case).await;
        let service = client(
            &stub.url,
            &golden,
            &case.service_id,
            RetentionPosture::Tenant,
            ManualClock::at(golden.frozen_now() + 1),
        );
        let (org, reason) = org_reason(&case);
        for attempt in 1..=2 {
            let error = service.bearer(&org, reason).await.expect_err(name);
            assert_eq!(error, expected, "{name}");
            assert_eq!(
                stub.request_count(),
                attempt,
                "{name}: a refusal is not cached"
            );
        }
        let typed = InferenceError::from(service.bearer(&org, reason).await.expect_err(name));
        assert_eq!(typed.wire_kind(), Some(kind), "{name}");
        assert_eq!(
            typed.disposition(),
            Disposition::Pending {
                after: Duration::from_secs(300)
            },
            "{name}: pending, retried in five minutes"
        );
        assert!(typed.to_string().contains(kind), "{typed}");
    }
}

#[tokio::test]
async fn an_organization_or_credential_auth_core_refuses_is_a_configuration_failure() {
    let golden = Golden::load();
    for name in ["organizationNotInDirectory", "wrongCredential"] {
        let case = golden.case(name);
        assert_eq!(case.status, 403, "{name}");
        let stub = Stub::always(&case).await;
        let service = client(
            &stub.url,
            &golden,
            &case.service_id,
            RetentionPosture::Tenant,
            ManualClock::at(golden.frozen_now() + 1),
        );
        let (org, reason) = org_reason(&case);
        let error = service.bearer(&org, reason).await.expect_err(name);
        assert_eq!(error, MintError::Refused { status: 403 }, "{name}");
        let typed = InferenceError::from(error);
        assert_eq!(typed.failure(), &Failure::Auth, "{name}");
        assert_eq!(typed.disposition(), Disposition::Terminal, "{name}");
    }
}

#[tokio::test]
async fn an_unreachable_auth_core_is_transient_and_not_cached() {
    let golden = Golden::load();
    // A port nothing listens on: bind, note it, release it.
    let closed = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        listener.local_addr().expect("address").port()
    };
    let service = client(
        &format!("http://127.0.0.1:{closed}/api/inference-core/internal-token"),
        &golden,
        "embedding-engine",
        RetentionPosture::Persistent,
        ManualClock::at(golden.frozen_now() + 1),
    );
    for _ in 0..2 {
        let error = service
            .bearer("org-contract-plain", Reason::DocumentEmbedding)
            .await
            .expect_err("nothing is listening");
        assert!(matches!(error, MintError::Unavailable(_)), "{error}");
        assert_eq!(error.failure(), Failure::Transient { retry_after: None });
    }
    assert_eq!(service.stats().mints, 2);
}

#[tokio::test]
async fn a_response_over_the_size_limit_is_refused() {
    let golden = Golden::load();
    let stub = Stub::start(|_| Reply {
        status: 201,
        body: vec![b' '; 70 * 1024],
    })
    .await;
    let service = client(
        &stub.url,
        &golden,
        "embedding-engine",
        RetentionPosture::Persistent,
        ManualClock::at(golden.frozen_now() + 1),
    );
    assert!(matches!(
        service
            .bearer("org-contract-plain", Reason::DocumentEmbedding)
            .await,
        Err(MintError::Malformed(_))
    ));
}

// ───────────────────────── configuration and standalone startup ─────────────────────────

#[tokio::test]
async fn standalone_startup_binds_without_a_credential_and_never_calls_auth_core() {
    let golden = Golden::load();
    let stub = Stub::always(&golden.case("embeddingEnginePersistent")).await;
    let service = InferenceTokenClient::with_clock(
        TokenClientConfig {
            token_url: stub.url.clone(),
            expected_issuer: golden.issuer(),
            service_id: "embedding-engine".to_owned(),
            service_api_key: String::new(),
            posture: RetentionPosture::Persistent,
            allow_unconfigured: true,
        },
        ManualClock::at(golden.frozen_now() + 1),
    )
    .expect("standalone startup may bind without the external principal");
    let error = service
        .bearer("org-contract-plain", Reason::DocumentEmbedding)
        .await
        .expect_err("a content request still fails closed");
    assert_eq!(error, MintError::CredentialUnavailable);
    assert_eq!(
        stub.request_count(),
        0,
        "no unauthenticated call is ever made"
    );
}

#[tokio::test]
async fn an_invalid_organization_never_reaches_auth_core() {
    let golden = Golden::load();
    let stub = Stub::always(&golden.case("embeddingEnginePersistent")).await;
    let service = client(
        &stub.url,
        &golden,
        "embedding-engine",
        RetentionPosture::Persistent,
        ManualClock::at(golden.frozen_now() + 1),
    );
    for bad in ["", "bad org", "org/1", "../org"] {
        assert_eq!(
            service
                .bearer(bad, Reason::DocumentEmbedding)
                .await
                .expect_err(bad),
            MintError::InvalidOrg,
            "{bad:?}"
        );
    }
    assert_eq!(stub.request_count(), 0);
}

#[test]
fn the_reasons_are_the_ones_auth_cores_golden_was_captured_with() {
    let golden = Golden::load();
    let mut ours: Vec<String> = Reason::ALL
        .into_iter()
        .map(|reason| reason.as_str().to_owned())
        .collect();
    let mut theirs = golden.reasons();
    ours.sort();
    theirs.sort();
    assert_eq!(
        ours, theirs,
        "Reason::ALL and the golden's `reasons` must be one list; regenerate the golden from Auth Core"
    );
    // Every one of them was actually minted by the real controller.
    for reason in Reason::ALL {
        assert!(
            golden
                .successes()
                .iter()
                .any(|case| case.reason == reason.as_str()),
            "no captured token for {reason}"
        );
    }
}
