//! Provenance attached to every read result, so a model (and a person reading its answer) can tell
//! where data came from, when, and how much of it this actually is. One response is **not** proof
//! of a complete collection, and a failed or empty read is "unknown", not "zero".

use serde_json::{json, Value};

/// `{source, fetched_at, tenant_id, ...extra}`.
pub fn meta(source: &str, tenant: Option<&str>, extra: Value) -> Value {
    let mut m = json!({
        "source": source,
        "fetched_at": chrono::Utc::now().to_rfc3339(),
        "tenant_id": tenant,
    });
    if let (Some(base), Value::Object(more)) = (m.as_object_mut(), extra) {
        base.extend(more);
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn carries_source_time_tenant_and_extras() {
        let m = meta(
            "erp_rest",
            Some("t1"),
            json!({"coverage": "single_page", "complete_collection": false}),
        );
        assert_eq!(
            (
                m["source"].as_str(),
                m["tenant_id"].as_str(),
                m["coverage"].as_str()
            ),
            (Some("erp_rest"), Some("t1"), Some("single_page"))
        );
        assert_eq!(m["complete_collection"], false);
        assert!(chrono::DateTime::parse_from_rfc3339(m["fetched_at"].as_str().unwrap()).is_ok());
        assert!(meta("x", None, json!({}))["tenant_id"].is_null());
    }
}
