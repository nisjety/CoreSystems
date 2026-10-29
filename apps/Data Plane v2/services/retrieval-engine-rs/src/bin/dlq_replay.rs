//! §16.2.4 — Dead-letter queue replay tool.
//!
//! Failed-after-N-retries messages land in `dataplane.dlq.*` subjects. They
//! pile up silently until someone notices. This binary drains a DLQ subject,
//! republishes each message to its original subject, and ACKs the DLQ entry
//! once the replay publish succeeds.
//!
//! ## D17 — why this tool changed
//!
//! It used to do a plain core-NATS `subscribe()` on the DLQ subject. Combined
//! with the fact that **no JetStream stream bound `dataplane.dlq.>`**, that
//! made the DLQ a black hole twice over: the publish had no subscriber and was
//! destroyed by the broker, and this tool could only ever catch a message that
//! happened to be published during the few seconds it was running. There was
//! no history to replay because nothing had ever stored one.
//!
//! `DATAPLANE_DLQ` (see `nats_connection::dlq`) now persists every
//! `dataplane.dlq.>` publish with `Limits` retention for 30 days, so this tool
//! replays **history** via a durable pull consumer.
//!
//! Usage:
//!   cargo run --bin dlq-replay -- \
//!       --dlq dataplane.dlq.embedding-engine \
//!       --target dataplane.knowledge.units.created \
//!       [--limit 100] [--dry-run] [--live]
//!
//! Modes:
//!   default    replay stored history through a durable consumer. The cursor
//!              persists, so a second run continues where the first stopped
//!              instead of re-replaying everything.
//!   --dry-run  peek from the beginning of the stream with a throwaway
//!              ephemeral consumer. Counts and logs; consumes nothing and
//!              never moves the durable cursor.
//!   --live     the pre-D17 behaviour: core-NATS subscribe, catching only what
//!              is published while the tool runs. Kept because it is the only
//!              mode that works against a broker where `DATAPLANE_DLQ` does
//!              not exist (an older deployment), and it is a useful tap while
//!              reproducing a failure by hand.
//!
//! Defaults:
//!   --limit 100
//!   --dry-run false
//!   --live false
//!
//! Env: `DPV2_NATS_URL` (or `SHARED_NATS_URL` / `NATS_URL`).
//!
//! Safety: each replayed message is republished as a FRESH NATS message, so
//! the consumer's `delivered` counter resets — which is exactly what we
//! want, otherwise the message would immediately re-DLQ.

use std::time::Duration;

use anyhow::{anyhow, Context};
use async_nats::jetstream::{self, consumer::PullConsumer};
use futures::StreamExt;

#[derive(Debug)]
struct Args {
    dlq: String,
    target: String,
    limit: usize,
    dry_run: bool,
    live: bool,
}

fn parse_args() -> anyhow::Result<Args> {
    let mut dlq: Option<String> = None;
    let mut target: Option<String> = None;
    let mut limit: usize = 100;
    let mut dry_run = false;
    let mut live = false;

    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--dlq" => dlq = it.next(),
            "--target" => target = it.next(),
            "--limit" => {
                limit = it
                    .next()
                    .and_then(|v| v.parse().ok())
                    .ok_or_else(|| anyhow!("--limit needs a number"))?;
            }
            "--dry-run" => dry_run = true,
            "--live" => live = true,
            "-h" | "--help" => {
                println!(
                    "dlq-replay --dlq <subject> --target <subject> \
                     [--limit N] [--dry-run] [--live]"
                );
                std::process::exit(0);
            }
            other => return Err(anyhow!("unknown arg: {other}")),
        }
    }

    Ok(Args {
        dlq: dlq.ok_or_else(|| anyhow!("--dlq required"))?,
        target: target.ok_or_else(|| anyhow!("--target required"))?,
        limit,
        dry_run,
        live,
    })
}

fn nats_url() -> anyhow::Result<String> {
    std::env::var("DPV2_NATS_URL")
        .or_else(|_| std::env::var("SHARED_NATS_URL"))
        .or_else(|_| std::env::var("NATS_URL"))
        .or_else(|_| std::env::var("NATS_LOCAL_URL"))
        .context("no NATS URL set (DPV2_NATS_URL / SHARED_NATS_URL / NATS_URL)")
}

/// Durable consumer name for a given DLQ subject.
///
/// One durable per replayed subject, so replaying `dataplane.dlq.graph-index`
/// does not advance the cursor of `dataplane.dlq.index-engine`. NATS consumer
/// names reject `.`, `*`, `>` and whitespace, so the subject is folded to a
/// safe token.
fn durable_name(dlq_subject: &str) -> String {
    let suffix: String = dlq_subject
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    format!("dlq-replay-{suffix}")
}

async fn open_consumer(js: &jetstream::Context, args: &Args) -> anyhow::Result<PullConsumer> {
    let stream = js
        .get_stream(nats_connection::DLQ_STREAM_NAME)
        .await
        .with_context(|| {
            format!(
                "stream {} not found — this broker predates the durable DLQ. \
                 Start any Data Plane consumer service to create it, or use \
                 --live for the legacy core-NATS tap.",
                nats_connection::DLQ_STREAM_NAME
            )
        })?;

    if args.dry_run {
        // Ephemeral + DeliverAll: reads history from the beginning and leaves
        // no trace, so a dry run never costs a real replay its position.
        let consumer = stream
            .create_consumer(jetstream::consumer::pull::Config {
                filter_subject: args.dlq.clone(),
                deliver_policy: jetstream::consumer::DeliverPolicy::All,
                ack_policy: jetstream::consumer::AckPolicy::None,
                inactive_threshold: Duration::from_secs(30),
                ..Default::default()
            })
            .await
            .context("create ephemeral dry-run consumer")?;
        return Ok(consumer);
    }

    let name = durable_name(&args.dlq);
    let consumer = stream
        .get_or_create_consumer(
            &name,
            jetstream::consumer::pull::Config {
                durable_name: Some(name.clone()),
                filter_subject: args.dlq.clone(),
                deliver_policy: jetstream::consumer::DeliverPolicy::All,
                ack_policy: jetstream::consumer::AckPolicy::Explicit,
                ack_wait: Duration::from_secs(30),
                ..Default::default()
            },
        )
        .await
        .with_context(|| format!("bind durable replay consumer {name}"))?;
    Ok(consumer)
}

async fn replay_from_stream(client: &async_nats::Client, args: &Args) -> anyhow::Result<usize> {
    let js = jetstream::new(client.clone());
    let consumer = open_consumer(&js, args).await?;

    let mut batch = consumer
        .batch()
        .max_messages(args.limit)
        .expires(Duration::from_secs(5))
        .messages()
        .await
        .context("fetch DLQ batch")?;

    let mut replayed = 0usize;
    while let Some(message) = batch.next().await {
        let msg = message.map_err(|e| anyhow!("read DLQ message: {e}"))?;
        if args.dry_run {
            tracing::info!(bytes = msg.payload.len(), "would replay (dry-run)");
        } else {
            client
                .publish(args.target.clone(), msg.payload.clone())
                .await
                .with_context(|| format!("publish to {}", args.target))?;
            // Ack only after the replay publish succeeded, so a crash between
            // the two redelivers rather than loses the dead letter.
            if let Err(e) = msg.ack().await {
                return Err(anyhow!("ack replayed DLQ message: {e}"));
            }
            tracing::info!(bytes = msg.payload.len(), "replayed");
        }
        replayed += 1;
    }
    Ok(replayed)
}

async fn replay_live(client: &async_nats::Client, args: &Args) -> anyhow::Result<usize> {
    tracing::warn!(
        "--live: core-NATS tap. Only messages published while this process runs \
         are seen; stored history in {} is NOT read.",
        nats_connection::DLQ_STREAM_NAME
    );
    let mut sub = client.subscribe(args.dlq.clone()).await?;
    sub.unsubscribe_after(args.limit as u64).await?;

    let mut replayed = 0usize;
    let timeout = Duration::from_secs(5);
    loop {
        let msg = match tokio::time::timeout(timeout, sub.next()).await {
            Ok(Some(m)) => m,
            Ok(None) | Err(_) => break, // unsubscribed or timed out
        };

        if args.dry_run {
            tracing::info!(bytes = msg.payload.len(), "would replay (dry-run)");
        } else {
            client
                .publish(args.target.clone(), msg.payload.clone())
                .await
                .with_context(|| format!("publish to {}", args.target))?;
            tracing::info!(bytes = msg.payload.len(), "replayed");
        }
        replayed += 1;
    }
    Ok(replayed)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let args = parse_args()?;
    let url = nats_url()?;

    tracing::info!(?args, %url, "dlq-replay starting");

    let client = nats_connection::connect(&url).await?;

    let replayed = if args.live {
        replay_live(&client, &args).await?
    } else {
        replay_from_stream(&client, &args).await?
    };

    if !args.dry_run {
        client.flush().await?;
    }
    tracing::info!(count = replayed, live = args.live, "dlq-replay done");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::durable_name;

    #[test]
    fn durable_name_is_a_legal_consumer_name() {
        let name = durable_name("dataplane.dlq.embedding-engine-page-images");
        assert_eq!(
            name,
            "dlq-replay-dataplane-dlq-embedding-engine-page-images"
        );
        assert!(!name.contains('.'), "consumer names reject '.'");
        assert!(!name.contains('*'));
        assert!(!name.contains('>'));
        assert!(!name.contains(' '));
    }

    #[test]
    fn each_dlq_subject_gets_its_own_cursor() {
        assert_ne!(
            durable_name("dataplane.dlq.index-engine"),
            durable_name("dataplane.dlq.graph-index"),
        );
    }
}
