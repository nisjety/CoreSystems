//! The batch pipeline against the rules it really runs under.
//!
//! Every test here that touches the database is gated on
//! `MIGRATED_TEST_DATABASE_URL` (see `testing`) and FAILS, rather than skips, when it
//! is not set: a contract that does not run proves nothing. They run on the real
//! migrated schema as the superuser every Data Plane service connects as, with the
//! mixed-organization batches the stream really builds.
//!
//! What each invariant of the design is proved by:
//!
//! * I1, the vector is a function of the text and the PERSISTED context: the text the
//!   embedder receives equals `compose(context in the database, text)`, and the
//!   database already holds that context when the embedder and the vector store are
//!   called (the fakes record the rows as they are at that moment).
//! * I2, provenance with the context: a trigger records, for every statement that
//!   changes `chunk_context`, whether the same statement also set the provenance.
//! * I3, nothing is silent: a batch that contains every kind of outcome leaves no unit
//!   without a status, and every unit that is not ready says why.
//! * I4, one tenant cannot hold up another: a provider that never answers for one
//!   organization leaves that organization's units pending while another
//!   organization's units in the same batch are contextualized and embedded.
//! * I7, a re-run costs nothing: a second run, and a redelivery after each kind of
//!   failure, make no further provider call.

use std::time::{Duration, Instant as StdInstant};

use chrono::{DateTime, Utc};
use futures::FutureExt;
use sqlx::PgPool;

use super::store;
use super::testing::{vector_for, Doc, Posture, Rig, Unit, UnitRow, COLLECTION, EMBEDDING_MODEL};
use super::Pipeline;
use crate::context::prompt::{compose_contextualized, stored_context_hash};
use crate::context::{ChunkInput, DocumentInput, DocumentMeta, InlineBudget, PromptVersion};
use crate::policy::{PolicyReader, ReadError};
use crate::provider::contextualize::testing::{context_for, Behaviour};

/// A batch that is not finished by now is a defect (the inline budgets are
/// milliseconds here), and a test must fail on it rather than hang.
const WITHIN: Duration = Duration::from_secs(30);

async fn run_within(rig: &Rig, items: Vec<super::BatchItem>) {
    tokio::time::timeout(WITHIN, rig.run(items))
        .await
        .unwrap_or_else(|_| panic!("the batch was not finished within {WITHIN:?}"))
        .expect("the batch succeeds");
}

const CHUNKS: [&str; 3] = [
    "Driftsmarginen økte til 31 prosent.",
    "Røyking av laks i Tromsø ble utvidet til to nye anlegg.",
    "Fjordfisk AS åpner et nytt anlegg i Hammerfest i 2026.",
];

fn composed(unit: &Unit) -> String {
    compose_contextualized(&context_for(&unit.text), &unit.text)
}

fn hash_of(context: &str) -> String {
    stored_context_hash(context)
}

/// What a context generated now for `unit` would record as its input hash.
fn input_hash(rig: &Rig, doc: &Doc, unit: &Unit) -> String {
    let prepared = rig.engine.prepare(DocumentInput {
        org_id: &doc.org,
        document_id: &doc.id,
        meta: DocumentMeta {
            title: &doc.title,
            source: "test",
            date: "",
        },
        text: &doc.content,
        restricted: false,
        prompt_version: None,
        total_chunks: Some(doc.units.len()),
    });
    rig.engine.input_hash(
        &prepared,
        &ChunkInput {
            index: unit.index,
            text: &unit.text,
        },
    )
}

fn seconds_from_now(at: Option<DateTime<Utc>>) -> i64 {
    (at.expect("a time was recorded") - Utc::now()).num_seconds()
}

fn assert_ready(row: &UnitRow, unit: &Unit, hash: &str) {
    let context = context_for(&unit.text);
    assert_eq!(row.knowledge_id, unit.knowledge_id);
    assert_eq!(row.chunk_context.as_deref(), Some(context.as_str()));
    assert_eq!(row.context_status.as_deref(), Some("ready"));
    assert_eq!(row.context_input_hash.as_deref(), Some(hash));
    // The provenance: the configured model alias (what the hash was made with), the
    // provider and residency the answer says it came from, and the prompt.
    assert_eq!(row.context_model.as_deref(), Some("gpt-4o-mini"));
    assert_eq!(row.context_provider.as_deref(), Some("azure-openai"));
    assert_eq!(row.context_residency.as_deref(), Some("eu"));
    assert_eq!(row.context_prompt_version.as_deref(), Some("ctx-v1"));
    assert!(row.context_generated_at.is_some());
    assert_eq!(row.context_error_class, None);
    assert_eq!(row.context_attempts, 0);
}

async fn install(pool: &PgPool, sql: String) {
    sqlx::raw_sql(&sql)
        .execute(pool)
        .await
        .expect("install a test trigger");
}

fn ready_write<'a>(unit: store::UnitRef<'a>, hash: &'a str) -> store::ReadyWrite<'a> {
    store::ReadyWrite {
        unit,
        expected_hash: None,
        context: "Kontekst skrevet av avstemmeren.",
        prompt_version: "ctx-v1",
        model: "gpt-4o-mini",
        provider: "azure-openai",
        residency: "eu",
        input_hash: hash,
    }
}

fn pending_status(unit: store::UnitRef<'_>) -> store::StatusWrite<'_> {
    store::StatusWrite {
        unit,
        expected_hash: None,
        status: "pending",
        error_class: Some("timeout"),
        count_attempt: false,
        due_in: Some(Duration::ZERO),
    }
}

fn done_write(unit: store::UnitRef<'_>) -> store::Done<'_> {
    store::Done {
        unit,
        context_hash: None,
    }
}

// ── write-ahead, vector coherence, provenance ───────────────────────────────

/// I1 and the write-ahead order: the context is in the database before anything is
/// embedded or stored, the vector is built from exactly it, and the unit is marked
/// done last, in one statement that also records what the vector was built from.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn the_context_is_persisted_before_the_vector_is_built_from_it() {
    let rig = Rig::new().await;
    let org = rig.org("wa");
    let doc = rig.plain_document(&org, &CHUNKS).await;
    rig.watch.set(doc.ids());

    rig.run(rig.items(&doc)).await.expect("the batch succeeds");

    // The end state of every unit.
    for (unit, row) in doc.units.iter().zip(rig.rows(&doc).await) {
        let context = context_for(&unit.text);
        assert_ready(&row, unit, &input_hash(&rig, &doc, unit));
        assert_eq!(row.embedding_status, "done");
        assert_eq!(row.embedding_model.as_deref(), Some(EMBEDDING_MODEL));
        assert_eq!(
            row.embedded_context_hash.as_deref(),
            Some(hash_of(&context).as_str()),
            "the vector says which context it was built from"
        );
        assert_eq!(
            row.embedded_at,
            Some(row.updated_at),
            "nothing writes the unit after it is marked done: a later write would make the stale-embedding detector flag it"
        );
    }

    // I1: what was embedded is the composition of the persisted context and the text.
    let mut embedded = rig.embedder.texts();
    embedded.sort();
    let mut expected: Vec<String> = doc.units.iter().map(composed).collect();
    expected.sort();
    assert_eq!(embedded, expected);
    assert!(rig
        .embedder
        .calls()
        .iter()
        .all(|call| !call.zdr && call.org_id == org));

    // Write-ahead: when the embedder was called, and again when the vectors were
    // stored, every context was already in the database, ready, with its input hash,
    // and no unit was done yet.
    let seen: Vec<_> = rig
        .embedder
        .calls()
        .into_iter()
        .map(|call| call.seen)
        .chain(rig.vectors.seen_at_upserts())
        .collect();
    assert_eq!(seen.len(), 2);
    for moment in seen {
        assert_eq!(moment.len(), doc.units.len());
        for unit in &doc.units {
            let seen = moment
                .iter()
                .find(|seen| seen.knowledge_id == unit.knowledge_id)
                .expect("the unit was watched");
            assert_eq!(
                seen.context.as_deref(),
                Some(context_for(&unit.text).as_str())
            );
            assert_eq!(seen.status.as_deref(), Some("ready"));
            assert!(seen.input_hash.is_some());
            assert_ne!(seen.embedding_status, "done");
        }
    }

    // The vector of each unit is the vector of its composed text, and the point
    // carries the chunk's own words, not the composition.
    for unit in &doc.units {
        let point = rig.vectors.stored(&unit.knowledge_id).expect("a point");
        assert_eq!(point.vector, vector_for(&composed(unit)));
        assert_eq!(point.text, unit.text);
    }

    // The announcement came last, after every unit was done and kept its context.
    let announced = rig.events.on("dataplane.documents.indexed");
    assert_eq!(announced.len(), 1);
    assert_eq!(announced[0].payload["document_id"], doc.id.as_str());
    for seen in &announced[0].seen {
        assert_eq!(seen.embedding_status, "done");
        assert!(seen.context.is_some());
    }
    assert_eq!(rig.model.call_count(), CHUNKS.len());
    assert!(rig.model.calls().iter().all(|call| !call.zdr));
}

/// The payload Qdrant receives is the one the retrieval engine reads, plus one key: the
/// `source` of the unit's document, read from the migrated `documents` table. CR-T8 changed
/// what is embedded and asserted the payload unchanged; the corpus scope (QB-M1) adds the
/// stamp here and nothing else.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn the_qdrant_payload_carries_the_documents_source() {
    let rig = Rig::new().await;
    let org = rig.org("pl");
    let doc = rig.plain_document(&org, &CHUNKS[..2]).await;
    rig.run(rig.items(&doc)).await.expect("the batch succeeds");

    let quarry = rig
        .document_from("quarry", &org, "internal", &CHUNKS[2..])
        .await;
    let mut dated = rig.items(&quarry);
    dated[0].document_date = Some(Utc::now());
    rig.run(dated).await.expect("the dated batch succeeds");

    let points = rig.vectors.points();
    assert_eq!(points.len(), 3);
    for point in &points {
        assert_eq!(point.org_id, org);
    }
    assert_eq!(points[0].document_id, doc.id);
    assert_eq!(points[0].chunk_index, 0);
    assert_eq!(points[1].chunk_index, 1);
    assert!(points[..2].iter().all(|point| point.metadata.is_empty()));
    assert!(points[2].metadata.contains_key("document_date"));
    // The stamp is the document's `source`, as the table has it: the rig's default
    // source for the first document, `quarry` for the second.
    assert_eq!(points[0].source.as_deref(), Some("test"));
    assert_eq!(points[1].source.as_deref(), Some("test"));
    assert_eq!(points[2].source.as_deref(), Some("quarry"));
    let plain: std::collections::BTreeSet<String> = [
        "knowledge_id",
        "document_id",
        "org_id",
        "chunk_index",
        "text",
        "source",
    ]
    .iter()
    .map(|key| (*key).to_owned())
    .collect();
    for point in &points[..2] {
        assert_eq!(point.payload_keys, plain);
    }
    let mut with_date = plain;
    with_date.insert("document_date".to_owned());
    assert_eq!(points[2].payload_keys, with_date);
    assert_eq!(
        rig.vectors.stored(&points[0].knowledge_id).unwrap().text,
        CHUNKS[0]
    );
    assert_eq!(rig.vectors.collections(), [COLLECTION, COLLECTION]);
}

/// The stamp is what the database says when the point is written, not what the event that
/// queued the unit said or what an earlier write of the same unit said: a document whose
/// source changed is stamped with the new one the next time its unit is embedded.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_changed_source_reaches_the_point_the_next_time_the_unit_is_embedded() {
    let rig = Rig::new().await;
    let org = rig.org("sc");
    let doc = rig
        .document_from("sharepoint", &org, "internal", &CHUNKS[..1])
        .await;
    let kid = doc.units[0].knowledge_id.clone();

    rig.run(rig.items(&doc)).await.expect("the batch succeeds");
    assert_eq!(
        rig.vectors.stored(&kid).unwrap().source.as_deref(),
        Some("sharepoint")
    );

    // The document is re-ingested from another source with the same chunks (the case in
    // which nothing re-chunks it), and the unit is embedded again.
    sqlx::query("UPDATE documents SET source = 'quarry' WHERE document_id = $1 AND org_id = $2")
        .bind(&doc.id)
        .bind(&org)
        .execute(&rig.pool)
        .await
        .expect("change the source");
    rig.run(rig.items(&doc))
        .await
        .expect("the second batch succeeds");

    assert_eq!(
        rig.vectors.stored(&kid).unwrap().source.as_deref(),
        Some("quarry"),
        "the point follows the document, not the first write"
    );
}

/// Documents of different sources and different organisations in one batch, each unit
/// stamped from its own document: the batch is mixed on purpose and runs unscoped, so the
/// lookup is the only thing keeping one organisation's document from lending its source to
/// another's unit.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn each_unit_in_a_mixed_batch_is_stamped_from_its_own_document() {
    let rig = Rig::new().await;
    let org_a = rig.org("ma");
    let org_b = rig.org("mb");
    let quarry = rig
        .document_from("quarry", &org_a, "internal", &CHUNKS[..2])
        .await;
    let upload = rig
        .document_from("chat-upload", &org_b, "internal", &CHUNKS[2..])
        .await;

    let mut items = rig.items(&quarry);
    items.extend(rig.items(&upload));
    rig.run(items).await.expect("the mixed batch succeeds");

    for unit in &quarry.units {
        let point = rig.vectors.stored(&unit.knowledge_id).expect("a point");
        assert_eq!(point.org_id, org_a);
        assert_eq!(point.source.as_deref(), Some("quarry"));
    }
    for unit in &upload.units {
        let point = rig.vectors.stored(&unit.knowledge_id).expect("a point");
        assert_eq!(point.org_id, org_b);
        assert_eq!(point.source.as_deref(), Some("chat-upload"));
    }
}

/// I2: no statement leaves a context without its provenance. A trigger records, for
/// every UPDATE that changes `chunk_context`, whether the very same statement also
/// set the status and every provenance column.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_context_is_never_written_without_its_provenance_in_the_same_statement() {
    let rig = Rig::new().await;
    let org = rig.org("i2");
    let tag = org.replace('-', "_").to_lowercase();
    let table = format!("crt8_audit_{tag}");
    install(
        &rig.pool,
        format!(
            "CREATE TABLE {table} (knowledge_id text NOT NULL, complete boolean NOT NULL);
             CREATE FUNCTION {table}_fn() RETURNS trigger LANGUAGE plpgsql AS $fn$
             BEGIN
               IF NEW.org_id = '{org}' AND NEW.chunk_context IS DISTINCT FROM OLD.chunk_context THEN
                 INSERT INTO {table} VALUES (NEW.knowledge_id, COALESCE(
                   NEW.context_status = 'ready' AND NEW.context_model IS NOT NULL
                   AND NEW.context_provider IS NOT NULL AND NEW.context_residency IS NOT NULL
                   AND NEW.context_prompt_version IS NOT NULL AND NEW.context_input_hash IS NOT NULL
                   AND NEW.context_generated_at IS NOT NULL, false));
               END IF;
               RETURN NEW;
             END $fn$;
             CREATE TRIGGER {table}_tr AFTER UPDATE ON knowledge_units
               FOR EACH ROW EXECUTE FUNCTION {table}_fn();"
        ),
    )
    .await;

    let doc = rig.plain_document(&org, &CHUNKS).await;
    rig.run(rig.items(&doc)).await.expect("the batch succeeds");

    let writes: Vec<(String, bool)> =
        sqlx::query_as(&format!("SELECT knowledge_id, complete FROM {table}"))
            .fetch_all(&rig.pool)
            .await
            .expect("read the audit");
    install(
        &rig.pool,
        format!(
            "DROP TRIGGER {table}_tr ON knowledge_units; DROP FUNCTION {table}_fn(); DROP TABLE {table};"
        ),
    )
    .await;

    assert_eq!(
        writes.len(),
        CHUNKS.len(),
        "each context is written exactly once: {writes:?}"
    );
    assert!(
        writes.iter().all(|(_, complete)| *complete),
        "a context was written without its provenance: {writes:?}"
    );
}

/// I7: a re-run with unchanged inputs makes zero provider calls, rewrites no context,
/// and still builds the same vectors from the same persisted contexts.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_second_run_makes_no_provider_call_and_rewrites_nothing() {
    let rig = Rig::new().await;
    let org = rig.org("i7");
    let doc = rig.plain_document(&org, &CHUNKS).await;
    rig.run(rig.items(&doc)).await.expect("first run");
    let first = rig.rows(&doc).await;
    assert_eq!(rig.model.call_count(), CHUNKS.len());

    rig.run(rig.items(&doc)).await.expect("second run");

    assert_eq!(
        rig.model.call_count(),
        CHUNKS.len(),
        "the second run asked the model again"
    );
    for (before, after) in first.iter().zip(rig.rows(&doc).await) {
        assert_eq!(after.chunk_context, before.chunk_context);
        assert_eq!(after.context_input_hash, before.context_input_hash);
        assert_eq!(
            after.context_generated_at, before.context_generated_at,
            "the context was rewritten"
        );
        assert_eq!(after.context_status.as_deref(), Some("ready"));
        assert_eq!(after.embedded_context_hash, before.embedded_context_hash);
    }
    // The vectors were built again (a re-embed was asked for), from the same contexts.
    assert_eq!(rig.embedder.calls().len(), 2);
    let mut texts = rig.embedder.texts();
    texts.sort();
    let mut expected: Vec<String> = doc
        .units
        .iter()
        .flat_map(|unit| [composed(unit), composed(unit)])
        .collect();
    expected.sort();
    assert_eq!(texts, expected);
}

// ── failure injection: every failure converges, and pays once ──────────────

/// The embedding model fails after the contexts were written. The batch fails (the
/// stream retries it), the contexts are in the database, and the retry generates
/// nothing: it embeds from what is stored.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_failure_in_the_embedding_leaves_the_context_and_the_retry_regenerates_nothing() {
    let rig = Rig::new().await;
    let org = rig.org("fe");
    let doc = rig.plain_document(&org, &CHUNKS).await;
    rig.embedder.fail_next(1);

    rig.run(rig.items(&doc))
        .await
        .expect_err("an embedding failure fails the batch (the broker redelivers it)");

    assert_eq!(rig.model.call_count(), CHUNKS.len());
    assert_eq!(rig.vectors.upsert_count(), 0);
    for (unit, row) in doc.units.iter().zip(rig.rows(&doc).await) {
        assert_ready(&row, unit, &input_hash(&rig, &doc, unit));
        assert_ne!(row.embedding_status, "done");
        assert_eq!(row.embedded_context_hash, None, "no vector exists yet");
        assert_ne!(
            row.embedding_status, "failed",
            "D18: the pipeline never writes the terminal failure"
        );
    }

    rig.run(rig.items(&doc)).await.expect("the redelivery");

    assert_eq!(
        rig.model.call_count(),
        CHUNKS.len(),
        "the redelivery paid for the contexts again"
    );
    for (unit, row) in doc.units.iter().zip(rig.rows(&doc).await) {
        assert_eq!(row.embedding_status, "done");
        assert_eq!(
            row.embedded_context_hash.as_deref(),
            Some(hash_of(&context_for(&unit.text)).as_str())
        );
        assert_eq!(
            rig.vectors.stored(&unit.knowledge_id).unwrap().vector,
            vector_for(&composed(unit))
        );
    }
}

/// The vector store fails after the embedding. Same convergence, and nothing is
/// marked done for a vector that does not exist.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_failure_in_the_vector_store_converges_after_a_retry() {
    let rig = Rig::new().await;
    let org = rig.org("fv");
    let doc = rig.plain_document(&org, &CHUNKS).await;
    rig.vectors.fail_next(1);

    rig.run(rig.items(&doc))
        .await
        .expect_err("the store failed");

    assert_eq!(rig.embedder.calls().len(), 1);
    assert!(rig.vectors.points().is_empty());
    for row in rig.rows(&doc).await {
        assert_ne!(row.embedding_status, "done", "marked done without a vector");
        assert!(
            row.chunk_context.is_some(),
            "the write-ahead context survives"
        );
    }

    rig.run(rig.items(&doc)).await.expect("the redelivery");

    assert_eq!(rig.model.call_count(), CHUNKS.len());
    for (unit, row) in doc.units.iter().zip(rig.rows(&doc).await) {
        assert_eq!(row.embedding_status, "done");
        assert_eq!(
            row.embedded_context_hash.as_deref(),
            Some(hash_of(&context_for(&unit.text)).as_str())
        );
    }
}

/// The database fails at the last step, after the vectors are stored. The vector
/// exists and the unit is not done; the retry stores it again (an upsert) and marks it
/// done with the hash of the context it was built from. Injected for real, by a
/// trigger that refuses the transition to done for this organization only.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_failure_after_the_vector_and_before_done_converges_after_a_retry() {
    let rig = Rig::new().await;
    let org = rig.org("fd");
    let tag = org.replace('-', "_").to_lowercase();
    let name = format!("crt8_refuse_{tag}");
    install(
        &rig.pool,
        format!(
            "CREATE FUNCTION {name}_fn() RETURNS trigger LANGUAGE plpgsql AS $fn$
             BEGIN
               IF NEW.org_id = '{org}' AND NEW.embedding_status = 'done'
                  AND OLD.embedding_status IS DISTINCT FROM 'done' THEN
                 RAISE EXCEPTION 'injected failure at mark-done';
               END IF;
               RETURN NEW;
             END $fn$;
             CREATE TRIGGER {name}_tr BEFORE UPDATE ON knowledge_units
               FOR EACH ROW EXECUTE FUNCTION {name}_fn();"
        ),
    )
    .await;
    let doc = rig.plain_document(&org, &CHUNKS).await;

    let failed = rig.run(rig.items(&doc)).await;
    install(
        &rig.pool,
        format!("DROP TRIGGER {name}_tr ON knowledge_units; DROP FUNCTION {name}_fn();"),
    )
    .await;
    failed.expect_err("the database refused the last step");

    assert_eq!(rig.vectors.upsert_count(), 1, "the vectors were stored");
    for row in rig.rows(&doc).await {
        assert_ne!(row.embedding_status, "done");
        assert_eq!(row.embedded_context_hash, None);
        assert!(row.chunk_context.is_some());
    }

    rig.run(rig.items(&doc)).await.expect("the redelivery");

    assert_eq!(
        rig.model.call_count(),
        CHUNKS.len(),
        "nothing was paid for twice"
    );
    for (unit, row) in doc.units.iter().zip(rig.rows(&doc).await) {
        assert_eq!(row.embedding_status, "done");
        assert_eq!(
            row.embedded_context_hash.as_deref(),
            Some(hash_of(&context_for(&unit.text)).as_str())
        );
        assert_eq!(
            rig.vectors.stored(&unit.knowledge_id).unwrap().vector,
            vector_for(&composed(unit))
        );
    }
}

/// A regeneration that fails never overwrites the context that was in force, and the
/// vector that is built (the batch was asked to embed again) is built from that
/// context, not from nothing: the unit stays coherent. A later regeneration that
/// succeeds replaces it and clears the error.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_failed_regeneration_keeps_the_previous_context_and_its_vector() {
    let rig = Rig::new().await;
    let org = rig.org("rg");
    let doc = rig.plain_document(&org, &CHUNKS).await;
    rig.run(rig.items(&doc)).await.expect("first run");
    let first = rig.rows(&doc).await;
    let calls_before = rig.model.call_count();

    // The document changes under its chunks: the window each context was read in is
    // no longer the one a new generation would read, so the stored contexts are stale.
    sqlx::query("UPDATE documents SET content = content || E'\\n\\nTillegg: ny avsluttende setning.' WHERE document_id = $1")
        .bind(&doc.id)
        .execute(&rig.pool)
        .await
        .expect("edit the document");
    let changed = Doc {
        content: format!("{}\n\nTillegg: ny avsluttende setning.", doc.content),
        ..doc.clone()
    };
    assert_ne!(
        input_hash(&rig, &changed, &changed.units[0]),
        first[0].context_input_hash.clone().unwrap(),
        "the edit must change the inputs"
    );
    rig.model
        .set_default(Behaviour::Fail("provider_unavailable"));

    rig.run(rig.items(&doc))
        .await
        .expect("a failed regeneration never fails the batch");

    assert_eq!(rig.model.call_count(), calls_before + CHUNKS.len());
    for ((unit, before), after) in doc.units.iter().zip(&first).zip(rig.rows(&doc).await) {
        assert_eq!(
            after.chunk_context, before.chunk_context,
            "the old context was overwritten"
        );
        assert_eq!(after.context_input_hash, before.context_input_hash);
        assert_eq!(after.context_status.as_deref(), Some("ready"));
        assert_eq!(
            after.context_error_class.as_deref(),
            Some("provider_unavailable")
        );
        assert_eq!(after.context_attempts, 1);
        // The first step of the ladder: a minute.
        let due = seconds_from_now(after.context_next_attempt_at);
        assert!((30..=90).contains(&due), "next attempt in {due} s");
        assert_eq!(after.embedding_status, "done");
        // Coherent: built from the context that is stored.
        assert_eq!(
            after.embedded_context_hash.as_deref(),
            Some(hash_of(&context_for(&unit.text)).as_str())
        );
        assert_eq!(
            rig.vectors.stored(&unit.knowledge_id).unwrap().vector,
            vector_for(&composed(unit)),
            "the vector is not built from the context in force"
        );
    }

    // The provider is back: the stale contexts are regenerated and the error clears.
    rig.model.set_default(Behaviour::Answer);
    rig.run(rig.items(&doc))
        .await
        .expect("regeneration succeeds");
    for (unit, after) in changed.units.iter().zip(rig.rows(&doc).await) {
        assert_ready(&after, unit, &input_hash(&rig, &changed, unit));
        assert!(after.context_next_attempt_at.is_none());
    }
}

/// A concurrent writer (the reconciler) finishes the same unit while this batch is
/// still generating. Its context stays (the write-ahead is a compare-and-swap on the
/// input hash it started from), and the vector is built from it, not from ours.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_writer_that_finishes_first_wins_and_the_vector_follows_it() {
    let rig = Rig::new().await;
    let org = rig.org("cs");
    let doc = rig.plain_document(&org, &CHUNKS).await;
    let winner = "Kontekst skrevet av en annen arbeider først.";
    let winner_hash = "a".repeat(64);
    let raced = doc.units[0].knowledge_id.clone();
    let once = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let pool = rig.pool.clone();
        let raced = raced.clone();
        let winner_hash = winner_hash.clone();
        rig.model.on_call(std::sync::Arc::new(move |_call| {
            let pool = pool.clone();
            let raced = raced.clone();
            let winner_hash = winner_hash.clone();
            let first = !once.swap(true, std::sync::atomic::Ordering::SeqCst);
            async move {
                if first {
                    sqlx::query(
                        "UPDATE knowledge_units SET chunk_context = $2, context_status = 'ready', \
                         context_model = 'gpt-4o-mini', context_provider = 'azure-openai', \
                         context_residency = 'eu', context_prompt_version = 'ctx-v1', \
                         context_input_hash = $3, context_generated_at = now() WHERE knowledge_id = $1",
                    )
                    .bind(&raced)
                    .bind(winner)
                    .bind(&winner_hash)
                    .execute(&pool)
                    .await
                    .expect("the other writer");
                }
            }
            .boxed()
        }));
    }

    rig.run(rig.items(&doc)).await.expect("the batch succeeds");

    let row = rig.row(&raced).await;
    assert_eq!(
        row.chunk_context.as_deref(),
        Some(winner),
        "the winner was overwritten"
    );
    assert_eq!(
        row.context_input_hash.as_deref(),
        Some(winner_hash.as_str())
    );
    assert_eq!(
        row.embedded_context_hash.as_deref(),
        Some(hash_of(winner).as_str())
    );
    assert_eq!(
        rig.vectors.stored(&raced).unwrap().vector,
        vector_for(&compose_contextualized(winner, &doc.units[0].text)),
        "the vector must follow the context in the database"
    );
    // The others were ours.
    for (unit, row) in doc.units.iter().zip(rig.rows(&doc).await).skip(1) {
        assert_ready(&row, unit, &input_hash(&rig, &doc, unit));
    }
}

// ── what is kept, what is generated ─────────────────────────────────────────

/// A change of prompt or model is not a reason to pay again on a re-embed: the refresh
/// is the reconciler's, gated, and capped. The stored context stays and the vector is
/// built from it.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_new_prompt_version_does_not_regenerate_on_a_re_embed() {
    let rig = Rig::new().await;
    let org = rig.org("pv");
    let doc = rig.plain_document(&org, &CHUNKS).await;
    rig.run(rig.items(&doc)).await.expect("first run");
    let first = rig.rows(&doc).await;

    let pinned = Rig::with(|settings| settings.prompt_version = PromptVersion::V2).await;
    pinned
        .run(pinned.items(&doc))
        .await
        .expect("re-embed under the new prompt");

    assert_eq!(
        pinned.model.call_count(),
        0,
        "a prompt change paid for every unit"
    );
    for (before, after) in first.iter().zip(rig.rows(&doc).await) {
        assert_eq!(after.chunk_context, before.chunk_context);
        assert_eq!(after.context_prompt_version.as_deref(), Some("ctx-v1"));
        assert_eq!(after.context_input_hash, before.context_input_hash);
        assert_eq!(after.context_status.as_deref(), Some("ready"));
    }
    for unit in &doc.units {
        assert_eq!(
            pinned.vectors.stored(&unit.knowledge_id).unwrap().vector,
            vector_for(&composed(unit))
        );
    }
}

/// The reconciler's prompt refresh (CR-T9a) is the one run that regenerates a stored context
/// made with another prompt version. A refresh that fails changes nothing (the old context and
/// its coherent vector stay in force); one that succeeds writes the new context with its
/// provenance under the new version and rebuilds the vector from it; and once it has run, an
/// ordinary re-embed under the same version asks no model.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_prompt_refresh_regenerates_under_the_new_version_and_a_failed_one_changes_nothing() {
    let rig = Rig::new().await;
    let org = rig.org("rf");
    let doc = rig.plain_document(&org, &CHUNKS).await;
    rig.run(rig.items(&doc)).await.expect("first run");
    let first = rig.rows(&doc).await;

    let pinned = Rig::with(|settings| settings.prompt_version = PromptVersion::V2).await;
    let refresh = Pipeline {
        pass: super::PassKind::RefreshPrompt,
        ..pinned.pipeline()
    };

    // The provider is down: the refresh asks, fails, and leaves everything as it was.
    pinned
        .model
        .set_default(Behaviour::Fail("provider_unavailable"));
    super::process_batch(pinned.items(&doc), &refresh)
        .await
        .expect("a failed refresh never fails the batch");
    assert_eq!(pinned.model.call_count(), CHUNKS.len(), "it did ask");
    for (before, after) in first.iter().zip(pinned.rows(&doc).await) {
        assert_eq!(after.chunk_context, before.chunk_context);
        assert_eq!(after.context_status.as_deref(), Some("ready"));
        assert_eq!(after.context_prompt_version.as_deref(), Some("ctx-v1"));
        assert_eq!(after.context_input_hash, before.context_input_hash);
        assert_eq!(
            after.embedded_context_hash, before.embedded_context_hash,
            "the vector still reflects the context in force"
        );
    }

    // The provider answers: every context is regenerated under ctx-v2, with its provenance.
    pinned.model.set_default(Behaviour::Answer);
    let calls = pinned.model.call_count();
    super::process_batch(pinned.items(&doc), &refresh)
        .await
        .expect("the refresh succeeds");
    assert_eq!(pinned.model.call_count() - calls, CHUNKS.len());
    for ((unit, before), after) in doc.units.iter().zip(&first).zip(pinned.rows(&doc).await) {
        assert_eq!(after.context_status.as_deref(), Some("ready"));
        assert_eq!(after.context_prompt_version.as_deref(), Some("ctx-v2"));
        assert_ne!(after.context_input_hash, before.context_input_hash);
        assert_eq!(
            after.context_input_hash.as_deref(),
            Some(input_hash(&pinned, &doc, unit).as_str())
        );
        assert_eq!(
            after.embedded_context_hash.as_deref(),
            Some(hash_of(after.chunk_context.as_deref().unwrap()).as_str())
        );
        assert_eq!(after.embedded_at, Some(after.updated_at));
    }

    // Done: an ordinary re-embed under ctx-v2, and a second refresh, ask nothing.
    let calls = pinned.model.call_count();
    pinned
        .run(pinned.items(&doc))
        .await
        .expect("an inline re-embed");
    super::process_batch(pinned.items(&doc), &refresh)
        .await
        .expect("a second refresh");
    assert_eq!(pinned.model.call_count(), calls, "the refresh paid twice");
}

/// A context written before provenance was recorded is kept, never regenerated, and
/// the vector is built from it. Its provenance stays unknown, so it stays `legacy`;
/// what the batch adds is the one thing it knows (which model embedded it, and which
/// context the vector was built from).
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_context_of_unknown_provenance_is_kept_and_the_vector_is_built_from_it() {
    let rig = Rig::new().await;
    let org = rig.org("lg");
    let doc = rig.plain_document(&org, &CHUNKS).await;
    let old = [
        "Gammel kontekst om driftsmarginen.",
        "Gammel kontekst om røykeriet.",
        "Gammel kontekst om Hammerfest.",
    ];
    // Two as the migration's backfill classified them, one as the old code left it
    // after the migration (a context and no status).
    for (index, unit) in doc.units.iter().enumerate() {
        let status = if index < 2 { Some("legacy") } else { None };
        sqlx::query("UPDATE knowledge_units SET chunk_context = $2, context_prompt_version = 'ctx-v1', context_status = $3 WHERE knowledge_id = $1")
            .bind(&unit.knowledge_id)
            .bind(old[index])
            .bind(status)
            .execute(&rig.pool)
            .await
            .expect("seed a legacy context");
    }

    rig.run(rig.items(&doc)).await.expect("the batch succeeds");

    assert_eq!(
        rig.model.call_count(),
        0,
        "a legacy context was regenerated"
    );
    for ((unit, context), row) in doc.units.iter().zip(old).zip(rig.rows(&doc).await) {
        assert_eq!(row.chunk_context.as_deref(), Some(context));
        assert_eq!(
            row.context_status.as_deref(),
            Some("legacy"),
            "I3: no unit keeps a NULL status"
        );
        assert_eq!(row.context_input_hash, None, "provenance is not invented");
        assert_eq!(row.context_model, None);
        assert_eq!(row.embedding_model.as_deref(), Some(EMBEDDING_MODEL));
        assert_eq!(
            row.embedded_context_hash.as_deref(),
            Some(hash_of(context).as_str())
        );
        assert_eq!(
            rig.vectors.stored(&unit.knowledge_id).unwrap().vector,
            vector_for(&compose_contextualized(context, &unit.text))
        );
    }
}

/// The reconciler's legacy verification pass (CR-T9a): the same pipeline, asked to verify. A
/// `legacy` unit (and one a pre-provenance writer left with a context and no status) is
/// embedded from its stored context, and because the hash recorded for the vector is then the
/// hash of that context, the unit is `ready`, in the one statement that marks it done. No
/// model is called, the context's own provenance is not invented, nothing writes the unit
/// after it is marked done, and an inline run of the same units leaves them `legacy`.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn the_verification_pass_promotes_a_legacy_unit_whose_vector_is_proven() {
    let rig = Rig::new().await;
    let org = rig.org("vp");
    let doc = rig.plain_document(&org, &CHUNKS).await;
    let old = [
        "Gammel kontekst om driftsmarginen.",
        "Gammel kontekst om røykeriet.",
        "Gammel kontekst om Hammerfest.",
    ];
    for (index, unit) in doc.units.iter().enumerate() {
        let status = if index < 2 { Some("legacy") } else { None };
        sqlx::query("UPDATE knowledge_units SET chunk_context = $2, context_prompt_version = 'ctx-v1', context_status = $3 WHERE knowledge_id = $1")
            .bind(&unit.knowledge_id)
            .bind(old[index])
            .bind(status)
            .execute(&rig.pool)
            .await
            .expect("seed a legacy context");
    }

    // The same units, run inline: they stay legacy (the inline path does not know enough
    // about a context of unknown provenance to promote it).
    let inline = Pipeline {
        contextualizer: None,
        ..rig.pipeline()
    };
    super::process_batch(rig.items(&doc), &inline)
        .await
        .expect("the inline run succeeds");
    for row in rig.rows(&doc).await {
        assert_eq!(row.context_status.as_deref(), Some("legacy"));
    }

    // The verification pass: no model at all, because none is passed.
    let verify = Pipeline {
        contextualizer: None,
        pass: super::PassKind::VerifyLegacy,
        ..rig.pipeline()
    };
    super::process_batch(rig.items(&doc), &verify)
        .await
        .expect("the verification pass succeeds");

    assert_eq!(
        rig.model.call_count(),
        0,
        "the verification pass asked a model"
    );
    for ((unit, context), row) in doc.units.iter().zip(old).zip(rig.rows(&doc).await) {
        assert_eq!(row.context_status.as_deref(), Some("ready"));
        assert_eq!(row.chunk_context.as_deref(), Some(context));
        assert_eq!(
            row.embedded_context_hash.as_deref(),
            Some(hash_of(context).as_str()),
            "the vector says which context it was built from"
        );
        assert_eq!(row.embedding_model.as_deref(), Some(EMBEDDING_MODEL));
        // Provenance the unit never had is not made up.
        assert_eq!(row.context_input_hash, None);
        assert_eq!(row.context_model, None);
        assert_eq!(row.context_provider, None);
        assert_eq!(row.context_prompt_version.as_deref(), Some("ctx-v1"));
        assert_eq!(row.context_error_class, None);
        assert_eq!(
            row.embedded_at,
            Some(row.updated_at),
            "nothing writes the unit after it is marked done"
        );
        assert_eq!(
            rig.vectors.stored(&unit.knowledge_id).unwrap().vector,
            vector_for(&compose_contextualized(context, &unit.text))
        );
    }
}

/// Promotion needs the proof, not the pass: only a `legacy` unit, only when the hash recorded
/// for the vector is the hash of the context the unit holds at that moment. A raw vector, a
/// hash of another context (the context changed after the batch read it) and every other
/// status are left as they are, and a run that is not the verification pass promotes nothing.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_legacy_unit_is_promoted_only_on_proof_of_its_stored_contexts_hash() {
    use super::store::{self, UnitRef};

    let rig = Rig::new().await;
    let org = rig.org("pp");
    let doc = rig
        .plain_document(&org, &["a", "b", "c", "d", "e", "f"])
        .await;
    let statuses = ["legacy", "legacy", "legacy", "legacy", "ready", "pending"];
    for (unit, status) in doc.units.iter().zip(statuses) {
        sqlx::query("UPDATE knowledge_units SET chunk_context = 'En gammel kontekst.', context_prompt_version = 'ctx-v1', context_status = $2 WHERE knowledge_id = $1")
            .bind(&unit.knowledge_id)
            .bind(status)
            .execute(&rig.pool)
            .await
            .expect("seed");
    }
    let right = hash_of("En gammel kontekst.");
    let wrong = hash_of("En annen kontekst.");
    let reference = |index: usize| UnitRef {
        knowledge_id: &doc.units[index].knowledge_id,
        org_id: &org,
    };
    let done = |index: usize, hash: Option<&str>| store::Done {
        unit: reference(index),
        context_hash: hash.map(str::to_owned),
    };
    let mut conn = rig.pool.acquire().await.expect("a connection");
    let marked = store::mark_done_with(
        &mut conn,
        &[
            done(0, Some(&right)), // proven: promoted
            done(1, Some(&wrong)), // the vector is of another context: stays legacy
            done(2, None),         // a raw vector: stays legacy
        ],
        "m",
        true,
    )
    .await
    .expect("verification marks");
    assert_eq!(marked, 3);
    // Proven too, but written by a run that is not the verification pass.
    store::mark_done_with(&mut conn, &[done(3, Some(&right))], "m", false)
        .await
        .expect("an inline mark");
    // Already ready, and pending: the pass touches neither status.
    store::mark_done_with(
        &mut conn,
        &[done(4, Some(&right)), done(5, Some(&right))],
        "m",
        true,
    )
    .await
    .expect("other statuses");
    drop(conn);

    let rows = rig.rows(&doc).await;
    let status = |index: usize| rows[index].context_status.clone();
    assert_eq!(status(0).as_deref(), Some("ready"), "proof of the hash");
    assert_eq!(
        status(1).as_deref(),
        Some("legacy"),
        "a vector of another context"
    );
    assert_eq!(status(2).as_deref(), Some("legacy"), "a raw vector");
    assert_eq!(
        status(3).as_deref(),
        Some("legacy"),
        "a run that is not the verification pass"
    );
    assert_eq!(status(4).as_deref(), Some("ready"));
    assert_eq!(
        status(5).as_deref(),
        Some("pending"),
        "pending is not a legacy unit"
    );
    // What each vector says it was built from is recorded as given, promoted or not.
    assert_eq!(
        rows[1].embedded_context_hash.as_deref(),
        Some(wrong.as_str())
    );
    assert_eq!(rows[2].embedded_context_hash, None);
}

/// The kill switch stops generation, not the use of what is stored: with contextual
/// retrieval off, a re-embed still builds the vector from the persisted context (a
/// raw vector next to a stored context is the incoherence this design removes), and a
/// unit that has no context is embedded raw with its status untouched.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn with_generation_off_the_stored_context_still_builds_the_vector() {
    let rig = Rig::new().await;
    let org = rig.org("ks");
    let with_context = rig.plain_document(&org, &CHUNKS[..2]).await;
    rig.run(rig.items(&with_context)).await.expect("first run");
    let without = rig.plain_document(&org, &CHUNKS[2..]).await;
    let calls = rig.model.call_count();

    let mut items = rig.items(&with_context);
    items.extend(rig.items(&without));
    super::process_batch(items, &rig.pipeline_without_generation())
        .await
        .expect("the batch succeeds");

    assert_eq!(rig.model.call_count(), calls, "nothing is generated");
    for unit in &with_context.units {
        assert_eq!(
            rig.vectors.stored(&unit.knowledge_id).unwrap().vector,
            vector_for(&composed(unit)),
            "the vector of a unit with a stored context must be built from it"
        );
    }
    let raw = &without.units[0];
    let row = rig.row(&raw.knowledge_id).await;
    assert_eq!(
        rig.vectors.stored(&raw.knowledge_id).unwrap().vector,
        vector_for(&raw.text)
    );
    assert_eq!(row.context_status, None, "the switch records nothing");
    assert_eq!(row.embedded_context_hash, None);
    assert_eq!(row.embedding_status, "done");
}

/// A status that says "do not generate" is respected by the inline path: a permanent failure is
/// not overridden by a re-embed. The unit is embedded raw and nothing is recorded over it.
/// (`skipped_policy` is not one of them since CR-T11, and `skipped_budget` is not since CR-T9b:
/// the organization's policy and budget are read now, see
/// `a_unit_skipped_by_policy_is_generated_again_when_the_policy_is_lifted` and
/// `a_unit_skipped_by_the_budget_is_generated_again_when_the_budget_allows_it`.)
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_status_that_says_do_not_generate_is_respected() {
    let rig = Rig::new().await;
    let org = rig.org("st");
    let doc = rig.plain_document(&org, &CHUNKS[..1]).await;
    let statuses = [("failed_permanent", Some("content_filter"))];
    for (unit, (status, class)) in doc.units.iter().zip(statuses) {
        sqlx::query("UPDATE knowledge_units SET context_status = $2, context_error_class = $3 WHERE knowledge_id = $1")
            .bind(&unit.knowledge_id)
            .bind(status)
            .bind(class)
            .execute(&rig.pool)
            .await
            .expect("seed a status");
    }

    rig.run(rig.items(&doc)).await.expect("the batch succeeds");

    assert_eq!(rig.model.call_count(), 0);
    for (unit, ((status, class), row)) in doc
        .units
        .iter()
        .zip(statuses.iter().zip(rig.rows(&doc).await))
    {
        assert_eq!(row.context_status.as_deref(), Some(*status));
        assert_eq!(row.context_error_class.as_deref(), *class);
        assert_eq!(row.chunk_context, None);
        assert_eq!(row.embedding_status, "done");
        assert_eq!(
            rig.vectors.stored(&unit.knowledge_id).unwrap().vector,
            vector_for(&unit.text)
        );
    }
}

// ── the organization's operator policy (CR-T11) ─────────────────────────────────

fn table_missing() -> ReadError {
    ReadError::TableUnreachable("relation \"org_processing_policy\" does not exist".into())
}

fn database_struggling() -> ReadError {
    ReadError::Transient("canceling statement due to statement timeout".into())
}

/// Off means zero provider calls. An organization whose operator policy is off makes no
/// provider call and does not even ask for a bearer, its units are marked `skipped_policy`
/// (never silent, no error class: it is a decision, not a failure) and are still embedded and
/// done from their raw text, and another organization in the SAME batch is contextualized as
/// usual. The coverage view counts the marker.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn an_organization_whose_policy_is_off_makes_no_provider_call_and_is_marked() {
    let rig = Rig::new().await;
    let off = rig.org("off");
    let on = rig.org("on");
    rig.set_policy(&off, "off", None).await;
    let off_doc = rig.plain_document(&off, &CHUNKS).await;
    let on_doc = rig.plain_document(&on, &CHUNKS).await;
    let mut items = rig.items(&off_doc);
    items.extend(rig.items(&on_doc));

    run_within(&rig, items).await;

    assert!(
        rig.model.calls_for(&off).is_empty(),
        "off means zero provider calls"
    );
    assert!(
        rig.tenants
            .requests()
            .iter()
            .all(|request| request["orgId"] != off.as_str()),
        "not even a bearer was asked for the organization that is off"
    );
    assert_eq!(rig.model.calls_for(&on).len(), CHUNKS.len());

    for (unit, row) in off_doc.units.iter().zip(rig.rows(&off_doc).await) {
        assert_eq!(row.context_status.as_deref(), Some("skipped_policy"));
        assert_eq!(row.context_error_class, None);
        assert_eq!(row.chunk_context, None);
        assert_eq!(row.embedding_status, "done");
        assert_eq!(row.embedded_context_hash, None);
        assert_eq!(
            rig.vectors.stored(&unit.knowledge_id).unwrap().vector,
            vector_for(&unit.text),
            "the unit is embedded from its raw text"
        );
    }
    for (unit, row) in on_doc.units.iter().zip(rig.rows(&on_doc).await) {
        assert_ready(&row, unit, &input_hash(&rig, &on_doc, unit));
    }

    // The marker is what the coverage view and the release gate see.
    let skipped: i64 =
        sqlx::query_scalar("SELECT skipped_policy FROM v_context_coverage WHERE org_id = $1")
            .bind(&off)
            .fetch_one(&rig.pool)
            .await
            .expect("the coverage row of the organization that is off");
    assert_eq!(skipped, CHUNKS.len() as i64);
}

/// The policy is read now: a unit that was skipped while the policy was off is generated again
/// once the policy is lifted (the row removed, or inherit, or on), not left raw for ever by a
/// decision that no longer stands.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_unit_skipped_by_policy_is_generated_again_when_the_policy_is_lifted() {
    for lift in ["remove the row", "inherit", "on"] {
        let rig = Rig::new().await;
        let org = rig.org("lift");
        rig.set_policy(&org, "off", None).await;
        let doc = rig.plain_document(&org, &CHUNKS).await;

        run_within(&rig, rig.items(&doc)).await;
        assert_eq!(rig.model.call_count(), 0, "{lift}: off");
        for row in rig.rows(&doc).await {
            assert_eq!(row.context_status.as_deref(), Some("skipped_policy"));
        }

        match lift {
            "remove the row" => rig.clear_policy(&org).await,
            other => rig.set_policy(&org, other, None).await,
        }
        run_within(&rig, rig.items(&doc)).await;

        assert_eq!(
            rig.model.call_count(),
            CHUNKS.len(),
            "{lift}: one call per unit"
        );
        for (unit, row) in doc.units.iter().zip(rig.rows(&doc).await) {
            assert_ready(&row, unit, &input_hash(&rig, &doc, unit));
            assert_eq!(
                row.embedded_context_hash.as_deref(),
                Some(hash_of(&context_for(&unit.text)).as_str()),
                "{lift}: the vector is rebuilt from the new context"
            );
        }
    }
}

/// Existing contexts are derived data in our own store: turning an organization off stops
/// generation and removes nothing. The vector of a unit that is embedded again is still built
/// from its stored context.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn an_organization_that_is_turned_off_keeps_the_contexts_it_already_has() {
    let rig = Rig::new().await;
    let org = rig.org("keep");
    let doc = rig.plain_document(&org, &CHUNKS).await;
    run_within(&rig, rig.items(&doc)).await;
    assert_eq!(rig.model.call_count(), CHUNKS.len());

    rig.set_policy(&org, "off", None).await;
    run_within(&rig, rig.items(&doc)).await;

    assert_eq!(
        rig.model.call_count(),
        CHUNKS.len(),
        "no provider call after the organization was turned off"
    );
    for (unit, row) in doc.units.iter().zip(rig.rows(&doc).await) {
        let context = context_for(&unit.text);
        assert_eq!(row.chunk_context.as_deref(), Some(context.as_str()));
        assert_eq!(row.context_status.as_deref(), Some("ready"));
        assert_eq!(
            row.embedded_context_hash.as_deref(),
            Some(hash_of(&context).as_str())
        );
        assert_eq!(
            rig.vectors.stored(&unit.knowledge_id).unwrap().vector,
            vector_for(&composed(unit))
        );
    }
}

/// A pin is the organization's prompt version: the canary of a prompt rollout. Another
/// organization, without one, keeps the service default.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn an_organizations_prompt_pin_decides_the_prompt_of_the_contexts_it_gets() {
    let rig = Rig::new().await;
    let pinned = rig.org("pin");
    let plain = rig.org("nopin");
    rig.set_policy(&pinned, "inherit", Some("ctx-v2")).await;
    let pinned_doc = rig.plain_document(&pinned, &CHUNKS[..1]).await;
    let plain_doc = rig.plain_document(&plain, &CHUNKS[..1]).await;
    let mut items = rig.items(&pinned_doc);
    items.extend(rig.items(&plain_doc));

    run_within(&rig, items).await;

    let pinned_row = rig.row(&pinned_doc.units[0].knowledge_id).await;
    let plain_row = rig.row(&plain_doc.units[0].knowledge_id).await;
    assert_eq!(pinned_row.context_status.as_deref(), Some("ready"));
    assert_eq!(pinned_row.context_prompt_version.as_deref(), Some("ctx-v2"));
    assert_eq!(plain_row.context_prompt_version.as_deref(), Some("ctx-v1"));
}

/// A policy that cannot be read for an organization nothing is known about is NOT a fallback to
/// on: nothing is generated, nothing is recorded, and the unit is picked up when the policy can
/// be read. (Falling open here is the one way an organization an operator switched off could be
/// sent content.)
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn an_unreadable_policy_defers_generation_instead_of_falling_open() {
    let rig = Rig::new().await;
    let org = rig.org("defer");
    rig.set_policy(&org, "off", None).await;
    let doc = rig.plain_document(&org, &CHUNKS).await;

    rig.policy_source.fail_with(Some(database_struggling));
    run_within(&rig, rig.items(&doc)).await;

    assert_eq!(
        rig.model.call_count(),
        0,
        "no call while the policy is unknown"
    );
    for (unit, row) in doc.units.iter().zip(rig.rows(&doc).await) {
        assert_eq!(row.context_status, None, "nothing is recorded");
        assert_eq!(
            row.embedding_status, "done",
            "the unit is still embedded, raw"
        );
        assert_eq!(
            rig.vectors.stored(&unit.knowledge_id).unwrap().vector,
            vector_for(&unit.text)
        );
    }

    // Once the policy can be read it is honoured: this organization is off.
    rig.policy_source.fail_with(None);
    run_within(&rig, rig.items(&doc)).await;
    assert_eq!(rig.model.call_count(), 0);
    for row in rig.rows(&doc).await {
        assert_eq!(row.context_status.as_deref(), Some("skipped_policy"));
    }
}

/// A database that has no policy table has no policy to honour: the deployment default (on).
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_database_without_the_policy_table_gets_the_deployment_default() {
    let rig = Rig::new().await;
    let org = rig.org("notable");
    let doc = rig.plain_document(&org, &CHUNKS).await;

    rig.policy_source.fail_with(Some(table_missing));
    run_within(&rig, rig.items(&doc)).await;

    assert_eq!(rig.model.call_count(), CHUNKS.len());
    for (unit, row) in doc.units.iter().zip(rig.rows(&doc).await) {
        assert_ready(&row, unit, &input_hash(&rig, &doc, unit));
    }
}

/// An organization that is known to be off stays off when the table becomes unreachable, even
/// after its cached answer has expired.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn an_organization_known_to_be_off_stays_off_when_the_policy_table_fails() {
    let rig = Rig::new().await;
    let org = rig.org("stale");
    rig.set_policy(&org, "off", None).await;
    let reader = PolicyReader::with_ttl(rig.policy_source.clone(), Duration::from_millis(50));
    let pipeline = Pipeline {
        policy: &reader,
        ..rig.pipeline()
    };

    let first = rig.plain_document(&org, &CHUNKS[..1]).await;
    super::process_batch(rig.items(&first), &pipeline)
        .await
        .expect("the first batch succeeds");
    assert_eq!(rig.model.call_count(), 0);

    // The table fails, and the cached answer expires.
    rig.policy_source.fail_with(Some(table_missing));
    tokio::time::sleep(Duration::from_millis(120)).await;
    let second = rig.plain_document(&org, &CHUNKS[1..2]).await;
    super::process_batch(rig.items(&second), &pipeline)
        .await
        .expect("the second batch succeeds");

    assert_eq!(
        rig.model.call_count(),
        0,
        "a failed read turned an organization that is off back on"
    );
    for row in rig.rows(&second).await {
        assert_eq!(row.context_status.as_deref(), Some("skipped_policy"));
    }
}

/// The global switch is the incident kill switch and comes first: with generation off the
/// policy is not consulted at all, and an organization set to on is not switched back on.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn the_global_switch_comes_before_any_organizations_policy() {
    let rig = Rig::new().await;
    let org = rig.org("global");
    rig.set_policy(&org, "on", None).await;
    let doc = rig.plain_document(&org, &CHUNKS).await;

    super::process_batch(rig.items(&doc), &rig.pipeline_without_generation())
        .await
        .expect("the batch succeeds");

    assert_eq!(
        rig.model.call_count(),
        0,
        "an organization on is not switched on against the kill switch"
    );
    assert_eq!(rig.policy_source.reads(), 0, "the policy is not even read");
    for row in rig.rows(&doc).await {
        assert_eq!(row.context_status, None);
        assert_eq!(row.embedding_status, "done");
    }
}

/// A unit that was skipped because its organization was zero-retention is generated
/// once the organization no longer is: the posture is asked again, not remembered.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_unit_skipped_for_zero_retention_is_generated_when_the_organization_no_longer_is() {
    let rig = Rig::new().await;
    let org = rig.org("zr");
    let doc = rig.plain_document(&org, &CHUNKS[..1]).await;
    rig.tenants.set(&org, Posture::Zdr);
    rig.run(rig.items(&doc)).await.expect("zero-retention run");
    let row = rig.row(&doc.units[0].knowledge_id).await;
    assert_eq!(row.context_status.as_deref(), Some("skipped_zdr"));
    assert_eq!(rig.model.call_count(), 0);

    // The organization is not zero-retention any more (a new rig answers every
    // organization as a plain one): nothing remembers the earlier answer.
    let fresh = Rig::with(|_| {}).await;
    fresh
        .run(fresh.items(&doc))
        .await
        .expect("run after the setting changed");

    assert_ready(
        &fresh.row(&doc.units[0].knowledge_id).await,
        &doc.units[0],
        &input_hash(&fresh, &doc, &doc.units[0]),
    );
}

// ── the organization's background budget (CR-T9b) ───────────────────────────────

use crate::budget::BudgetMode;

/// The deployment has no figure and no prices, which is what every deployment starts with: the
/// gate holds nothing back, and does not even read the usage ledger. Whatever an organization
/// spends, the default mode never stops.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn the_default_budget_changes_nothing_and_reads_nothing() {
    let rig = Rig::new().await;
    let org = rig.org("bd0");
    rig.spend.set_usd(&org, 1e9);
    let doc = rig.plain_document(&org, &CHUNKS).await;

    run_within(&rig, rig.items(&doc)).await;

    assert_eq!(rig.model.call_count(), CHUNKS.len());
    for (unit, row) in doc.units.iter().zip(rig.rows(&doc).await) {
        assert_ready(&row, unit, &input_hash(&rig, &doc, unit));
    }
    assert_eq!(rig.spend.reads(), 0, "the ledger was read for no figure");
    for mode in BudgetMode::ALL {
        assert_eq!(rig.budget.exceeded_total(mode), 0);
    }
}

/// An alert-only budget over its figure (the owner's default mode) alerts and counts, and the
/// work goes on exactly as it would have: never a stop, whatever the spend.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn an_alert_only_budget_over_its_figure_counts_and_still_generates() {
    let mut rig = Rig::new().await;
    rig.deployment_budget(BudgetMode::AlertOnly, Some(10.0));
    let org = rig.org("bda");
    rig.spend.set_usd(&org, 50.0);
    let doc = rig.plain_document(&org, &CHUNKS).await;

    run_within(&rig, rig.items(&doc)).await;

    assert_eq!(
        rig.model.call_count(),
        CHUNKS.len(),
        "alert only never stops"
    );
    for (unit, row) in doc.units.iter().zip(rig.rows(&doc).await) {
        assert_ready(&row, unit, &input_hash(&rig, &doc, unit));
    }
    assert_eq!(rig.spend.reads(), 1, "one reading serves the whole batch");
    assert_eq!(rig.budget.exceeded_total(BudgetMode::AlertOnly), 1);
    assert_eq!(rig.budget.exceeded_total(BudgetMode::Stop), 0);
}

/// A stop (explicit, on the organization's own row) makes zero provider calls, asks for no bearer,
/// and marks the units `skipped_budget`: never silent, no error class (a decision, not a failure),
/// embedded and done from their raw text, with nothing written after the embedding (the
/// stale-embedding detector sees no change). Another organization in the SAME batch is
/// contextualized as usual, and the coverage view counts the marker.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_stopped_budget_makes_no_provider_call_asks_for_no_bearer_and_marks_the_units() {
    let mut rig = Rig::new().await;
    // The deployment prices the tokens and sets no figure; the organization sets its own, and
    // the stop.
    rig.deployment_budget(BudgetMode::AlertOnly, None);
    let stopped = rig.org("bds");
    let free = rig.org("bdf");
    rig.set_budget(&stopped, "stop", Some(10.0)).await;
    rig.spend.set_usd(&stopped, 50.0);
    let stopped_doc = rig.plain_document(&stopped, &CHUNKS).await;
    let free_doc = rig.plain_document(&free, &CHUNKS).await;
    let mut items = rig.items(&stopped_doc);
    items.extend(rig.items(&free_doc));

    run_within(&rig, items).await;

    assert!(
        rig.model.calls_for(&stopped).is_empty(),
        "a stop means zero provider calls"
    );
    assert!(
        rig.tenants
            .requests()
            .iter()
            .all(|request| request["orgId"] != stopped.as_str()),
        "not even a bearer was asked for the organization that is stopped"
    );
    assert_eq!(rig.model.calls_for(&free).len(), CHUNKS.len());
    assert_eq!(
        rig.spend.asked(),
        std::slice::from_ref(&stopped),
        "only a figure is read"
    );

    let rows = rig.rows(&stopped_doc).await;
    for (unit, row) in stopped_doc.units.iter().zip(&rows) {
        assert_eq!(row.context_status.as_deref(), Some("skipped_budget"));
        assert_eq!(row.context_error_class, None);
        assert_eq!(row.context_attempts, 0, "a stop is not the unit's fault");
        assert_eq!(
            row.context_next_attempt_at, None,
            "the gate lifts it, not a clock"
        );
        assert_eq!(row.chunk_context, None);
        assert_eq!(row.embedding_status, "done");
        assert_eq!(row.embedded_context_hash, None);
        assert_eq!(
            row.embedded_at,
            Some(row.updated_at),
            "written after it was embedded: the stale-embedding detector would report it"
        );
        assert_eq!(
            rig.vectors.stored(&unit.knowledge_id).unwrap().vector,
            vector_for(&unit.text),
            "the unit is embedded from its raw text"
        );
    }
    for (unit, row) in free_doc.units.iter().zip(rig.rows(&free_doc).await) {
        assert_ready(&row, unit, &input_hash(&rig, &free_doc, unit));
    }
    assert_eq!(rig.budget.exceeded_total(BudgetMode::Stop), 1);

    // The marker is what the coverage view and the release gate see.
    let skipped: i64 =
        sqlx::query_scalar("SELECT skipped_budget FROM v_context_coverage WHERE org_id = $1")
            .bind(&stopped)
            .fetch_one(&rig.pool)
            .await
            .expect("the coverage row of the organization that is stopped");
    assert_eq!(skipped, CHUNKS.len() as i64);
}

/// A stop is a pause, not a deletion. A unit the gate marked is generated again, with no help,
/// the moment the budget allows it (a raised figure, another mode, a row removed): the status of
/// a decision that no longer stands never keeps a unit raw.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_unit_skipped_by_the_budget_is_generated_again_when_the_budget_allows_it() {
    for lift in ["raise the figure", "alert only", "remove the row"] {
        let mut rig = Rig::new().await;
        rig.deployment_budget(BudgetMode::AlertOnly, None);
        let org = rig.org("bdl");
        rig.set_budget(&org, "stop", Some(10.0)).await;
        rig.spend.set_usd(&org, 50.0);
        let doc = rig.plain_document(&org, &CHUNKS).await;

        run_within(&rig, rig.items(&doc)).await;
        assert_eq!(rig.model.call_count(), 0, "{lift}: stopped");
        for row in rig.rows(&doc).await {
            assert_eq!(row.context_status.as_deref(), Some("skipped_budget"));
        }

        match lift {
            "raise the figure" => rig.set_budget(&org, "stop", Some(100.0)).await,
            "alert only" => rig.set_budget(&org, "alert_only", Some(10.0)).await,
            _ => rig.clear_policy(&org).await,
        }
        run_within(&rig, rig.items(&doc)).await;

        assert_eq!(
            rig.model.call_count(),
            CHUNKS.len(),
            "{lift}: one call per unit"
        );
        for (unit, row) in doc.units.iter().zip(rig.rows(&doc).await) {
            assert_ready(&row, unit, &input_hash(&rig, &doc, unit));
            assert_eq!(
                row.embedded_context_hash.as_deref(),
                Some(hash_of(&context_for(&unit.text)).as_str()),
                "{lift}: the vector is rebuilt from the new context"
            );
        }
    }
}

/// A stop stops generation; it removes nothing. The context a unit already has stays, and the
/// vector of a unit that is embedded again is still built from it, even when the document changed
/// under the chunk (the regeneration that would have replaced the context is what is withheld).
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_stopped_budget_keeps_the_contexts_the_units_already_have() {
    let mut rig = Rig::new().await;
    rig.deployment_budget(BudgetMode::AlertOnly, None);
    let org = rig.org("bdk");
    let doc = rig.plain_document(&org, &CHUNKS).await;
    run_within(&rig, rig.items(&doc)).await;
    let before = rig.rows(&doc).await;
    assert_eq!(rig.model.call_count(), CHUNKS.len());

    // The document changes under every chunk: with no budget this would regenerate them all.
    sqlx::query("UPDATE documents SET content = content || $2 WHERE document_id = $1")
        .bind(&doc.id)
        .bind("\n\nEt tillegg som endrer vinduet.")
        .execute(&rig.pool)
        .await
        .expect("change the document");
    rig.set_budget(&org, "stop", Some(10.0)).await;
    rig.spend.set_usd(&org, 50.0);
    run_within(&rig, rig.items(&doc)).await;

    assert_eq!(
        rig.model.call_count(),
        CHUNKS.len(),
        "nothing more was asked"
    );
    assert_eq!(
        rig.spend.reads(),
        1,
        "the gate was asked: a regeneration was about to be made"
    );
    for ((unit, row), earlier) in doc.units.iter().zip(rig.rows(&doc).await).zip(&before) {
        assert_eq!(row.context_status.as_deref(), Some("ready"));
        assert_eq!(row.chunk_context, earlier.chunk_context);
        assert_eq!(row.context_input_hash, earlier.context_input_hash);
        assert_eq!(
            rig.vectors.stored(&unit.knowledge_id).unwrap().vector,
            vector_for(&composed(unit)),
            "the vector is built from the stored context"
        );
    }
}

/// A throttle defers instead of skipping: an over-budget organization is worked one chunk at a
/// time. Each batch lets one unit through, the rest wait as `pending` (due at once, no error class,
/// nothing is lost) and are embedded raw meanwhile; the units are all contextualized after as many
/// batches, never more than one provider call each.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_throttled_budget_lets_one_unit_through_per_batch_and_defers_the_rest() {
    let mut rig = Rig::new().await;
    rig.deployment_budget(BudgetMode::AlertOnly, None);
    let org = rig.org("bdt");
    rig.set_budget(&org, "throttle", Some(10.0)).await;
    rig.spend.set_usd(&org, 50.0);
    let doc = rig.plain_document(&org, &CHUNKS).await;

    run_within(&rig, rig.items(&doc)).await;

    assert_eq!(rig.model.call_count(), 1, "one chunk at a time");
    let rows = rig.rows(&doc).await;
    assert_eq!(rows[0].context_status.as_deref(), Some("ready"));
    for (unit, row) in doc.units.iter().zip(&rows).skip(1) {
        assert_eq!(row.context_status.as_deref(), Some("pending"));
        assert_eq!(row.context_error_class, None, "nothing went wrong");
        assert_eq!(row.context_attempts, 0);
        assert!(
            seconds_from_now(row.context_next_attempt_at) <= 1,
            "due at once: the reconciler's pace is the throttle"
        );
        assert_eq!(row.chunk_context, None);
        assert_eq!(row.embedding_status, "done");
        assert_eq!(
            rig.vectors.stored(&unit.knowledge_id).unwrap().vector,
            vector_for(&unit.text),
            "embedded raw meanwhile"
        );
    }
    assert_eq!(rig.budget.exceeded_total(BudgetMode::Throttle), 1);

    // The next batch lets the next one through (the one that is already ready is kept, and the
    // one that was deferred and is still pending is not written again).
    run_within(&rig, rig.items(&doc)).await;
    assert_eq!(rig.model.call_count(), 2);
    run_within(&rig, rig.items(&doc)).await;
    assert_eq!(rig.model.call_count(), 3);
    for (unit, row) in doc.units.iter().zip(rig.rows(&doc).await) {
        assert_ready(&row, unit, &input_hash(&rig, &doc, unit));
    }
}

/// The gate stops before the next provider call, not after the next batch: what a batch just spent
/// is added to the organization's figure at once, so a batch that starts before the usage ledger
/// has the events of the one before it still knows. The figure here is a little over what one
/// chunk costs (612 input tokens at the test's dollar a million).
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_batch_that_crosses_the_figure_is_not_followed_by_one_that_does_not_know() {
    let mut rig = Rig::new().await;
    rig.deployment_budget(BudgetMode::AlertOnly, None);
    let org = rig.org("bdc");
    rig.set_budget(&org, "stop", Some(0.001)).await;
    // The ledger has nothing for the organization, and never will in this test: every dollar
    // the gate sees after the first reading is the process's own accounting.
    let docs = [
        rig.plain_document(&org, &CHUNKS[..1]).await,
        rig.plain_document(&org, &CHUNKS[1..2]).await,
        rig.plain_document(&org, &CHUNKS[2..3]).await,
    ];

    for (index, doc) in docs.iter().enumerate() {
        run_within(&rig, rig.items(doc)).await;
        let expected = (index + 1).min(2);
        assert_eq!(
            rig.model.call_count(),
            expected,
            "after batch {}: two chunks fit in the figure (0.0012 against 0.001 crosses it), the third must not be asked",
            index + 1
        );
    }
    let last = rig.row(&docs[2].units[0].knowledge_id).await;
    assert_eq!(last.context_status.as_deref(), Some("skipped_budget"));
    assert_eq!(last.chunk_context, None);
    for doc in &docs[..2] {
        assert_eq!(
            rig.row(&doc.units[0].knowledge_id)
                .await
                .context_status
                .as_deref(),
            Some("ready")
        );
    }
    assert_eq!(
        rig.spend.reads(),
        1,
        "the ledger was read once and the rest is accounting"
    );
}

/// An organization with no row inherits the deployment's mode and figure, and may opt out of a
/// deployment-wide stop with its own mode.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn an_organization_with_no_row_inherits_the_deployments_stop_and_one_may_opt_out() {
    let mut rig = Rig::new().await;
    rig.deployment_budget(BudgetMode::Stop, Some(10.0));
    let inherits = rig.org("bdi");
    let opts_out = rig.org("bdo");
    rig.set_budget(&opts_out, "alert_only", None).await;
    rig.spend.set_usd(&inherits, 50.0);
    rig.spend.set_usd(&opts_out, 50.0);
    let inherits_doc = rig.plain_document(&inherits, &CHUNKS).await;
    let opts_out_doc = rig.plain_document(&opts_out, &CHUNKS).await;
    let mut items = rig.items(&inherits_doc);
    items.extend(rig.items(&opts_out_doc));

    run_within(&rig, items).await;

    assert!(
        rig.model.calls_for(&inherits).is_empty(),
        "the deployment's stop"
    );
    assert_eq!(rig.model.calls_for(&opts_out).len(), CHUNKS.len());
    for row in rig.rows(&inherits_doc).await {
        assert_eq!(row.context_status.as_deref(), Some("skipped_budget"));
    }
    for (unit, row) in opts_out_doc.units.iter().zip(rig.rows(&opts_out_doc).await) {
        assert_ready(&row, unit, &input_hash(&rig, &opts_out_doc, unit));
    }
}

/// A budget that cannot be measured is alerted, never enforced on a guess: no prices, a ledger
/// that cannot be read, a ledger that is off. The work goes on in every one of them, whatever
/// mode the organization set.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_budget_that_cannot_be_measured_never_stops_the_work() {
    // No prices (the default gate has none) and a stop with a figure on the organization's row.
    let rig = Rig::new().await;
    let org = rig.org("bdn");
    rig.set_budget(&org, "stop", Some(0.0)).await;
    let doc = rig.plain_document(&org, &CHUNKS).await;
    run_within(&rig, rig.items(&doc)).await;
    assert_eq!(rig.model.call_count(), CHUNKS.len(), "no prices");
    assert_eq!(rig.spend.reads(), 0);

    // A ledger that cannot be read.
    let mut rig = Rig::new().await;
    rig.deployment_budget(BudgetMode::AlertOnly, None);
    let org = rig.org("bdu");
    rig.set_budget(&org, "stop", Some(0.0)).await;
    rig.spend.fail(true);
    let doc = rig.plain_document(&org, &CHUNKS).await;
    run_within(&rig, rig.items(&doc)).await;
    assert_eq!(rig.model.call_count(), CHUNKS.len(), "an unreadable ledger");
    for mode in BudgetMode::ALL {
        assert_eq!(rig.budget.exceeded_total(mode), 0, "nothing was found over");
    }
}

/// The incident kill switch comes first: with contextual retrieval switched off nothing is
/// generated, so there is no spend to hold. The gate is not consulted (the ledger is not read),
/// nothing is marked, and the units stay as they are.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn the_kill_switch_comes_before_the_budget() {
    let mut rig = Rig::new().await;
    rig.deployment_budget(BudgetMode::AlertOnly, None);
    let org = rig.org("bdx");
    rig.set_budget(&org, "stop", Some(10.0)).await;
    rig.spend.set_usd(&org, 50.0);
    let doc = rig.plain_document(&org, &CHUNKS).await;

    super::process_batch(rig.items(&doc), &rig.pipeline_without_generation())
        .await
        .expect("the batch succeeds");

    assert_eq!(rig.model.call_count(), 0);
    assert_eq!(
        rig.spend.reads(),
        0,
        "the gate was consulted with nothing to generate"
    );
    for row in rig.rows(&doc).await {
        assert_eq!(
            row.context_status, None,
            "nothing is recorded under the kill switch"
        );
        assert_eq!(row.embedding_status, "done");
    }
    for mode in BudgetMode::ALL {
        assert_eq!(rig.budget.exceeded_total(mode), 0);
    }
}

/// A restricted document keeps its zero-retention mark under a stop: that is the stronger and
/// permanent fact, and it must survive the budget being lifted.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_restricted_document_keeps_its_zero_retention_mark_under_a_stopped_budget() {
    let mut rig = Rig::new().await;
    rig.deployment_budget(BudgetMode::AlertOnly, None);
    let org = rig.org("bdr");
    rig.set_budget(&org, "stop", Some(10.0)).await;
    rig.spend.set_usd(&org, 50.0);
    let doc = rig.document(&org, "restricted", &CHUNKS[..1]).await;

    run_within(&rig, rig.items(&doc)).await;

    let row = rig.row(&doc.units[0].knowledge_id).await;
    assert_eq!(row.context_status.as_deref(), Some("skipped_zdr"));
    assert_eq!(row.context_error_class.as_deref(), Some("zdr_document"));
    assert_eq!(rig.model.call_count(), 0);
}

// ── the month's spend, from the ledger ──────────────────────────────────────

/// The statement the gate runs: this organization's background LLM events (every `llm` feature:
/// contextual retrieval, graph extraction, community summaries) of this month, and nothing else.
/// Not an embedding, not a rerank, not another organization's, not last quarter's, not a version
/// 1 row that has no exact tokens. This is the "spend aggregation excludes other features" test.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn the_months_spend_counts_this_organizations_llm_events_of_this_month_and_nothing_else() {
    use crate::budget::{PgSpendSource, SpendSource};

    let rig = Rig::new().await;
    let (a, b) = (rig.org("spend-a"), rig.org("spend-b"));
    let insert = |org: String,
                  event_type: &'static str,
                  feature: &'static str,
                  input: i64,
                  output: i64,
                  age: &'static str| {
        let pool = rig.pool.clone();
        async move {
            sqlx::query(
                "INSERT INTO cost_events (event_type, model, org_id, count, estimated_tokens, idempotency_key, \
                     schema_version, feature, provider_used, input_tokens, output_tokens, cache_read_tokens, \
                     cost_basis, estimated, zdr, request_id, created_at) \
                 VALUES ($1, 'gpt-4o-mini', $2, 1, 0, $3, 2, $4, 'azure-openai', $5, $6, 0, 'metered', false, false, $3, \
                     now() - $7::interval)",
            )
            .bind(event_type)
            .bind(org)
            .bind(format!("spend-{}", uuid::Uuid::new_v4()))
            .bind(feature)
            .bind(input)
            .bind(output)
            .bind(age)
            .execute(&pool)
            .await
            .expect("a usage event");
        }
    };
    // Counted: the three background LLM features.
    insert(
        a.clone(),
        "llm",
        "contextual-retrieval",
        1_000,
        100,
        "0 seconds",
    )
    .await;
    insert(a.clone(), "llm", "graph-extraction", 500, 50, "0 seconds").await;
    insert(
        a.clone(),
        "llm",
        "graph-community-summary",
        250,
        25,
        "0 seconds",
    )
    .await;
    // Not counted: other kinds of call, other months, other organizations.
    insert(a.clone(), "embedding", "embedding", 9_999, 0, "0 seconds").await;
    insert(a.clone(), "rerank", "rerank", 9_999, 0, "0 seconds").await;
    insert(a.clone(), "rerank", "web-rerank", 9_999, 0, "0 seconds").await;
    insert(
        a.clone(),
        "llm",
        "contextual-retrieval",
        7_777,
        7_777,
        "70 days",
    )
    .await;
    insert(
        b.clone(),
        "llm",
        "contextual-retrieval",
        4_000,
        400,
        "0 seconds",
    )
    .await;
    // A version 1 row: the count-only event of the producers that predate the ledger. It has no
    // exact tokens and is never part of a spend.
    sqlx::query(
        "INSERT INTO cost_events (event_type, model, org_id, count, estimated_tokens, idempotency_key) \
         VALUES ('embedding', 'embed-v-4-0', $1, 3, 123456, $2)",
    )
    .bind(&a)
    .bind(format!("spend-v1-{}", uuid::Uuid::new_v4()))
    .execute(&rig.pool)
    .await
    .expect("a version 1 event");

    let source = PgSpendSource::new(rig.pool.clone());
    assert_eq!(
        source.month_llm_tokens(&a).await.expect("spend"),
        (1_750, 175),
        "this organization's background LLM events of this month: not an embedding, not a rerank, \
         not last quarter, not another organization's, not a version 1 row"
    );
    assert_eq!(
        source.month_llm_tokens(&b).await.expect("spend"),
        (4_000, 400)
    );
    assert_eq!(
        source
            .month_llm_tokens(&rig.org("spend-none"))
            .await
            .expect("spend"),
        (0, 0)
    );
}

// ── zero retention, unresolved posture ──────────────────────────────────────

/// A zero-retention organization: zero provider calls, an explicit marker with a
/// class that says why, the unit embedded raw, and a recheck due (the setting
/// changes).
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_zero_retention_organization_makes_no_provider_call_and_says_so() {
    let rig = Rig::new().await;
    let org = rig.org("zo");
    rig.tenants.set(&org, Posture::Zdr);
    let doc = rig.plain_document(&org, &CHUNKS).await;

    rig.run(rig.items(&doc)).await.expect("the batch succeeds");

    assert_eq!(
        rig.model.call_count(),
        0,
        "content of a zero-retention organization reached the provider"
    );
    for (unit, row) in doc.units.iter().zip(rig.rows(&doc).await) {
        assert_eq!(row.context_status.as_deref(), Some("skipped_zdr"));
        assert_eq!(row.context_error_class.as_deref(), Some("zdr_org"));
        assert_eq!(row.chunk_context, None);
        assert_eq!(row.embedded_context_hash, None);
        assert_eq!(row.embedding_status, "done");
        assert_eq!(row.context_attempts, 0, "a posture is not the unit's fault");
        let due = seconds_from_now(row.context_next_attempt_at);
        assert!((3_500..=3_700).contains(&due), "rechecked in {due} s");
        assert_eq!(
            rig.vectors.stored(&unit.knowledge_id).unwrap().vector,
            vector_for(&unit.text)
        );
    }
}

/// A restricted document is never read, windowed or sent, and no bearer is even asked
/// for: `skipped_zdr` with the class that tells it from a zero-retention organization.
/// Its vector goes through the zero-retention path of the embedding client.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_restricted_document_is_neither_read_nor_sent_and_says_so() {
    let rig = Rig::new().await;
    let org = rig.org("rd");
    let doc = rig.document(&org, "restricted", &CHUNKS).await;

    rig.run(rig.items(&doc)).await.expect("the batch succeeds");

    assert_eq!(rig.model.call_count(), 0);
    assert!(
        rig.tenants.requests().is_empty(),
        "a bearer was asked for a restricted document"
    );
    for row in rig.rows(&doc).await {
        assert_eq!(row.context_status.as_deref(), Some("skipped_zdr"));
        assert_eq!(row.context_error_class.as_deref(), Some("zdr_document"));
        assert_eq!(
            row.context_next_attempt_at, None,
            "a restricted document is final"
        );
        assert_eq!(row.chunk_context, None);
        assert_eq!(row.embedding_status, "done");
    }
    assert!(
        rig.embedder.calls().iter().all(|call| call.zdr),
        "restricted text must be embedded as zero-retention"
    );
}

/// Auth Core cannot resolve the organization's posture and issues no token: the unit
/// waits (pending, five minutes) and nothing is sent, and the wait is not counted as
/// an attempt.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn an_unresolved_posture_defers_and_sends_nothing() {
    let rig = Rig::new().await;
    let org = rig.org("up");
    rig.tenants.set(&org, Posture::Unresolved);
    let doc = rig.plain_document(&org, &CHUNKS).await;

    rig.run(rig.items(&doc)).await.expect("the batch succeeds");

    assert_eq!(rig.model.call_count(), 0);
    for row in rig.rows(&doc).await {
        assert_eq!(row.context_status.as_deref(), Some("pending"));
        assert_eq!(
            row.context_error_class.as_deref(),
            Some("posture_unavailable")
        );
        assert_eq!(row.context_attempts, 0);
        let due = seconds_from_now(row.context_next_attempt_at);
        assert!((240..=330).contains(&due), "retried in {due} s");
        assert_eq!(
            row.embedding_status, "done",
            "ingest is not held up by a posture"
        );
        assert_eq!(row.embedded_context_hash, None);
    }
}

// ── I3 and I4 ───────────────────────────────────────────────────────────────

/// I3: a batch with every kind of outcome in it, from different organizations at once
/// (as the stream builds them), leaves no unit without a status and none that is not
/// ready without a reason. The coverage view, read as the restricted application role
/// inside the org-scope helper (so row-level security is real), says the same.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn every_unit_ends_with_a_status_and_a_reason_when_it_is_not_ready() {
    let rig = Rig::with(|settings| {
        settings.inline_budget = InlineBudget::new(2_000, 4_000).expect("a small budget");
    })
    .await;
    let ok = rig.org("ok");
    let zdr = rig.org("zd");
    let unresolved = rig.org("un");
    let refused = rig.org("rf");
    let slow = rig.org("sl");
    let limited = rig.org("rl");
    let blank = rig.org("bl");
    rig.tenants.set(&zdr, Posture::Zdr);
    rig.tenants.set(&unresolved, Posture::Unresolved);
    rig.tenants.set(&refused, Posture::Refused);
    rig.model.set_for_org(&slow, Behaviour::Hang);
    rig.model
        .set_for_org(&limited, Behaviour::Fail("provider_rate_limited"));

    let ok_doc = rig.plain_document(&ok, &CHUNKS).await;
    let restricted = rig.document(&ok, "restricted", &CHUNKS[..2]).await;
    let zdr_doc = rig.plain_document(&zdr, &CHUNKS[..2]).await;
    let unresolved_doc = rig.plain_document(&unresolved, &CHUNKS[..2]).await;
    let refused_doc = rig.plain_document(&refused, &CHUNKS[..2]).await;
    let slow_doc = rig.plain_document(&slow, &CHUNKS[..2]).await;
    let limited_doc = rig.plain_document(&limited, &CHUNKS[..2]).await;
    let blank_doc = rig.plain_document(&blank, &CHUNKS[..2]).await;
    sqlx::query("UPDATE documents SET content = '   ' WHERE document_id = $1")
        .bind(&blank_doc.id)
        .execute(&rig.pool)
        .await
        .expect("blank the document");

    let docs = [
        &ok_doc,
        &restricted,
        &zdr_doc,
        &unresolved_doc,
        &refused_doc,
        &slow_doc,
        &limited_doc,
        &blank_doc,
    ];
    // One buffer, the units interleaved the way a busy stream delivers them.
    let mut per_doc: Vec<_> = docs.iter().map(|doc| rig.items(doc).into_iter()).collect();
    let mut items = Vec::new();
    loop {
        let mut any = false;
        for iterator in &mut per_doc {
            if let Some(item) = iterator.next() {
                items.push(item);
                any = true;
            }
        }
        if !any {
            break;
        }
    }

    let started = StdInstant::now();
    run_within(&rig, items).await;
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "the batch took {:?}",
        started.elapsed()
    );

    let expectations: [(&Doc, &str, Option<&str>); 8] = [
        (&ok_doc, "ready", None),
        (&restricted, "skipped_zdr", Some("zdr_document")),
        (&zdr_doc, "skipped_zdr", Some("zdr_org")),
        (&unresolved_doc, "pending", Some("posture_unavailable")),
        (&refused_doc, "failed_transient", Some("auth")),
        (&slow_doc, "pending", Some("timeout")),
        (&limited_doc, "failed_transient", Some("rate_limited")),
        (&blank_doc, "failed_permanent", Some("doc_missing")),
    ];
    for (doc, status, class) in expectations {
        for row in rig.rows(doc).await {
            assert_eq!(
                row.context_status.as_deref(),
                Some(status),
                "org {}",
                doc.org
            );
            assert_eq!(row.context_error_class.as_deref(), class, "org {}", doc.org);
            assert_eq!(
                row.embedding_status, "done",
                "ingest is never held up: org {}",
                doc.org
            );
        }
    }

    // The same, through the coverage view as the application role.
    let mut tx = pg_org_scope::begin_org_scoped(&rig.pool, &ok)
        .await
        .expect("scope");
    let coverage: (i64, i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT eligible, ready, status_unknown, vector_incoherent, ready_unverified, excluded_by_classification \
           FROM v_context_coverage WHERE org_id = $1",
    )
    .bind(&ok)
    .fetch_one(&mut *tx)
    .await
    .expect("coverage");
    tx.commit().await.expect("commit");
    assert_eq!(
        coverage,
        (3, 3, 0, 0, 0, 2),
        "eligible, ready, unknown, incoherent, unverified, excluded"
    );
    let mut tx = pg_org_scope::begin_org_scoped(&rig.pool, &slow)
        .await
        .expect("scope");
    let pending: (i64, i64, i64) = sqlx::query_as(
        "SELECT eligible, pending, status_unknown FROM v_context_coverage WHERE org_id = $1",
    )
    .bind(&slow)
    .fetch_one(&mut *tx)
    .await
    .expect("coverage");
    tx.commit().await.expect("commit");
    assert_eq!(pending, (2, 2, 0));
}

/// I4: a provider that never answers for one organization leaves that organization's
/// units pending, due at once and embedded raw, within the inline budget, while another
/// organization's units in the same batch are contextualized, persisted and embedded
/// from their contexts.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn one_tenants_provider_that_never_answers_does_not_hold_up_another() {
    let rig = Rig::with(|settings| {
        settings.inline_budget = InlineBudget::new(2_000, 5_000).expect("a small budget");
    })
    .await;
    let slow = rig.org("sl");
    let fast = rig.org("fa");
    rig.model.set_for_org(&slow, Behaviour::Hang);
    let slow_doc = rig.plain_document(&slow, &CHUNKS).await;
    let fast_doc = rig.plain_document(&fast, &CHUNKS).await;
    let mut items = rig.items(&slow_doc);
    items.extend(rig.items(&fast_doc));

    let started = StdInstant::now();
    run_within(&rig, items).await;
    let took = started.elapsed();

    assert!(
        took < Duration::from_secs(15),
        "the slow tenant held the batch for {took:?}"
    );
    assert!(
        !rig.model.calls_for(&slow).is_empty(),
        "the slow tenant was tried"
    );
    assert_eq!(rig.model.calls_for(&fast).len(), CHUNKS.len());
    for (unit, row) in slow_doc.units.iter().zip(rig.rows(&slow_doc).await) {
        assert_eq!(row.context_status.as_deref(), Some("pending"));
        assert_eq!(row.context_error_class.as_deref(), Some("timeout"));
        assert!(
            seconds_from_now(row.context_next_attempt_at) <= 1,
            "due at once"
        );
        assert_eq!(row.chunk_context, None);
        assert_eq!(row.embedding_status, "done");
        assert_eq!(row.embedded_context_hash, None);
        assert_eq!(
            rig.vectors.stored(&unit.knowledge_id).unwrap().vector,
            vector_for(&unit.text)
        );
    }
    for (unit, row) in fast_doc.units.iter().zip(rig.rows(&fast_doc).await) {
        assert_ready(&row, unit, &input_hash(&rig, &fast_doc, unit));
        assert_eq!(
            rig.vectors.stored(&unit.knowledge_id).unwrap().vector,
            vector_for(&composed(unit))
        );
    }
}

// ── tenancy of the statements ───────────────────────────────────────────────

/// The batch is unscoped by design (it mixes organizations on the superuser
/// connection, where row-level security is inert), so every statement names the
/// organization itself. An item that claims another organization's unit is not that
/// unit: the real unit is not read, written, embedded or marked.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_unit_claimed_under_the_wrong_organization_is_left_alone() {
    let rig = Rig::new().await;
    let a = rig.org("ta");
    let b = rig.org("tb");
    let mine = rig.plain_document(&a, &CHUNKS[..1]).await;
    let theirs = rig.plain_document(&b, &CHUNKS[1..2]).await;
    let mut items = rig.items(&mine);
    let mut forged = rig.items(&theirs);
    forged[0].org_id = a.clone();
    items.append(&mut forged);

    rig.run(items).await.expect("the batch succeeds");

    let untouched = rig.row(&theirs.units[0].knowledge_id).await;
    assert_eq!(untouched.org_id, b);
    assert_eq!(untouched.embedding_status, "pending");
    assert_eq!(untouched.chunk_context, None);
    assert_eq!(untouched.context_status, None);
    assert_eq!(untouched.embedded_context_hash, None);
    assert!(rig.vectors.stored(&theirs.units[0].knowledge_id).is_none());
    assert!(rig.model.calls().iter().all(|call| call.org_id == a));
    assert_ready(
        &rig.row(&mine.units[0].knowledge_id).await,
        &mine.units[0],
        &input_hash(&rig, &mine, &mine.units[0]),
    );
}

/// The same, one level down: a unit claimed under another document of the same organization
/// would be given that other document's text to be situated in, so it is not the unit the
/// caller named and is left alone.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_unit_claimed_under_another_document_is_left_alone() {
    let rig = Rig::new().await;
    let org = rig.org("td");
    let first = rig.plain_document(&org, &CHUNKS[..1]).await;
    let second = rig.plain_document(&org, &CHUNKS[1..2]).await;
    let mut forged = rig.items(&first);
    forged[0].document_id = second.id.clone();

    rig.run(forged).await.expect("the batch succeeds");

    let row = rig.row(&first.units[0].knowledge_id).await;
    assert_eq!(row.embedding_status, "pending");
    assert_eq!(row.chunk_context, None);
    assert_eq!(rig.model.call_count(), 0);
    assert!(rig.vectors.points().is_empty());
}

/// An event that did not carry its document is not lost: the database is the authority on
/// what a unit belongs to, so the unit is processed under the document it has, and the
/// vector's payload names it (an empty document id in the payload is what this replaces).
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn an_item_without_a_document_id_takes_the_one_the_database_has() {
    let rig = Rig::new().await;
    let org = rig.org("nd");
    let doc = rig.plain_document(&org, &CHUNKS[..2]).await;
    let mut items = rig.items(&doc);
    for item in &mut items {
        item.document_id.clear();
    }

    rig.run(items).await.expect("the batch succeeds");

    for (unit, row) in doc.units.iter().zip(rig.rows(&doc).await) {
        assert_ready(&row, unit, &input_hash(&rig, &doc, unit));
        assert_eq!(row.embedding_status, "done");
    }
    assert!(rig
        .vectors
        .points()
        .iter()
        .all(|point| point.document_id == doc.id));
}

/// The same unit twice in one buffer (a redelivery next to the original) is one unit:
/// generated once, embedded once.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn the_same_unit_twice_in_one_batch_is_generated_and_embedded_once() {
    let rig = Rig::new().await;
    let org = rig.org("dd");
    let doc = rig.plain_document(&org, &CHUNKS).await;
    let mut items = rig.items(&doc);
    items.extend(rig.items(&doc));

    rig.run(items).await.expect("the batch succeeds");

    assert_eq!(
        rig.model.call_count(),
        CHUNKS.len(),
        "a unit was paid for twice"
    );
    let asked: std::collections::BTreeSet<String> = rig
        .model
        .calls()
        .into_iter()
        .map(|call| call.chunk)
        .collect();
    assert_eq!(asked.len(), CHUNKS.len(), "each chunk was asked about once");
    assert_eq!(rig.embedder.texts().len(), CHUNKS.len());
    assert_eq!(rig.vectors.points().len(), CHUNKS.len());
}

/// One unit's context that the database itself refuses (a constraint, a trigger, a
/// deadlock) costs that unit its context and nothing else: the batch succeeds, the other
/// units keep theirs, and the refused unit is embedded raw with a retryable status that says
/// it was the database. Injected for real, by a trigger that refuses one chunk's context.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_context_the_database_refuses_costs_that_unit_its_context_and_nothing_else() {
    let rig = Rig::new().await;
    let org = rig.org("rf");
    let tag = org.replace('-', "_").to_lowercase();
    let name = format!("crt8_poison_{tag}");
    install(
        &rig.pool,
        format!(
            "CREATE FUNCTION {name}_fn() RETURNS trigger LANGUAGE plpgsql AS $fn$
             BEGIN
               IF NEW.org_id = '{org}' AND NEW.chunk_context LIKE '%GIFT-avsnitt%' THEN
                 RAISE EXCEPTION 'injected refusal of one context';
               END IF;
               RETURN NEW;
             END $fn$;
             CREATE TRIGGER {name}_tr BEFORE UPDATE ON knowledge_units
               FOR EACH ROW EXECUTE FUNCTION {name}_fn();"
        ),
    )
    .await;
    let chunks = [CHUNKS[0], "GIFT-avsnitt om Hammerfest.", CHUNKS[2]];
    let doc = rig.plain_document(&org, &chunks).await;

    let result = tokio::time::timeout(WITHIN, rig.run(rig.items(&doc))).await;
    install(
        &rig.pool,
        format!("DROP TRIGGER {name}_tr ON knowledge_units; DROP FUNCTION {name}_fn();"),
    )
    .await;
    result
        .expect("in time")
        .expect("a refused context never fails the batch");

    let rows = rig.rows(&doc).await;
    for index in [0, 2] {
        assert_ready(
            &rows[index],
            &doc.units[index],
            &input_hash(&rig, &doc, &doc.units[index]),
        );
        assert_eq!(rows[index].embedding_status, "done");
    }
    let refused = &rows[1];
    assert_eq!(refused.chunk_context, None);
    assert_eq!(refused.context_status.as_deref(), Some("failed_transient"));
    assert_eq!(refused.context_error_class.as_deref(), Some("internal"));
    assert_eq!(refused.context_attempts, 1);
    assert_eq!(
        refused.embedding_status, "done",
        "the unit is still embedded, raw"
    );
    assert_eq!(refused.embedded_context_hash, None);
    assert_eq!(
        rig.vectors
            .stored(&doc.units[1].knowledge_id)
            .unwrap()
            .vector,
        vector_for(&doc.units[1].text)
    );
}

/// The statements run as the restricted application role inside the real org-scope helper,
/// which is how the reconciler runs them: row-level security is real there, and the
/// explicit organization in every statement and the policy agree. Another organization's
/// unit is neither visible nor writable, and its document is not announced.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn the_statements_work_inside_the_org_scope_as_the_application_role() {
    use super::store::{self, UnitRef};

    let rig = Rig::new().await;
    let (a, b) = (rig.org("sa"), rig.org("sb"));
    let mine = rig.plain_document(&a, &CHUNKS[..2]).await;
    let theirs = rig.plain_document(&b, &CHUNKS[2..]).await;
    let mine_unit = |index: usize| UnitRef {
        knowledge_id: &mine.units[index].knowledge_id,
        org_id: &a,
    };
    let theirs_unit = UnitRef {
        knowledge_id: &theirs.units[0].knowledge_id,
        org_id: &b,
    };
    let hash = "b".repeat(64);

    let mut tx = pg_org_scope::begin_org_scoped(&rig.pool, &a)
        .await
        .expect("scope");
    let (role,): (String,) = sqlx::query_as("SELECT current_user::text")
        .fetch_one(&mut *tx)
        .await
        .expect("role");
    assert_eq!(
        role, "dataplane_app",
        "this must run under row-level security"
    );

    // Reading: the other organization's unit is not there, though it was named properly.
    let found = store::load(&mut tx, &[mine_unit(0), mine_unit(1), theirs_unit])
        .await
        .expect("load");
    assert_eq!(found.len(), 2);
    let documents = store::documents(
        &mut tx,
        &[
            (mine.id.as_str(), a.as_str()),
            (theirs.id.as_str(), b.as_str()),
        ],
    )
    .await
    .expect("documents");
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].total_chunks, 2);

    // Writing a context: only the organization's own unit.
    let written = store::write_ready(
        &mut tx,
        &[
            ready_write(mine_unit(0), &hash),
            ready_write(theirs_unit, &hash),
        ],
    )
    .await
    .expect("write_ready");
    let expected: std::collections::HashSet<String> =
        std::iter::once(mine.units[0].knowledge_id.clone()).collect();
    assert_eq!(written, expected);

    // Writing a status.
    let written = store::write_status(
        &mut tx,
        &[pending_status(mine_unit(1)), pending_status(theirs_unit)],
    )
    .await
    .expect("write_status");
    let expected: std::collections::HashSet<String> =
        std::iter::once(mine.units[1].knowledge_id.clone()).collect();
    assert_eq!(written, expected);

    // Marking done, and the announcement.
    let marked = store::mark_done(
        &mut tx,
        &[
            done_write(mine_unit(0)),
            done_write(mine_unit(1)),
            done_write(theirs_unit),
        ],
        "m",
    )
    .await
    .expect("mark_done");
    assert_eq!(marked, 2);
    let announced = store::indexed_documents(
        &mut tx,
        &[
            (mine.id.as_str(), a.as_str()),
            (theirs.id.as_str(), b.as_str()),
        ],
    )
    .await
    .expect("indexed");
    assert_eq!(announced.len(), 1);
    assert_eq!(announced[0].document_id, mine.id);
    tx.commit().await.expect("commit");

    // As the superuser: the organization's units changed, the other's did not.
    assert_eq!(
        rig.row(&mine.units[0].knowledge_id)
            .await
            .context_status
            .as_deref(),
        Some("ready")
    );
    assert_eq!(
        rig.row(&mine.units[1].knowledge_id)
            .await
            .context_status
            .as_deref(),
        Some("pending")
    );
    let untouched = rig.row(&theirs.units[0].knowledge_id).await;
    assert_eq!(untouched.embedding_status, "pending");
    assert_eq!(untouched.context_status, None);
    assert_eq!(untouched.chunk_context, None);
}

/// On the connection every service really uses, the superuser, row-level security is inert, so
/// the organization written into each statement is the ONLY tenant check there is. Every
/// statement is given another organization's unit and document under the wrong organization,
/// and must read, write, mark and announce nothing.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn every_statement_names_the_organization_even_where_row_level_security_is_inert() {
    use super::store::{self, UnitRef};

    let rig = Rig::new().await;
    let (a, b) = (rig.org("ua"), rig.org("ub"));
    let theirs = rig.plain_document(&b, &CHUNKS[..1]).await;
    let forged = UnitRef {
        knowledge_id: &theirs.units[0].knowledge_id,
        org_id: &a,
    };
    let hash = "c".repeat(64);
    let mut conn = rig.pool.acquire().await.expect("a connection");
    let (role,): (String,) = sqlx::query_as("SELECT current_user::text")
        .fetch_one(&mut *conn)
        .await
        .expect("role");
    assert_ne!(role, "dataplane_app", "this must run where RLS is inert");

    assert!(store::load(&mut conn, &[forged])
        .await
        .expect("load")
        .is_empty());
    assert!(
        store::documents(&mut conn, &[(theirs.id.as_str(), a.as_str())])
            .await
            .expect("documents")
            .is_empty()
    );
    assert!(store::write_ready(&mut conn, &[ready_write(forged, &hash)])
        .await
        .expect("write_ready")
        .is_empty());
    assert!(store::write_status(&mut conn, &[pending_status(forged)])
        .await
        .expect("write_status")
        .is_empty());
    assert_eq!(
        store::mark_done(&mut conn, &[done_write(forged)], "m")
            .await
            .expect("mark_done"),
        0
    );
    assert!(
        store::indexed_documents(&mut conn, &[(theirs.id.as_str(), a.as_str())])
            .await
            .expect("indexed")
            .is_empty()
    );
    drop(conn);

    let untouched = rig.row(&theirs.units[0].knowledge_id).await;
    assert_eq!(untouched.embedding_status, "pending");
    assert_eq!(untouched.chunk_context, None);
    assert_eq!(untouched.context_status, None);
    let (status,): (String,) =
        sqlx::query_as("SELECT status FROM documents WHERE document_id = $1")
            .bind(&theirs.id)
            .fetch_one(&rig.pool)
            .await
            .expect("the document");
    assert_eq!(
        status, "pending",
        "the document was announced under the wrong organization"
    );
}

/// The lookup behind the `source` stamp (`source_stamp::load`, which the batch pipeline, the
/// video-caption path and the backfill tool share) finds a document only under the
/// organization it is asked for. As the application role inside the real org-scope helper,
/// row-level security and the explicit predicate agree; on the superuser connection every
/// service uses, where RLS is inert, the predicate alone keeps one organization's document from
/// lending its source to another's point.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn the_source_lookup_finds_a_document_only_under_its_own_organization() {
    let rig = Rig::new().await;
    let (a, b) = (rig.org("la"), rig.org("lb"));
    let mine = rig
        .document_from("quarry", &a, "internal", &CHUNKS[..1])
        .await;
    let theirs = rig
        .document_from("chat-upload", &b, "internal", &CHUNKS[1..2])
        .await;
    let gone = rig
        .document_from("sharepoint", &a, "internal", &CHUNKS[2..])
        .await;
    sqlx::query("UPDATE documents SET deleted_at = now() WHERE document_id = $1 AND org_id = $2")
        .bind(&gone.id)
        .bind(&a)
        .execute(&rig.pool)
        .await
        .expect("soft-delete a document");

    // As the application role: only the organization's own documents are visible at all.
    let mut tx = pg_org_scope::begin_org_scoped(&rig.pool, &a)
        .await
        .expect("scope");
    let (role,): (String,) = sqlx::query_as("SELECT current_user::text")
        .fetch_one(&mut *tx)
        .await
        .expect("role");
    assert_eq!(
        role, "dataplane_app",
        "this must run under row-level security"
    );
    let found = crate::source_stamp::load(
        &mut tx,
        &[
            (mine.id.as_str(), a.as_str()),
            (theirs.id.as_str(), b.as_str()),
        ],
    )
    .await
    .expect("lookup under the scope");
    tx.commit().await.expect("commit");
    assert_eq!(found.len(), 1);
    assert_eq!(found[&(mine.id.clone(), a.clone())], "quarry");

    // On the superuser connection RLS is inert: the pair is the only tenant check.
    let mut conn = rig.pool.acquire().await.expect("a connection");
    let (role,): (String,) = sqlx::query_as("SELECT current_user::text")
        .fetch_one(&mut *conn)
        .await
        .expect("role");
    assert_ne!(role, "dataplane_app", "this must run where RLS is inert");
    let found = crate::source_stamp::load(
        &mut conn,
        &[
            // Each document under its own organization: found.
            (mine.id.as_str(), a.as_str()),
            (theirs.id.as_str(), b.as_str()),
            // The other organization's document claimed under this one, and a document that
            // does not exist: not found, never the other organization's source.
            (theirs.id.as_str(), a.as_str()),
            (mine.id.as_str(), b.as_str()),
            ("no-such-document", a.as_str()),
            // A soft-deleted document still has a source: the stamp is a fact about the row.
            (gone.id.as_str(), a.as_str()),
            // A pair that names nothing is not looked up.
            ("", a.as_str()),
            (mine.id.as_str(), ""),
            // The same pair twice is one pair.
            (mine.id.as_str(), a.as_str()),
        ],
    )
    .await
    .expect("lookup on the superuser connection");
    let expected: std::collections::HashMap<(String, String), String> = [
        ((mine.id.clone(), a.clone()), "quarry"),
        ((theirs.id.clone(), b.clone()), "chat-upload"),
        ((gone.id.clone(), a.clone()), "sharepoint"),
    ]
    .into_iter()
    .map(|(pair, source)| (pair, source.to_owned()))
    .collect();
    assert_eq!(found, expected);
    assert!(crate::source_stamp::load(&mut conn, &[])
        .await
        .expect("an empty lookup")
        .is_empty());
}

/// The service refuses to start against a database that lacks the columns it writes, and
/// says which migration to run. Against the migrated schema it starts.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn the_service_names_the_missing_migration_at_startup() {
    let rig = Rig::new().await;
    super::ensure_schema(&rig.pool)
        .await
        .expect("the migrated schema has every column");

    // A schema from before the provenance migration: only what the old pipeline wrote.
    let schema = format!(
        "crt8_old_{}",
        rig.org("sc").replace('-', "_").to_lowercase()
    );
    sqlx::raw_sql(&format!(
        "CREATE SCHEMA {schema};
         CREATE TABLE {schema}.knowledge_units (
             knowledge_id text PRIMARY KEY, org_id text, embedding_model text,
             chunk_context text, context_prompt_version text);"
    ))
    .execute(&rig.pool)
    .await
    .expect("create the old schema");
    let url = std::env::var(super::testing::GATE).expect("the gate");
    let search_path = format!("SET search_path TO {schema}");
    let old = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .after_connect(move |connection, _| {
            let statement = search_path.clone();
            Box::pin(async move {
                sqlx::query(&statement).execute(connection).await?;
                Ok(())
            })
        })
        .connect(&url)
        .await
        .expect("connect with the old schema first");

    let error = super::ensure_schema(&old)
        .await
        .expect_err("the old schema must be refused");
    sqlx::raw_sql(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&rig.pool)
        .await
        .expect("drop the old schema");
    let message = error.to_string();
    assert!(
        message.contains("20261003130000_context_provenance"),
        "{message}"
    );
    assert!(
        message.contains("context_status") && message.contains("embedded_context_hash"),
        "{message}"
    );
    assert!(
        !message.contains("chunk_context"),
        "a column that exists is not reported: {message}"
    );
}

/// A unit deleted while its event waited is not embedded: a vector for a row that is
/// gone would be an orphan nothing ever removes.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_unit_deleted_while_queued_is_not_embedded() {
    let rig = Rig::new().await;
    let org = rig.org("dl");
    let doc = rig.plain_document(&org, &CHUNKS).await;
    sqlx::query("DELETE FROM knowledge_units WHERE knowledge_id = $1")
        .bind(&doc.units[1].knowledge_id)
        .execute(&rig.pool)
        .await
        .expect("delete a unit");

    rig.run(rig.items(&doc)).await.expect("the batch succeeds");

    assert!(
        rig.vectors.stored(&doc.units[1].knowledge_id).is_none(),
        "an orphan vector"
    );
    assert_eq!(
        rig.model.call_count(),
        2,
        "nothing was generated for the deleted unit"
    );
    assert_eq!(rig.embedder.texts().len(), 2);
    for unit in [&doc.units[0], &doc.units[2]] {
        assert_eq!(rig.row(&unit.knowledge_id).await.embedding_status, "done");
    }
}

// ── review additions: zero retention read again, and the usage a batch reports ──────────

/// The classification the stream read when it queued a unit is not the last word. A
/// document that became restricted while its units waited (the batch can hold them for
/// most of a minute) is read again when the context is decided: its text is not sent, no
/// bearer is asked for, and the unit says why. (Before this, only the flag the stream
/// carried decided it.)
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_document_that_became_restricted_while_its_units_waited_is_not_sent() {
    let rig = Rig::new().await;
    let org = rig.org("rw");
    let doc = rig.plain_document(&org, &CHUNKS).await;
    // The items as the stream built them: the document was not restricted then.
    let items = rig.items(&doc);
    assert!(items.iter().all(|item| !item.zdr));
    sqlx::query("UPDATE documents SET zdr_classification = 'restricted' WHERE document_id = $1")
        .bind(&doc.id)
        .execute(&rig.pool)
        .await
        .expect("restrict the document");

    rig.run(items).await.expect("the batch succeeds");

    assert_eq!(
        rig.model.call_count(),
        0,
        "the text of a restricted document reached the provider"
    );
    assert!(
        rig.tenants.requests().is_empty(),
        "a bearer was asked for a restricted document"
    );
    for row in rig.rows(&doc).await {
        assert_eq!(row.context_status.as_deref(), Some("skipped_zdr"));
        assert_eq!(row.context_error_class.as_deref(), Some("zdr_document"));
        assert_eq!(row.context_next_attempt_at, None);
        assert_eq!(row.chunk_context, None);
    }
    assert!(
        !rig.embedder.calls().is_empty() && rig.embedder.calls().iter().all(|call| call.zdr),
        "the text of a restricted document was embedded as if it were not zero-retention"
    );
}

/// What the usage ledger (CR-T10) is built from: every provider call a batch made, once,
/// by organization and document. A restricted document and a unit whose context was
/// reused make none; the per-call tokens are the ones the provider reported.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn the_provider_calls_a_batch_made_are_reported_once_each_by_organization_and_document() {
    let rig = Rig::new().await;
    let (a, b, c) = (rig.org("ma"), rig.org("mb"), rig.org("mc"));
    let doc_a = rig.plain_document(&a, &CHUNKS).await;
    let doc_b = rig.plain_document(&b, &CHUNKS[..2]).await;
    let doc_c = rig.document(&c, "restricted", &CHUNKS[..1]).await;
    let mut items = rig.items(&doc_a);
    items.extend(rig.items(&doc_b));
    items.extend(rig.items(&doc_c));

    let (_, report) = super::contexts::ensure(&rig.pipeline(), items)
        .await
        .expect("the context phase succeeds");

    let mut reported: Vec<(String, String, usize)> = report
        .metered
        .iter()
        .map(|document| {
            (
                document.org_id.clone(),
                document.document_id.clone(),
                document.calls.len(),
            )
        })
        .collect();
    reported.sort();
    let mut expected = vec![
        (a.clone(), doc_a.id.clone(), 3),
        (b.clone(), doc_b.id.clone(), 2),
    ];
    expected.sort();
    assert_eq!(reported, expected, "one entry per document that was called");
    assert_eq!(
        usize::try_from(report.provider_calls()).expect("small"),
        rig.model.call_count(),
        "what is reported is what the provider was asked"
    );
    let requests: std::collections::BTreeSet<&str> = report
        .metered
        .iter()
        .flat_map(|document| &document.calls)
        .map(|call| call.request_id.as_str())
        .collect();
    assert_eq!(requests.len(), 5, "a call is reported once, not twice");
    // The fake provider reports 612 prompt and 48 completion tokens per call.
    assert_eq!(report.billed_tokens(), (5 * 612, 5 * 48, 0));

    // A second pass reuses what is stored: nothing is called, so nothing is reported.
    let again = rig.items(&doc_a);
    let (_, report) = super::contexts::ensure(&rig.pipeline(), again)
        .await
        .expect("the second pass succeeds");
    assert!(report.metered.is_empty());
    assert_eq!(report.provider_calls(), 0);
    assert_eq!(rig.model.call_count(), 5);
}

/// "Whatever a caller puts in `chunk_context` is ignored" (see `BatchItem`): the vector is
/// built from the context the DATABASE holds, and a unit that has none is embedded raw
/// and says so (`embedded_context_hash` NULL). A restricted document has no context
/// stored and none generated, so a context carried on its items is one nobody can
/// reproduce.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_context_put_on_an_item_by_the_caller_is_not_what_the_vector_is_built_from() {
    let rig = Rig::new().await;
    let org = rig.org("fc");
    let doc = rig.document(&org, "restricted", &CHUNKS[..2]).await;
    let mut items = rig.items(&doc);
    for item in &mut items {
        item.chunk_context = Some("Utdiktet kontekst fra den som kalte.".to_owned());
    }

    rig.run(items).await.expect("the batch succeeds");

    for (unit, row) in doc.units.iter().zip(rig.rows(&doc).await) {
        assert_eq!(row.chunk_context, None);
        assert_eq!(
            row.embedded_context_hash, None,
            "a raw vector must not claim a context"
        );
        assert_eq!(
            rig.vectors.stored(&unit.knowledge_id).unwrap().vector,
            vector_for(&unit.text),
            "the vector was built from a context the database does not hold"
        );
    }
    let mut embedded = rig.embedder.texts();
    embedded.sort();
    let mut raw: Vec<String> = doc.units.iter().map(|unit| unit.text.clone()).collect();
    raw.sort();
    assert_eq!(embedded, raw);
}

/// The compare-and-swap covers the status as well as the context: a worker whose attempt
/// failed must not write its failure (an error class, an attempt, a next attempt) over a
/// unit another worker has just finished, or the reconciler would regenerate a context
/// that is already right.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_failed_attempt_does_not_write_its_status_over_a_context_another_writer_finished() {
    let rig = Rig::new().await;
    let org = rig.org("sw");
    let doc = rig.plain_document(&org, &CHUNKS[..1]).await;
    let winner = "Kontekst skrevet av en annen arbeider først.";
    let winner_hash = "a".repeat(64);
    let raced = doc.units[0].knowledge_id.clone();
    rig.model
        .set_default(Behaviour::Fail("provider_unavailable"));
    {
        let pool = rig.pool.clone();
        let raced = raced.clone();
        let winner_hash = winner_hash.clone();
        rig.model.on_call(std::sync::Arc::new(move |_call| {
            let pool = pool.clone();
            let raced = raced.clone();
            let winner_hash = winner_hash.clone();
            async move {
                sqlx::query(
                    "UPDATE knowledge_units SET chunk_context = $2, context_status = 'ready', \
                     context_model = 'gpt-4o-mini', context_provider = 'azure-openai', \
                     context_residency = 'eu', context_prompt_version = 'ctx-v1', \
                     context_input_hash = $3, context_generated_at = now() WHERE knowledge_id = $1",
                )
                .bind(&raced)
                .bind(winner)
                .bind(&winner_hash)
                .execute(&pool)
                .await
                .expect("the other writer");
            }
            .boxed()
        }));
    }

    rig.run(rig.items(&doc)).await.expect("the batch succeeds");

    let row = rig.row(&raced).await;
    assert_eq!(row.chunk_context.as_deref(), Some(winner));
    assert_eq!(row.context_status.as_deref(), Some("ready"));
    assert_eq!(
        row.context_error_class, None,
        "a failed attempt was written over a finished context"
    );
    assert_eq!(row.context_attempts, 0);
    assert_eq!(row.context_next_attempt_at, None);
    assert_eq!(
        row.embedded_context_hash.as_deref(),
        Some(hash_of(winner).as_str())
    );
}

// ── the usage ledger's wiring (CR-T10) ──────────────────────────────────────
//
// `usage.rs` tests the events one at a time. These run the whole pipeline with a ledger and
// prove where the events come from and that nothing is booked twice: with the ledger on, the
// paid calls are the ledger's events and the version 1 cost event is NOT published (it would
// book the same embeddings a second time); with it off, the version 1 event is published as it
// always was and the ledger has nothing.

use usage_ledger_rs::MemoryLedger;

async fn run_with_ledger(rig: &Rig, ledger: &MemoryLedger, items: Vec<super::BatchItem>) {
    let pipeline = Pipeline {
        usage: Some(ledger),
        ..rig.pipeline()
    };
    tokio::time::timeout(WITHIN, super::process_batch(items, &pipeline))
        .await
        .unwrap_or_else(|_| panic!("the batch was not finished within {WITHIN:?}"))
        .expect("the batch succeeds");
}

#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn with_the_ledger_on_every_paid_call_is_an_event_and_no_v1_event_books_it_again() {
    let rig = Rig::new().await;
    let org = rig.org("ul");
    let doc = rig.plain_document(&org, &CHUNKS).await;
    let ledger = MemoryLedger::new();

    run_with_ledger(&rig, &ledger, rig.items(&doc)).await;

    let events = ledger.events();
    assert_eq!(
        events.len(),
        2,
        "one event for the contexts and one for the embeddings: {events:?}"
    );
    let of = |feature: &str| {
        events
            .iter()
            .filter(|event| event.feature == feature)
            .collect::<Vec<_>>()
    };

    let contexts = of("contextual-retrieval");
    assert_eq!(contexts.len(), 1);
    let context = contexts[0];
    // The fake provider reports 612 prompt and 48 completion tokens per call: three calls.
    assert_eq!(
        (context.count, context.input_tokens, context.output_tokens),
        (3, 3 * 612, 3 * 48)
    );
    assert_eq!(
        (context.estimated, context.zdr, context.org_id.as_str()),
        (false, false, org.as_str())
    );

    let embeddings = of("embedding");
    assert_eq!(embeddings.len(), 1);
    let embedding = embeddings[0];
    let sent: u64 = rig
        .embedder
        .texts()
        .iter()
        .map(|text| text.len() as u64 / 4)
        .sum();
    assert_eq!(
        (embedding.count, embedding.input_tokens),
        (3, sent),
        "the estimate is of what was embedded: the composed text, not the bare chunk"
    );
    assert!(embedding.estimated, "an embedding request reports no usage");
    assert_eq!(embedding.model_used, EMBEDDING_MODEL);
    assert_eq!(embedding.org_id, org);

    assert!(
        rig.events.on(super::SUBJECT_COST_LEDGER).is_empty(),
        "the version 1 cost event was published as well as the ledger's: the embeddings are booked twice"
    );
}

/// A redelivery reuses the stored contexts and calls nothing, so the contexts are recorded once;
/// the embeddings are paid for again, and that second payment is a second event with a key of
/// its own (not dropped as a duplicate of the first).
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_redelivered_batch_records_its_contexts_once_and_pays_for_its_embeddings_again() {
    let rig = Rig::new().await;
    let org = rig.org("ur");
    let doc = rig.plain_document(&org, &CHUNKS).await;
    let ledger = MemoryLedger::new();

    run_with_ledger(&rig, &ledger, rig.items(&doc)).await;
    run_with_ledger(&rig, &ledger, rig.items(&doc)).await;

    assert_eq!(
        rig.embedder.calls().len(),
        2,
        "the redelivered units were embedded again"
    );
    let events = ledger.events();
    let contexts: Vec<_> = events
        .iter()
        .filter(|event| event.feature == "contextual-retrieval")
        .collect();
    let embeddings: Vec<_> = events
        .iter()
        .filter(|event| event.feature == "embedding")
        .collect();
    assert_eq!(
        contexts.len(),
        1,
        "the second pass reused the stored contexts and called no provider"
    );
    assert_eq!(embeddings.len(), 2);
    assert_ne!(
        embeddings[0].idempotency_key, embeddings[1].idempotency_key,
        "the second payment would be dropped as a duplicate of the first"
    );
}

/// A restricted (zero-retention) document is embedded and recorded, as counters: the event is
/// flagged `zdr`, names no user and no document, and no context call was made for it.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_restricted_documents_embedding_is_recorded_as_counters_and_names_nobody() {
    let rig = Rig::new().await;
    let org = rig.org("uz");
    let doc = rig.document(&org, "restricted", &CHUNKS[..2]).await;
    let ledger = MemoryLedger::new();

    run_with_ledger(&rig, &ledger, rig.items(&doc)).await;

    let events = ledger.events();
    assert_eq!(
        events.len(),
        1,
        "only the embedding: nothing was sent to a chat model: {events:?}"
    );
    let event = &events[0];
    assert_eq!(event.feature, "embedding");
    assert!(event.zdr, "the event must carry the zero-retention posture");
    assert_eq!((event.count, event.org_id.as_str()), (2, org.as_str()));
    let payload = String::from_utf8(event.payload().expect("payload")).expect("utf-8");
    for private in std::iter::once("user-1")
        .chain(std::iter::once(doc.id.as_str()))
        .chain(doc.units.iter().map(|unit| unit.knowledge_id.as_str()))
    {
        assert!(
            !payload.contains(private),
            "the event names {private}: {payload}"
        );
    }
    assert!(rig.events.on(super::SUBJECT_COST_LEDGER).is_empty());
}

/// Recording never fails a batch: a ledger that refuses every event loses those events (loudly)
/// and the units are embedded and marked done all the same.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_ledger_that_cannot_be_reached_never_fails_the_batch() {
    let rig = Rig::new().await;
    let org = rig.org("uf");
    let doc = rig.plain_document(&org, &CHUNKS).await;
    let ledger = MemoryLedger::new();
    ledger.refuse_next(10);

    run_with_ledger(&rig, &ledger, rig.items(&doc)).await;

    assert!(ledger.events().is_empty(), "every event was refused");
    for row in rig.rows(&doc).await {
        assert_eq!(row.embedding_status, "done");
    }
}

/// The provider is paid when it answers, not when the batch's database writes succeed: a
/// database that goes away between the calls and the write-ahead fails the batch (it is
/// redelivered, generates again and pays again), and the calls already made must still be in the
/// ledger. Recorded after the batch, they were lost with it.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn calls_paid_for_before_the_database_failed_are_still_recorded() {
    let rig = Rig::new().await;
    let org = rig.org("ud");
    let doc = rig.plain_document(&org, &CHUNKS[..1]).await;
    let ledger = MemoryLedger::new();
    {
        // The first provider call takes the database away: everything after it that needs
        // a connection (the write-ahead of the contexts) fails.
        let pool = rig.pool.clone();
        rig.model.on_call(std::sync::Arc::new(move |_call| {
            let pool = pool.clone();
            async move { pool.close().await }.boxed()
        }));
    }
    let pipeline = Pipeline {
        usage: Some(&ledger),
        ..rig.pipeline()
    };

    let result = tokio::time::timeout(WITHIN, super::process_batch(rig.items(&doc), &pipeline))
        .await
        .unwrap_or_else(|_| panic!("the batch was not finished within {WITHIN:?}"));

    assert!(
        result.is_err(),
        "the database was gone: the batch must fail so that it is redelivered"
    );
    let events = ledger.events();
    assert_eq!(
        events.len(),
        1,
        "the provider call that was made was not recorded: {events:?}"
    );
    assert_eq!(
        (
            events[0].feature,
            events[0].count,
            events[0].input_tokens,
            events[0].output_tokens
        ),
        ("contextual-retrieval", 1, 612, 48)
    );
}

/// With the ledger off the version 1 event is published as it always was, one per
/// (organization, user, zero-retention) group and each with a key of its own: the key used to be
/// one value for the whole batch, and the consumer stores it behind the organization only, so
/// a second user of the organization was dropped as a duplicate.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn with_the_usage_ledger_off_the_version_1_event_is_published_per_user_with_its_own_key() {
    let rig = Rig::new().await;
    let org = rig.org("u1");
    let doc = rig.plain_document(&org, &CHUNKS).await;
    let mut items = rig.items(&doc);
    items[1].user_id = Some("user-2".to_owned());

    run_within(&rig, items).await;

    let cost = rig.events.on(super::SUBJECT_COST_LEDGER);
    assert_eq!(cost.len(), 2, "one event per user");
    let mut counts: Vec<u64> = cost
        .iter()
        .map(|event| event.payload["count"].as_u64().expect("a count"))
        .collect();
    counts.sort_unstable();
    assert_eq!(counts, vec![1, 2]);
    assert_ne!(
        cost[0].payload["idempotency_key"], cost[1].payload["idempotency_key"],
        "two users of one organization share a key, so the consumer drops the second"
    );
}
