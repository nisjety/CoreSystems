use redis::AsyncCommands;
use retrieval_engine::cache::{hash_text, CacheLayer};

fn cache_test_url() -> Option<String> {
    std::env::var("DPV2_CACHE_TEST_URL").ok()
}

#[tokio::test]
async fn cache_layer_supports_dragonfly_command_surface() {
    let Some(url) = cache_test_url() else {
        eprintln!("skipping Dragonfly cache compatibility test; set DPV2_CACHE_TEST_URL");
        return;
    };

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

    cache
        .set_retrieval(&org_id, 7, &retrieval_key, &payload)
        .await;
    assert_eq!(
        cache.get_retrieval(&org_id, 7, &retrieval_key).await,
        Some(payload)
    );

    let deleted = cache.invalidate_org_retrieval(&org_id).await;
    assert_eq!(deleted, 1, "expected one retrieval cache key to be deleted");
    assert_eq!(cache.get_retrieval(&org_id, 7, &retrieval_key).await, None);

    let client = redis::Client::open(url).expect("cache client");
    let mut conn = client
        .get_multiplexed_async_connection()
        .await
        .expect("cache cleanup connection");
    let embed_key = format!("dpv2:embed:{model_version}:{query_hash}");
    let _: usize = conn.del(embed_key).await.expect("delete embed cache key");
}
