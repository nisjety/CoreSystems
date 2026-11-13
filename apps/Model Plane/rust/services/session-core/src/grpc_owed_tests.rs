//! Owed `grpc.rs` tests (verification plan 3.2), a test-only child module of `grpc`.
//!
//! * DEC-V-R8 / migration 0128: a subscription turn stores only the opaque connection id, and a
//!   malformed id is refused before anything is written (DB tests, `#[ignore]`d, need
//!   `DATABASE_URL` to a disposable Postgres with the session-core migrations).
//! * EP-6 P3 [W3-EP-6-P3-d]: the ListThreads agent filter narrows the caller's visible set only
//!   (a source pin of the one clause; a DB test of the RPC needs the harness).

use super::*;
use sqlx::PgPool;

#[test]
fn the_agent_thread_filter_is_one_and_clause_that_only_narrows() {
    let source = include_str!("grpc.rs");
    let clause = "AND (NULLIF($14, '') IS NULL
                           OR EXISTS (SELECT 1 FROM runs ar
                                      WHERE ar.thread_id = t.id AND ar.agent_subject_id = $14))";
    assert_eq!(source.matches(clause).count(), 1, "the filter clause, once");
    let at = source.find(clause).unwrap();
    let page = &source[..at];
    let page = &page[page.rfind("WITH page AS (").expect("the page query")..];
    // The owner and org predicates come first, unchanged, so the filter can only remove rows.
    assert!(page.contains("WHERE t.org_id = $1"));
    assert!(page.contains("AND ($6::BIGINT IS NOT NULL OR t.user_id = $2)"));
    assert!(!page.contains(" OR t.id IN"), "no branch widens the set");
    assert_eq!(
        source.matches(".bind(&req.agent_subject_id)").count(),
        1,
        "bound once, as $14"
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

async fn seed_thread(pool: &PgPool, org: &str, thread: &str) {
    sqlx::query("INSERT INTO threads (id, session_key, org_id, user_id) VALUES ($1, $1, $2, 'u1')")
        .bind(thread)
        .bind(org)
        .execute(pool)
        .await
        .unwrap();
}

fn turn(thread: &str, subscription: &str) -> pb::AppendMessageRequest {
    pb::AppendMessageRequest {
        thread_id: thread.to_owned(),
        role: "user".to_owned(),
        content: "hei".to_owned(),
        subscription_connection_id: subscription.to_owned(),
        ..Default::default()
    }
}

async fn stored_subscription(pool: &PgPool, message_id: &str) -> Option<String> {
    sqlx::query_scalar("SELECT subscription_connection_id FROM messages WHERE id = $1")
        .bind(message_id)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn message_count(pool: &PgPool, thread: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM messages WHERE thread_id = $1")
        .bind(thread)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
async fn a_subscription_turn_stores_only_its_opaque_connection_id() {
    let pool = connect().await;
    let org = format!("r8-org-{}", mp_ids::new_ulid());
    let thread = format!("r8-t-{}", mp_ids::new_ulid());
    seed_thread(&pool, &org, &thread).await;

    let mut tx = pool.begin().await.unwrap();
    let on_subscription = append_message_in_tx(&mut tx, &turn(&thread, " conn_0f9c-1.a "), "u1")
        .await
        .unwrap();
    let platform = append_message_in_tx(&mut tx, &turn(&thread, ""), "u1")
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        stored_subscription(&pool, &on_subscription.message_id)
            .await
            .as_deref(),
        Some("conn_0f9c-1.a"),
        "the trimmed id, nothing else"
    );
    assert_eq!(
        stored_subscription(&pool, &platform.message_id).await,
        None,
        "a send without an id never touches the column"
    );
    let model: Option<String> =
        sqlx::query_scalar("SELECT model_connection_id FROM messages WHERE id = $1")
            .bind(&on_subscription.message_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        model, None,
        "the subscription id is not written as a gateway connection"
    );
    sqlx::query("DELETE FROM threads WHERE id = $1")
        .bind(&thread)
        .execute(&pool)
        .await
        .ok();
}

#[tokio::test]
#[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
async fn a_malformed_subscription_connection_id_is_refused_and_writes_nothing() {
    let pool = connect().await;
    let org = format!("r8-org-{}", mp_ids::new_ulid());
    let thread = format!("r8-t-{}", mp_ids::new_ulid());
    seed_thread(&pool, &org, &thread).await;
    for bad in ["conn/1", "conn 1", "sk-live?key=1", &"c".repeat(129)] {
        let mut tx = pool.begin().await.unwrap();
        let refused = append_message_in_tx(&mut tx, &turn(&thread, bad), "u1")
            .await
            .err()
            .unwrap_or_else(|| panic!("{bad:?} is refused"));
        assert_eq!(refused.code(), tonic::Code::InvalidArgument, "{bad:?}");
        tx.commit().await.unwrap();
    }
    assert_eq!(
        message_count(&pool, &thread).await,
        0,
        "a refused send writes no turn"
    );
    sqlx::query("DELETE FROM threads WHERE id = $1")
        .bind(&thread)
        .execute(&pool)
        .await
        .ok();
}

// ---- EP-6 P3 [W3-EP-6-P3-d]: ListThreads agent filter through the RPC ----------------------

fn list_service(pool: PgPool) -> SessionService {
    SessionService {
        pool,
        retrieval_client: None,
        graph_client: None,
        knowledge_client: None,
        letta_memory: None,
        audit_publisher: None,
        auth: None,
        grant_columns: crate::grpc::ThreadGrantColumns::present(),
    }
}

async fn listed(svc: &SessionService, org: &str, user: &str, agent: &str) -> Vec<String> {
    use mp_contracts::model_plane::v1::session_core_server::SessionCore;
    let mut request = Request::new(pb::ListThreadsRequest {
        org_id: org.to_owned(),
        user_id: user.to_owned(),
        agent_subject_id: agent.to_owned(),
        limit: 100,
        ..Default::default()
    });
    request
        .extensions_mut()
        .insert(VerifiedIdentity::user_for_test(org, user));
    let mut ids: Vec<String> = svc
        .list_threads(request)
        .await
        .expect("list_threads")
        .into_inner()
        .threads
        .into_iter()
        .map(|thread| thread.thread_id)
        .collect();
    ids.sort();
    ids
}

async fn owned_thread(pool: &PgPool, org: &str, user: &str, thread: &str) {
    sqlx::query("INSERT INTO threads (id, session_key, org_id, user_id) VALUES ($1, $1, $2, $3)")
        .bind(thread)
        .bind(org)
        .bind(user)
        .execute(pool)
        .await
        .unwrap();
}

/// One completed run of `thread`. A run an agent ran on was woken by the post that named it, so the
/// post and the agent are set together or not at all (`runs_wake_pair_chk`, migration 0075).
async fn agent_run(pool: &PgPool, org: &str, user: &str, thread: &str, agent: Option<&str>) {
    let trigger = match agent {
        Some(_) => {
            let post = format!("ep6-msg-{}", mp_ids::new_ulid());
            sqlx::query("INSERT INTO messages (id, thread_id, role, content) VALUES ($1, $2, 'user', 'hei')")
                .bind(&post)
                .bind(thread)
                .execute(pool)
                .await
                .unwrap();
            Some(post)
        }
        None => None,
    };
    sqlx::query(
        "INSERT INTO runs (id, thread_id, goal, org_id, user_id, status, agent_subject_id, trigger_message_id)
         VALUES ($1, $2, 'g', $3, $4, 'completed', $5, $6)",
    )
    .bind(format!("ep6-run-{}", mp_ids::new_ulid()))
    .bind(thread)
    .bind(org)
    .bind(user)
    .bind(agent)
    .bind(trigger)
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
#[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
async fn the_list_threads_agent_filter_narrows_the_callers_own_threads_and_never_widens() {
    let pool = connect().await;
    let id = mp_ids::new_ulid().to_lowercase();
    let org = format!("ep6-org-{id}");
    let me = "ep6-user-me";
    let other = "ep6-user-other";
    let agent = "agent:release-notes";
    let mine_with_agent = format!("ep6-a-mine-agent-{id}");
    let mine_other_agent = format!("ep6-b-mine-other-agent-{id}");
    let mine_no_run = format!("ep6-c-mine-no-run-{id}");
    let theirs_with_agent = format!("ep6-d-theirs-agent-{id}");
    for thread in [&mine_with_agent, &mine_other_agent, &mine_no_run] {
        owned_thread(&pool, &org, me, thread).await;
    }
    owned_thread(&pool, &org, other, &theirs_with_agent).await;
    agent_run(&pool, &org, me, &mine_with_agent, Some(agent)).await;
    agent_run(&pool, &org, me, &mine_with_agent, None).await;
    agent_run(&pool, &org, me, &mine_other_agent, Some("agent:someone-else")).await;
    agent_run(&pool, &org, other, &theirs_with_agent, Some(agent)).await;
    // The same agent in another organisation never reaches this caller either.
    let foreign_org = format!("ep6-foreign-{id}");
    let foreign = format!("ep6-e-foreign-{id}");
    owned_thread(&pool, &foreign_org, me, &foreign).await;
    agent_run(&pool, &foreign_org, me, &foreign, Some(agent)).await;

    let svc = list_service(pool.clone());
    let mut unfiltered = vec![
        mine_with_agent.clone(),
        mine_other_agent.clone(),
        mine_no_run.clone(),
    ];
    unfiltered.sort();
    assert_eq!(
        listed(&svc, &org, me, "").await,
        unfiltered,
        "an empty filter is the list as before"
    );
    assert_eq!(
        listed(&svc, &org, me, agent).await,
        vec![mine_with_agent.clone()],
        "only the caller's own thread the agent ran on; another user's thread never appears"
    );
    assert!(
        listed(&svc, &org, me, "agent:never-ran").await.is_empty(),
        "an agent that ran nowhere lists nothing"
    );
    assert_eq!(
        listed(&svc, &org, other, agent).await,
        vec![theirs_with_agent.clone()],
        "the other user sees only their own"
    );
}
