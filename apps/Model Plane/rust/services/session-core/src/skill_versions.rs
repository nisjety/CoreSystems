//! `SkillVersionService`: reads of immutable skill versions (Wave 3 EP-11 S2).
//!
//! Migration 0108 cuts a numbered version with a digest every time the digest of an accepted
//! `agent_skills` row changes, by trigger, so session-core's RPCs and capability-core's registry SQL
//! are covered alike and neither writes a version itself (EP-11 D10). This module only reads them:
//! the gateway asks for the body of a version a run pinned (EP-11 S3), and capability-core serves
//! the Skills page's history from the same table over HTTP.
//!
//! Visibility is `list_agent_skills`'s rule: org skills, the reader's own, and accepted skills
//! shared with the reader. A service caller reads as the user it names in `for_user_id`, and as no
//! user otherwise; a user caller always reads as itself. Skill text is org configuration, not
//! conversation content, so a zero-retention credential may read it as it may list skills. The
//! switch is [`ENABLED_ENV`], default off; while it is off every RPC answers `unavailable`
//! `skill_versions_disabled`.

use chrono::{DateTime, Utc};
use mp_contracts::model_plane::v1::{
    self as pb, skill_version_service_server::SkillVersionService,
};
use serde::Deserialize;
use sqlx::PgPool;
use std::time::Instant;
use tonic::{Request, Response, Status};

use crate::auth::{authorize_operation, identity, VerifiedIdentity};
use crate::grpc::record_metrics;

/// The environment switch. capability-core reads the same name for its HTTP history route, so one
/// setting turns the version reads on for both. Default off; only the literal `true` turns it on,
/// which is also how capability-core reads it, so the two services cannot disagree on a value.
pub(crate) const ENABLED_ENV: &str = "SKILL_VERSIONS_API";

const DISABLED: &str = "skill_versions_disabled";
/// A listing's page when the caller names none, and the most one page returns.
const DEFAULT_LIST_LIMIT: i32 = 20;
const MAX_LIST_LIMIT: i32 = 100;
/// The longest skill id accepted; capability-core's ids are `skill_` and a UUID.
const MAX_SKILL_ID_CHARS: usize = 128;

/// The visibility predicate of `list_agent_skills`, over `agent_skills` with `$1` the org, `$2` the
/// skill and `$3` the reader. A proposal stays private to its owner even if a share was written on it.
const VISIBLE_SKILL_SQL: &str = "SELECT EXISTS (
        SELECT 1 FROM agent_skills
        WHERE id = $2 AND org_id = $1
          AND (scope = 'org' OR owner_user_id = $3
               OR (shared_with ? $3 AND review_state = 'accepted')))";

/// Read where it is used: the RPCs hold no switch of their own.
pub(crate) fn skill_versions_enabled() -> bool {
    enabled_from(std::env::var(ENABLED_ENV).ok().as_deref())
}

/// The switch's reading, on the value alone. Exact, as capability-core's `SkillVersionsAPIFromLookup`
/// is: no trimming and no case folding, so `TRUE` and ` true ` leave the version reads off.
fn enabled_from(value: Option<&str>) -> bool {
    value == Some("true")
}

/// The version RPCs. They hold the pool and nothing else.
pub struct SkillVersionGrpc {
    pub(crate) pool: PgPool,
}

#[tonic::async_trait]
impl SkillVersionService for SkillVersionGrpc {
    async fn list_agent_skill_versions(
        &self,
        request: Request<pb::ListAgentSkillVersionsRequest>,
    ) -> Result<Response<pb::ListAgentSkillVersionsResponse>, Status> {
        let started = Instant::now();
        let result = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "session:read")?;
            if !skill_versions_enabled() {
                return Err(Status::unavailable(DISABLED));
            }
            list_versions(&self.pool, &caller, request.into_inner())
                .await
                .map(Response::new)
        }
        .await;
        record_metrics("list_agent_skill_versions", started, result.is_ok());
        result
    }

    async fn get_agent_skill_version(
        &self,
        request: Request<pb::GetAgentSkillVersionRequest>,
    ) -> Result<Response<pb::GetAgentSkillVersionResponse>, Status> {
        let started = Instant::now();
        let result = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "session:read")?;
            if !skill_versions_enabled() {
                return Err(Status::unavailable(DISABLED));
            }
            get_version(&self.pool, &caller, request.into_inner())
                .await
                .map(Response::new)
        }
        .await;
        record_metrics("get_agent_skill_version", started, result.is_ok());
        result
    }
}

/// Who the read is for: a service caller reads as the user it names, a user caller as itself, so
/// `for_user_id` can never be used to read another person's private skill.
fn reader_id(caller: &VerifiedIdentity, for_user_id: &str) -> String {
    if caller.is_service() && !for_user_id.trim().is_empty() {
        for_user_id.trim().to_owned()
    } else {
        caller.user_id().unwrap_or_default().to_owned()
    }
}

#[allow(clippy::result_large_err)]
fn check_skill_ref(caller: &VerifiedIdentity, org_id: &str, skill_id: &str) -> Result<(), Status> {
    caller.authorize_org(org_id)?;
    let skill_id = skill_id.trim();
    if skill_id.is_empty() || skill_id.chars().count() > MAX_SKILL_ID_CHARS {
        return Err(Status::invalid_argument("skill_id is required"));
    }
    Ok(())
}

async fn skill_visible(
    pool: &PgPool,
    org_id: &str,
    skill_id: &str,
    reader: &str,
) -> Result<bool, Status> {
    sqlx::query_scalar(VISIBLE_SKILL_SQL)
        .bind(org_id)
        .bind(skill_id)
        .bind(reader)
        .fetch_one(pool)
        .await
        .map_err(internal)
}

/// The versioned fields as migration 0108's `agent_skill_version_body` writes them. Every field
/// defaults, so a body written before a later migration adds a field still reads.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct VersionBody {
    name: String,
    description: String,
    content: String,
    trigger_keywords: Vec<String>,
    trigger_file_patterns: Vec<String>,
    tool_restrictions: Vec<String>,
}

#[derive(sqlx::FromRow)]
struct VersionRow {
    skill_id: String,
    version: i32,
    digest: String,
    body: sqlx::types::Json<VersionBody>,
    bundle_sha256: String,
    actor_kind: String,
    actor_id: String,
    created_at: DateTime<Utc>,
}

impl From<VersionRow> for pb::AgentSkillVersion {
    fn from(row: VersionRow) -> Self {
        let body = row.body.0;
        Self {
            skill_id: row.skill_id,
            version: row.version,
            digest: row.digest,
            name: body.name,
            description: body.description,
            content: body.content,
            trigger_keywords: body.trigger_keywords,
            trigger_file_patterns: body.trigger_file_patterns,
            tool_restrictions: body.tool_restrictions,
            bundle_sha256: row.bundle_sha256,
            actor_kind: row.actor_kind,
            actor_id: row.actor_id,
            created_at: row.created_at.to_rfc3339(),
        }
    }
}

/// The columns every read returns. `$4` says whether the body keeps its text.
const VERSION_COLUMNS: &str = "skill_id, version, digest,
        CASE WHEN $4 THEN body ELSE body - 'content' END AS body,
        bundle_sha256, actor_kind, actor_id, created_at";

pub(crate) async fn list_versions(
    pool: &PgPool,
    caller: &VerifiedIdentity,
    request: pb::ListAgentSkillVersionsRequest,
) -> Result<pb::ListAgentSkillVersionsResponse, Status> {
    check_skill_ref(caller, &request.org_id, &request.skill_id)?;
    if request.limit < 0 || request.before_version < 0 {
        return Err(Status::invalid_argument(
            "limit and before_version must not be negative",
        ));
    }
    let skill_id = request.skill_id.trim();
    let reader = reader_id(caller, &request.for_user_id);
    if !skill_visible(pool, &request.org_id, skill_id, &reader).await? {
        return Err(Status::not_found("skill not found"));
    }
    let limit = match request.limit {
        0 => DEFAULT_LIST_LIMIT,
        n => n.min(MAX_LIST_LIMIT),
    };
    // One more than the page, to say whether older versions remain.
    let mut rows: Vec<VersionRow> = sqlx::query_as(&format!(
        "SELECT {VERSION_COLUMNS}
         FROM agent_skill_versions
         WHERE org_id = $1 AND skill_id = $2 AND ($3 = 0 OR version < $3)
         ORDER BY version DESC
         LIMIT $5"
    ))
    .bind(&request.org_id)
    .bind(skill_id)
    .bind(request.before_version)
    .bind(request.include_content)
    .bind(i64::from(limit) + 1)
    .fetch_all(pool)
    .await
    .map_err(internal)?;
    let has_more = rows.len() > usize::try_from(limit).unwrap_or(usize::MAX);
    rows.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
    Ok(pb::ListAgentSkillVersionsResponse {
        versions: rows.into_iter().map(Into::into).collect(),
        has_more,
    })
}

pub(crate) async fn get_version(
    pool: &PgPool,
    caller: &VerifiedIdentity,
    request: pb::GetAgentSkillVersionRequest,
) -> Result<pb::GetAgentSkillVersionResponse, Status> {
    check_skill_ref(caller, &request.org_id, &request.skill_id)?;
    if request.version < 0 {
        return Err(Status::invalid_argument("version must not be negative"));
    }
    let skill_id = request.skill_id.trim();
    let reader = reader_id(caller, &request.for_user_id);
    // A skill the reader cannot see reads exactly as one that is gone, so the answer says nothing
    // about another person's private skill.
    if !skill_visible(pool, &request.org_id, skill_id, &reader).await? {
        return Ok(gone());
    }
    let row: Option<VersionRow> = sqlx::query_as(&format!(
        "SELECT {VERSION_COLUMNS}
         FROM agent_skill_versions
         WHERE org_id = $1 AND skill_id = $2 AND ($3 = 0 OR version = $3)
         ORDER BY version DESC
         LIMIT 1"
    ))
    .bind(&request.org_id)
    .bind(skill_id)
    .bind(request.version)
    .bind(true)
    .fetch_optional(pool)
    .await
    .map_err(internal)?;
    Ok(
        row.map_or_else(gone, |row| pb::GetAgentSkillVersionResponse {
            version: Some(row.into()),
            gone: false,
        }),
    )
}

fn gone() -> pb::GetAgentSkillVersionResponse {
    pb::GetAgentSkillVersionResponse {
        version: None,
        gone: true,
    }
}

fn internal(error: sqlx::Error) -> Status {
    tracing::warn!(%error, "skill version read failed");
    Status::internal("skill version read failed")
}

#[cfg(test)]
mod tests {
    use super::*;
    use mp_ids::new_ulid;

    const OWNER: &str = "user-skv-owner";
    const SHAREE: &str = "user-skv-sharee";
    const STRANGER: &str = "user-skv-stranger";

    #[test]
    fn the_switch_is_off_unless_it_reads_true() {
        assert!(!enabled_from(None));
        assert!(!enabled_from(Some("")));
        assert!(!enabled_from(Some("1")));
        assert!(!enabled_from(Some("TRUE")));
        assert!(!enabled_from(Some(" true ")));
        assert!(enabled_from(Some("true")));
    }

    #[test]
    fn reader_is_the_caller_unless_a_service_names_a_user() {
        let user = VerifiedIdentity::user_for_test("org-1", OWNER);
        assert_eq!(
            reader_id(&user, STRANGER),
            OWNER,
            "a user never reads as someone else"
        );
        let service = VerifiedIdentity::service_for_test("org-1", &["session:read"], false);
        assert_eq!(reader_id(&service, &format!(" {SHAREE} ")), SHAREE);
        assert_eq!(
            reader_id(&service, ""),
            "",
            "a service naming no one sees only org skills"
        );
    }

    /// EP-11 D2: the digest covers exactly these fields, in this order. A change here moves every
    /// digest, so it must come with a migration that replaces `agent_skill_version_body` and says so.
    #[test]
    fn digest_field_list_is_pinned() {
        let migration = include_str!("../migrations/0108_agent_skill_versions.sql");
        let start = migration
            .find("CREATE OR REPLACE FUNCTION agent_skill_version_body(s agent_skills)")
            .expect("the body function");
        let end = start
            + migration[start..]
                .find("$$ LANGUAGE sql")
                .expect("the body function's end");
        let keys: Vec<&str> = migration[start..end]
            .lines()
            .filter_map(|line| line.trim().strip_prefix('\''))
            .filter_map(|line| line.split('\'').next())
            .collect();
        assert_eq!(
            keys,
            [
                "name",
                "description",
                "content",
                "trigger_keywords",
                "trigger_file_patterns",
                "tool_restrictions",
                "kind",
                "settings",
                "bundle_sha256",
            ]
        );
        assert!(
            migration.contains(
                "encode(sha256(convert_to(agent_skill_version_body(s)::text, 'UTF8')), 'hex')"
            ),
            "the digest is lowercase hex sha256 of the body's UTF-8 text"
        );
    }

    /// The version store has no writer here: a write path would be a second writer beside the trigger.
    #[test]
    fn this_module_never_writes_versions() {
        let source = include_str!("skill_versions.rs");
        let code = &source[..source.find("#[cfg(test)]").expect("test module")];
        for verb in [
            "INSERT INTO agent_skill_versions",
            "UPDATE agent_skill_versions",
            "DELETE FROM agent_skill_versions",
        ] {
            assert!(!code.contains(verb), "skill_versions.rs must not {verb}");
        }
    }

    // The database tests below run with `--ignored` against a disposable Postgres, as the other
    // session-core database tests do (scripts/tests/session-core-db-tests.sh, one thread). They call
    // the store functions and plain SQL directly, so the switch is not needed;
    // `rpc_is_refused_while_the_switch_is_off` tests the switch.

    async fn connect() -> PgPool {
        let pool =
            PgPool::connect(&std::env::var("DATABASE_URL").expect("disposable database required"))
                .await
                .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    fn new_org() -> String {
        format!("skv-org-{}", new_ulid())
    }

    /// One skill row with `first text` as its content, written with no actor named.
    async fn seed_skill(
        pool: &PgPool,
        org: &str,
        scope: &str,
        owner: &str,
        review_state: &str,
        shared_with: &[&str],
    ) -> String {
        let id = format!("skill_{}", new_ulid());
        sqlx::query(
            "INSERT INTO agent_skills
                (id, org_id, name, description, content, trigger_keywords, trigger_file_patterns,
                 tool_restrictions, enabled, origin, scope, owner_user_id, shared_with, review_state)
             VALUES ($1, $2, $3, 'd', 'first text', '[\"deploy\"]', '[]', '[]', true, 'user',
                     $4, $5, $6, $7)",
        )
        .bind(&id)
        .bind(org)
        .bind(format!("skill {id}"))
        .bind(scope)
        .bind(owner)
        .bind(sqlx::types::Json(shared_with))
        .bind(review_state)
        .execute(pool)
        .await
        .unwrap();
        id
    }

    /// Run `sql` (binding the skill id as `$1`) in a transaction that names `actor` the way both
    /// writers do (0064's settings), or no actor for `None`.
    async fn write_as(pool: &PgPool, actor: Option<(&str, &str)>, sql: &str, skill_id: &str) {
        let mut tx = pool.begin().await.unwrap();
        if let Some((kind, id)) = actor {
            sqlx::query(
                "SELECT set_config('mp.actor_kind', $1, true), set_config('mp.actor_id', $2, true)",
            )
            .bind(kind)
            .bind(id)
            .execute(&mut *tx)
            .await
            .unwrap();
        }
        sqlx::query(sql)
            .bind(skill_id)
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
    }

    async fn live(pool: &PgPool, skill_id: &str) -> (i32, String) {
        sqlx::query_as("SELECT version, digest FROM agent_skills WHERE id = $1")
            .bind(skill_id)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    async fn version_rows(pool: &PgPool, skill_id: &str) -> Vec<(i32, String, String, String)> {
        sqlx::query_as(
            "SELECT version, digest, actor_kind, actor_id FROM agent_skill_versions
             WHERE skill_id = $1 ORDER BY version",
        )
        .bind(skill_id)
        .fetch_all(pool)
        .await
        .unwrap()
    }

    fn owner() -> VerifiedIdentity {
        VerifiedIdentity::user_for_test("org-placeholder", OWNER)
    }

    fn user(org: &str, id: &str) -> VerifiedIdentity {
        VerifiedIdentity::user_for_test(org, id)
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn version_increments_only_when_digest_changes() {
        let pool = connect().await;
        let org = new_org();
        let skill = seed_skill(&pool, &org, "org", "", "accepted", &[]).await;
        let (v1, d1) = live(&pool, &skill).await;
        assert_eq!(v1, 1, "an accepted insert is v1");
        assert_eq!(d1.len(), 64);

        // The same text written again is no change.
        write_as(
            &pool,
            None,
            "UPDATE agent_skills SET content = 'first text' WHERE id = $1",
            &skill,
        )
        .await;
        assert_eq!(live(&pool, &skill).await, (1, d1.clone()));

        write_as(
            &pool,
            None,
            "UPDATE agent_skills SET content = 'second text' WHERE id = $1",
            &skill,
        )
        .await;
        let (v2, d2) = live(&pool, &skill).await;
        assert_eq!(v2, 2);
        assert_ne!(d2, d1);

        // A restore writes the old body as a new version (D2); its digest is the old one's.
        write_as(
            &pool,
            None,
            "UPDATE agent_skills SET content = 'first text' WHERE id = $1",
            &skill,
        )
        .await;
        let (v3, d3) = live(&pool, &skill).await;
        assert_eq!(v3, 3);
        assert_eq!(d3, d1);

        // A writer cannot set the columns: the trigger owns them.
        write_as(
            &pool,
            None,
            "UPDATE agent_skills SET version = 99, digest = 'x' WHERE id = $1",
            &skill,
        )
        .await;
        assert_eq!(live(&pool, &skill).await, (3, d1));
        let versions: Vec<i32> = version_rows(&pool, &skill)
            .await
            .iter()
            .map(|row| row.0)
            .collect();
        assert_eq!(versions, [1, 2, 3]);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn toggling_enabled_does_not_cut_a_version() {
        let pool = connect().await;
        let org = new_org();
        let skill = seed_skill(&pool, &org, "user", OWNER, "accepted", &[]).await;
        write_as(
            &pool,
            None,
            "UPDATE agent_skills SET enabled = false WHERE id = $1",
            &skill,
        )
        .await;
        write_as(
            &pool,
            None,
            "UPDATE agent_skills SET enabled = true, shared_with = '[\"x\"]' WHERE id = $1",
            &skill,
        )
        .await;
        assert_eq!(live(&pool, &skill).await.0, 1);
        assert_eq!(version_rows(&pool, &skill).await.len(), 1);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn proposal_has_no_version_until_accepted() {
        let pool = connect().await;
        let org = new_org();
        let skill = seed_skill(&pool, &org, "user", OWNER, "proposed", &[]).await;
        write_as(
            &pool,
            None,
            "UPDATE agent_skills SET content = 'refreshed proposal' WHERE id = $1",
            &skill,
        )
        .await;
        assert_eq!(live(&pool, &skill).await, (0, String::new()));
        assert!(version_rows(&pool, &skill).await.is_empty());
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn accepting_a_proposal_cuts_v1() {
        let pool = connect().await;
        let org = new_org();
        let skill = seed_skill(&pool, &org, "user", OWNER, "proposed", &[]).await;
        write_as(
            &pool,
            Some(("user", OWNER)),
            "UPDATE agent_skills SET review_state = 'accepted', enabled = true WHERE id = $1",
            &skill,
        )
        .await;
        let rows = version_rows(&pool, &skill).await;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, 1);
        assert_eq!((rows[0].2.as_str(), rows[0].3.as_str()), ("user", OWNER));

        // Rejected and accepted again with the same text: the text is version 1 already.
        write_as(
            &pool,
            None,
            "UPDATE agent_skills SET review_state = 'rejected' WHERE id = $1",
            &skill,
        )
        .await;
        write_as(
            &pool,
            None,
            "UPDATE agent_skills SET review_state = 'accepted' WHERE id = $1",
            &skill,
        )
        .await;
        assert_eq!(live(&pool, &skill).await.0, 1);
    }

    /// capability-core's PATCH is plain SQL in a transaction that set mp.actor_kind = 'user'.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn capability_core_patch_cuts_a_version_with_actor() {
        let pool = connect().await;
        let org = new_org();
        let skill = seed_skill(&pool, &org, "org", "", "accepted", &[]).await;
        write_as(
            &pool,
            Some(("user", "user-admin")),
            "UPDATE agent_skills SET description = 'patched', updated_at = now() WHERE id = $1",
            &skill,
        )
        .await;
        let rows = version_rows(&pool, &skill).await;
        assert_eq!(rows.len(), 2);
        assert_eq!(
            (rows[1].2.as_str(), rows[1].3.as_str()),
            ("user", "user-admin")
        );
        assert_eq!(
            (rows[0].2.as_str(), rows[0].3.as_str()),
            ("unattributed", ""),
            "a write that names no actor is still a version, marked unattributed"
        );
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn bundle_sha256_change_cuts_a_version() {
        let pool = connect().await;
        let org = new_org();
        let skill = seed_skill(&pool, &org, "org", "", "accepted", &[]).await;
        let manifest = "a".repeat(64);
        write_as(
            &pool,
            Some(("user", "user-admin")),
            &format!("UPDATE agent_skills SET bundle_sha256 = '{manifest}' WHERE id = $1"),
            &skill,
        )
        .await;
        assert_eq!(live(&pool, &skill).await.0, 2);
        let bundle: String = sqlx::query_scalar(
            "SELECT bundle_sha256 FROM agent_skill_versions WHERE skill_id = $1 AND version = 2",
        )
        .bind(&skill)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(bundle, manifest);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn versions_are_immutable() {
        let pool = connect().await;
        let org = new_org();
        let skill = seed_skill(&pool, &org, "org", "", "accepted", &[]).await;
        for sql in [
            "UPDATE agent_skill_versions SET body = '{}' WHERE skill_id = $1",
            "DELETE FROM agent_skill_versions WHERE skill_id = $1",
        ] {
            let error = sqlx::query(sql)
                .bind(&skill)
                .execute(&pool)
                .await
                .unwrap_err();
            assert!(
                error.to_string().contains("skill_version_immutable"),
                "{sql}: {error}"
            );
        }
        // Even under the erasure setting an update is refused: only a delete is allowed there.
        let mut tx = pool.begin().await.unwrap();
        sqlx::query("SELECT set_config('mp.version_erasure', 'on', true)")
            .execute(&mut *tx)
            .await
            .unwrap();
        let error =
            sqlx::query("UPDATE agent_skill_versions SET actor_id = 'x' WHERE skill_id = $1")
                .bind(&skill)
                .execute(&mut *tx)
                .await
                .unwrap_err();
        assert!(error.to_string().contains("skill_version_immutable"));
        drop(tx);
        assert_eq!(version_rows(&pool, &skill).await.len(), 1);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn skill_delete_removes_its_versions() {
        let pool = connect().await;
        let org = new_org();
        let skill = seed_skill(&pool, &org, "org", "", "accepted", &[]).await;
        let kept = seed_skill(&pool, &org, "org", "", "accepted", &[]).await;
        write_as(
            &pool,
            None,
            "UPDATE agent_skills SET content = 'v2' WHERE id = $1",
            &skill,
        )
        .await;
        let mut tx = pool.begin().await.unwrap();
        sqlx::query("DELETE FROM agent_skills WHERE id = $1")
            .bind(&skill)
            .execute(&mut *tx)
            .await
            .unwrap();
        // The skill's delete put the erasure setting back: the rest of the transaction cannot
        // delete another skill's version through it.
        let error = sqlx::query("DELETE FROM agent_skill_versions WHERE skill_id = $1")
            .bind(&kept)
            .execute(&mut *tx)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("skill_version_immutable"));
        drop(tx);
        sqlx::query("DELETE FROM agent_skills WHERE id = $1")
            .bind(&skill)
            .execute(&pool)
            .await
            .unwrap();
        assert!(version_rows(&pool, &skill).await.is_empty());
        assert_eq!(version_rows(&pool, &kept).await.len(), 1);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn org_purge_removes_versions() {
        let pool = connect().await;
        let org = new_org();
        let other = new_org();
        let skill = seed_skill(&pool, &org, "user", OWNER, "accepted", &[]).await;
        let kept = seed_skill(&pool, &other, "org", "", "accepted", &[]).await;
        write_as(
            &pool,
            None,
            "UPDATE agent_skills SET content = 'v2' WHERE id = $1",
            &skill,
        )
        .await;
        let summary = crate::gdpr::purge_organization_data(&pool, &org, None)
            .await
            .expect("purge");
        assert_eq!(
            summary.agent_skill_versions, 2,
            "the purge counts the versions it erased"
        );
        assert!(version_rows(&pool, &skill).await.is_empty());
        assert_eq!(
            version_rows(&pool, &kept).await.len(),
            1,
            "another org keeps its versions"
        );
        crate::gdpr::purge_organization_data(&pool, &other, None)
            .await
            .expect("cleanup");
    }

    /// The backfill statements of 0108, run against a row written while the triggers were off
    /// (as every accepted row was before the migration).
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn backfill_gives_every_accepted_skill_v1() {
        let pool = connect().await;
        let org = new_org();
        let mut tx = pool.begin().await.unwrap();
        // Replica mode skips ordinary triggers: the row looks as it did before 0108.
        sqlx::query("SET LOCAL session_replication_role = replica")
            .execute(&mut *tx)
            .await
            .unwrap();
        let mut ids = Vec::new();
        for state in ["accepted", "proposed"] {
            let id = format!("skill_{}", new_ulid());
            sqlx::query(
                "INSERT INTO agent_skills (id, org_id, name, content, review_state, scope, owner_user_id)
                 VALUES ($1, $2, $1, 'old text', $3, 'user', $4)",
            )
            .bind(&id)
            .bind(&org)
            .bind(state)
            .bind(OWNER)
            .execute(&mut *tx)
            .await
            .unwrap();
            ids.push(id);
        }
        tx.commit().await.unwrap();
        let revisions_before: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM skill_revisions WHERE org_id = $1")
                .bind(&org)
                .fetch_one(&pool)
                .await
                .unwrap();

        let migration = include_str!("../migrations/0108_agent_skill_versions.sql");
        let backfill = &migration[migration
            .find("SELECT set_config('mp.actor_kind', 'system'")
            .expect("the backfill")..];
        let mut tx = pool.begin().await.unwrap();
        for statement in backfill
            .split(";\n")
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            sqlx::query(statement).execute(&mut *tx).await.unwrap();
        }
        tx.commit().await.unwrap();

        let accepted = version_rows(&pool, &ids[0]).await;
        assert_eq!(accepted.len(), 1);
        assert_eq!(accepted[0].0, 1);
        assert_eq!(
            (accepted[0].2.as_str(), accepted[0].3.as_str()),
            ("system", "migration:0108")
        );
        assert!(
            version_rows(&pool, &ids[1]).await.is_empty(),
            "a proposal gets no version"
        );
        let revisions_after: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM skill_revisions WHERE org_id = $1")
                .bind(&org)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            revisions_after, revisions_before,
            "the backfill writes no revision"
        );
    }

    /// Regression for 0064: the revision log still records every mutation beside the versions.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn revision_trigger_still_records_every_mutation() {
        let pool = connect().await;
        let org = new_org();
        let skill = seed_skill(&pool, &org, "org", "", "accepted", &[]).await;
        write_as(
            &pool,
            Some(("user", "user-admin")),
            "UPDATE agent_skills SET content = 'edited' WHERE id = $1",
            &skill,
        )
        .await;
        write_as(
            &pool,
            Some(("user", "user-admin")),
            "UPDATE agent_skills SET enabled = false WHERE id = $1",
            &skill,
        )
        .await;
        write_as(
            &pool,
            Some(("user", "user-admin")),
            "DELETE FROM agent_skills WHERE id = $1",
            &skill,
        )
        .await;
        let actions: Vec<String> = sqlx::query_scalar(
            "SELECT action FROM skill_revisions WHERE skill_id = $1 ORDER BY revision",
        )
        .bind(&skill)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(actions, ["created", "edited", "disabled", "deleted"]);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn list_and_get_follow_the_list_agent_skills_visibility() {
        let pool = connect().await;
        let org = new_org();
        let shared = seed_skill(&pool, &org, "user", OWNER, "accepted", &[SHAREE]).await;
        write_as(
            &pool,
            None,
            "UPDATE agent_skills SET content = 'v2' WHERE id = $1",
            &shared,
        )
        .await;

        let request = |reader: &str| pb::ListAgentSkillVersionsRequest {
            org_id: org.clone(),
            skill_id: shared.clone(),
            for_user_id: reader.to_owned(),
            ..Default::default()
        };
        let listed = list_versions(&pool, &user(&org, OWNER), request(""))
            .await
            .unwrap();
        assert_eq!(
            listed
                .versions
                .iter()
                .map(|v| v.version)
                .collect::<Vec<_>>(),
            [2, 1]
        );
        assert!(
            listed.versions[0].content.is_empty(),
            "a listing omits the text unless asked"
        );
        assert_eq!(listed.versions[0].name, format!("skill {shared}"));
        assert!(list_versions(&pool, &user(&org, SHAREE), request(""))
            .await
            .is_ok());

        let refused = list_versions(&pool, &user(&org, STRANGER), request(OWNER))
            .await
            .unwrap_err();
        assert_eq!(
            refused.code(),
            tonic::Code::NotFound,
            "for_user_id is ignored for a user caller"
        );
        let service = VerifiedIdentity::service_for_test(&org, &["session:read"], false);
        assert!(list_versions(&pool, &service, request(SHAREE))
            .await
            .is_ok());

        let mut paged = request("");
        paged.limit = 1;
        let page = list_versions(&pool, &user(&org, OWNER), paged)
            .await
            .unwrap();
        assert_eq!(page.versions.len(), 1);
        assert!(page.has_more);

        let first = get_as(&pool, &user(&org, OWNER), &org, &shared, 1)
            .await
            .unwrap();
        assert_eq!(first.version.unwrap().content, "first text");
        let newest = get_as(&pool, &user(&org, OWNER), &org, &shared, 0)
            .await
            .unwrap();
        assert_eq!(newest.version.unwrap().version, 2);
        assert!(
            get_as(&pool, &user(&org, OWNER), &org, &shared, 7)
                .await
                .unwrap()
                .gone
        );
        let hidden = get_as(&pool, &user(&org, STRANGER), &org, &shared, 1)
            .await
            .unwrap();
        assert!(hidden.gone, "a skill the reader cannot see reads as gone");
        let other_org = get_as(&pool, &owner(), &org, &shared, 1).await.unwrap_err();
        assert_eq!(other_org.code(), tonic::Code::PermissionDenied);
    }

    async fn get_as(
        pool: &PgPool,
        reader: &VerifiedIdentity,
        org: &str,
        skill: &str,
        version: i32,
    ) -> Result<pb::GetAgentSkillVersionResponse, Status> {
        get_version(
            pool,
            reader,
            pb::GetAgentSkillVersionRequest {
                org_id: org.to_owned(),
                skill_id: skill.to_owned(),
                version,
                for_user_id: String::new(),
            },
        )
        .await
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn rpc_is_refused_while_the_switch_is_off() {
        let pool = connect().await;
        // The suite runs with the switch unset, which is the default this test proves.
        let mut request = Request::new(pb::GetAgentSkillVersionRequest {
            org_id: "org-placeholder".to_owned(),
            skill_id: "skill_x".to_owned(),
            ..Default::default()
        });
        request.extensions_mut().insert(owner());
        let refused = SkillVersionGrpc { pool }
            .get_agent_skill_version(request)
            .await
            .unwrap_err();
        assert_eq!(refused.code(), tonic::Code::Unavailable);
        assert_eq!(refused.message(), DISABLED);
    }
}
