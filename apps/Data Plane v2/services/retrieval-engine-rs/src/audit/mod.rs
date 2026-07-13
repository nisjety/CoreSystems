//! Access audit log writer (Wave 3 §15-E).
//!
//! `record_access` appends one row per authenticated request. Called from
//! both HTTP middleware and gRPC interceptor at response time so we capture
//! status + latency + which docs the caller actually saw.
//!
//! Best-effort: a write failure here MUST NOT change the user-visible
//! response. We log + metric the failure and move on. This mirrors the
//! `persist_trace` design from v2.2.

use sqlx::PgPool;

use crate::authz::AuthContext;

#[derive(Clone)]
pub struct AccessEvent<'a> {
    pub ctx: &'a AuthContext,
    pub endpoint: &'a str,
    pub http_status: u16,
    pub latency_ms: u32,
    pub document_ids: Vec<String>,
    pub cause: &'a str,
}

#[tracing::instrument(
    name = "postgres.access_audit",
    skip_all,
    fields(
        otel.kind = "client",
        db.system = "postgresql",
        db.operation = "INSERT",
        org_id = event.ctx.org_id.as_str(),
        user_id = event.ctx.user_id.as_deref().unwrap_or(""),
        endpoint = event.endpoint,
        status = event.http_status,
    ),
)]
pub async fn record_access(pool: &PgPool, event: AccessEvent<'_>) {
    let result = sqlx::query(
        r#"
        INSERT INTO access_audit_log
            (request_id, user_id, org_id, endpoint, http_status,
             latency_ms, auth_method, document_ids, cause)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        "#,
    )
    .bind(&event.ctx.request_id)
    .bind(event.ctx.user_id.as_deref())
    .bind(&event.ctx.org_id)
    .bind(event.endpoint)
    .bind(event.http_status as i32)
    .bind(event.latency_ms as i32)
    .bind(event.ctx.auth_method.as_str())
    .bind(&event.document_ids)
    .bind(event.cause)
    .execute(pool)
    .await;

    if let Err(e) = result {
        tracing::warn!(
            error = %e,
            org_id = %event.ctx.org_id,
            endpoint = event.endpoint,
            "access audit write failed (best-effort, response unaffected)"
        );
        crate::metrics::record_audit_write_failure(&event.ctx.org_id);
    }
}

/// Wave-3.2 §16.5.3 — admin-action audit log writer.
/// Use from every admin endpoint: orphan cleanup, sweeps, hard deletes,
/// reindex jobs. `actor` is the resolved subject from the AuthContext
/// (user_id or "internal:<service>"); `action` is a stable verb like
/// `cleanup_orphans` or `hard_delete_document`.
#[derive(Debug, Clone)]
pub struct AdminEvent<'a> {
    pub org_id: Option<&'a str>,
    pub actor: &'a str,
    pub action: &'a str,
    pub target_kind: Option<&'a str>,
    pub target_id: Option<&'a str>,
    pub request_id: Option<&'a str>,
    pub payload: Option<serde_json::Value>,
    pub outcome: &'a str,
    pub error: Option<&'a str>,
}

#[tracing::instrument(
    name = "postgres.admin_audit",
    skip_all,
    fields(
        otel.kind = "client",
        db.system = "postgresql",
        db.operation = "INSERT",
        org_id = event.org_id.unwrap_or(""),
        actor = event.actor,
        action = event.action,
        outcome = event.outcome,
    ),
)]
pub async fn record_admin(pool: &PgPool, event: AdminEvent<'_>) {
    let result = sqlx::query(
        r#"
        INSERT INTO admin_audit_log
            (org_id, actor, action, target_kind, target_id, request_id,
             payload, outcome, error)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        "#,
    )
    .bind(event.org_id)
    .bind(event.actor)
    .bind(event.action)
    .bind(event.target_kind)
    .bind(event.target_id)
    .bind(event.request_id)
    .bind(event.payload.as_ref())
    .bind(event.outcome)
    .bind(event.error)
    .execute(pool)
    .await;

    if let Err(e) = result {
        tracing::warn!(
            error = %e,
            actor = event.actor,
            action = event.action,
            "admin audit write failed (best-effort, response unaffected)"
        );
    }
}
