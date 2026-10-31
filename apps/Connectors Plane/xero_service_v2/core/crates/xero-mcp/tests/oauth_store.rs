use std::sync::{Arc, Barrier};

use sha2::{Digest, Sha256};
use xero_mcp::oauth_store::{AuthorizationRequest, ConnectionBinding, OAuthStore, TokenPair};

fn binding(tenant_id: &str) -> ConnectionBinding {
    ConnectionBinding {
        tenant_id: tenant_id.into(),
        connected_at: chrono::Utc::now(),
    }
}

fn request() -> AuthorizationRequest {
    AuthorizationRequest {
        client_id: "claude-client".into(),
        resource: "https://tools.aquatiq.com/mcp".into(),
        redirect_uri: "https://claude.ai/api/mcp/auth_callback".into(),
        client_state: "client-state".into(),
        code_challenge: xero_auth::PkceChallenge::challenge_for(&"v".repeat(43)),
        upstream_verifier: "upstream-verifier".into(),
        nonce: "upstream-nonce".into(),
    }
}

fn issue_tokens(store: &OAuthStore) -> TokenPair {
    let state = store.begin(request()).unwrap();
    store.approve(&state).unwrap().unwrap();
    let pending = store.finish(&state).unwrap().unwrap();
    let code = store
        .issue_code(
            pending,
            "xero-user",
            &[binding("5f3c9a2e-0000-0000-0000-000000000000")],
        )
        .unwrap();
    store
        .exchange_code(
            &code,
            "claude-client",
            "https://claude.ai/api/mcp/auth_callback",
            &"v".repeat(43),
        )
        .unwrap()
        .unwrap()
}

#[test]
fn authorization_codes_require_pkce_and_are_one_time() {
    let directory = tempfile::tempdir().unwrap();
    let store = OAuthStore::open(directory.path().join("auth.sqlite")).unwrap();
    let state = store.begin(request()).unwrap();
    assert!(store.finish("wrong-state").unwrap().is_none());
    assert!(store.finish(&state).unwrap().is_none());
    assert!(store.approve(&state).unwrap().is_some());
    let pending = store.finish(&state).unwrap().unwrap();
    assert!(store.finish(&state).unwrap().is_none());
    let code = store
        .issue_code(
            pending,
            "xero-user",
            &[binding("5f3c9a2e-0000-0000-0000-000000000000")],
        )
        .unwrap();
    assert!(store
        .exchange_code(
            &code,
            "claude-client",
            "https://claude.ai/api/mcp/auth_callback",
            &"w".repeat(43),
        )
        .unwrap()
        .is_none());
    assert!(store
        .exchange_code(
            &code,
            "claude-client",
            "https://claude.ai/api/mcp/auth_callback",
            &"v".repeat(43),
        )
        .unwrap()
        .is_none());
}

#[test]
fn refresh_rotates_and_preserves_exact_user_and_tenants() {
    let directory = tempfile::tempdir().unwrap();
    let store = OAuthStore::open(directory.path().join("auth.sqlite")).unwrap();
    let state = store.begin(request()).unwrap();
    store.approve(&state).unwrap().unwrap();
    let pending = store.finish(&state).unwrap().unwrap();
    let tenant = "5f3c9a2e-0000-0000-0000-000000000000";
    let code = store
        .issue_code(pending, "xero-user", &[binding(tenant)])
        .unwrap();
    let tokens = store
        .exchange_code(
            &code,
            "claude-client",
            "https://claude.ai/api/mcp/auth_callback",
            &"v".repeat(43),
        )
        .unwrap()
        .unwrap();
    let grant = store.access(&tokens.access_token).unwrap().unwrap();
    assert_eq!(grant.user_sub, "xero-user");
    assert_eq!(grant.resource, "https://tools.aquatiq.com/mcp");
    assert_eq!(grant.tenant_ids, vec![tenant]);
    assert!(store
        .exchange_code(
            &code,
            "claude-client",
            "https://claude.ai/api/mcp/auth_callback",
            &"v".repeat(43)
        )
        .unwrap()
        .is_none());
    let replacement = store
        .refresh(&tokens.refresh_token, "claude-client")
        .unwrap()
        .unwrap();
    assert_eq!(
        store
            .access(&replacement.access_token)
            .unwrap()
            .unwrap()
            .resource,
        "https://tools.aquatiq.com/mcp"
    );
    assert_eq!(
        store
            .access(&replacement.access_token)
            .unwrap()
            .unwrap()
            .tenant_ids,
        vec![tenant]
    );
    assert!(store
        .refresh(&tokens.refresh_token, "claude-client")
        .unwrap()
        .is_none());
}

#[test]
fn replayed_refresh_token_revokes_its_family_after_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("auth.sqlite");
    let store = OAuthStore::open(&path).unwrap();
    let first = issue_tokens(&store);
    let second = store
        .refresh(&first.refresh_token, "claude-client")
        .unwrap()
        .unwrap();
    drop(store);

    let reopened = OAuthStore::open(&path).unwrap();
    assert!(reopened
        .refresh(&first.refresh_token, "claude-client")
        .unwrap()
        .is_none());
    assert!(reopened
        .refresh(&second.refresh_token, "claude-client")
        .unwrap()
        .is_none());
    assert!(reopened.access(&first.access_token).unwrap().is_none());
    assert!(reopened.access(&second.access_token).unwrap().is_none());
}

#[test]
fn existing_token_table_is_upgraded_for_new_families() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("auth.sqlite");
    drop(OAuthStore::open(&path).unwrap());
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute_batch(
            "DROP TABLE tokens;
             CREATE TABLE tokens (key BLOB PRIMARY KEY, kind TEXT NOT NULL, payload TEXT NOT NULL, expires_at INTEGER NOT NULL);",
        )
        .unwrap();
    let legacy_token = "legacy-refresh";
    let legacy_access = "legacy-access";
    let expires_at = chrono::Utc::now().timestamp() + 3600;
    let payload = serde_json::json!({
        "client_id": "claude-client",
        "user_sub": "xero-user",
        "tenant_ids": ["5f3c9a2e-0000-0000-0000-000000000000"],
        "not_after": expires_at
    });
    connection
        .execute(
            "INSERT INTO tokens (key, kind, payload, expires_at) VALUES (?1, 'refresh', ?2, ?3)",
            rusqlite::params![
                Sha256::digest(legacy_token.as_bytes()).as_slice(),
                payload.to_string(),
                expires_at
            ],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO tokens (key, kind, payload, expires_at) VALUES (?1, 'access', ?2, ?3)",
            rusqlite::params![
                Sha256::digest(legacy_access.as_bytes()).as_slice(),
                payload.to_string(),
                expires_at
            ],
        )
        .unwrap();
    drop(connection);

    let store = OAuthStore::open(&path).unwrap();
    assert!(store.access(legacy_access).unwrap().is_none());
    assert!(store
        .refresh(legacy_token, "claude-client")
        .unwrap()
        .is_none());

    let first = issue_tokens(&store);
    let second = store
        .refresh(&first.refresh_token, "claude-client")
        .unwrap()
        .unwrap();
    assert!(store
        .refresh(&first.refresh_token, "claude-client")
        .unwrap()
        .is_none());
    assert!(store
        .refresh(&second.refresh_token, "claude-client")
        .unwrap()
        .is_none());
}

#[test]
fn replay_revokes_later_generations_but_not_another_family() {
    let directory = tempfile::tempdir().unwrap();
    let store = OAuthStore::open(directory.path().join("auth.sqlite")).unwrap();
    let first = issue_tokens(&store);
    let unrelated = issue_tokens(&store);

    assert!(store
        .refresh(&first.refresh_token, "other-client")
        .unwrap()
        .is_none());
    let second = store
        .refresh(&first.refresh_token, "claude-client")
        .unwrap()
        .unwrap();
    let third = store
        .refresh(&second.refresh_token, "claude-client")
        .unwrap()
        .unwrap();
    assert!(store
        .refresh(&first.refresh_token, "other-client")
        .unwrap()
        .is_none());
    assert!(store.access(&third.access_token).unwrap().is_some());

    assert!(store
        .refresh(&first.refresh_token, "claude-client")
        .unwrap()
        .is_none());
    assert!(store
        .refresh(&third.refresh_token, "claude-client")
        .unwrap()
        .is_none());
    assert!(store.access(&third.access_token).unwrap().is_none());
    assert!(store
        .refresh(&unrelated.refresh_token, "claude-client")
        .unwrap()
        .is_some());
}

#[test]
fn concurrent_refresh_replay_revokes_the_winner() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("auth.sqlite");
    let store = OAuthStore::open(&path).unwrap();
    let first = issue_tokens(&store);
    let barrier = Arc::new(Barrier::new(2));
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let path = path.clone();
            let token = first.refresh_token.clone();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                let store = OAuthStore::open(path).unwrap();
                barrier.wait();
                store.refresh(&token, "claude-client").unwrap()
            })
        })
        .collect();
    let replacements: Vec<_> = handles
        .into_iter()
        .filter_map(|handle| handle.join().unwrap())
        .collect();

    assert_eq!(replacements.len(), 1);
    assert!(store
        .refresh(&replacements[0].refresh_token, "claude-client")
        .unwrap()
        .is_none());
    assert!(store
        .access(&replacements[0].access_token)
        .unwrap()
        .is_none());
}

#[test]
fn expired_refresh_tombstones_are_cleaned_on_rotation() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("auth.sqlite");
    let store = OAuthStore::open(&path).unwrap();
    let first = issue_tokens(&store);
    let second = store
        .refresh(&first.refresh_token, "claude-client")
        .unwrap()
        .unwrap();
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute("UPDATE used_refresh SET expires_at = 0", [])
        .unwrap();

    let third = store
        .refresh(&second.refresh_token, "claude-client")
        .unwrap()
        .unwrap();
    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM used_refresh", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1);
    assert!(store
        .refresh(&first.refresh_token, "claude-client")
        .unwrap()
        .is_none());
    assert!(store
        .refresh(&third.refresh_token, "claude-client")
        .unwrap()
        .is_some());
}
