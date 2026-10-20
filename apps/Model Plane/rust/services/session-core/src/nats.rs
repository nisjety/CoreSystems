//! NATS `JetStream` consumer for tool-completion events.
//!
//! Subscribes to `tools.completions.*`, decodes JSON payloads, and inserts a
//! `STEP_COMPLETED` row into the `events` table with
//! `org_id`/`user_id` hydrated from the `runs` table.

use crate::store::Pool;
use async_nats::jetstream::{self, consumer::PullConsumer, AckKind};
use futures::StreamExt;
use metrics::{counter, histogram};
use mp_events::idempotency::derive_idempotency_hash;
use mp_events::subjects::{subscriber_subjects, CompatMode};
use serde_json::Value;
use std::collections::HashSet;
use std::time::{Duration, Instant};
use tracing::{error, info, warn};

const STREAM_NAME: &str = "TOOLS_COMPLETIONS";
const SUBJECT_FILTER: &str = "tools.completions.*";
const DURABLE_NAME: &str = "session-core-tools";
const STEP_COMPLETED_EVENT_TYPE: &str = "STEP_COMPLETED";
const TOOL_COMPLETION_TYPE_URL: &str = "type.googleapis.com/model_plane.v1.ToolCompletion";

/// Event types session-core writes itself, and so never takes from a publisher. The loop fold
/// store writes `CHECKPOINT_SAVED` and `CHECKPOINT_RESTORED` inside its own transactions, and the
/// park events `RUN_PARKED` and `RUN_RESUMED` are recorded only through `RecordOrchestrationEvent`,
/// where they are checked against the run's own fold. That check reads `CHECKPOINT_RESTORED` as
/// the store's own record that a park was claimed, so a row of one of these types arriving over
/// NATS would be the same forgery by another door.
const RESERVED_EVENT_TYPES: [&str; 4] = [
    "RUN_PARKED",
    "RUN_RESUMED",
    "CHECKPOINT_SAVED",
    "CHECKPOINT_RESTORED",
];

/// What became of one message.
#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Stored,
    /// Not stored: its event type is one session-core owns.
    Refused,
}

/// Whether `event_type` is one of [`RESERVED_EVENT_TYPES`], however it is cased or padded.
fn is_reserved_event_type(event_type: &str) -> bool {
    let event_type = event_type.trim();
    RESERVED_EVENT_TYPES
        .iter()
        .any(|reserved| reserved.eq_ignore_ascii_case(event_type))
}

pub async fn run(pool: Pool, nats_url: String) -> anyhow::Result<()> {
    info!(%nats_url, "session-core NATS consumer connecting");

    let compat_mode = CompatMode::from_env();
    let subscribed_subjects = subscriber_subjects(SUBJECT_FILTER, compat_mode)
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;

    let client = crate::nats_connection::connect(&nats_url).await?;
    let js = jetstream::new(client);

    // Deployment provisioning owns this stream and its fixed consumer. Runtime
    // credentials can bind/pull/ACK but cannot mutate JetStream topology.
    let stream = js.get_stream(STREAM_NAME).await?;
    let mut consumer_streams = futures::stream::SelectAll::new();

    for (index, filter_subject) in subscribed_subjects.iter().enumerate() {
        let durable_name = if index == 0 {
            DURABLE_NAME.to_string()
        } else {
            format!("{DURABLE_NAME}-{index}")
        };

        if index > 0 {
            return Err(anyhow::anyhow!(
                "deployment topology does not provision legacy dual-read tool consumers"
            ));
        }
        let consumer: PullConsumer = stream
            .get_consumer(&durable_name)
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        if consumer.cached_info().config.filter_subject != *filter_subject {
            return Err(anyhow::anyhow!(
                "pre-provisioned tool consumer filter mismatch"
            ));
        }
        consumer_streams.push(consumer.messages().await?);
    }

    info!(
        mode = %compat_mode.as_str(),
        subjects = ?subscribed_subjects,
        "session-core NATS consumer ready"
    );

    let mut seen_event_ids = HashSet::<String>::new();

    while let Some(msg) = consumer_streams.next().await {
        let msg = match msg {
            Ok(m) => m,
            Err(e) => {
                warn!(error = %e, "error receiving message");
                counter!("mp_session_events_total", "type" => "error").increment(1);
                tokio::time::sleep(Duration::from_millis(500)).await;
                continue;
            }
        };

        let started = Instant::now();
        if compat_mode == CompatMode::DualRead {
            if let Some(event_id) = read_message_event_id(&msg.payload) {
                if !seen_event_ids.insert(event_id) {
                    counter!("mp_session_events_total", "type" => "duplicate").increment(1);
                    if let Err(e) = msg.ack().await {
                        warn!(error = %e, "ack duplicate failed");
                    }
                    continue;
                }
            }
        }

        match handle_message(&pool, &msg.payload).await {
            Ok(Outcome::Stored) => {
                let elapsed = started.elapsed().as_secs_f64();
                histogram!("mp_session_events_persist_duration_seconds").record(elapsed);
                counter!("mp_session_events_total", "type" => "step_completed").increment(1);
                if let Err(e) = msg.ack().await {
                    warn!(error = %e, "ack failed");
                }
            }
            Ok(Outcome::Refused) => {
                // Terminated, not NAKed: a refused message never becomes storable, so a NAK would
                // redeliver it up to the consumer's MaxDeliver (5) for nothing. Only the subject
                // is logged, and its last token is the publisher's.
                warn!(
                    subject = %msg.subject,
                    "refused a tool completion whose event type session-core owns"
                );
                counter!("mp_session_events_total", "type" => "refused").increment(1);
                if let Err(e) = msg.ack_with(AckKind::Term).await {
                    warn!(error = %e, "terminate of a refused message failed");
                }
            }
            Err(e) => {
                let elapsed = started.elapsed().as_secs_f64();
                histogram!("mp_session_events_persist_duration_seconds").record(elapsed);
                error!(error = %e, "failed to persist tool completion");
                counter!("mp_session_events_total", "type" => "error").increment(1);
                if let Err(ack_err) = msg.ack_with(AckKind::Nak(None)).await {
                    warn!(error = %ack_err, "nak failed");
                }
            }
        }
    }

    warn!("session-core NATS message stream ended");
    Ok(())
}

async fn handle_message(pool: &Pool, payload: &[u8]) -> anyhow::Result<Outcome> {
    let body: Value = serde_json::from_slice(payload)?;

    let run_id = read_str(&body, &["run_id", "runId"])
        .ok_or_else(|| anyhow::anyhow!("payload missing run_id"))?;
    let event_id = read_str(&body, &["event_id", "eventId"]).unwrap_or_else(mp_ids::new_ulid);
    let event_type = read_str(&body, &["event_type", "eventType"])
        .unwrap_or_else(|| STEP_COMPLETED_EVENT_TYPE.to_owned());
    // Before anything is derived from the body or written: these types are session-core's own.
    if is_reserved_event_type(&event_type) {
        return Ok(Outcome::Refused);
    }
    let step_id = read_str(&body, &["step_id", "stepId"]).unwrap_or_default();
    let resource_ref = read_str(&body, &["resource_ref", "resourceRef"]).unwrap_or_else(|| {
        if step_id.is_empty() {
            format!("run:{run_id}")
        } else {
            format!("run:{run_id}:step:{step_id}")
        }
    });
    let correlation_id =
        read_str(&body, &["correlation_id", "correlationId"]).unwrap_or_else(|| run_id.clone());
    let causation_id = read_str(&body, &["causation_id", "causationId"]).unwrap_or_default();
    let idempotency_key =
        read_str(&body, &["idempotency_key", "idempotencyKey"]).unwrap_or_else(|| {
            if step_id.is_empty() {
                format!("{run_id}:{event_type}")
            } else {
                format!("{run_id}:{step_id}")
            }
        });
    let producer =
        read_str(&body, &["producer", "_source"]).unwrap_or_else(|| "execution-core".to_owned());
    let type_url = read_str(&body, &["type_url", "typeUrl"])
        .unwrap_or_else(|| TOOL_COMPLETION_TYPE_URL.to_owned());
    let schema_version = read_u32(&body, &["schema_version", "schemaVersion"]).unwrap_or(1);
    let org_id = read_str(&body, &["org_id", "orgId"]).unwrap_or_default();
    let user_id = read_str(&body, &["user_id", "userId"]).unwrap_or_default();

    let result = sqlx::query(
        "
        INSERT INTO events (
            id, event_type, run_id, payload, ts,
            org_id, user_id, correlation_id, causation_id,
            idempotency_key, resource_ref, type_url, producer, schema_version
        )
        SELECT
            $1, $2, r.id, $3, now(),
            COALESCE(NULLIF($4, ''), r.org_id),
            COALESCE(NULLIF($5, ''), r.user_id),
            $6, $7, $8, $9, $10, $11, $12
        FROM runs r
        WHERE r.id = $13
        ",
    )
    .bind(&event_id)
    .bind(&event_type)
    .bind(&body)
    .bind(&org_id)
    .bind(&user_id)
    .bind(&correlation_id)
    .bind(&causation_id)
    .bind(derive_idempotency_hash(
        &producer,
        &event_type,
        &resource_ref,
        &idempotency_key,
    ))
    .bind(&resource_ref)
    .bind(&type_url)
    .bind(&producer)
    .bind(i32::try_from(schema_version).unwrap_or(i32::MAX))
    .bind(&run_id)
    .execute(pool)
    .await?;

    if result.rows_affected() == 0 {
        return Err(anyhow::anyhow!("run_id not found: {run_id}"));
    }

    Ok(Outcome::Stored)
}

fn read_message_event_id(payload: &[u8]) -> Option<String> {
    let body: Value = serde_json::from_slice(payload).ok()?;
    read_str(&body, &["event_id", "eventId"])
}

fn read_str(body: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        body.get(*key)
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .or_else(|| {
                body.get("data")?
                    .get(*key)
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
            })
    })
}

fn read_u32(body: &Value, keys: &[&str]) -> Option<u32> {
    keys.iter().find_map(|key| {
        body.get(*key)
            .and_then(Value::as_u64)
            .and_then(|v| u32::try_from(v).ok())
            .or_else(|| {
                body.get("data")?
                    .get(*key)
                    .and_then(Value::as_u64)
                    .and_then(|v| u32::try_from(v).ok())
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loop_checkpoint::test_support::{scratch_pool, seed_run};
    use serde_json::json;

    #[test]
    fn the_reserved_event_types_are_the_ones_the_fold_store_and_the_park_events_write() {
        let mut reserved = RESERVED_EVENT_TYPES.to_vec();
        reserved.sort_unstable();
        assert_eq!(
            reserved,
            [
                "CHECKPOINT_RESTORED",
                "CHECKPOINT_SAVED",
                "RUN_PARKED",
                "RUN_RESUMED"
            ]
        );
    }

    #[test]
    fn a_reserved_event_type_is_recognised_however_it_is_cased_or_padded() {
        for reserved in RESERVED_EVENT_TYPES {
            assert!(is_reserved_event_type(reserved), "{reserved}");
            assert!(
                is_reserved_event_type(&reserved.to_lowercase()),
                "{reserved}"
            );
            assert!(
                is_reserved_event_type(&format!("  {reserved}\t")),
                "{reserved}"
            );
        }
    }

    #[test]
    fn the_event_types_the_ingress_exists_for_are_not_reserved() {
        for allowed in [
            STEP_COMPLETED_EVENT_TYPE,
            "ACTION_COMPLETED",
            "RUN_STARTED",
            "RUN_PARKED_X",
            "RUN_PARK",
            "",
        ] {
            assert!(!is_reserved_event_type(allowed), "{allowed:?}");
        }
    }

    fn message(run_id: &str, event_type: Option<&str>, nested: bool) -> Vec<u8> {
        let mut body = json!({ "run_id": run_id, "step_id": "step-1" });
        if let Some(event_type) = event_type {
            if nested {
                body["data"] = json!({ "eventType": event_type });
            } else {
                body["event_type"] = json!(event_type);
            }
        }
        serde_json::to_vec(&body).expect("message json")
    }

    // A message of a reserved type is refused BEFORE the database is touched: the pool is lazy and
    // unreachable, so reaching the insert would answer an error, not `Refused`.
    #[tokio::test]
    async fn a_message_of_a_reserved_type_is_refused_before_the_database_is_reached() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .acquire_timeout(std::time::Duration::from_millis(200))
            .connect_lazy("postgresql://session-core@127.0.0.1:1/session_core")
            .expect("lazy pool");
        for reserved in RESERVED_EVENT_TYPES {
            for nested in [false, true] {
                let outcome = handle_message(&pool, &message("run-x", Some(reserved), nested))
                    .await
                    .expect("a refusal is not an error");
                assert_eq!(outcome, Outcome::Refused, "{reserved}, nested {nested}");
            }
        }
        // A type that is not reserved reaches the insert, which cannot be served here.
        assert!(
            handle_message(&pool, &message("run-x", Some("ACTION_COMPLETED"), false))
                .await
                .is_err()
        );
        assert!(handle_message(&pool, &message("run-x", None, false))
            .await
            .is_err());
    }

    // Every way a message can carry its event type is read by the filter, and when two keys
    // disagree the one that is checked is the one that is stored (the DB test below stores).
    #[tokio::test]
    async fn the_filter_reads_every_key_and_nesting_the_event_type_can_arrive_under() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .acquire_timeout(std::time::Duration::from_millis(200))
            .connect_lazy("postgresql://session-core@127.0.0.1:1/session_core")
            .expect("lazy pool");
        for reserved in RESERVED_EVENT_TYPES {
            for body in [
                json!({ "run_id": "run-x", "event_type": reserved }),
                json!({ "run_id": "run-x", "eventType": reserved }),
                json!({ "run_id": "run-x", "data": { "event_type": reserved } }),
                json!({ "run_id": "run-x", "data": { "eventType": reserved } }),
                // The first key read wins, whichever key the reserved type is under.
                json!({ "run_id": "run-x", "event_type": reserved, "eventType": "STEP_COMPLETED" }),
            ] {
                let payload = serde_json::to_vec(&body).expect("message json");
                let outcome = handle_message(&pool, &payload)
                    .await
                    .expect("a refusal is not an error");
                assert_eq!(outcome, Outcome::Refused, "{body}");
            }
        }
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn a_reserved_type_writes_no_row_and_an_ordinary_completion_still_does() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let (run_id, _org, _user) = seed_run(&pool, "persistent").await;
        let count = || async {
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM events WHERE run_id = $1")
                .bind(&run_id)
                .fetch_one(&pool)
                .await
                .expect("count events")
        };

        for reserved in RESERVED_EVENT_TYPES {
            let outcome = handle_message(&pool, &message(&run_id, Some(reserved), false))
                .await
                .expect("a refusal is not an error");
            assert_eq!(outcome, Outcome::Refused, "{reserved}");
        }
        assert_eq!(count().await, 0, "a refused message writes no events row");

        // The ingress still does what it exists for: the default type and another ordinary one.
        let stored = handle_message(&pool, &message(&run_id, None, false))
            .await
            .expect("an ordinary completion");
        assert_eq!(stored, Outcome::Stored);
        let other = serde_json::to_vec(&json!({
            "run_id": run_id,
            "step_id": "step-2",
            "event_type": "ACTION_COMPLETED",
        }))
        .expect("message json");
        assert_eq!(
            handle_message(&pool, &other).await.expect("another type"),
            Outcome::Stored
        );
        // Two keys that disagree: the type that is checked is the one that is stored. The first
        // key read is `event_type`, so the benign type is stored and the reserved one under the
        // other key never is.
        let conflicting = serde_json::to_vec(&json!({
            "run_id": run_id,
            "step_id": "step-3",
            "event_type": "ACTION_COMPLETED",
            "eventType": "RUN_PARKED",
        }))
        .expect("message json");
        assert_eq!(
            handle_message(&pool, &conflicting)
                .await
                .expect("conflicting keys"),
            Outcome::Stored
        );
        let types: Vec<String> = sqlx::query_scalar(
            "SELECT event_type FROM events WHERE run_id = $1 ORDER BY event_type",
        )
        .bind(&run_id)
        .fetch_all(&pool)
        .await
        .expect("event types");
        assert_eq!(
            types,
            ["ACTION_COMPLETED", "ACTION_COMPLETED", "STEP_COMPLETED"]
        );

        sqlx::query("DELETE FROM events WHERE run_id = $1")
            .bind(&run_id)
            .execute(&pool)
            .await
            .expect("cleanup events");
    }

    // The handler of this file must keep its refusal ahead of every use of the body and of the
    // insert, and must terminate (not NAK) what it refuses.
    #[test]
    fn the_refusal_comes_before_the_body_is_used_and_a_refused_message_is_terminated() {
        let source = include_str!("nats.rs").replace("\r\n", "\n");
        let production = &source[..source.find("\n#[cfg(test)]\nmod tests").expect("tests")];
        let start = production
            .find("async fn handle_message(")
            .expect("the handler");
        let handler = &production[start..];
        let at = |needle: &str| {
            handler
                .find(needle)
                .unwrap_or_else(|| panic!("the handler lost `{needle}`"))
        };
        assert!(
            at("if is_reserved_event_type(&event_type) {") < at("let step_id"),
            "the refusal is after the body is used"
        );
        assert!(
            at("return Ok(Outcome::Refused);") < at("INSERT INTO events"),
            "the refusal is after the insert"
        );
        // The type that is checked is the type that is stored: derived once, bound once.
        assert_eq!(
            handler.matches("let event_type").count(),
            1,
            "the event type is derived more than once"
        );
        assert_eq!(
            handler.matches(".bind(&event_type)").count(),
            1,
            "the stored event type is not the checked one"
        );
        // The loop terminates a refusal and never NAKs it (a NAK would redeliver it up to MaxDeliver).
        let run = &production[..start];
        let refused_arm = &run[run.find("Ok(Outcome::Refused) =>").expect("refused arm")..];
        let refused_arm = &refused_arm[..refused_arm.find("Err(e) =>").expect("error arm")];
        assert!(
            refused_arm.contains("AckKind::Term"),
            "a refusal is terminated"
        );
        assert!(!refused_arm.contains("Nak"), "a refusal is never NAKed");
    }
}
