use redis::AsyncCommands;
use retrieval_engine::cache::{hash_text, viewer_scope_token, CacheLayer};

fn cache_test_url() -> Option<String> {
    std::env::var("DPV2_CACHE_TEST_URL").ok()
}

#[tokio::test]
#[ignore = "requires DPV2_CACHE_TEST_URL pointing to disposable Dragonfly/Redis"]
async fn cache_layer_supports_dragonfly_command_surface() {
    let url = cache_test_url().expect("DPV2_CACHE_TEST_URL is required for this ignored test");

    let cache = CacheLayer::connect(&url).await.expect("connect cache");
    assert!(cache.health_check().await, "cache health check failed");

    let namespace = uuid::Uuid::new_v4().to_string();
    let model_version = format!("dragonfly-test-model-{namespace}");
    let query_hash = hash_text(&format!("query-{namespace}"));
    let embedding = vec![0.25_f32, -1.5, 2.75, 4.0];

    cache
        .set_embedding(&model_version, &query_hash, &embedding)
        .await;
    assert_eq!(
        cache.get_embedding(&model_version, &query_hash).await,
        Some(embedding)
    );

    let org_id = format!("dragonfly-test-org-{namespace}");
    let retrieval_key = hash_text(&format!("retrieval-{namespace}"));
    let payload = serde_json::json!({
        "candidates": [{"knowledge_id": "k1", "text": "cached"}],
    })
    .to_string();

    // P1-6: the retrieval tier is viewer-scoped. `scope` is required; see
    // `CacheLayer::get_retrieval` for the cross-user leak it prevents.
    let scope = viewer_scope_token(Some("user-a"), &["doc-1".to_string()]);
    cache
        .set_retrieval(&org_id, 7, &scope, &retrieval_key, &payload)
        .await;
    assert_eq!(
        cache
            .get_retrieval(&org_id, 7, &scope, &retrieval_key)
            .await,
        Some(payload.clone())
    );

    // A different viewer must NOT see it, even at the same org + org_version +
    // query. This is the leak the scope exists to prevent.
    let other = viewer_scope_token(Some("user-b"), &[]);
    assert_eq!(
        cache
            .get_retrieval(&org_id, 7, &other, &retrieval_key)
            .await,
        None,
        "a different viewer must never read another viewer's cached results"
    );
    // So must a caller who supplies no scope at all — fail closed, not org-wide.
    assert_eq!(
        cache.get_retrieval(&org_id, 7, "", &retrieval_key).await,
        None,
        "empty scope must fail closed"
    );

    // Invalidation is org-wide and must clear every scope for the org.
    let deleted = cache.invalidate_org_retrieval(&org_id).await;
    assert_eq!(deleted, 1, "expected one retrieval cache key to be deleted");
    assert_eq!(
        cache
            .get_retrieval(&org_id, 7, &scope, &retrieval_key)
            .await,
        None
    );

    let client = redis::Client::open(url).expect("cache client");
    let mut conn = client
        .get_multiplexed_async_connection()
        .await
        .expect("cache cleanup connection");
    let embed_key = format!("dpv2:embed:{model_version}:{query_hash}");
    let _: usize = conn.del(embed_key).await.expect("delete embed cache key");
}
