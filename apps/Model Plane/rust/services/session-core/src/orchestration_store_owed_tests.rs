//! Owed approvals tests (verification plan 3.2 and 3.3), a test-only child module of
//! `orchestration_store`.
//!
//! * Pure tier: the rule-proposal refusal codes and the unmatched-resolution table
//!   [W2-WS13-S7-proposal-store-a]; every resolve statement bound to id, org and user with its own
//!   state predicate; the decided-descriptor delete keeps only a DENIED `browser_human_assist`
//!   descriptor [FU-25].
//! * DB tier (every test `#[ignore]`d, needs `DATABASE_URL` to a scratch Postgres whose name
//!   contains `test`): a pinned MCP external write is grantable and its grant releases only the
//!   digest it was created with, and an unpinned one is not grantable [W2-WS13-S7-grant-digest];
//!   a deny keeps a `browser_human_assist` descriptor and a timeout or another cause's deny deletes
//!   it [FU-25]; the proposal claim, confirm and dismiss lifecycle, owner-bound
//!   [W2-WS13-S7-proposal-store-a].

use super::*;
use base64::Engine as _;
use sqlx::postgres::PgPoolOptions;

// ---- pure ----------------------------------------------------------------------------------

fn proposal_row(state: &str, capability_id: &str, policy_id: &str) -> ApprovalRuleProposalRow {
    ApprovalRuleProposalRow {
        id: "arp_1".to_owned(),
        tool_name: "mcp__fixture__create_note".to_owned(),
        tool_digest: "a".repeat(64),
        capability_id: capability_id.to_owned(),
        state: state.to_owned(),
        created_from_approval_id: "appr_1".to_owned(),
        created_at: Utc::now(),
        resolved_at: None,
        policy_id: policy_id.to_owned(),
    }
}

#[test]
fn rule_proposal_refusals_reach_the_caller_as_closed_codes() {
    assert_eq!(
        RuleProposalRefusal::Resolved.code(),
        "rule_proposal_resolved"
    );
    assert_eq!(RuleProposalRefusal::Stale.code(), "rule_proposal_stale");
    assert_eq!(RuleProposalRefusal::Busy.code(), "rule_proposal_busy");
    assert_eq!(
        RuleProposalRefusal::NotClaimed.code(),
        "rule_proposal_not_claimed"
    );
    let error: anyhow::Error = RuleProposalRefusal::Busy.into();
    assert_eq!(
        error.downcast_ref::<RuleProposalRefusal>(),
        Some(&RuleProposalRefusal::Busy),
        "the store error downcasts like ScopeGrantRefusal"
    );
}

#[test]
fn an_unmatched_resolution_is_a_replay_or_a_typed_refusal() {
    use RuleProposalRefusal as R;
    let cap = "cap.mcp_tool.fixture.create_note";
    let claim = RuleProposalResolution::Claim;
    let confirm = RuleProposalResolution::Confirmed {
        policy_id: "tpol_1",
    };
    let dismiss = RuleProposalResolution::Dismissed;
    let cases: [(RuleProposalResolution<'_>, &str, &str, &str, Result<(), R>); 12] = [
        // A claim that finds the row confirmed is the confirm's replay.
        (claim, "confirmed", cap, "tpol_1", Ok(())),
        (claim, "proposed", "", "", Err(R::Stale)),
        (claim, "confirming", "", "", Err(R::Stale)),
        (claim, "dismissed", cap, "", Err(R::Resolved)),
        (confirm, "confirmed", cap, "tpol_1", Ok(())),
        (confirm, "confirmed", cap, "tpol_other", Err(R::Resolved)),
        (confirm, "proposed", cap, "", Err(R::NotClaimed)),
        (confirm, "dismissed", cap, "", Err(R::Resolved)),
        (dismiss, "dismissed", cap, "", Ok(())),
        (dismiss, "confirming", cap, "", Err(R::Busy)),
        (dismiss, "confirmed", cap, "tpol_1", Err(R::Resolved)),
        (claim, "dismissed", "", "", Err(R::Resolved)),
    ];
    for (resolution, state, capability, policy, expected) in cases {
        let got = rule_proposal_unmatched(resolution, proposal_row(state, capability, policy))
            .map(|row| assert_eq!(row.state, state, "a replay returns the row as it stands"));
        assert_eq!(
            got, expected,
            "{resolution:?} on {state} ({capability:?}, {policy:?})"
        );
    }
}

fn squash(sql: &str) -> String {
    sql.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn every_resolve_statement_is_owner_bound_with_its_own_state_predicate() {
    for sql in [
        RESOLVE_RULE_PROPOSAL_CLAIM_SQL,
        RESOLVE_RULE_PROPOSAL_CONFIRMED_SQL,
        RESOLVE_RULE_PROPOSAL_DISMISSED_SQL,
    ] {
        assert!(
            squash(sql).contains("WHERE id = $1 AND org_id = $2 AND user_id = $3"),
            "owner-bound: {sql}"
        );
    }
    let claim = squash(RESOLVE_RULE_PROPOSAL_CLAIM_SQL);
    assert!(claim.contains("state IN ('proposed', 'confirming') AND capability_id <> ''"));
    assert!(claim.contains("SET state = 'confirming', claimed_at = now()"));
    let confirmed = squash(RESOLVE_RULE_PROPOSAL_CONFIRMED_SQL);
    assert!(confirmed.contains("AND state = 'confirming'"));
    assert!(confirmed.contains("policy_id = $4"));
    let dismissed = squash(RESOLVE_RULE_PROPOSAL_DISMISSED_SQL);
    assert!(dismissed.contains(
        "state = 'proposed' OR (state = 'confirming' AND claimed_at < now() - interval '60 seconds')"
    ));
    assert_eq!(
        RULE_PROPOSAL_CLAIM_SECS, 60,
        "the dismiss SQL mirrors the claim window"
    );
    // The open list and the single read are owner-bound too.
    // (Their texts are built at call time; the column list is the one the row type reads.)
    assert!(APPROVAL_RULE_PROPOSAL_COLUMNS.contains("policy_id"));
}

#[test]
fn the_decided_descriptor_delete_keeps_only_a_denied_browser_assist_descriptor() {
    let sql = squash(DELETE_DECIDED_DESCRIPTOR_SQL);
    assert!(sql.contains("WHERE d.approval_id = $1 AND d.org_id = $2"));
    assert!(
        sql.contains("a.status = 'denied' AND a.metadata #>> '{cause,code}' = $3"),
        "only a DENIED pause of the bound cause keeps its descriptor; a timeout keeps nothing"
    );
    assert_eq!(
        mp_contracts::approval_cause::CauseCode::BrowserHumanAssist.as_str(),
        "browser_human_assist"
    );
}

// ---- DB --------------------------------------------------------------------------------------

async fn scratch_pool() -> Option<Pool> {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("skipping: DATABASE_URL unset");
        return None;
    };
    assert!(
        url.contains("test"),
        "the scratch database name must say test"
    );
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .expect("connect pg");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("migrate");
    // The retained descriptor is encrypted: the process needs its key (the value the other
    // descriptor tests in this file set).
    std::env::set_var(
        "SESSION_CORE_CONTINUATION_DESCRIPTOR_KEY",
        base64::engine::general_purpose::STANDARD.encode([7_u8; 32]),
    );
    Some(pool)
}

const FINGERPRINT: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const DIGEST: &str = "2b6a1f0c9d8e7f6a5b4c3d2e1f0a9b8c7d6e5f4a3b2c1d0e9f8a7b6c5d4e3f2a";
const OTHER_DIGEST: &str = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";

struct Seeded {
    approval_id: String,
    org: String,
    user: String,
    thread_id: String,
    run_id: String,
}

/// One persistent run on an ordinary thread with ONE requested approval carrying `cause`, and a
/// retained descriptor when `with_descriptor`.
async fn seeded(pool: &Pool, tag: &str, cause: JsonValue, with_descriptor: bool) -> Seeded {
    let sfx = mp_ids::new_ulid();
    let (thread_id, run_id, org, user) = (
        format!("ow-t-{tag}-{sfx}"),
        format!("ow-r-{tag}-{sfx}"),
        format!("ow-org-{sfx}"),
        "u1".to_owned(),
    );
    sqlx::query("INSERT INTO threads (id,session_key,org_id,user_id) VALUES ($1,$1,$2,$3)")
        .bind(&thread_id)
        .bind(&org)
        .bind(&user)
        .execute(pool)
        .await
        .expect("seed thread");
    sqlx::query("INSERT INTO runs (id,thread_id,goal,org_id,user_id) VALUES ($1,$2,'g',$3,$4)")
        .bind(&run_id)
        .bind(&thread_id)
        .bind(&org)
        .bind(&user)
        .execute(pool)
        .await
        .expect("seed run");
    let approval_id = format!("appr_{}", mp_ids::new_ulid());
    let descriptor = serde_json::json!({
        "version": 1,
        "run_id": run_id,
        "org_id": org,
        "user_id": user,
        "step_id": "tool_1_call-1",
        "action_kind": "tool_call",
        "action_fingerprint": FINGERPRINT,
        "tool_name": "browser_navigate",
        "input": {},
        "permission_mode": "ask",
    });
    request_approval(
        pool,
        &approval_id,
        &run_id,
        None,
        "tool_call",
        &user,
        &org,
        &user,
        "",
        &serde_json::json!({ "cause": cause }),
        with_descriptor.then_some(&descriptor),
        Some(Utc::now() + chrono::Duration::minutes(5)),
        "",
    )
    .await
    .expect("request approval")
    .expect("inserted");
    Seeded {
        approval_id,
        org,
        user,
        thread_id,
        run_id,
    }
}

async fn descriptor_rows(pool: &Pool, id: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM approval_continuation_descriptors WHERE approval_id = $1",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .expect("count descriptors")
}

async fn decide_with(
    pool: &Pool,
    s: &Seeded,
    status: &str,
    scope: &str,
) -> Result<ApprovalDecisionWrite> {
    let record = DecisionRecord {
        source: "user",
        scope,
        audit: false,
        grant_ttl_secs: 3600,
        rule_proposed: false,
        authz_audit: false,
    };
    decide_approval_recorded(
        pool,
        &s.approval_id,
        &s.org,
        Some(s.user.as_str()),
        status,
        &s.user,
        "decided",
        &record,
    )
    .await
}

#[tokio::test]
#[ignore = "requires DATABASE_URL to a scratch Postgres"]
async fn a_denied_browser_assist_pause_keeps_its_descriptor_and_a_timeout_or_other_cause_does_not()
{
    let Some(pool) = scratch_pool().await else {
        return;
    };
    let assist = serde_json::json!({"code": "browser_human_assist", "tool": "browser_navigate"});
    let denied = seeded(&pool, "assist-deny", assist.clone(), true).await;
    assert_eq!(
        descriptor_rows(&pool, &denied.approval_id).await,
        1,
        "premise"
    );
    decide_with(&pool, &denied, "denied", "once")
        .await
        .expect("deny");
    assert_eq!(
        descriptor_rows(&pool, &denied.approval_id).await,
        1,
        "Skip this site: the denied assist pause keeps its encrypted descriptor"
    );

    let timed_out = seeded(&pool, "assist-timeout", assist, true).await;
    decide_with(&pool, &timed_out, "timed_out", "once")
        .await
        .expect("time out");
    assert_eq!(
        descriptor_rows(&pool, &timed_out.approval_id).await,
        0,
        "a timed-out assist pause keeps nothing"
    );

    let other = seeded(
        &pool,
        "other-deny",
        serde_json::json!({"code": "risky_effect_under_ask", "tool": "browser_navigate"}),
        true,
    )
    .await;
    decide_with(&pool, &other, "denied", "once")
        .await
        .expect("deny");
    assert_eq!(
        descriptor_rows(&pool, &other.approval_id).await,
        0,
        "any other cause's deny deletes the descriptor, as before"
    );
}

fn mcp_write_cause(digest: &str) -> JsonValue {
    serde_json::json!({
        "code": "risky_effect_under_ask",
        "tool": "mcp__fixture__create_note",
        "effect_class": "external_write",
        "action_key": "mcp__fixture__create_note",
        "tool_digest": digest,
    })
}

fn use_of<'a>(s: &'a Seeded, digest: &'a str) -> ScopeGrantUse<'a> {
    ScopeGrantUse {
        org_id: &s.org,
        user_id: &s.user,
        thread_id: &s.thread_id,
        run_id: &s.run_id,
        tool_name: "mcp__fixture__create_note",
        action_key: "mcp__fixture__create_note",
        effect_class: "external_write",
        tool_digest: digest,
    }
}

#[tokio::test]
#[ignore = "requires DATABASE_URL to a scratch Postgres"]
async fn a_pinned_mcp_external_write_grant_releases_only_its_own_digest() {
    let Some(pool) = scratch_pool().await else {
        return;
    };
    let pinned = seeded(&pool, "pinned", mcp_write_cause(DIGEST), false).await;
    decide_with(&pool, &pinned, "granted", "thread")
        .await
        .expect("a pinned MCP external write is grantable for the thread");
    let digest: String = sqlx::query_scalar(
        "SELECT tool_digest FROM approval_scope_grants WHERE org_id = $1 AND scope = 'thread'",
    )
    .bind(&pinned.org)
    .fetch_one(&pool)
    .await
    .expect("the grant");
    assert_eq!(
        digest, DIGEST,
        "the grant is bound to the reviewed definition"
    );

    assert!(
        use_approval_scope_grant(&pool, &use_of(&pinned, OTHER_DIGEST))
            .await
            .expect("use")
            .is_none(),
        "a drifted definition is not released"
    );
    assert!(
        use_approval_scope_grant(&pool, &use_of(&pinned, ""))
            .await
            .expect("use")
            .is_none(),
        "a call that states no digest is not released"
    );
    assert!(
        use_approval_scope_grant(&pool, &use_of(&pinned, DIGEST))
            .await
            .expect("use")
            .is_some(),
        "the same definition is released"
    );
}

#[tokio::test]
#[ignore = "requires DATABASE_URL to a scratch Postgres"]
async fn an_unpinned_mcp_external_write_is_not_grantable() {
    let Some(pool) = scratch_pool().await else {
        return;
    };
    let unpinned = seeded(&pool, "unpinned", mcp_write_cause(""), false).await;
    let refused = decide_with(&pool, &unpinned, "granted", "thread")
        .await
        .expect_err("no digest, no standing scope");
    assert_eq!(
        refused.downcast_ref::<ScopeGrantRefusal>(),
        Some(&ScopeGrantRefusal::NotOffered)
    );
    let grants: i64 =
        sqlx::query_scalar("SELECT count(*) FROM approval_scope_grants WHERE org_id = $1")
            .bind(&unpinned.org)
            .fetch_one(&pool)
            .await
            .expect("count");
    assert_eq!(grants, 0);
}

/// The approval a person's own run raised in `org`: the decision a proposal names as the one that
/// created it (`created_from_approval_id`, migration 0070). Written as production writes it: a
/// thread, a run on it, then a requested approval on that run. Returns the approval id.
async fn seed_requested_approval(pool: &Pool, org: &str, user: &str) -> String {
    let sfx = mp_ids::new_ulid();
    let (thread_id, run_id, approval_id) = (
        format!("arp-t-{sfx}"),
        format!("arp-r-{sfx}"),
        format!("appr_{sfx}"),
    );
    sqlx::query("INSERT INTO threads (id,session_key,org_id,user_id) VALUES ($1,$1,$2,$3)")
        .bind(&thread_id)
        .bind(org)
        .bind(user)
        .execute(pool)
        .await
        .expect("seed thread");
    sqlx::query("INSERT INTO runs (id,thread_id,goal,org_id,user_id) VALUES ($1,$2,'g',$3,$4)")
        .bind(&run_id)
        .bind(&thread_id)
        .bind(org)
        .bind(user)
        .execute(pool)
        .await
        .expect("seed run");
    sqlx::query(
        "INSERT INTO approvals (id,run_id,kind,status,org_id,user_id) \
         VALUES ($1,$2,'tool_call','requested',$3,$4)",
    )
    .bind(&approval_id)
    .bind(&run_id)
    .bind(org)
    .bind(user)
    .execute(pool)
    .await
    .expect("seed approval");
    approval_id
}

async fn insert_proposal(pool: &Pool, org: &str, user: &str, capability_id: &str) -> String {
    let approval = seed_requested_approval(pool, org, user).await;
    let id = format!("arp_{}", mp_ids::new_ulid());
    sqlx::query(
        "INSERT INTO approval_rule_proposals \
         (id, org_id, user_id, created_from_approval_id, tool_name, tool_digest, capability_id) \
         VALUES ($1, $2, $3, $4, 'mcp__fixture__create_note', $5, $6)",
    )
    .bind(&id)
    .bind(org)
    .bind(user)
    .bind(&approval)
    .bind(format!(
        "{:0>64}",
        mp_ids::new_ulid()
            .to_lowercase()
            .chars()
            .filter(char::is_ascii_hexdigit)
            .collect::<String>()
    ))
    .bind(capability_id)
    .execute(pool)
    .await
    .expect("seed proposal");
    id
}

fn refusal(error: anyhow::Error) -> RuleProposalRefusal {
    *error
        .downcast_ref::<RuleProposalRefusal>()
        .expect("a typed refusal")
}

#[tokio::test]
#[ignore = "requires DATABASE_URL to a scratch Postgres"]
async fn a_proposal_is_claimed_confirmed_and_replayed_and_a_fresh_claim_refuses_a_dismiss() {
    let Some(pool) = scratch_pool().await else {
        return;
    };
    let org = format!("arp-org-{}", mp_ids::new_ulid());
    let id = insert_proposal(&pool, &org, "u1", "cap.mcp_tool.fixture.create_note").await;

    let claimed = resolve_rule_proposal(&pool, &org, "u1", &id, RuleProposalResolution::Claim)
        .await
        .expect("claim")
        .expect("row");
    assert_eq!(claimed.state, "confirming");
    // A dismiss cannot land while the claim is fresh.
    let busy = resolve_rule_proposal(&pool, &org, "u1", &id, RuleProposalResolution::Dismissed)
        .await
        .expect_err("busy");
    assert_eq!(refusal(busy), RuleProposalRefusal::Busy);
    // Re-claiming one's own claim is the idempotent retry.
    assert_eq!(
        resolve_rule_proposal(&pool, &org, "u1", &id, RuleProposalResolution::Claim)
            .await
            .expect("re-claim")
            .expect("row")
            .state,
        "confirming"
    );
    // Another person's proposal reads as absent.
    assert!(
        resolve_rule_proposal(&pool, &org, "u2", &id, RuleProposalResolution::Claim)
            .await
            .expect("other owner")
            .is_none()
    );

    let confirm = RuleProposalResolution::Confirmed {
        policy_id: "tpol_abc-1",
    };
    let confirmed = resolve_rule_proposal(&pool, &org, "u1", &id, confirm)
        .await
        .expect("confirm")
        .expect("row");
    assert_eq!(confirmed.state, "confirmed");
    assert_eq!(confirmed.policy_id, "tpol_abc-1");
    assert!(confirmed.resolved_at.is_some());
    // Replays.
    assert_eq!(
        resolve_rule_proposal(&pool, &org, "u1", &id, confirm)
            .await
            .expect("replay")
            .expect("row")
            .state,
        "confirmed"
    );
    let other_policy = resolve_rule_proposal(
        &pool,
        &org,
        "u1",
        &id,
        RuleProposalResolution::Confirmed {
            policy_id: "tpol_other",
        },
    )
    .await
    .expect_err("another policy");
    assert_eq!(refusal(other_policy), RuleProposalRefusal::Resolved);
    assert!(
        list_open_rule_proposals(&pool, &org, "u1")
            .await
            .expect("list")
            .is_empty(),
        "a confirmed proposal is not open"
    );
}

#[tokio::test]
#[ignore = "requires DATABASE_URL to a scratch Postgres"]
async fn a_stale_claim_is_dismissible_and_a_pre_0126_proposal_cannot_be_claimed() {
    let Some(pool) = scratch_pool().await else {
        return;
    };
    let org = format!("arp-org-{}", mp_ids::new_ulid());
    let stale = insert_proposal(&pool, &org, "u1", "").await;
    let refused = resolve_rule_proposal(&pool, &org, "u1", &stale, RuleProposalResolution::Claim)
        .await
        .expect_err("stale");
    assert_eq!(refusal(refused), RuleProposalRefusal::Stale);
    let not_claimed = resolve_rule_proposal(
        &pool,
        &org,
        "u1",
        &stale,
        RuleProposalResolution::Confirmed {
            policy_id: "tpol_1",
        },
    )
    .await
    .expect_err("not claimed");
    assert_eq!(refusal(not_claimed), RuleProposalRefusal::NotClaimed);

    let id = insert_proposal(&pool, &org, "u1", "cap.mcp_tool.fixture.create_note").await;
    resolve_rule_proposal(&pool, &org, "u1", &id, RuleProposalResolution::Claim)
        .await
        .expect("claim");
    sqlx::query("UPDATE approval_rule_proposals SET claimed_at = now() - interval '61 seconds' WHERE id = $1")
        .bind(&id)
        .execute(&pool)
        .await
        .expect("age the claim");
    let dismissed =
        resolve_rule_proposal(&pool, &org, "u1", &id, RuleProposalResolution::Dismissed)
            .await
            .expect("dismiss")
            .expect("row");
    assert_eq!(dismissed.state, "dismissed");
    // A late confirm of the dead claim is refused.
    let late = resolve_rule_proposal(
        &pool,
        &org,
        "u1",
        &id,
        RuleProposalResolution::Confirmed {
            policy_id: "tpol_1",
        },
    )
    .await
    .expect_err("late confirm");
    assert_eq!(refusal(late), RuleProposalRefusal::Resolved);
}
