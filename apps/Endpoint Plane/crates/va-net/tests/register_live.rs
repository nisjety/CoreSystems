//! Live integration tests against a REAL local Support Plane (hbbs) — not a
//! fake or mocked peer. Ignored by default since they need real network
//! infrastructure; run explicitly after `docker compose up -d hbbs hbbr
//! readiness` in `apps/Support Plane` (see its README):
//!
//! ```bash
//! cargo test -p va-net --test register_live -- --ignored --nocapture
//! ```
//!
//! These exist to close the M0 gap `docs/architecture.md` called out:
//! `remote-core`'s own research only captured a single ad hoc `RegisterPk`
//! poke, not a sustained registration proof. These prove `RegisterPk` is
//! accepted, stays accepted across the server's abuse-rate-limit window, and
//! is correctly rejected when a different identity tries to claim an
//! already-registered id.

use std::net::SocketAddr;
use std::time::Duration;

use va_crypto::IdentityKeyPair;
use va_net::{NetError, RegistrationClient};
use va_protocol::RegisterPkResult;

fn hbbs_addr() -> SocketAddr {
    "127.0.0.1:21116".parse().expect("hardcoded literal address")
}

/// A fresh 9-digit numeric id per test run, matching the real client's id
/// shape and avoiding any collision with a previous run's registration
/// state on the shared local hbbs.
fn random_device_id() -> String {
    use rand::Rng;
    rand::rng().random_range(100_000_000u32..1_000_000_000u32).to_string()
}

#[tokio::test]
#[ignore = "requires a real local hbbs (apps/Support Plane) reachable at 127.0.0.1:21116"]
async fn register_pk_is_accepted_and_stays_registered_across_a_rate_limit_window() {
    let identity = IdentityKeyPair::generate();
    let pk = identity.public_key().to_vec();
    // Real client fallback when no OS machine-UID is available: reuse the
    // identity's own public-key bytes as the uuid.
    let client = RegistrationClient::connect(hbbs_addr(), random_device_id(), pk.clone(), pk)
        .await
        .expect("bind/connect to local hbbs");

    client.register_once().await.expect("first RegisterPk should be accepted");
    client
        .register_once()
        .await
        .expect("a second immediate RegisterPk (within the 2-per-6s allowance) should also be accepted");

    // hbbs's per-peer registration-frequency window resets after 6s of no
    // attempts; waiting past that and registering again is the "sustained
    // registration" proof this crate didn't have before — not just a single
    // ad hoc poke.
    tokio::time::sleep(Duration::from_secs(7)).await;
    client
        .register_once()
        .await
        .expect("re-registering after the rate-limit window resets should still be accepted");
}

#[tokio::test]
#[ignore = "requires a real local hbbs (apps/Support Plane) reachable at 127.0.0.1:21116"]
async fn registering_a_different_identity_under_the_same_id_is_rejected() {
    let id = random_device_id();

    let first_pk = IdentityKeyPair::generate().public_key().to_vec();
    let client_a = RegistrationClient::connect(hbbs_addr(), id.clone(), first_pk.clone(), first_pk)
        .await
        .expect("bind/connect to local hbbs");
    client_a.register_once().await.expect("first registration establishes the id");

    let second_pk = IdentityKeyPair::generate().public_key().to_vec();
    let client_b = RegistrationClient::connect(hbbs_addr(), id, second_pk.clone(), second_pk)
        .await
        .expect("bind/connect to local hbbs");
    let err = client_b
        .register_once()
        .await
        .expect_err("a different identity claiming the same id should be rejected");
    assert!(
        matches!(err, NetError::Rejected(RegisterPkResult::UuidMismatch)),
        "expected UuidMismatch, got {err:?}"
    );
}
