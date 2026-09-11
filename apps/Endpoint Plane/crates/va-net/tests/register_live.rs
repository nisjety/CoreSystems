//! Live integration tests against a REAL local Support Plane (hbbs) — not a
//! fake or mocked peer. Ignored by default since they need real network
//! infrastructure; run explicitly after `docker compose up -d hbbs hbbr
//! readiness` in `apps/Support Plane` (see its README):
//!
//! ```bash
//! cargo test -p va-net --test register_live -- --ignored --nocapture
//! ```
//!
//! These exist to close two gaps `docs/architecture.md` called out:
//! - M0: `remote-core`'s own research only captured a single ad hoc
//!   `RegisterPk` poke, not a sustained registration proof. The first two
//!   tests prove `RegisterPk` is accepted, stays accepted across the
//!   server's abuse-rate-limit window, and is correctly rejected when a
//!   different identity tries to claim an already-registered id.
//! - A live-discovered gap on top of that: `RegisterPk` alone does NOT keep
//!   a peer online for punch-hole routing — only `RegisterPeer` refreshes
//!   hbbs's `last_reg_time`. The remaining tests exercise
//!   [`RegistrationClient::register_peer_once`] and
//!   [`RegistrationClient::ensure_online_once`] directly against a real
//!   hbbs, proving `request_pk` starts `true` for a brand-new id and flips
//!   to `false` once a `pk` is on file.

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

    client.register_pk_once().await.expect("first RegisterPk should be accepted");
    client
        .register_pk_once()
        .await
        .expect("a second immediate RegisterPk (within the 2-per-6s allowance) should also be accepted");

    // hbbs's per-peer registration-frequency window resets after 6s of no
    // attempts; waiting past that and registering again is the "sustained
    // registration" proof this crate didn't have before — not just a single
    // ad hoc poke.
    tokio::time::sleep(Duration::from_secs(7)).await;
    client
        .register_pk_once()
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
    client_a.register_pk_once().await.expect("first registration establishes the id");

    let second_pk = IdentityKeyPair::generate().public_key().to_vec();
    let client_b = RegistrationClient::connect(hbbs_addr(), id, second_pk.clone(), second_pk)
        .await
        .expect("bind/connect to local hbbs");
    let err = client_b
        .register_pk_once()
        .await
        .expect_err("a different identity claiming the same id should be rejected");
    assert!(
        matches!(err, NetError::Rejected(RegisterPkResult::UuidMismatch)),
        "expected UuidMismatch, got {err:?}"
    );
}

#[tokio::test]
#[ignore = "requires a real local hbbs (apps/Support Plane) reachable at 127.0.0.1:21116"]
async fn register_peer_requests_a_fresh_pk_for_a_brand_new_id_then_stops_asking() {
    let pk = IdentityKeyPair::generate().public_key().to_vec();
    let client = RegistrationClient::connect(hbbs_addr(), random_device_id(), pk.clone(), pk)
        .await
        .expect("bind/connect to local hbbs");

    let request_pk = client
        .register_peer_once()
        .await
        .expect("RegisterPeer for a brand-new id should be accepted");
    assert!(
        request_pk,
        "hbbs has no pk on file for a brand-new id -- it must ask for one, this is the live-discovered mechanism this fix relies on"
    );

    client.register_pk_once().await.expect("RegisterPk should be accepted");

    let request_pk_after = client
        .register_peer_once()
        .await
        .expect("RegisterPeer should still be accepted once a pk is on file");
    assert!(
        !request_pk_after,
        "hbbs now has a pk on file and our source address hasn't changed -- it must stop asking, and (per source) this exact call is what refreshes last_reg_time"
    );
}

#[tokio::test]
#[ignore = "requires a real local hbbs (apps/Support Plane) reachable at 127.0.0.1:21116"]
async fn ensure_online_once_completes_the_full_dance_from_a_fresh_identity() {
    let pk = IdentityKeyPair::generate().public_key().to_vec();
    let client = RegistrationClient::connect(hbbs_addr(), random_device_id(), pk.clone(), pk)
        .await
        .expect("bind/connect to local hbbs");

    // First call must internally: RegisterPeer (request_pk=true) ->
    // RegisterPk (accepted) -> RegisterPeer again (request_pk=false).
    client.ensure_online_once().await.expect("first ensure_online_once should complete the full dance");

    // A second call right away should be the cheap steady-state path: just
    // one RegisterPeer, no RegisterPk needed.
    client
        .ensure_online_once()
        .await
        .expect("steady-state ensure_online_once should succeed without needing a fresh RegisterPk");
}
