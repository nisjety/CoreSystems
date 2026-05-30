//! NATS `JetStream` consumer for tool-completion events.
//!
//! Subscribes to `tools.completions.*`, decodes JSON payloads, and inserts a
//! `STEP_COMPLETED` row into the `events` table with
//! `org_id`/`user_id` hydrated from the `runs` table.

use crate::store::Pool;
use async_nats::jetstream::{
    self,
    consumer::{pull::Config as PullConfig, AckPolicy, DeliverPolicy},
    stream::{Config as StreamConfig, RetentionPolicy},
    AckKind,
};
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

pub async fn run(pool: Pool, nats_url: String) -> anyhow::Result<()> {
    info!(%nats_url, "session-core NATS consumer connecting");

    let compat_mode = CompatMode::from_env();
    let subscribed_subjects = subscriber_subjects(SUBJECT_FILTER, compat_mode)
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;

    let client = async_nats::connect(&nats_url).await?;
    let js = jetstream::new(client);

    // Ensure stream exists (idempotent).
    js.get_or_create_stream(StreamConfig {
        name: STREAM_NAME.to_string(),
        subjects: subscribed_subjects.clone(),
        retention: RetentionPolicy::Limits,
        max_age: Duration::from_secs(60 * 60 * 24 * 7),
        ..Default::default()
    })
    .await?;

    // Create durable pull consumer(s), one filter subject per consumer.
    let stream = js.get_stream(STREAM_NAME).await?;
    let mut consumer_streams = futures::stream::SelectAll::new();

    for (index, filter_subject) in subscribed_subjects.iter().enumerate() {
        let durable_name = if index == 0 {
            DURABLE_NAME.to_string()
        } else {
            format!("{DURABLE_NAME}-{index}")
        };

        let consumer = stream
            .get_or_create_consumer(
                &durable_name,
                PullConfig {
                    durable_name: Some(durable_name.clone()),
                    filter_subject: filter_subject.clone(),
                    ack_policy: AckPolicy::Explicit,
                    ack_wait: Duration::from_secs(30),
                    max_deliver: 5,
                    deliver_policy: DeliverPolicy::All,
                    ..Default::default()
                },
            )
            .await?;
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
            Ok(()) => {
                let elapsed = started.elapsed().as_secs_f64();
                histogram!("mp_session_events_persist_duration_seconds").record(elapsed);
                counter!("mp_session_events_total", "type" => "step_completed").increment(1);
                if let Err(e) = msg.ack().await {
                    warn!(error = %e, "ack failed");
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

async fn handle_message(pool: &Pool, payload: &[u8]) -> anyhow::Result<()> {
    let body: Value = serde_json::from_slice(payload)?;

    let run_id = read_str(&body, &["run_id", "runId"])
        .ok_or_else(|| anyhow::anyhow!("payload missing run_id"))?;
    let event_id = read_str(&body, &["event_id", "eventId"]).unwrap_or_else(mp_ids::new_ulid);
    let event_type = read_str(&body, &["event_type", "eventType"])
        .unwrap_or_else(|| STEP_COMPLETED_EVENT_TYPE.to_owned());
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

    Ok(())
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
