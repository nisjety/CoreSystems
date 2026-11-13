//! EP-6 S9 run metadata [W3-EP-6-S9-runmeta]: the agent version pin is validated for every caller,
//! written into `runs.metadata.agent` for a persistent run, and never kept for a ZDR run. A
//! test-only child module of `terminalization`. The DB tests are `#[ignore]`d and need
//! `DATABASE_URL` to a scratch Postgres whose name contains `test`.

use super::*;

const DIGEST: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn pin(agent_ref: &str, version: u32, digest: &str) -> pb::AgentVersionPin {
    pb::AgentVersionPin {
        agent_ref: agent_ref.to_owned(),
        version,
        digest: digest.to_owned(),
    }
}

#[test]
fn an_agent_version_pin_is_ids_and_a_digest_only() {
    assert!(validate_agent_version_pin(&pin("agent:research.v1_x-y", 1, DIGEST)).is_ok());
    assert!(validate_agent_version_pin(&pin(&"a".repeat(128), 7, DIGEST)).is_ok());
    let upper = DIGEST.replace('a', "A");
    for (bad, why) in [
        (pin("", 1, DIGEST), "empty ref"),
        (pin(&"a".repeat(129), 1, DIGEST), "ref too long"),
        (pin("agent/1", 1, DIGEST), "slash in ref"),
        (pin("agent 1", 1, DIGEST), "space in ref"),
        (pin("agent-1", 0, DIGEST), "version 0"),
        (pin("agent-1", 1, &DIGEST[7..]), "no sha256: prefix"),
        (pin("agent-1", 1, &DIGEST[..DIGEST.len() - 1]), "63 hex"),
        (pin("agent-1", 1, &upper), "uppercase hex"),
        (pin("agent-1", 1, ""), "empty digest"),
    ] {
        let refused = validate_agent_version_pin(&bad).expect_err(why);
        assert_eq!(refused.code(), tonic::Code::InvalidArgument, "{why}");
        assert_eq!(refused.message(), "invalid_agent_version_pin", "{why}");
    }
}

fn start_request(
    thread_id: &str,
    agent_pin: Option<pb::AgentVersionPin>,
) -> pb::StartManagedRunRequest {
    pb::StartManagedRunRequest {
        wake: None,
        thread_id: thread_id.to_owned(),
        parent_run_id: String::new(),
        agent_id: "general-v1".to_owned(),
        goal: "goal".to_owned(),
        mode: "execute".to_owned(),
        org_id: "org-1".to_owned(),
        user_id: "u1".to_owned(),
        start_key: format!("ap-{}", new_ulid()),
        terminal_source: pb::ManagedRunSource::ExecutionAgent as i32,
        agent_version_pin: agent_pin,
    }
}

#[test]
fn a_zdr_start_keeps_no_pin_and_a_persistent_one_keeps_it() {
    let request = start_request("", Some(pin("agent-1", 2, DIGEST)));
    assert!(managed_start_persistence(&request, true)
        .agent_version_pin
        .is_none());
    assert_eq!(
        managed_start_persistence(&request, false).agent_version_pin,
        request.agent_version_pin
    );
}

#[test]
fn a_malformed_pin_is_refused_for_a_zdr_caller_too() {
    let request = start_request("", Some(pin("agent-1", 0, DIGEST)));
    for zdr in [true, false] {
        assert_eq!(
            validate_managed_start_request(&request, zdr)
                .unwrap_err()
                .message(),
            "invalid_agent_version_pin",
            "zdr={zdr}"
        );
    }
}

async fn scratch_pool() -> Option<PgPool> {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("skipping: DATABASE_URL unset");
        return None;
    };
    let database = url
        .rsplit('/')
        .next()
        .and_then(|tail| tail.split('?').next())
        .unwrap_or_default();
    assert!(
        database.contains("test"),
        "DATABASE_URL must name a scratch database"
    );
    let pool = PgPool::connect(&url).await.expect("scratch Postgres");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("migrate");
    Some(pool)
}

async fn agent_metadata(pool: &PgPool, run_id: &str) -> Option<serde_json::Value> {
    sqlx::query_scalar("SELECT metadata -> 'agent' FROM runs WHERE id = $1")
        .bind(run_id)
        .fetch_one(pool)
        .await
        .expect("the run")
}

#[tokio::test]
#[ignore = "requires DATABASE_URL to a scratch Postgres whose name contains \"test\""]
async fn metadata_agent_is_written_for_a_persistent_run_and_absent_for_zdr() {
    let Some(pool) = scratch_pool().await else {
        return;
    };
    let sfx = new_ulid();
    let (org, thread) = (format!("ap-org-{sfx}"), format!("ap-t-{sfx}"));
    sqlx::query("INSERT INTO threads (id, session_key, org_id, user_id) VALUES ($1, $1, $2, 'u1')")
        .bind(&thread)
        .bind(&org)
        .execute(&pool)
        .await
        .expect("thread");
    let mut request = start_request(&thread, Some(pin("agent-1", 3, DIGEST)));
    request.org_id = org.clone();
    let persistent =
        start_managed_run_with_retention_inner(&pool, request, RunRetention::persistent())
            .await
            .expect("persistent start");
    assert_eq!(
        agent_metadata(&pool, &persistent.run_id).await,
        Some(serde_json::json!({"ref": "agent-1", "version": 3, "digest": DIGEST}))
    );

    let mut zdr_request = start_request("", Some(pin("agent-1", 3, DIGEST)));
    zdr_request.org_id = org.clone();
    let zdr = start_managed_run_with_retention_inner(&pool, zdr_request, RunRetention::zdr())
        .await
        .expect("zdr start");
    assert_eq!(
        agent_metadata(&pool, &zdr.run_id).await,
        None,
        "a ZDR run keeps no pin"
    );

    let mut unpinned = start_request(&thread, None);
    unpinned.org_id = org;
    let plain = start_managed_run_with_retention_inner(&pool, unpinned, RunRetention::persistent())
        .await
        .expect("unpinned start");
    assert_eq!(
        agent_metadata(&pool, &plain.run_id).await,
        None,
        "no pin, no key"
    );
}
