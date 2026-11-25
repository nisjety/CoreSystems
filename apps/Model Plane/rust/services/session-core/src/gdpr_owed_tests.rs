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
fn the_thread_memory_switch_takes_only_the_exact_literal_true() {
    assert!(thread_memory_erasure_switch(Some("true")));
    for off in [
        None,
        Some(""),
        Some("1"),
        Some("yes"),
        Some("on"),
        Some("TRUE"),
        Some("True"),
        Some(" true "),
        Some("true\n"),
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

/// F-f01-16: a person's erasure removes their own user-scope skills with their versions and revisions, in the
/// order the org purge uses (versions under the erasure setting, then the skills, then the revisions that
/// hold their text), after the promote counters and before the audit row, and its audit detail carries the
/// three counts. Checked on the source text, so no database is needed.
#[test]
fn a_person_erasure_removes_the_persons_own_skills_in_the_purge_order() {
    let source = include_str!("gdpr.rs");
    let helper = squash(body_of(source, "async fn erase_user_skills_in_tx("));
    let at = |needle: &str| {
        helper
            .find(needle)
            .unwrap_or_else(|| panic!("the skill erasure states {needle}"))
    };
    let on = at("set_config('mp.version_erasure', 'on', true)");
    let versions = at(
        "DELETE FROM agent_skill_versions WHERE org_id = $1 AND scope = 'user' AND owner_user_id = $2",
    );
    let off = at("set_config('mp.version_erasure', '', true)");
    let skills =
        at("DELETE FROM agent_skills WHERE org_id = $1 AND scope = 'user' AND owner_user_id = $2");
    let revisions = at("DELETE FROM skill_revisions WHERE org_id = $1 AND owner_user_id = $2");
    assert!(
        on < versions && versions < off && off < skills && skills < revisions,
        "the versions go under the setting, then the skills, then the revisions that hold their text"
    );
    assert_eq!(
        helper.matches("mp.version_erasure").count(),
        2,
        "the setting is on for the one version delete and put back at once"
    );
    assert!(
        !helper.contains("DELETE FROM agent_skills WHERE org_id = $1 AND scope = 'org'")
            && !helper.contains("DELETE FROM agent_skills WHERE org_id = $1 AND owner_user_id"),
        "an org-scope skill is never named by the person's erasure"
    );

    let body = squash(body_of(source, "pub async fn erase_user_personal_data_for("));
    let promote = body
        .find("DELETE FROM promote_use_counts WHERE org_id = $1 AND user_id = $2")
        .expect("the promote-on-use counter delete");
    let call = body
        .find(
            "erase_user_skills_in_tx(&mut tx, org_id, user_id, skill_bundle_scope_rules_enabled())",
        )
        .expect("the skill erasure, passed the bundle switch");
    let audit = body
        .find("INSERT INTO memory_control_events")
        .expect("the audit row");
    assert!(
        promote < call && call < audit,
        "the skills go after the promote counters and before the audit row"
    );
    for detail in [
        "'agent_skills', $10::bigint, 'skill_revisions', $11::bigint",
        "'agent_skills', $8::bigint, 'skill_revisions', $9::bigint",
    ] {
        assert!(
            body.contains(detail),
            "the audit detail counts the skills: {detail}"
        );
    }
}

/// The bundle switch takes only the exact literal `true`, as the other per-user switches do (P2-go-09-rel1).
#[test]
fn the_skill_bundle_scope_rules_switch_takes_only_the_exact_literal_true() {
    assert!(skill_bundle_scope_rules_switch(Some("true")));
    for off in [
        None,
        Some(""),
        Some("1"),
        Some("yes"),
        Some("on"),
        Some("TRUE"),
        Some("True"),
        Some(" true "),
        Some("true\n"),
        Some("false"),
    ] {
        assert!(!skill_bundle_scope_rules_switch(off), "{off:?}");
    }
    assert_eq!(SKILL_BUNDLE_SCOPE_RULES_ENV, "SKILL_BUNDLE_SCOPE_RULES");
}

/// P2-go-09-rel1: a person's erasure removes the bundles of the skills it deletes, in its own transaction, and
/// only under `SKILL_BUNDLE_SCOPE_RULES`. The skills go first and return their ids. The bundle step follows
/// them and comes before the revisions, and it runs only inside the switch. Inside the step the table guard
/// comes first, then the organization's bundle lock (the lock capability-core takes for a bundle write), the
/// bundle delete and the blob collection. The erasure passes the switch it reads. Checked on the source text,
/// so no database is needed.
#[test]
fn a_person_erasure_removes_the_bundles_of_the_skills_it_deletes_under_the_switch() {
    let source = include_str!("gdpr.rs");
    let helper = squash(body_of(source, "async fn erase_user_skills_in_tx("));
    let at = |needle: &str| {
        helper
            .find(needle)
            .unwrap_or_else(|| panic!("the skill erasure states {needle}"))
    };
    let skills = at(
        "DELETE FROM agent_skills WHERE org_id = $1 AND scope = 'user' AND owner_user_id = $2 RETURNING id",
    );
    let bundles = at(
        "let skill_bundles = if remove_bundles { erase_skill_bundles_in_tx(tx, org_id, &skill_ids).await? } else { 0 };",
    );
    let revisions = at("DELETE FROM skill_revisions WHERE org_id = $1 AND owner_user_id = $2");
    assert!(
        skills < bundles && bundles < revisions,
        "the bundles of the deleted skills go after the skills and before the revisions"
    );

    let bundle_step = squash(body_of(source, "async fn erase_skill_bundles_in_tx("));
    let step = |needle: &str| {
        bundle_step
            .find(needle)
            .unwrap_or_else(|| panic!("the bundle step states {needle}"))
    };
    let guard = step("to_regclass('public.skill_bundles') IS NOT NULL");
    let lock = step("pg_advisory_xact_lock(hashtextextended('skill_bundles:' || $1::text, 0))");
    let delete = step("DELETE FROM skill_bundles WHERE org_id = $1 AND skill_id = ANY($2::text[])");
    let collect = step("DELETE FROM skill_bundle_blobs b");
    assert!(
        guard < lock && lock < delete && delete < collect,
        "the table guard, the bundle lock, the bundle delete, then the blob collection"
    );
    assert!(
        bundle_step.contains("WHERE sb.org_id = b.org_id AND f.sha256 = b.sha256"),
        "the blob collection keeps every blob a bundle file still names"
    );

    let body = squash(body_of(
        source,
        "pub async fn erase_user_personal_data_for(",
    ));
    assert!(
        body.contains(
            "erase_user_skills_in_tx(&mut tx, org_id, user_id, skill_bundle_scope_rules_enabled())"
        ),
        "the erasure passes the switch it reads"
    );
    assert!(
        body.contains("skill_bundles: skills.skill_bundles"),
        "the summary counts the removed bundles"
    );
}

// ---- DB (P2-go-09-rel1) ----------------------------------------------------------------------

const BUNDLE_ROWS: &str = "SELECT count(*) FROM skill_bundles WHERE org_id = $1 AND skill_id = $2";
const BUNDLE_FILE_ROWS: &str = "SELECT count(*) FROM skill_bundle_files f \
     JOIN skill_bundles b ON b.id = f.bundle_id WHERE b.org_id = $1 AND b.skill_id = $2";
const BLOB_ROWS: &str = "SELECT count(*) FROM skill_bundle_blobs WHERE org_id = $1 AND sha256 = $2";
const SKILL_ROWS: &str = "SELECT count(*) FROM agent_skills WHERE org_id = $1 AND id = $2";

/// Creates capability-core's bundle tables on a database that has only the session-core migrations, with the
/// columns and checks of its migration 0021. `IF NOT EXISTS`, so a table that is already there stays as it is.
async fn ensure_skill_bundle_tables(pool: &Pool) {
    for statement in [
        "CREATE TABLE IF NOT EXISTS skill_bundles (\
            id TEXT PRIMARY KEY, org_id TEXT NOT NULL, skill_id TEXT NOT NULL, \
            manifest_sha256 TEXT NOT NULL, file_count INTEGER NOT NULL, total_bytes BIGINT NOT NULL, \
            created_by TEXT NOT NULL, created_at TIMESTAMPTZ NOT NULL DEFAULT now(), \
            CONSTRAINT skill_bundles_manifest_check CHECK (manifest_sha256 ~ '^[0-9a-f]{64}$'), \
            CONSTRAINT skill_bundles_file_count_check CHECK (file_count BETWEEN 1 AND 256), \
            CONSTRAINT skill_bundles_total_bytes_check CHECK (total_bytes BETWEEN 0 AND 8388608))",
        "CREATE TABLE IF NOT EXISTS skill_bundle_files (\
            bundle_id TEXT NOT NULL REFERENCES skill_bundles (id) ON DELETE CASCADE, \
            path TEXT NOT NULL, sha256 TEXT NOT NULL, size_bytes INTEGER NOT NULL, \
            executable BOOLEAN NOT NULL DEFAULT FALSE, is_binary BOOLEAN NOT NULL DEFAULT FALSE, \
            PRIMARY KEY (bundle_id, path), \
            CONSTRAINT skill_bundle_files_sha256_check CHECK (sha256 ~ '^[0-9a-f]{64}$'), \
            CONSTRAINT skill_bundle_files_size_check CHECK (size_bytes BETWEEN 0 AND 1048576))",
        "CREATE TABLE IF NOT EXISTS skill_bundle_blobs (\
            org_id TEXT NOT NULL, sha256 TEXT NOT NULL, content BYTEA NOT NULL, size_bytes INTEGER NOT NULL, \
            PRIMARY KEY (org_id, sha256), \
            CONSTRAINT skill_bundle_blobs_sha256_check CHECK (sha256 ~ '^[0-9a-f]{64}$'), \
            CONSTRAINT skill_bundle_blobs_size_check CHECK (size_bytes BETWEEN 0 AND 1048576))",
    ] {
        sqlx::query(statement)
            .execute(pool)
            .await
            .expect("create a bundle table");
    }
}

/// Seeds one org with a personal skill of `erased-user` (digest `a`), another member's personal skill (`b`)
/// and an org skill (`c`), each with one live bundle of one file whose blob carries its digest. Returns the
/// skill ids in that order. A skill id is a primary key, so each id carries the org.
async fn seed_bundled_skills(pool: &Pool, org: &str) -> Vec<String> {
    let mut ids = Vec::new();
    for (name, scope, owner, digit) in [
        ("mine", "user", "erased-user", "a"),
        ("theirs", "user", "other-user", "b"),
        ("shared", "org", "", "c"),
    ] {
        let skill = format!("p2-go-09-r1-{name}-{org}");
        let digest = digit.repeat(64);
        sqlx::query(
            "INSERT INTO agent_skills (id, org_id, name, content, scope, owner_user_id, created_at, updated_at) \
             VALUES ($1, $2, $3, 'the skill text', $4, $5, now(), now())",
        )
        .bind(&skill)
        .bind(org)
        .bind(name)
        .bind(scope)
        .bind(owner)
        .execute(pool)
        .await
        .expect("seed skill");
        let bundle = format!("bundle-{skill}");
        sqlx::query(
            "INSERT INTO skill_bundles (id, org_id, skill_id, manifest_sha256, file_count, total_bytes, created_by) \
             VALUES ($1, $2, $3, $4, 1, 5, 'admin-1')",
        )
        .bind(&bundle)
        .bind(org)
        .bind(&skill)
        .bind(&digest)
        .execute(pool)
        .await
        .expect("seed bundle");
        sqlx::query(
            "INSERT INTO skill_bundle_files (bundle_id, path, sha256, size_bytes) VALUES ($1, 'SKILL.md', $2, 5)",
        )
        .bind(&bundle)
        .bind(&digest)
        .execute(pool)
        .await
        .expect("seed bundle file");
        sqlx::query(
            "INSERT INTO skill_bundle_blobs (org_id, sha256, content, size_bytes) VALUES ($1, $2, $3, 5)",
        )
        .bind(org)
        .bind(&digest)
        .bind(b"hello".to_vec())
        .execute(pool)
        .await
        .expect("seed bundle blob");
        ids.push(skill);
    }
    ids
}

async fn count_rows(pool: &Pool, sql: &str, org: &str, key: &str) -> i64 {
    sqlx::query_scalar(sql)
        .bind(org)
        .bind(key)
        .fetch_one(pool)
        .await
        .expect("count the rows")
}

/// P2-go-09-rel1 (DB tier): with the switch on, a person's erasure removes the bundle, its file and its blob
/// of the person's own skill in the erasing transaction, and touches neither another member's skill nor the
/// org skill. With the switch off the same erasure removes the skill and leaves its bundle, file and blob.
#[tokio::test]
#[ignore = "requires DATABASE_URL to a disposable Postgres whose name contains \"test\""]
async fn a_person_erasure_removes_the_bundles_of_their_skills_only_under_the_switch() {
    use crate::space::test_support;
    let Some(pool) = test_support::connect().await else {
        return;
    };
    ensure_skill_bundle_tables(&pool).await;
    let nonce = mp_ids::new_ulid();
    let on_org = format!("p2-go-09-r1-on-{nonce}");
    let off_org = format!("p2-go-09-r1-off-{nonce}");
    let on = seed_bundled_skills(&pool, &on_org).await;
    let off = seed_bundled_skills(&pool, &off_org).await;

    let mut tx = pool.begin().await.expect("begin the erasing transaction");
    let removed = erase_user_skills_in_tx(&mut tx, &on_org, "erased-user", true)
        .await
        .expect("the erasure with the switch on");
    tx.commit().await.expect("commit the erasure");
    assert_eq!(
        (removed.agent_skills, removed.skill_bundles),
        (1, 1),
        "the person's skill and its bundle"
    );
    let (mine, theirs, shared) = (&on[0], &on[1], &on[2]);
    assert_eq!(count_rows(&pool, SKILL_ROWS, &on_org, mine).await, 0);
    assert_eq!(count_rows(&pool, BUNDLE_ROWS, &on_org, mine).await, 0);
    assert_eq!(
        count_rows(&pool, BUNDLE_FILE_ROWS, &on_org, mine).await,
        0,
        "the file goes with the bundle"
    );
    assert_eq!(
        count_rows(&pool, BLOB_ROWS, &on_org, &"a".repeat(64)).await,
        0,
        "the blob no bundle names any more is collected"
    );
    assert_eq!(count_rows(&pool, SKILL_ROWS, &on_org, theirs).await, 1);
    assert_eq!(count_rows(&pool, BUNDLE_ROWS, &on_org, theirs).await, 1);
    assert_eq!(
        count_rows(&pool, BUNDLE_FILE_ROWS, &on_org, theirs).await,
        1
    );
    assert_eq!(
        count_rows(&pool, BLOB_ROWS, &on_org, &"b".repeat(64)).await,
        1
    );
    assert_eq!(count_rows(&pool, SKILL_ROWS, &on_org, shared).await, 1);
    assert_eq!(count_rows(&pool, BUNDLE_ROWS, &on_org, shared).await, 1);
    assert_eq!(
        count_rows(&pool, BLOB_ROWS, &on_org, &"c".repeat(64)).await,
        1
    );

    let mut tx = pool
        .begin()
        .await
        .expect("begin the second erasing transaction");
    let kept = erase_user_skills_in_tx(&mut tx, &off_org, "erased-user", false)
        .await
        .expect("the erasure with the switch off");
    tx.commit().await.expect("commit the second erasure");
    assert_eq!(
        (kept.agent_skills, kept.skill_bundles),
        (1, 0),
        "the skill goes and no bundle does"
    );
    let off_mine = &off[0];
    assert_eq!(count_rows(&pool, SKILL_ROWS, &off_org, off_mine).await, 0);
    assert_eq!(
        count_rows(&pool, BUNDLE_ROWS, &off_org, off_mine).await,
        1,
        "the bundle stays, as before"
    );
    assert_eq!(
        count_rows(&pool, BUNDLE_FILE_ROWS, &off_org, off_mine).await,
        1
    );
    assert_eq!(
        count_rows(&pool, BLOB_ROWS, &off_org, &"a".repeat(64)).await,
        1
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

/// The approval a person's own run raised on `thread`: the decision a proposal or grant names as the
/// one that created it. Returns its id. Written as production writes it: a run, then a requested
/// approval on that run (`approvals.run_id` references `runs`).
async fn requested_approval(pool: &Pool, org: &str, thread: &str, user: &str) -> String {
    let nonce = mp_ids::new_ulid();
    let run = format!("od59-run-{nonce}");
    sqlx::query(
        "INSERT INTO runs (id, thread_id, goal, org_id, user_id) VALUES ($1, $2, 'g', $3, $4)",
    )
    .bind(&run)
    .bind(thread)
    .bind(org)
    .bind(user)
    .execute(pool)
    .await
    .expect("seed run");
    let approval = format!("appr_{nonce}");
    sqlx::query(
        "INSERT INTO approvals (id, run_id, kind, status, org_id, user_id) \
         VALUES ($1, $2, 'tool_call', 'requested', $3, $4)",
    )
    .bind(&approval)
    .bind(&run)
    .bind(org)
    .bind(user)
    .execute(pool)
    .await
    .expect("seed approval");
    approval
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
    for user in ["erased-user", "other-user"] {
        let thread = format!("od59-auth-t-{user}-{nonce}");
        personal_thread(&pool, &org, &thread, user).await;
        // Each proposal and grant names the approval the person's own run raised: both reference
        // approvals (migrations 0062 and 0070), so the approval is seeded first.
        let approval = requested_approval(&pool, &org, &thread, user).await;
        let proposal = format!("arp_{}", mp_ids::new_ulid());
        sqlx::query(
            "INSERT INTO approval_rule_proposals \
             (id, org_id, user_id, created_from_approval_id, tool_name, tool_digest, capability_id) \
             VALUES ($1, $2, $3, $4, 'mcp__fixture__create_note', repeat('a', 64), '')",
        )
        .bind(&proposal)
        .bind(&org)
        .bind(user)
        .bind(&approval)
        .execute(&pool)
        .await
        .expect("seed proposal");
        sqlx::query(
            "INSERT INTO approval_scope_grants \
             (id, org_id, user_id, thread_id, run_id, scope, tool_name, action_key, effect_class, \
              cause_code, created_from_approval_id, expires_at, rule_id, tool_digest) \
             VALUES ($1, $2, $3, $4, NULL, 'thread', 'knowledge_search', 'knowledge_search', 'read', \
                     'consent_to_disclose', $5, now() + interval '1 hour', $6, '')",
        )
        .bind(format!("apg_{}", mp_ids::new_ulid()))
        .bind(&org)
        .bind(user)
        .bind(&thread)
        .bind(&approval)
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
    // Grants and proposals reference approvals, approvals reference runs, and runs reference threads,
    // so they go in that order.
    for table in [
        "approval_scope_grants",
        "approval_rule_proposals",
        "approvals",
        "runs",
        "threads",
    ] {
        sqlx::query(&format!("DELETE FROM {table} WHERE org_id = $1"))
            .bind(&org)
            .execute(&pool)
            .await
            .ok();
    }
}

/// The rows of `table` in `org` whose owner is `owner`.
async fn owned_rows(pool: &Pool, table: &str, org: &str, owner: &str) -> i64 {
    sqlx::query_scalar(&format!(
        "SELECT count(*) FROM {table} WHERE org_id = $1 AND owner_user_id = $2"
    ))
    .bind(org)
    .bind(owner)
    .fetch_one(pool)
    .await
    .expect("count the owned rows")
}

#[tokio::test]
#[ignore = "requires DATABASE_URL to a disposable Postgres whose name contains \"test\""]
async fn a_person_erasure_removes_their_own_skills_their_versions_and_their_revisions() {
    use crate::space::test_support;
    let Some(pool) = test_support::connect().await else {
        return;
    };
    let nonce = mp_ids::new_ulid();
    let org = format!("f01-16-org-{nonce}");
    let (mine, theirs, shared) = (
        format!("f01-16-mine-{nonce}"),
        format!("f01-16-theirs-{nonce}"),
        format!("f01-16-shared-{nonce}"),
    );
    for (id, name, scope, owner) in [
        (&mine, "mine", "user", "erased-user"),
        (&theirs, "theirs", "user", "other-user"),
        (&shared, "shared", "org", ""),
    ] {
        sqlx::query(
            "INSERT INTO agent_skills (id, org_id, name, content, scope, owner_user_id, created_at, updated_at) \
             VALUES ($1, $2, $3, 'the skill text', $4, $5, now(), now())",
        )
        .bind(id)
        .bind(&org)
        .bind(name)
        .bind(scope)
        .bind(owner)
        .execute(&pool)
        .await
        .expect("seed skill");
    }
    // An edit writes a further version and revision of the person's text.
    sqlx::query("UPDATE agent_skills SET content = 'the edited skill text' WHERE id = $1")
        .bind(&mine)
        .execute(&pool)
        .await
        .expect("edit the personal skill");

    // Counted from the rows themselves, so the assertions hold whatever the triggers wrote.
    let before_versions = owned_rows(&pool, "agent_skill_versions", &org, "erased-user").await;
    let before_revisions = owned_rows(&pool, "skill_revisions", &org, "erased-user").await;
    assert!(before_versions >= 1 && before_revisions >= 1, "the person's text is versioned and revised");

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
    assert_eq!(
        summary.agent_skills, 1,
        "the person's one personal skill row"
    );
    assert_eq!(
        summary.skill_revisions, before_revisions as u64,
        "every revision of their text"
    );
    assert_eq!(
        summary.agent_skill_versions, before_versions as u64,
        "every version of their text"
    );
    assert_eq!(
        owned_rows(&pool, "agent_skills", &org, "erased-user").await,
        0
    );
    assert_eq!(
        owned_rows(&pool, "skill_revisions", &org, "erased-user").await,
        0,
        "the deleted revision that holds the text goes too"
    );
    assert_eq!(
        owned_rows(&pool, "agent_skill_versions", &org, "erased-user").await,
        0
    );

    // Another member's skill and the org skill survive, with their own history.
    assert_eq!(
        owned_rows(&pool, "agent_skills", &org, "other-user").await,
        1
    );
    assert!(owned_rows(&pool, "skill_revisions", &org, "other-user").await >= 1);
    let (shared_rows,): (i64,) =
        sqlx::query_as("SELECT count(*) FROM agent_skills WHERE org_id = $1 AND id = $2")
            .bind(&org)
            .bind(&shared)
            .fetch_one(&pool)
            .await
            .expect("count the org skill");
    assert_eq!(shared_rows, 1, "the org skill is not the person's to erase");

    // The audit row counts the three removals and holds no text.
    let details: Vec<serde_json::Value> = sqlx::query_scalar(
        "SELECT detail FROM memory_control_events WHERE org_id = $1 AND user_id = $2 AND event = 'erased'",
    )
    .bind(&org)
    .bind("erased-user")
    .fetch_all(&pool)
    .await
    .expect("the audit rows");
    assert!(
        details.iter().any(|detail| {
            detail["agent_skills"] == 1
                && detail["skill_revisions"] == before_revisions
                && detail["agent_skill_versions"] == before_versions
        }),
        "the audit row counts the skill removals"
    );
    assert!(
        details
            .iter()
            .all(|detail| !detail.to_string().contains("skill text")),
        "the audit row holds no skill text"
    );

    // Redelivery finds nothing left to remove.
    let again = erase_user_personal_data_for(
        &pool,
        &org,
        "erased-user",
        UserErasureKind::Erase,
        &CheckedThreads::none(),
        None,
    )
    .await
    .expect("redelivered erasure");
    assert_eq!(
        (
            again.agent_skills,
            again.skill_revisions,
            again.agent_skill_versions
        ),
        (0, 0, 0),
        "a second run deletes nothing"
    );
}
