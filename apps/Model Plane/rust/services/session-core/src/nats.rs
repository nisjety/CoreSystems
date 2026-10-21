//! NATS `JetStream` consumer for tool-completion events.
//!
//! Subscribes to `tools.completions.*`, decodes JSON payloads, and inserts a
//! `STEP_COMPLETED` row into the `events` table with
//! `org_id`/`user_id` taken from the `runs` table. Only `STEP_COMPLETED` is stored (a message
//! of any other event type is refused), and every row names the tool-completion producer and type
//! URL and the run's own tenant and user, whatever the publisher sent.

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

/// The producer every stored row names. Never the publisher's: a publisher that names
/// `session-core` would make its row read as session-core's own.
const TOOL_COMPLETION_PRODUCER: &str = "execution-core";

/// What became of one message.
#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Stored,
    /// Not stored: its event type is not `STEP_COMPLETED`.
    Refused,
}

/// Whether `event_type` is `STEP_COMPLETED`, however it is cased or padded. This ingress is an
/// ALLOWLIST of that one type, the only one it exists for. Every other type is session-core's own
/// or another workflow's: the loop fold store writes `CHECKPOINT_SAVED` and `CHECKPOINT_RESTORED`
/// inside its own transactions, the park events `RUN_PARKED` and `RUN_RESUMED` are recorded only
/// through `RecordOrchestrationEvent` (where they are checked against the run's own fold, which
/// reads `CHECKPOINT_RESTORED` as the store's own record of a park claim), and a row of any of
/// them arriving over NATS would be a forgery by another door.
fn is_accepted_event_type(event_type: &str) -> bool {
    event_type
        .trim()
        .eq_ignore_ascii_case(STEP_COMPLETED_EVENT_TYPE)
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
                    "refused a tool completion whose event type is not STEP_COMPLETED"
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
    // Before anything else is derived from the body or written: the allowlist. What is stored is
    // then the constant, never the publisher's spelling of it.
    if !is_accepted_event_type(&event_type) {
        return Ok(Outcome::Refused);
    }
    let event_type = STEP_COMPLETED_EVENT_TYPE;
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
    // Neither the producer nor the type URL is the publisher's to choose: a row of this ingress
    // always names the tool-completion producer and the tool-completion message.
    let producer = TOOL_COMPLETION_PRODUCER;
    let type_url = TOOL_COMPLETION_TYPE_URL;
    let schema_version = read_u32(&body, &["schema_version", "schemaVersion"]).unwrap_or(1);

    // The tenant and the user of a row are the RUN'S, read from `runs` in the insert itself: the
    // message names neither (an `org_id` or `user_id` in it is ignored), so a publisher cannot
    // attribute a row to a tenant or a user the run does not belong to.
    let result = sqlx::query(
        "
        INSERT INTO events (
            id, event_type, run_id, payload, ts,
            org_id, user_id, correlation_id, causation_id,
            idempotency_key, resource_ref, type_url, producer, schema_version
        )
        SELECT
            $1, $2, r.id, $3, now(),
            r.org_id, r.user_id,
            $4, $5, $6, $7, $8, $9, $10
        FROM runs r
        WHERE r.id = $11
        ",
    )
    .bind(&event_id)
    .bind(event_type)
    .bind(&body)
    .bind(&correlation_id)
    .bind(&causation_id)
    .bind(derive_idempotency_hash(
        producer,
        event_type,
        &resource_ref,
        &idempotency_key,
    ))
    .bind(&resource_ref)
    .bind(type_url)
    .bind(producer)
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

    /// Event types session-core writes itself (see [`is_accepted_event_type`]), and two ordinary
    /// ones another workflow might use: none of them is this ingress's to store.
    const NOT_STEP_COMPLETED: [&str; 8] = [
        "RUN_PARKED",
        "RUN_RESUMED",
        "CHECKPOINT_SAVED",
        "CHECKPOINT_RESTORED",
        "ACTION_COMPLETED",
        "RUN_STARTED",
        "RUN_COMPLETED",
        "THREAD_CREATED",
    ];

    #[test]
    fn only_step_completed_is_accepted_however_it_is_cased_or_padded() {
        for accepted in [
            "STEP_COMPLETED",
            "step_completed",
            "Step_Completed",
            "  STEP_COMPLETED\t",
        ] {
            assert!(is_accepted_event_type(accepted), "{accepted:?}");
        }
        for refused in NOT_STEP_COMPLETED.iter().copied().chain([
            "",
            " ",
            "STEP",
            "STEP_COMPLETED_X",
            "X_STEP_COMPLETED",
            "STEP COMPLETED",
            "RUN_PARKED\0STEP_COMPLETED",
        ]) {
            assert!(!is_accepted_event_type(refused), "{refused:?}");
            assert!(
                !is_accepted_event_type(&refused.to_lowercase()),
                "{refused:?}"
            );
        }
    }

    #[test]
    fn the_stored_producer_and_type_url_are_the_tool_completion_ones() {
        assert_eq!(STEP_COMPLETED_EVENT_TYPE, "STEP_COMPLETED");
        assert_eq!(TOOL_COMPLETION_PRODUCER, "execution-core");
        assert_eq!(
            TOOL_COMPLETION_TYPE_URL,
            "type.googleapis.com/model_plane.v1.ToolCompletion"
        );
    }

    fn lazy_pool() -> Pool {
        sqlx::postgres::PgPoolOptions::new()
            .acquire_timeout(std::time::Duration::from_millis(200))
            .connect_lazy("postgresql://session-core@127.0.0.1:1/session_core")
            .expect("lazy pool")
    }

    async fn outcome_of(pool: &Pool, body: &Value) -> anyhow::Result<Outcome> {
        handle_message(pool, &serde_json::to_vec(body).expect("message json")).await
    }

    // A message that is not STEP_COMPLETED is refused BEFORE the database is reached: the pool is
    // lazy and unreachable, so reaching the insert would answer an error, not `Refused`. Every key
    // and nesting the event type can arrive under is read, and when two keys disagree the first
    // one read decides, for the check and for what would be stored.
    #[tokio::test]
    async fn a_message_of_another_type_is_refused_before_the_database_is_reached() {
        let pool = lazy_pool();
        for other in NOT_STEP_COMPLETED {
            for body in [
                json!({ "run_id": "run-x", "event_type": other }),
                json!({ "run_id": "run-x", "eventType": other }),
                json!({ "run_id": "run-x", "data": { "event_type": other } }),
                json!({ "run_id": "run-x", "data": { "eventType": other } }),
                json!({ "run_id": "run-x", "event_type": other, "eventType": "STEP_COMPLETED" }),
            ] {
                let outcome = outcome_of(&pool, &body)
                    .await
                    .expect("a refusal is not an error");
                assert_eq!(outcome, Outcome::Refused, "{body}");
            }
        }
        // STEP_COMPLETED, named or by default, reaches the insert, which cannot be served here.
        for body in [
            json!({ "run_id": "run-x" }),
            json!({ "run_id": "run-x", "event_type": "STEP_COMPLETED" }),
            json!({ "run_id": "run-x", "eventType": " step_completed " }),
            json!({ "run_id": "run-x", "event_type": "STEP_COMPLETED", "eventType": "RUN_PARKED" }),
        ] {
            assert!(outcome_of(&pool, &body).await.is_err(), "{body}");
        }
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn only_step_completed_is_stored_under_the_fixed_producer_type_url_and_the_runs_tenant() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let (run_id, org, user) = seed_run(&pool, "persistent").await;
        let rows = || async {
            sqlx::query_as::<_, (String, String, String, String, String)>(
                "SELECT event_type, type_url, producer, org_id, user_id FROM events \
                 WHERE run_id = $1 ORDER BY resource_ref",
            )
            .bind(&run_id)
            .fetch_all(&pool)
            .await
            .expect("read events")
        };

        // Every other type is refused and writes no row.
        for other in NOT_STEP_COMPLETED {
            let body = json!({ "run_id": run_id, "step_id": "refused", "event_type": other });
            assert_eq!(
                outcome_of(&pool, &body)
                    .await
                    .expect("a refusal is not an error"),
                Outcome::Refused,
                "{other}"
            );
        }
        assert!(rows().await.is_empty(), "a refused message writes no row");

        // STEP_COMPLETED is stored: by default, named, spelled any way, or first of two
        // disagreeing keys. What is stored is always the constant, and the publisher's own
        // producer, type URL, org and user are ignored, under both spellings of each key, nested
        // or not (the row's tenant and user are the run's).
        let publisher_chosen = [
            json!({
                "step_id": "s1",
            }),
            json!({
                "step_id": "s2", "event_type": "step_completed",
            }),
            json!({
                "step_id": "s3", "event_type": "STEP_COMPLETED", "eventType": "RUN_PARKED",
            }),
            json!({
                "step_id": "s4", "producer": "session-core",
                "type_url": "type.googleapis.com/model_plane.v1.CheckpointSaved",
            }),
            json!({
                "step_id": "s5", "_source": "session-core",
                "typeUrl": "type.googleapis.com/model_plane.v1.CheckpointRestored",
            }),
            json!({
                "step_id": "s6", "data": { "producer": "session-core",
                "type_url": "type.googleapis.com/model_plane.v1.RunParked" },
            }),
            json!({
                "step_id": "s10", "event_type": " STEP_COMPLETED\t",
            }),
            json!({
                "step_id": "s7", "org_id": "org-forged", "user_id": "user-forged",
            }),
            json!({
                "step_id": "s8", "orgId": "org-forged", "userId": "user-forged",
            }),
            json!({
                "step_id": "s9", "data": { "org_id": "org-forged", "userId": "user-forged" },
            }),
        ];
        for mut body in publisher_chosen {
            body["run_id"] = json!(run_id);
            assert_eq!(
                outcome_of(&pool, &body).await.expect("a completion"),
                Outcome::Stored,
                "{body}"
            );
        }
        let stored = rows().await;
        assert_eq!(stored.len(), 10);
        for (event_type, type_url, producer, org_id, user_id) in stored {
            assert_eq!(event_type, "STEP_COMPLETED");
            assert_eq!(
                type_url,
                "type.googleapis.com/model_plane.v1.ToolCompletion"
            );
            assert_eq!(producer, "execution-core");
            assert_eq!(org_id, org, "the row's tenant is the run's");
            assert_eq!(user_id, user, "the row's user is the run's");
        }

        sqlx::query("DELETE FROM events WHERE run_id = $1")
            .bind(&run_id)
            .execute(&pool)
            .await
            .expect("cleanup events");
    }

    // The handler of this file must keep its allowlist ahead of every other use of the body and of
    // the insert, store the constants and never the publisher's type, producer or type URL, and
    // must terminate (not NAK) what it refuses.
    #[test]
    fn the_allowlist_comes_first_the_row_is_fixed_and_a_refusal_is_terminated() {
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
        // Read once, checked, then replaced by the constant before anything else is derived.
        let order = [
            "let event_type = read_str(",
            "if !is_accepted_event_type(&event_type) {",
            "return Ok(Outcome::Refused);",
            "let event_type = STEP_COMPLETED_EVENT_TYPE;",
            "let step_id",
            "INSERT INTO events",
        ];
        let mut last = 0;
        for needle in order {
            let found = at(needle);
            assert!(found >= last, "`{needle}` is out of order in the handler");
            last = found;
        }
        assert_eq!(
            handler.matches("let event_type").count(),
            2,
            "the event type is derived more than twice"
        );
        assert_eq!(
            handler.matches(".bind(event_type)").count(),
            1,
            "the stored event type is not the one the allowlist left"
        );
        // The producer and the type URL are the constants, never read from the message.
        for needle in [
            "let producer = TOOL_COMPLETION_PRODUCER;",
            "let type_url = TOOL_COMPLETION_TYPE_URL;",
            ".bind(type_url)",
            ".bind(producer)",
        ] {
            assert!(handler.contains(needle), "the handler lost `{needle}`");
        }
        for key in [
            "\"producer\"",
            "\"_source\"",
            "\"type_url\"",
            "\"typeUrl\"",
            "\"org_id\"",
            "\"orgId\"",
            "\"user_id\"",
            "\"userId\"",
        ] {
            assert!(
                !handler.contains(key),
                "the handler reads the publisher's {key}"
            );
        }
        // The tenant and the user come from `runs` in the insert itself, with no override.
        assert!(
            handler.contains("r.org_id, r.user_id,"),
            "the row's tenant and user are not the run's"
        );
        assert!(
            !handler.contains("COALESCE"),
            "a value of the message overrides the run's"
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
