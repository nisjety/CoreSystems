//! `visma_sync_status`: how fresh is the warehouse feed? Reads the sync state database.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::postgres::{PgPool, PgPoolOptions};

use crate::meta::meta;
use crate::tool::{parse_args, Tool, ToolDef, ToolError, ToolKind};
use crate::visma::Tenants;

pub const NAME: &str = "visma_sync_status";
const DEFAULT_LIMIT: i64 = 20;
const MAX_LIMIT: i64 = 100;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Filter {
    /// Only these companies; `None` for all of them.
    pub tenant_ids: Option<Vec<String>>,
    pub endpoint: Option<String>,
    pub run_status: Option<String>,
    pub stale_hours: Option<f64>,
    pub limit: i64,
}

#[async_trait]
pub trait SyncStatusSource: Send + Sync {
    /// `{ "checkpoints": [...], "recent_runs": [...], "runs_last_24h": {status: n} }`
    async fn status(&self, f: &Filter) -> Result<Value, String>;
}

pub struct PgSyncStatus {
    pool: PgPool,
}

impl PgSyncStatus {
    /// Lazy connection (the server starts even if the database is down) and a session that
    /// refuses writes.
    pub fn connect_lazy(dsn: &str) -> Result<Self, sqlx::Error> {
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .after_connect(|conn, _| {
                Box::pin(async move {
                    sqlx::query("SET default_transaction_read_only = on")
                        .execute(conn)
                        .await?;
                    Ok(())
                })
            })
            .connect_lazy(dsn)?;
        Ok(Self { pool })
    }
}

const CHECKPOINTS_SQL: &str = "SELECT to_jsonb(t) FROM ( \
    SELECT tenant_id, endpoint, source, MAX(last_sync_at) AS last_sync_at, \
           MAX(last_modified_watermark) AS watermark, SUM(records_seen)::bigint AS records_seen, \
           ROUND((EXTRACT(EPOCH FROM (now() - MAX(last_sync_at))) / 3600.0)::numeric, 1)::float8 AS age_hours \
    FROM vsv2.sync_checkpoint \
    WHERE ($1::text[] IS NULL OR tenant_id = ANY($1)) AND ($2::text IS NULL OR endpoint = $2) \
    GROUP BY tenant_id, endpoint, source \
    HAVING ($3::float8 IS NULL OR MAX(last_sync_at) < now() - make_interval(secs => $3 * 3600)) \
    ORDER BY MAX(last_sync_at) DESC NULLS LAST LIMIT $4) t";

const RUNS_SQL: &str = "SELECT to_jsonb(t) FROM ( \
    SELECT run_id, tenant_id, endpoint, source, status, started_at, finished_at, \
           records_in, records_written, LEFT(error, 300) AS error \
    FROM vsv2.sync_run \
    WHERE ($1::text[] IS NULL OR tenant_id = ANY($1)) AND ($2::text IS NULL OR endpoint = $2) \
      AND ($3::text IS NULL OR status = $3) \
    ORDER BY started_at DESC LIMIT $4) t";

const SUMMARY_SQL: &str = "SELECT status, COUNT(*)::bigint FROM vsv2.sync_run \
    WHERE started_at > now() - interval '24 hours' AND ($1::text[] IS NULL OR tenant_id = ANY($1)) \
    GROUP BY status";

#[async_trait]
impl SyncStatusSource for PgSyncStatus {
    async fn status(&self, f: &Filter) -> Result<Value, String> {
        let e = |what: &str, err: sqlx::Error| format!("sync state database ({what}): {err}");
        let checkpoints: Vec<sqlx::types::Json<Value>> = sqlx::query_scalar(CHECKPOINTS_SQL)
            .bind(&f.tenant_ids)
            .bind(&f.endpoint)
            .bind(f.stale_hours)
            .bind(f.limit)
            .fetch_all(&self.pool)
            .await
            .map_err(|x| e("checkpoints", x))?;
        let runs: Vec<sqlx::types::Json<Value>> = sqlx::query_scalar(RUNS_SQL)
            .bind(&f.tenant_ids)
            .bind(&f.endpoint)
            .bind(&f.run_status)
            .bind(f.limit)
            .fetch_all(&self.pool)
            .await
            .map_err(|x| e("runs", x))?;
        let summary: Vec<(String, i64)> = sqlx::query_as(SUMMARY_SQL)
            .bind(&f.tenant_ids)
            .fetch_all(&self.pool)
            .await
            .map_err(|x| e("summary", x))?;
        Ok(json!({
            "checkpoints": checkpoints.into_iter().map(|j| j.0).collect::<Vec<_>>(),
            "recent_runs": runs.into_iter().map(|j| j.0).collect::<Vec<_>>(),
            "runs_last_24h": summary.into_iter().collect::<std::collections::BTreeMap<_, _>>(),
        }))
    }
}

pub struct SyncStatus {
    pub tenants: Tenants,
    pub source: std::sync::Arc<dyn SyncStatusSource>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    #[serde(default)]
    tenant: Option<String>,
    #[serde(default)]
    endpoint: Option<String>,
    /// Only show runs with this status.
    #[serde(default)]
    run_status: Option<String>,
    /// Only list endpoints whose last sync is older than this many hours.
    #[serde(default)]
    stale_hours: Option<f64>,
    #[serde(default)]
    limit: Option<i64>,
}

const RUN_STATUSES: &[&str] = &[
    "pending",
    "running",
    "succeeded",
    "failed",
    "partial_success",
];

#[async_trait]
impl Tool for SyncStatus {
    fn def(&self) -> ToolDef {
        ToolDef {
            name: NAME,
            description:
                "How fresh is the Visma data feed? Per tenant+endpoint last sync time, watermark \
                and age in hours, recent sync runs (with errors), and run counts for the last 24h. \
                Filter by tenant/endpoint, show only failed runs (run_status=failed) or only stale \
                endpoints (stale_hours). Reflects the sync state database this server is \
                connected to. Read-only."
                    .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "tenant": {"type": "string"},
                    "endpoint": {"type": "string"},
                    "run_status": {"type": "string", "enum": RUN_STATUSES},
                    "stale_hours": {"type": "number", "minimum": 0},
                    "limit": {"type": "integer", "minimum": 1, "maximum": MAX_LIMIT}
                },
                "additionalProperties": false
            }),
            kind: ToolKind::Read,
        }
    }

    async fn call(&self, args: Value) -> Result<Value, ToolError> {
        let a: Args = parse_args(args)?;
        // A person who signed in sees the feed of their own companies only, whether or not they name one.
        let tenant_ids = match a.tenant.as_deref() {
            Some(t) => Some(vec![self.tenants.resolve(Some(t))?.to_string()]),
            None if self.tenants.is_restricted() => Some(self.tenants.ids()),
            None => None,
        };
        if let Some(ep) = &a.endpoint {
            let ok = !ep.is_empty()
                && ep.len() <= 100
                && ep.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
            if !ok {
                return Err(ToolError::InvalidArgs(format!(
                    "invalid endpoint name {ep:?}"
                )));
            }
        }
        if let Some(s) = &a.run_status {
            if !RUN_STATUSES.contains(&s.as_str()) {
                return Err(ToolError::InvalidArgs(format!(
                    "run_status must be one of {}",
                    RUN_STATUSES.join(", ")
                )));
            }
        }
        if a.stale_hours.is_some_and(|h| !h.is_finite() || h < 0.0) {
            return Err(ToolError::InvalidArgs(
                "stale_hours must be a non-negative number".into(),
            ));
        }
        let filter = Filter {
            tenant_ids,
            endpoint: a.endpoint,
            run_status: a.run_status,
            stale_hours: a.stale_hours,
            limit: a.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT),
        };
        let mut out = self
            .source
            .status(&filter)
            .await
            .map_err(ToolError::Failed)?;
        out["filter"] = json!({
            "tenant_ids": filter.tenant_ids, "endpoint": filter.endpoint, "run_status": filter.run_status,
            "stale_hours": filter.stale_hours, "limit": filter.limit
        });
        let single = filter.tenant_ids.as_deref().and_then(|ids| match ids {
            [one] => Some(one.as_str()),
            _ => None,
        });
        out["meta"] = meta(
            "sync_state_db",
            single,
            json!({"note": "Reflects the sync state database this server is connected to, not Visma itself."}),
        );
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::visma::tenants::test_tenants;
    use std::sync::{Arc, Mutex};

    struct Fake(Mutex<Vec<Filter>>, Result<Value, String>);
    #[async_trait]
    impl SyncStatusSource for Fake {
        async fn status(&self, f: &Filter) -> Result<Value, String> {
            self.0.lock().unwrap().push(f.clone());
            self.1.clone()
        }
    }

    fn tool(reply: Result<Value, String>) -> (SyncStatus, Arc<Fake>) {
        let f = Arc::new(Fake(Mutex::new(vec![]), reply));
        (
            SyncStatus {
                tenants: test_tenants(),
                source: f.clone(),
            },
            f,
        )
    }

    #[tokio::test]
    async fn passes_resolved_tenant_clamped_limit_and_filters_to_the_source() {
        let (t, f) = tool(Ok(
            json!({"checkpoints": [], "recent_runs": [], "runs_last_24h": {}}),
        ));
        let out = t.call(json!({"tenant": "aquatiq ab", "endpoint": "customers", "run_status": "failed", "stale_hours": 24, "limit": 9999})).await.unwrap();
        let got = f.0.lock().unwrap()[0].clone();
        assert_eq!(
            got,
            Filter {
                tenant_ids: Some(vec!["t-ab".into()]),
                endpoint: Some("customers".into()),
                run_status: Some("failed".into()),
                stale_hours: Some(24.0),
                limit: 100
            }
        );
        assert_eq!(out["filter"]["tenant_ids"], json!(["t-ab"]));
    }

    #[tokio::test]
    async fn the_feed_of_a_signed_in_person_is_limited_to_their_companies_even_when_none_is_named()
    {
        let f = Arc::new(Fake(Mutex::new(vec![]), Ok(json!({}))));
        let person = SyncStatus {
            tenants: Tenants::for_sign_in(
                Vec::new(),
                &[
                    ("t-b".into(), Some("B".into())),
                    ("t-a".into(), Some("A".into())),
                ],
                "t-a",
            ),
            source: f.clone(),
        };
        person.call(json!({})).await.unwrap();
        person.call(json!({"tenant": "A"})).await.unwrap();
        assert!(person
            .call(json!({"tenant": "someone-elses"}))
            .await
            .is_err());
        {
            let seen = f.0.lock().unwrap();
            assert_eq!(
                seen[0].tenant_ids,
                Some(vec!["t-a".to_string(), "t-b".to_string()])
            );
            assert_eq!(seen[1].tenant_ids, Some(vec!["t-a".to_string()]));
        }
        // The service identity, which sees every configured company, still asks for all of them with no filter.
        let (t, f) = tool(Ok(json!({})));
        t.call(json!({})).await.unwrap();
        assert_eq!(f.0.lock().unwrap()[0].tenant_ids, None);
    }

    #[tokio::test]
    async fn defaults_apply_and_bad_input_never_reaches_the_database() {
        let (t, f) = tool(Ok(json!({})));
        t.call(json!({})).await.unwrap();
        assert_eq!(f.0.lock().unwrap()[0].limit, 20);
        for bad in [
            json!({"endpoint": "a; DROP"}),
            json!({"run_status": "exploded"}),
            json!({"stale_hours": -1}),
            json!({"tenant": "nobody"}),
            json!({"x": 1}),
        ] {
            assert!(matches!(t.call(bad).await, Err(ToolError::InvalidArgs(_))));
        }
        assert_eq!(f.0.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn database_errors_become_tool_failures() {
        let (t, _) = tool(Err("connection refused".into()));
        assert!(
            matches!(t.call(json!({})).await, Err(ToolError::Failed(m)) if m.contains("connection refused"))
        );
    }
}
