//! EP-11 S5: the draft beside a skill's live text (migration 0110).
//!
//! The source tests pin what the migration must keep from 0109: the revision snapshot it
//! replaces names every key 0109 named, plus a `has_draft` boolean and never the draft text. The
//! ignored tests run against a Postgres with session-core's migrations (scripts/tests/
//! session-core-db-tests.sh) and prove the two promises capability-core's draft routes rely on:
//! a draft write cuts no version, and capability-core's publish statement cuts exactly one,
//! attributed to the person who published.

const MIGRATION: &str = include_str!("../migrations/0110_agent_skill_drafts.sql");
const KIND_MIGRATION: &str = include_str!("../migrations/0109_authored_behaviour_kind.sql");
const REVERT: &str = include_str!("../migrations/revert/0110_agent_skill_drafts.down.sql");

/// The keys of the first `jsonb_build_object(` after `function` in `sql`, in order.
fn snapshot_keys(sql: &str, function: &str) -> Vec<String> {
    let start = sql.find(function).expect("the snapshot function");
    let body = &sql[start..];
    let body = &body[body.find("jsonb_build_object(").expect("the snapshot object")..];
    let body = &body[..body.find(");").expect("the object's end")];
    body.lines()
        .filter_map(|line| line.trim().strip_prefix('\''))
        .filter_map(|line| line.split('\'').next())
        .map(str::to_owned)
        .collect()
}

const SNAPSHOT_FN: &str = "CREATE OR REPLACE FUNCTION agent_skill_revision_snapshot(s agent_skills)";

#[test]
fn draft_snapshot_keeps_every_0109_key_and_adds_only_has_draft() {
    let before = snapshot_keys(KIND_MIGRATION, SNAPSHOT_FN);
    let after = snapshot_keys(MIGRATION, SNAPSHOT_FN);
    let mut expected = before.clone();
    expected.push("has_draft".to_owned());
    assert_eq!(after, expected, "0110 must extend 0109's snapshot, not reorder or drop a key");
    assert!(
        MIGRATION.contains("'has_draft', s.draft IS NOT NULL"),
        "a revision carries only whether a draft exists"
    );
    assert!(
        !MIGRATION.contains("'draft', s.draft"),
        "the draft text must stay out of revisions, which keep 50 entries of live text"
    );
}

#[test]
fn draft_revert_restores_the_0109_snapshot() {
    assert_eq!(
        snapshot_keys(REVERT, SNAPSHOT_FN),
        snapshot_keys(KIND_MIGRATION, SNAPSHOT_FN),
        "the revert must put back exactly 0109's snapshot"
    );
    let replaced = REVERT.find(SNAPSHOT_FN).expect("the snapshot function");
    let dropped = REVERT.find("DROP COLUMN IF EXISTS draft").expect("the column drop");
    assert!(
        replaced < dropped,
        "the snapshot must stop naming draft before the column goes"
    );
}

#[test]
fn draft_columns_never_feed_the_version_digest() {
    // 0108/0109's body function is the digest's only input; 0110 must not replace it.
    assert!(!MIGRATION.contains("agent_skill_version_body(s agent_skills)"));
    assert!(!MIGRATION.contains("agent_skill_digest"));
}

#[cfg(test)]
mod db {
    /// A pool on a migrated database, or `None` (the test then skips) without DATABASE_URL.
    async fn pool() -> Option<sqlx::PgPool> {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping: DATABASE_URL unset");
            return None;
        };
        let pool = sqlx::PgPool::connect(&url).await.expect("connect pg");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrate");
        Some(pool)
    }

    /// Runs `statement` as capability-core's registry does: one transaction that first names the
    /// person as the actor, with the settings the revision and version triggers read.
    async fn as_person(pool: &sqlx::PgPool, actor: &str, statement: &str, binds: &[&str]) {
        let mut tx = pool.begin().await.expect("begin");
        sqlx::query(
            "SELECT set_config('mp.actor_kind', 'user', true), set_config('mp.actor_id', $1, true)",
        )
        .bind(actor)
        .execute(&mut *tx)
        .await
        .expect("name the actor");
        let mut query = sqlx::query(statement);
        for value in binds {
            query = query.bind(*value);
        }
        query.execute(&mut *tx).await.expect("registry statement");
        tx.commit().await.expect("commit");
    }

    async fn accepted_skill(pool: &sqlx::PgPool) -> (String, String) {
        let org = format!("skd-{}", mp_ids::new_ulid());
        let id = format!("sk-{}", mp_ids::new_ulid());
        as_person(
            pool,
            "admin-1",
            "INSERT INTO agent_skills (id, org_id, name, content, scope, owner_user_id, created_at, updated_at)
             VALUES ($1, $2, 'Drafted', 'live body', 'org', '', now(), now())",
            &[&id, &org],
        )
        .await;
        (id, org)
    }

    async fn version_and_content(pool: &sqlx::PgPool, id: &str) -> (i32, String) {
        sqlx::query_as("SELECT version, content FROM agent_skills WHERE id = $1")
            .bind(id)
            .fetch_one(pool)
            .await
            .expect("the skill")
    }

    /// The draft write capability-core's PATCH makes while SKILL_DRAFT_PUBLISH is on.
    const DRAFT_WRITE: &str = "UPDATE agent_skills
        SET draft = COALESCE(draft, agent_skill_version_body(agent_skills)) || jsonb_build_object('content', $3::text),
            draft_updated_at = now(), draft_by = 'admin-1'
        WHERE id = $1 AND org_id = $2";

    /// The live columns capability-core's publish copies out of the draft, clearing it.
    const PUBLISH: &str = "UPDATE agent_skills
        SET name = COALESCE(draft->>'name', name),
            description = COALESCE(draft->>'description', description),
            content = COALESCE(draft->>'content', content),
            trigger_keywords = COALESCE(draft->'trigger_keywords', trigger_keywords),
            trigger_file_patterns = COALESCE(draft->'trigger_file_patterns', trigger_file_patterns),
            tool_restrictions = COALESCE(draft->'tool_restrictions', tool_restrictions),
            settings = COALESCE(draft->'settings', settings),
            draft = NULL, draft_updated_at = NULL, draft_by = '', updated_at = now()
        WHERE id = $1 AND org_id = $2 AND review_state = 'accepted' AND draft IS NOT NULL";

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a Postgres with session-core migrations"]
    async fn draft_edit_cuts_no_version() {
        let Some(pool) = pool().await else {
            return;
        };
        let (id, org) = accepted_skill(&pool).await;
        assert_eq!(version_and_content(&pool, &id).await, (1, "live body".to_owned()));
        as_person(&pool, "admin-1", DRAFT_WRITE, &[&id, &org, "draft one"]).await;
        as_person(&pool, "admin-1", DRAFT_WRITE, &[&id, &org, "draft two"]).await;
        assert_eq!(
            version_and_content(&pool, &id).await,
            (1, "live body".to_owned()),
            "a draft must leave the live text and the version alone"
        );
        let versions: i64 =
            sqlx::query_scalar("SELECT count(*) FROM agent_skill_versions WHERE skill_id = $1")
                .bind(&id)
                .fetch_one(&pool)
                .await
                .expect("count versions");
        assert_eq!(versions, 1);
        // Starting the draft is one `edited` revision; saving over it records nothing.
        let drafted: Vec<(String, bool)> = sqlx::query_as(
            "SELECT action, COALESCE((after->>'has_draft')::boolean, false)
             FROM skill_revisions WHERE skill_id = $1 ORDER BY revision",
        )
        .bind(&id)
        .fetch_all(&pool)
        .await
        .expect("revisions");
        assert_eq!(
            drafted,
            vec![("created".to_owned(), false), ("edited".to_owned(), true)]
        );
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a Postgres with session-core migrations"]
    async fn publish_from_capability_core_sql_cuts_version_with_actor() {
        let Some(pool) = pool().await else {
            return;
        };
        let (id, org) = accepted_skill(&pool).await;
        as_person(&pool, "admin-1", DRAFT_WRITE, &[&id, &org, "published body"]).await;
        as_person(&pool, "member-2", PUBLISH, &[&id, &org]).await;
        assert_eq!(
            version_and_content(&pool, &id).await,
            (2, "published body".to_owned())
        );
        let (actor_kind, actor_id, content): (String, String, String) = sqlx::query_as(
            "SELECT actor_kind, actor_id, body->>'content' FROM agent_skill_versions
             WHERE skill_id = $1 AND version = 2",
        )
        .bind(&id)
        .fetch_one(&pool)
        .await
        .expect("version 2");
        assert_eq!(
            (actor_kind.as_str(), actor_id.as_str(), content.as_str()),
            ("user", "member-2", "published body")
        );
        let draft_left: bool =
            sqlx::query_scalar("SELECT draft IS NOT NULL FROM agent_skills WHERE id = $1")
                .bind(&id)
                .fetch_one(&pool)
                .await
                .expect("the skill");
        assert!(!draft_left, "publish clears the draft in the same statement");
        // A second publish has no draft to copy and cuts nothing.
        as_person(&pool, "member-2", PUBLISH, &[&id, &org]).await;
        assert_eq!(version_and_content(&pool, &id).await.0, 2);
    }
}
