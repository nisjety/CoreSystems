//! F-f10-16: a reader never sees a Space skill's history through an empty or foreign owner match.
//! A test-only child module of `skill_versions`. The pure test pins the predicate; the DB tests are
//! `#[ignore]`d and need `DATABASE_URL` to a disposable Postgres with the session-core migrations,
//! like the module's own DB tests.

use super::*;
use mp_ids::new_ulid;
use sqlx::PgPool;

#[test]
fn the_visibility_predicate_never_matches_a_space_row_or_an_empty_reader_through_an_owner() {
    let sql: String = VISIBLE_SKILL_SQL
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(sql.contains("WHERE id = $2 AND org_id = $1"));
    assert!(sql.contains("OR (scope <> 'space' AND $3 <> '' AND owner_user_id = $3)"));
    assert!(sql.contains(
        "OR ($3 <> '' AND scope <> 'space' AND shared_with ? $3 AND review_state = 'accepted')"
    ));
    assert!(
        !sql.contains("OR owner_user_id = $3"),
        "the unguarded owner branch is gone"
    );
}

#[test]
fn a_service_reads_as_the_user_it_names_and_a_user_as_itself() {
    let service = VerifiedIdentity::service_for_test("org-1", &["session:read"], false);
    assert_eq!(
        reader_id(&service, ""),
        "",
        "a service naming no one reads as no one"
    );
    assert_eq!(reader_id(&service, " u-2 "), "u-2");
    let user = VerifiedIdentity::user_for_test("org-1", "u-1");
    assert_eq!(
        reader_id(&user, "u-2"),
        "u-1",
        "a user cannot read as another person"
    );
}

async fn connect() -> PgPool {
    let pool =
        PgPool::connect(&std::env::var("DATABASE_URL").expect("disposable database required"))
            .await
            .unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    pool
}

async fn seed(pool: &PgPool, org: &str, scope: &str, scope_id: &str, owner: &str) -> String {
    let id = format!("skill_{}", new_ulid());
    sqlx::query(
        "INSERT INTO agent_skills
            (id, org_id, name, description, content, trigger_keywords, trigger_file_patterns,
             tool_restrictions, enabled, origin, scope, scope_id, owner_user_id, shared_with,
             review_state)
         VALUES ($1, $2, $3, 'd', 'first text', '[\"deploy\"]', '[]', '[]', true, 'user',
                 $4, $5, $6, '[]', 'accepted')",
    )
    .bind(&id)
    .bind(org)
    .bind(format!("skill {id}"))
    .bind(scope)
    .bind(scope_id)
    .bind(owner)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("UPDATE agent_skills SET content = 'second text' WHERE id = $1")
        .bind(&id)
        .execute(pool)
        .await
        .unwrap();
    id
}

fn list(org: &str, skill: &str, for_user: &str) -> pb::ListAgentSkillVersionsRequest {
    pb::ListAgentSkillVersionsRequest {
        org_id: org.to_owned(),
        skill_id: skill.to_owned(),
        for_user_id: for_user.to_owned(),
        ..Default::default()
    }
}

fn get(org: &str, skill: &str, for_user: &str) -> pb::GetAgentSkillVersionRequest {
    pb::GetAgentSkillVersionRequest {
        org_id: org.to_owned(),
        skill_id: skill.to_owned(),
        version: 1,
        for_user_id: for_user.to_owned(),
    }
}

#[tokio::test]
#[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
async fn a_service_reader_with_no_for_user_id_does_not_see_a_space_rows_versions() {
    let pool = connect().await;
    let org = format!("skv-space-org-{}", new_ulid());
    let space_skill = seed(&pool, &org, "space", "space-1", "").await;
    let service = VerifiedIdentity::service_for_test(&org, &["session:read"], false);

    let refused = list_versions(&pool, &service, list(&org, &space_skill, ""))
        .await
        .unwrap_err();
    assert_eq!(refused.code(), tonic::Code::NotFound);
    let read = get_version(&pool, &service, get(&org, &space_skill, ""))
        .await
        .unwrap();
    assert!(read.gone, "a Space row's version reads as gone");
    assert!(read.version.is_none());
    // Naming a person does not open a Space row either.
    assert!(
        list_versions(&pool, &service, list(&org, &space_skill, "u-1"))
            .await
            .is_err()
    );
    // Nor does a person reading directly.
    let person = VerifiedIdentity::user_for_test(&org, "u-1");
    assert!(list_versions(&pool, &person, list(&org, &space_skill, ""))
        .await
        .is_err());
}

#[tokio::test]
#[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
async fn an_empty_reader_sees_org_skills_only_and_a_user_their_own_and_org_ones() {
    let pool = connect().await;
    let org = format!("skv-space-org-{}", new_ulid());
    let org_skill = seed(&pool, &org, "org", "", "").await;
    let private = seed(&pool, &org, "user", "", "u-1").await;
    let service = VerifiedIdentity::service_for_test(&org, &["session:read"], false);
    assert!(list_versions(&pool, &service, list(&org, &org_skill, ""))
        .await
        .is_ok());
    assert_eq!(
        list_versions(&pool, &service, list(&org, &private, ""))
            .await
            .unwrap_err()
            .code(),
        tonic::Code::NotFound,
        "an empty reader never matches an owner"
    );
    let owner = VerifiedIdentity::user_for_test(&org, "u-1");
    assert!(list_versions(&pool, &owner, list(&org, &private, ""))
        .await
        .is_ok());
    assert!(list_versions(&pool, &owner, list(&org, &org_skill, ""))
        .await
        .is_ok());
    let stranger = VerifiedIdentity::user_for_test(&org, "u-2");
    assert!(list_versions(&pool, &stranger, list(&org, &private, ""))
        .await
        .is_err());
}
