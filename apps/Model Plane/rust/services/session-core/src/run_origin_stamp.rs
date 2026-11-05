//! The origin a run is stamped with (WS20 S2, EP-12 S1).
//!
//! A run's origin comes from the verified caller, never from a request field
//! (DEC-17, EP-12 D1). A person's run is stamped `chat`. A system owner's run
//! carries no stamp here, because a scheduled run is stamped by its own binding.
//! Behind `SESSION_RUN_ORIGIN_INHERIT`, a child run also inherits its parent's
//! token, so a subagent of an API run is an API run and a subagent of a
//! scheduled run is a scheduled run. The parent is read on the transaction that
//! inserts the child, in the child's org, and a stamp outside the closed token
//! set is never copied. With the switch off, every run is stamped as it always
//! was.
//!
//! EP-12 S4: a run an `api_key` principal starts is stamped `api`, naming the
//! key and the person who owns it. The key owns the run (D2); the person is
//! named here as attribution only, never as an owner.

use mp_contracts::run_origin::{RunOrigin, ORIGIN_METADATA_KEY};
use tonic::Status;

use crate::auth::VerifiedIdentity;

/// The switch that makes a child run inherit its parent's origin. Default off.
pub(crate) const INHERIT_ENV: &str = "SESSION_RUN_ORIGIN_INHERIT";

/// Whether the inherit switch is on. The repo's strict parser reads it: only the
/// literal `true`, in any case and with surrounding whitespace, enables it.
#[must_use]
pub(crate) fn inherit_enabled(raw: Option<&str>) -> bool {
    mp_contracts::loop_fold::switch_from(raw)
}

/// The `runs.metadata` key naming the API key that started a run.
pub(crate) const API_KEY_ID_METADATA_KEY: &str = "api_key_id";
/// The `runs.metadata` key naming the person who owns that key. Attribution only.
pub(crate) const API_KEY_OWNER_METADATA_KEY: &str = "api_key_owner";

/// The API key that starts a run, as the run's stamp names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ApiKeyStamp<'a> {
    key_id: &'a str,
    owner: &'a str,
}

impl<'a> ApiKeyStamp<'a> {
    /// The stamp of a verified `api_key` caller; `None` for every other caller.
    #[must_use]
    pub(crate) fn of(caller: &'a VerifiedIdentity) -> Option<Self> {
        Some(Self {
            key_id: caller.api_key_id()?,
            owner: caller.acting_user_id()?,
        })
    }
}

/// The `runs.metadata` stamp of a new run. A run that inherits an origin carries
/// that token, and a run an API key starts is otherwise stamped `api`. Any other
/// run keeps the stamp it has always had: `chat` for a person, and none for a
/// system owner. A key's run names the key and its owner whichever token it
/// carries, so an inherited token still wins without losing who started the run.
#[must_use]
pub(crate) fn origin_at_birth(
    user_id: &str,
    inherited: Option<RunOrigin>,
    api_key: Option<ApiKeyStamp<'_>>,
) -> serde_json::Value {
    let mut stamp = match (inherited, api_key) {
        (Some(origin), _) => serde_json::json!({ ORIGIN_METADATA_KEY: origin.as_str() }),
        (None, Some(_)) => serde_json::json!({ ORIGIN_METADATA_KEY: RunOrigin::Api.as_str() }),
        (None, None) if crate::auth::is_system_run_owner(user_id) => serde_json::json!({}),
        (None, None) => serde_json::json!({ ORIGIN_METADATA_KEY: RunOrigin::Chat.as_str() }),
    };
    if let (Some(key), Some(fields)) = (api_key, stamp.as_object_mut()) {
        fields.insert(API_KEY_ID_METADATA_KEY.to_owned(), key.key_id.into());
        fields.insert(API_KEY_OWNER_METADATA_KEY.to_owned(), key.owner.into());
    }
    stamp
}

/// The origin token a child run inherits from `parent_run_id`, or `None` when
/// nothing is inherited: the switch is off, the run has no parent, the parent is
/// not a run of `org_id`, or its stamp is not one of the closed tokens. The read
/// is on the caller's transaction, so it sees the parent that the receipt read
/// has just verified.
///
/// # Errors
///
/// `internal` when the lookup fails. A child whose parent cannot be read is not
/// started, rather than started under a stamp it did not inherit.
#[allow(clippy::result_large_err)]
pub(crate) async fn inherited_origin(
    connection: &mut sqlx::PgConnection,
    org_id: &str,
    parent_run_id: &str,
    inherit: bool,
) -> Result<Option<RunOrigin>, Status> {
    if !inherit || parent_run_id.trim().is_empty() {
        return Ok(None);
    }
    let parent: Option<(serde_json::Value,)> = sqlx::query_as(
        "SELECT COALESCE(metadata, '{}'::jsonb) FROM runs WHERE id = $1 AND org_id = $2",
    )
    .bind(parent_run_id)
    .bind(org_id)
    .fetch_optional(connection)
    .await
    .map_err(|error| {
        tracing::error!(%error, "run origin parent lookup failed");
        Status::internal("run origin lookup failed")
    })?;
    Ok(parent.and_then(|(metadata,)| RunOrigin::from_metadata(&metadata)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mp_ids::new_ulid;
    use serde_json::json;
    use sqlx::PgPool;

    /// The scratch Postgres these tests write to. Without `DATABASE_URL` they skip
    /// locally; when `REQUIRE_CONTRACT` or `CI` is set they fail instead. The
    /// database name must say it is a test database, because these tests write rows.
    async fn scratch_pool() -> Option<PgPool> {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            assert!(
                std::env::var_os("REQUIRE_CONTRACT").is_none() && std::env::var_os("CI").is_none(),
                "DATABASE_URL must name a scratch Postgres when REQUIRE_CONTRACT or CI is set"
            );
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
            "these tests write rows: DATABASE_URL must name a scratch database whose name contains \"test\""
        );
        let pool = PgPool::connect(&url)
            .await
            .expect("scratch Postgres is reachable");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("session-core migrations apply in order");
        Some(pool)
    }

    /// A thread of `org` owned by `user`, and a persistent run on it whose stored
    /// metadata is `metadata`. Returns the run id.
    async fn run_with_metadata(
        pool: &PgPool,
        org: &str,
        user: &str,
        metadata: serde_json::Value,
    ) -> String {
        let thread = new_ulid();
        sqlx::query(
            "INSERT INTO threads (id, session_key, org_id, user_id, created_at)
             VALUES ($1, $2, $3, $4, now())",
        )
        .bind(&thread)
        .bind(format!("origin-{thread}"))
        .bind(org)
        .bind(user)
        .execute(pool)
        .await
        .expect("thread seeded");
        let run = new_ulid();
        sqlx::query(
            "INSERT INTO runs (id, thread_id, goal, org_id, user_id, retention_class, metadata)
             VALUES ($1, $2, 'origin goal', $3, $4, 'persistent', $5)",
        )
        .bind(&run)
        .bind(&thread)
        .bind(org)
        .bind(user)
        .bind(metadata)
        .execute(pool)
        .await
        .expect("run seeded");
        run
    }

    /// Against Postgres: a child of an API parent inherits `api` when the switch is
    /// on, and nothing is copied when it is off, when the parent's stamp is outside
    /// the closed set, or when the parent is in another org.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres whose name contains \"test\" (session-core receipts CI job)"]
    async fn api_parent_child_run_is_stamped_api() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let org = format!("origin-org-{}", new_ulid());
        let user = format!("origin-user-{}", new_ulid());
        let api_parent = run_with_metadata(&pool, &org, &user, json!({"origin": "api"})).await;
        let unknown_parent =
            run_with_metadata(&pool, &org, &user, json!({"origin": "sheduled"})).await;
        let mut connection = pool.acquire().await.expect("a pooled connection");

        let inherited = inherited_origin(&mut connection, &org, &api_parent, true)
            .await
            .expect("the parent is read");
        assert_eq!(inherited, Some(RunOrigin::Api));
        assert_eq!(
            origin_at_birth(&user, inherited, None),
            json!({"origin": "api"})
        );

        let switched_off = inherited_origin(&mut connection, &org, &api_parent, false)
            .await
            .expect("no read is an answer, not an error");
        assert_eq!(switched_off, None);
        assert_eq!(
            origin_at_birth(&user, switched_off, None),
            json!({"origin": "chat"})
        );

        let unknown = inherited_origin(&mut connection, &org, &unknown_parent, true)
            .await
            .expect("the parent is read");
        assert_eq!(unknown, None);

        let other_org = format!("origin-other-{}", new_ulid());
        let foreign = inherited_origin(&mut connection, &other_org, &api_parent, true)
            .await
            .expect("a parent outside the org is an answer, not an error");
        assert_eq!(foreign, None);
    }

    #[test]
    fn child_inherits_parent_origin_when_switch_on() {
        // An inherited token is stamped whoever owns the child.
        assert_eq!(
            origin_at_birth("user-123", Some(RunOrigin::Api), None),
            json!({"origin": "api"})
        );
        assert_eq!(
            origin_at_birth("service:orchestrator-core", Some(RunOrigin::Schedule), None),
            json!({"origin": "schedule"})
        );
    }

    #[test]
    fn switch_off_stamp_is_todays_stamp() {
        // Without an inherited token, a run is stamped exactly as before EP-12 S1.
        assert_eq!(
            origin_at_birth("user-123", None, None),
            json!({"origin": "chat"})
        );
        assert_eq!(
            origin_at_birth("service:orchestrator-core", None, None),
            json!({})
        );
    }

    #[test]
    fn unknown_parent_token_is_not_copied() {
        // A stamp outside the closed set reads as no origin, so the child keeps its
        // caller's own stamp.
        let unknown = RunOrigin::from_metadata(&json!({"origin": "sheduled"}));
        assert_eq!(unknown, None);
        assert_eq!(
            origin_at_birth("user-123", unknown, None),
            json!({"origin": "chat"})
        );
    }

    #[test]
    fn system_owner_without_parent_stays_unstamped() {
        assert_eq!(
            origin_at_birth("service:orchestrator-core", None, None),
            json!({})
        );
    }

    #[test]
    fn api_key_run_is_stamped_api_with_attribution() {
        let key = VerifiedIdentity::api_key_for_test("org-1", "key-1", "user-1", false);
        let stamp = ApiKeyStamp::of(&key);
        assert_eq!(
            stamp,
            Some(ApiKeyStamp {
                key_id: "key-1",
                owner: "user-1"
            })
        );
        assert_eq!(
            origin_at_birth("apikey:key-1", None, stamp),
            json!({"origin": "api", "api_key_id": "key-1", "api_key_owner": "user-1"})
        );
        // An inherited token still wins for a child run, and the key stays named.
        assert_eq!(
            origin_at_birth("apikey:key-1", Some(RunOrigin::Schedule), stamp),
            json!({"origin": "schedule", "api_key_id": "key-1", "api_key_owner": "user-1"})
        );
        // A person and a service are not keys: their stamps are unchanged.
        let person = VerifiedIdentity::user_for_test("org-1", "user-1");
        let service = VerifiedIdentity::service_for_test("org-1", &[], false);
        assert_eq!(ApiKeyStamp::of(&person), None);
        assert_eq!(ApiKeyStamp::of(&service), None);
        assert_eq!(
            origin_at_birth("user-1", None, ApiKeyStamp::of(&person)),
            json!({"origin": "chat"})
        );
    }

    #[test]
    fn inherit_parser_takes_only_literal_true() {
        for raw in ["true", " true ", "TRUE"] {
            assert!(inherit_enabled(Some(raw)), "{raw:?}");
        }
        for raw in [
            None,
            Some(""),
            Some("1"),
            Some("on"),
            Some("yes"),
            Some("false"),
            Some("tru"),
        ] {
            assert!(!inherit_enabled(raw), "{raw:?}");
        }
    }
}
