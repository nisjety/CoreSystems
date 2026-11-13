//! Owed GDPR tests (verification plan 3.2), a test-only child module of `gdpr`.
//!
//! * Pure tier: the OD-59 switch parser takes only `true` [R2-OD-59-a1]; the person's standing
//!   approval authority is erased by two statements bound to org and user, grants before proposals,
//!   inside the erasing transaction [W2-WS13-S7-user-erasure]; the org purge deletes proposals
//!   right after grants and before approvals [W2-WS13-S7-proposal-store-a].
//! * DB tier (every test `#[ignore]`d, needs `DATABASE_URL` to a disposable Postgres whose name
//!   contains `test`) [R2-OD-59-a2]: with the threads the hold check cleared, the person's own
//!   personal-thread rows are deleted and recorded as forgotten, another member's own thread rows are
//!   untouched, a Space-thread row the person alone stated is deleted and a shared one narrowed, and
//!   the person's grants and proposals go while another person's stay. The hold refusal of the
//!   thread check needs the process switch, which is read once per process; it is not driven here.

use super::*;

#[test]
fn the_thread_memory_switch_takes_only_true() {
    assert!(thread_memory_erasure_switch(Some("true")));
    assert!(
        thread_memory_erasure_switch(Some(" true ")),
        "surrounding whitespace is trimmed"
    );
    for off in [
        None,
        Some(""),
        Some("1"),
        Some("yes"),
        Some("on"),
        Some("TRUE"),
        Some("True"),
        Some("false"),
    ] {
        assert!(!thread_memory_erasure_switch(off), "{off:?}");
    }
    assert_eq!(
        THREAD_MEMORY_ERASURE_ENV,
        "SESSION_CORE_GDPR_THREAD_MEMORY_ERASURE"
    );
}

/// The text of the function that starts at `signature`, up to the next top-level item.
fn body_of<'a>(source: &'a str, signature: &str) -> &'a str {
    let start = source
        .find(signature)
        .expect("the function is in the source");
    let rest = &source[start..];
    let end = rest[1..].find("\npub ").map_or(rest.len(), |at| at + 1);
    &rest[..end]
}

fn squash(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn a_person_erasure_removes_their_grants_then_their_proposals_bound_to_org_and_user() {
    let source = include_str!("gdpr.rs");
    let body = squash(body_of(
        source,
        "pub async fn erase_user_personal_data_for(",
    ));
    let grants = "sqlx::query(\"DELETE FROM approval_scope_grants WHERE org_id = $1 AND user_id = $2\") .bind(org_id) .bind(user_id)";
    let proposals = "sqlx::query(\"DELETE FROM approval_rule_proposals WHERE org_id = $1 AND user_id = $2\") .bind(org_id) .bind(user_id)";
    let at_grants = body
        .find(grants)
        .expect("the grant statement, bound to org and user");
    let at_proposals = body
        .find(proposals)
        .expect("the proposal statement, bound to org and user");
    assert!(
        at_grants < at_proposals,
        "grants are erased before proposals"
    );
    let at_commit = body
        .find("tx.commit()")
        .expect("the erasing transaction commits");
    let at_audit = body
        .find("INSERT INTO memory_control_events")
        .expect("the audit row");
    assert!(
        at_proposals < at_audit && at_audit < at_commit,
        "inside the erasing transaction"
    );
    assert_eq!(body.matches("DELETE FROM approval_scope_grants").count(), 1);
    assert_eq!(
        body.matches("DELETE FROM approval_rule_proposals").count(),
        1
    );
}

#[test]
fn the_org_purge_deletes_proposals_after_grants_and_before_approvals() {
    let source = include_str!("gdpr.rs");
    let body = body_of(source, "pub async fn purge_organization_data(");
    let grants = body
        .find("DELETE FROM approval_scope_grants WHERE org_id = $1")
        .expect("grants");
    let proposals = body
        .find("DELETE FROM approval_rule_proposals WHERE org_id = $1")
        .expect("proposals");
    let approvals = body
        .find("DELETE FROM approvals WHERE org_id = $1")
        .expect("approvals");
    assert!(grants < proposals && proposals < approvals);
}

#[test]
fn the_user_erasure_summary_counts_the_standing_authority() {
    let summary = UserErasureSummary {
        approval_scope_grants: 2,
        approval_rule_proposals: 1,
        ..UserErasureSummary::default()
    };
    assert_eq!(summary.approval_scope_grants, 2);
    assert_eq!(summary.approval_rule_proposals, 1);
    assert!(
        summary.erasure_is_complete(),
        "counts do not change completeness"
    );
}

// ---- DB --------------------------------------------------------------------------------------

async fn personal_thread(pool: &Pool, org: &str, thread: &str, owner: &str) {
    sqlx::query("INSERT INTO threads (id, session_key, org_id, user_id) VALUES ($1, $1, $2, $3)")
        .bind(thread)
        .bind(org)
        .bind(owner)
        .execute(pool)
        .await
        .expect("seed personal thread");
}

/// One thread-bound memory row of `thread`, with `owner` and `links`. Returns its id.
async fn thread_row(
    pool: &Pool,
    org: &str,
    thread: &str,
    owner: &str,
    key: &str,
    links: Vec<String>,
) -> String {
    let id = mp_ids::new_ulid();
    sqlx::query(
        "INSERT INTO agent_memory (id, org_id, session_id, scope, key, content, kind, confidence, owner, \
         source_links, review_state, is_latest) \
         VALUES ($1, $2, $3, 'thread', $4, 'c', 'fact', 0.9, $5, $6, 'accepted', true)",
    )
    .bind(&id)
    .bind(org)
    .bind(thread)
    .bind(key)
    .bind(owner)
    .bind(links)
    .execute(pool)
    .await
    .expect("seed thread row");
    id
}

async fn links_of(pool: &Pool, id: &str) -> Option<Vec<String>> {
    sqlx::query_scalar("SELECT source_links FROM agent_memory WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
        .expect("read row")
}

async fn forgotten(pool: &Pool, org: &str, memory_id: &str) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT user_id FROM forgotten_memory_ids WHERE org_id = $1 AND memory_id = $2",
    )
    .bind(org)
    .bind(memory_id)
    .fetch_all(pool)
    .await
    .expect("read forgotten")
}

#[tokio::test]
#[ignore = "requires DATABASE_URL to a disposable Postgres whose name contains \"test\""]
async fn the_persons_own_thread_rows_are_deleted_and_forgotten_and_a_space_row_is_attributed() {
    use crate::space::test_support;
    let Some(pool) = test_support::connect().await else {
        return;
    };
    let nonce = mp_ids::new_ulid();
    let org = format!("od59-org-{nonce}");
    let space = format!("od59-space-{nonce}");
    let (mine, theirs, room) = (
        format!("od59-mine-{nonce}"),
        format!("od59-theirs-{nonce}"),
        format!("od59-room-{nonce}"),
    );
    personal_thread(&pool, &org, &mine, "erased-user").await;
    personal_thread(&pool, &org, &theirs, "other-user").await;
    test_support::seed_space_thread(&pool, &org, &space, &room, "owner-1").await;
    let (said, other_said) = (format!("od59-msg-a-{nonce}"), format!("od59-msg-b-{nonce}"));
    test_support::seed_space_message(&pool, &space, &room, &said, "user", Some("erased-user"))
        .await;
    test_support::seed_space_message(
        &pool,
        &space,
        &room,
        &other_said,
        "user",
        Some("other-user"),
    )
    .await;

    let own = thread_row(&pool, &org, &mine, "erased-user", "own", vec![]).await;
    let own_unowned = thread_row(&pool, &org, &mine, "", "own-unowned", vec![]).await;
    let other_own = thread_row(&pool, &org, &theirs, "other-user", "other", vec![]).await;
    let sole = thread_row(
        &pool,
        &org,
        &room,
        "",
        "sole",
        vec![format!("message:{said}")],
    )
    .await;
    let shared = thread_row(
        &pool,
        &org,
        &room,
        "",
        "shared",
        vec![format!("message:{said}"), format!("message:{other_said}")],
    )
    .await;

    // The threads the hold check would clear with the switch on (its own query is exercised by the
    // existing hold tests); bound here so this test does not depend on the process switch.
    let checked = CheckedThreads(vec![mine.clone(), room.clone()]);
    let summary = erase_user_personal_data_for(
        &pool,
        &org,
        "erased-user",
        UserErasureKind::Erase,
        &checked,
        None,
    )
    .await
    .expect("erasure");
    assert_eq!(
        summary.thread_memory, 3,
        "own, own-unowned and the sole Space-thread row"
    );
    assert_eq!(summary.thread_memory_narrowed, 1);

    assert_eq!(links_of(&pool, &own).await, None);
    assert_eq!(links_of(&pool, &own_unowned).await, None);
    assert_eq!(
        forgotten(&pool, &org, &own).await,
        vec!["erased-user".to_owned()]
    );
    assert_eq!(
        forgotten(&pool, &org, &own_unowned).await,
        vec!["erased-user".to_owned()],
        "a row with no owner is forgotten for the subject"
    );
    assert!(
        links_of(&pool, &other_own).await.is_some(),
        "another member's own thread is untouched"
    );
    assert_eq!(
        links_of(&pool, &sole).await,
        None,
        "a row only the person stated is gone"
    );
    assert_eq!(
        links_of(&pool, &shared).await,
        Some(vec![format!("message:{other_said}")]),
        "a shared row keeps the other member's source only"
    );

    // Redelivery finds nothing left.
    let again = erase_user_personal_data_for(
        &pool,
        &org,
        "erased-user",
        UserErasureKind::Erase,
        &checked,
        None,
    )
    .await
    .expect("redelivery");
    assert_eq!(again.thread_memory, 0);
    assert_eq!(again.thread_memory_narrowed, 0);

    sqlx::query("DELETE FROM agent_memory WHERE org_id = $1")
        .bind(&org)
        .execute(&pool)
        .await
        .ok();
    sqlx::query("DELETE FROM forgotten_memory_ids WHERE org_id = $1")
        .bind(&org)
        .execute(&pool)
        .await
        .ok();
    sqlx::query("DELETE FROM threads WHERE org_id = $1 AND space_id IS NULL")
        .bind(&org)
        .execute(&pool)
        .await
        .ok();
    test_support::clean_space(&pool, &org, &space).await;
}

#[tokio::test]
#[ignore = "requires DATABASE_URL to a disposable Postgres whose name contains \"test\""]
async fn with_no_checked_thread_no_thread_row_is_touched() {
    use crate::space::test_support;
    let Some(pool) = test_support::connect().await else {
        return;
    };
    let nonce = mp_ids::new_ulid();
    let org = format!("od59-off-org-{nonce}");
    let thread = format!("od59-off-{nonce}");
    personal_thread(&pool, &org, &thread, "erased-user").await;
    let row = thread_row(&pool, &org, &thread, "erased-user", "own", vec![]).await;
    let summary = erase_user_personal_data_for(
        &pool,
        &org,
        "erased-user",
        UserErasureKind::Erase,
        &CheckedThreads::none(),
        None,
    )
    .await
    .expect("erasure");
    assert_eq!(summary.thread_memory, 0);
    assert!(
        links_of(&pool, &row).await.is_some(),
        "switch off: thread rows stay"
    );
    sqlx::query("DELETE FROM agent_memory WHERE org_id = $1")
        .bind(&org)
        .execute(&pool)
        .await
        .ok();
    sqlx::query("DELETE FROM threads WHERE org_id = $1")
        .bind(&org)
        .execute(&pool)
        .await
        .ok();
}

#[tokio::test]
#[ignore = "requires DATABASE_URL to a disposable Postgres whose name contains \"test\""]
async fn a_person_erasure_removes_their_grants_and_proposals_and_no_one_elses() {
    use crate::space::test_support;
    let Some(pool) = test_support::connect().await else {
        return;
    };
    let nonce = mp_ids::new_ulid();
    let org = format!("od59-auth-org-{nonce}");
    let thread = format!("od59-auth-t-{nonce}");
    personal_thread(&pool, &org, &thread, "erased-user").await;
    for user in ["erased-user", "other-user"] {
        let proposal = format!("arp_{}", mp_ids::new_ulid());
        sqlx::query(
            "INSERT INTO approval_rule_proposals \
             (id, org_id, user_id, created_from_approval_id, tool_name, tool_digest, capability_id) \
             VALUES ($1, $2, $3, 'appr_x', 'mcp__fixture__create_note', repeat('a', 64), '')",
        )
        .bind(&proposal)
        .bind(&org)
        .bind(user)
        .execute(&pool)
        .await
        .expect("seed proposal");
        sqlx::query(
            "INSERT INTO approval_scope_grants \
             (id, org_id, user_id, thread_id, run_id, scope, tool_name, action_key, effect_class, \
              cause_code, created_from_approval_id, expires_at, rule_id, tool_digest) \
             VALUES ($1, $2, $3, $4, NULL, 'thread', 'knowledge_search', 'knowledge_search', 'read', \
                     'consent_to_disclose', 'appr_x', now() + interval '1 hour', $5, '')",
        )
        .bind(format!("apg_{}", mp_ids::new_ulid()))
        .bind(&org)
        .bind(user)
        .bind(&thread)
        .bind(&proposal)
        .execute(&pool)
        .await
        .expect("seed grant");
    }
    let summary = erase_user_personal_data_for(
        &pool,
        &org,
        "erased-user",
        UserErasureKind::Anonymize,
        &CheckedThreads::none(),
        None,
    )
    .await
    .expect("erasure");
    assert_eq!(summary.approval_scope_grants, 1);
    assert_eq!(summary.approval_rule_proposals, 1);
    let left = |table: &'static str, user: &'static str| {
        let (pool, org) = (pool.clone(), org.clone());
        async move {
            sqlx::query_scalar::<_, i64>(&format!(
                "SELECT count(*) FROM {table} WHERE org_id = $1 AND user_id = $2"
            ))
            .bind(org)
            .bind(user)
            .fetch_one(&pool)
            .await
            .expect("count")
        }
    };
    assert_eq!(left("approval_scope_grants", "erased-user").await, 0);
    assert_eq!(left("approval_rule_proposals", "erased-user").await, 0);
    assert_eq!(left("approval_scope_grants", "other-user").await, 1);
    assert_eq!(left("approval_rule_proposals", "other-user").await, 1);
    for table in [
        "approval_scope_grants",
        "approval_rule_proposals",
        "threads",
    ] {
        sqlx::query(&format!("DELETE FROM {table} WHERE org_id = $1"))
            .bind(&org)
            .execute(&pool)
            .await
            .ok();
    }
}
