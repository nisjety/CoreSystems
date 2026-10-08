//! The context half of a batch: decide, generate, write ahead, read back.
//!
//! This runs before anything is embedded, because it decides what text is embedded. Its
//! contract with the rest of the pipeline is one sentence: **when it returns, every unit's
//! `chunk_context` is what the database holds**, and the vector is built from that and
//! from nothing else. The order inside is the write-ahead:
//!
//! 1. read what the database holds about each unit (one statement, every unit named with
//!    its organization; a unit that is not found is dropped from the batch);
//! 2. read the documents of the units that might need a context, once each;
//! 3. decide per unit (`plan::decide`): keep, generate, record, or leave;
//! 4. generate what is to be generated, inside the inline time box
//!    (`generate::run`), with bounded concurrency across documents;
//! 5. write, in this order and before any embedding: the contexts that were generated
//!    (each with its provenance and `ready`, in one statement, compare-and-swap on the
//!    input hash), then the status of every unit that did not get one (with the content-free
//!    class that says why and when it is due again);
//! 6. read the contexts back, and put them on the items.
//!
//! Step 6 is what makes the contract hold without bookkeeping: whether a write won, lost
//! a race to another worker, or was refused by the database, the vector follows what is
//! stored.
//!
//! Failing the batch because of a context is never the answer: a provider that is down, a
//! tenant that cannot be resolved, a database that refuses one unit's write each leave
//! that unit raw, with a status that says why, and the batch carries on. What does fail
//! the batch is the database being unreachable, because nothing after this could be
//! recorded either.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use anyhow::Context as _;
use sqlx::PgConnection;
use tokio::time::Instant;

use crate::context::{
    ChunkInput, ChunkResult, ContextOutcome, DocumentInput, DocumentMeta, ErrorClass, MeteredCall,
    PreparedDocument, PromptVersion,
};
use crate::policy::OrgSetting;

use super::plan::{decide, deferred, Current, Decision, Reading};
use super::schedule::next_due;
use super::store::{self, ReadyWrite, StatusWrite, Stored, UnitRef};
use super::{generate, usage, BatchItem, PassKind, Pipeline};

/// What the context half did, for the one line the batch logs about it.
#[derive(Debug, Default)]
pub(super) struct ContextReport {
    pub dropped: usize,
    pub kept: usize,
    pub generated: usize,
    pub lost_race: usize,
    pub left: usize,
    pub by_status: BTreeMap<&'static str, usize>,
    /// Every provider call that completed, by document: what the usage ledger is built from
    /// (CR-T10), including a call whose answer was then rejected, because the provider billed
    /// for it all the same.
    pub metered: Vec<MeteredDocument>,
    /// How many of the usage events built from `metered` the ledger did not acknowledge (they
    /// are logged loudly when it happens). They are published inside [`ensure`], as soon as the
    /// calls are made: see there.
    pub usage_lost: usize,
    /// Units the budget gate (CR-T9b) withheld a context from because the organization's budget
    /// is used up and its mode is `stop`: marked `skipped_budget`, or already so.
    pub budget_stopped: usize,
    /// Units a `throttle` deferred: they wait for the reconciler, which works the organization
    /// one chunk at a time.
    pub budget_deferred: usize,
}

/// The provider calls made for one document in this batch.
#[derive(Debug)]
pub(super) struct MeteredDocument {
    pub org_id: String,
    pub document_id: String,
    pub calls: Vec<MeteredCall>,
}

impl ContextReport {
    pub fn provider_calls(&self) -> u64 {
        self.metered
            .iter()
            .map(|document| document.calls.len() as u64)
            .sum()
    }

    /// `(input, output, cache read)` tokens of the calls that were billed (a call answered
    /// from inference-core's response cache cost nothing).
    pub fn billed_tokens(&self) -> (u64, u64, u64) {
        let mut totals = (0u64, 0u64, 0u64);
        for call in self.metered.iter().flat_map(|document| &document.calls) {
            if call.usage.is_billable() {
                totals.0 += u64::from(call.usage.input_tokens);
                totals.1 += u64::from(call.usage.output_tokens);
                totals.2 += u64::from(call.usage.cache_read_input_tokens);
            }
        }
        totals
    }
}

fn status_write<'a>(
    unit: UnitRef<'a>,
    stored: &'a Stored,
    outcome: &ContextOutcome,
) -> StatusWrite<'a> {
    StatusWrite {
        unit,
        expected_hash: stored.input_hash.as_deref(),
        status: outcome.status().as_str(),
        error_class: outcome.error_class().map(ErrorClass::as_str),
        count_attempt: outcome.counts_as_attempt(),
        due_in: next_due(outcome, stored.attempts),
    }
}

/// A unit that carries a context and no status: recorded as `legacy`, the same
/// classification the migration's backfill gives (provenance unknown).
fn legacy_write<'a>(unit: UnitRef<'a>, stored: &'a Stored) -> StatusWrite<'a> {
    StatusWrite {
        unit,
        expected_hash: stored.input_hash.as_deref(),
        status: "legacy",
        error_class: None,
        count_attempt: false,
        due_in: None,
    }
}

/// Whether contexts may be generated for the organization: its operator policy is not off and
/// could be determined. (Every organization of a batch is in `settings` whenever generation is
/// enabled at all.)
fn generates(settings: &HashMap<String, OrgSetting>, org_id: &str) -> bool {
    matches!(settings.get(org_id), Some(OrgSetting::Generate { .. }))
}

/// The organization's prompt-version pin (the canary of a prompt rollout), if it has one.
fn pin_of(settings: &HashMap<String, OrgSetting>, org_id: &str) -> Option<PromptVersion> {
    match settings.get(org_id) {
        Some(OrgSetting::Generate { pin, .. }) => *pin,
        _ => None,
    }
}

/// The items of the batch with every `chunk_context` set to what the database holds, and a
/// report. Units that do not exist (deleted while their event waited, or claimed under the
/// wrong organization) are not in the result: a vector for them would be an orphan.
pub(super) async fn ensure(
    pipeline: &Pipeline<'_>,
    items: Vec<BatchItem>,
) -> anyhow::Result<(Vec<BatchItem>, ContextReport)> {
    let mut report = ContextReport::default();

    // The same unit twice in one buffer (a redelivery next to the original) is one unit:
    // generating it twice is paying twice.
    let mut seen = HashSet::new();
    let mut items: Vec<BatchItem> = items
        .into_iter()
        .filter(|item| seen.insert(item.knowledge_id.clone()))
        .collect();

    let engine = pipeline.contextualizer;

    // What each organization's operator policy says (CR-T11). The global switch comes first:
    // with generation off (`engine` is `None`) the policy is not consulted at all. Read before
    // a connection is taken, so that the policy read never waits for a pool that this
    // function is holding a connection of. A policy that is off (or cannot be determined)
    // means no document of that organization is read for generation and no job is built, so
    // the provider is not called and no bearer is asked for.
    let settings: HashMap<String, OrgSetting> = match engine {
        Some(_) => {
            let orgs: Vec<&str> = items.iter().map(|item| item.org_id.as_str()).collect();
            pipeline.policy.settings(&orgs).await
        }
        None => HashMap::new(),
    };

    let mut conn = pipeline
        .pool
        .acquire()
        .await
        .context("acquire a connection to read the units")?;
    let refs: Vec<UnitRef<'_>> = items
        .iter()
        .map(|item| UnitRef {
            knowledge_id: &item.knowledge_id,
            org_id: &item.org_id,
        })
        .collect();
    let stored: HashMap<String, Stored> = store::load(&mut conn, &refs)
        .await
        .context("read the units' stored contexts")?
        .into_iter()
        .map(|unit| (unit.knowledge_id.clone(), unit))
        .collect();
    // An event that did not carry its document (an empty id) is told the document the
    // database has for the unit: the database is the authority on what a unit belongs to,
    // and the vector's payload names it.
    for item in &mut items {
        if item.document_id.is_empty() {
            if let Some(unit) = stored.get(&item.knowledge_id) {
                item.document_id = unit.document_id.clone();
            }
        }
    }
    // A unit is the one the caller named only if it exists under that organization AND
    // that document: an item that claims a unit under another document of the same
    // organization would be given the wrong document's text to be situated in.
    let (mut items, missing): (Vec<BatchItem>, Vec<BatchItem>) =
        items.into_iter().partition(|item| {
            stored
                .get(&item.knowledge_id)
                .is_some_and(|unit| unit.document_id == item.document_id)
        });
    report.dropped = missing.len();
    if !missing.is_empty() {
        tracing::warn!(
            units = missing.len(),
            "units that no longer exist (or belong to another organization or document) are not embedded"
        );
    }
    if items.is_empty() {
        return Ok((items, report));
    }

    // The documents of the units that might need a context: one copy of each. An organization
    // whose policy is off (or unknown) has none read: nothing of it is windowed or sent.
    let rows = match engine {
        Some(_) => {
            let pairs: Vec<(&str, &str)> = items
                .iter()
                .filter(|item| {
                    !item.zdr && !item.document_id.is_empty() && generates(&settings, &item.org_id)
                })
                .map(|item| (item.document_id.as_str(), item.org_id.as_str()))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            store::documents(&mut conn, &pairs)
                .await
                .context("read the documents of the units")?
        }
        None => Vec::new(),
    };
    // Not held across the generation, which can take most of a minute.
    drop(conn);

    let dates: Vec<String> = rows
        .iter()
        .map(|row| {
            row.date
                .map(|date| date.format("%Y-%m-%d").to_string())
                .unwrap_or_default()
        })
        .collect();
    let mut prepared: HashMap<(&str, &str), PreparedDocument<'_>> = HashMap::new();
    if let Some(engine) = engine {
        for (row, date) in rows.iter().zip(&dates) {
            // A restricted document is never windowed or sent, and a document with no
            // text has nothing to situate a chunk in.
            if row.restricted || row.content.trim().is_empty() {
                continue;
            }
            prepared.insert(
                (row.document_id.as_str(), row.org_id.as_str()),
                engine.prepare(DocumentInput {
                    org_id: &row.org_id,
                    document_id: &row.document_id,
                    meta: DocumentMeta {
                        title: &row.title,
                        source: &row.source,
                        date,
                    },
                    text: &row.content,
                    restricted: false,
                    prompt_version: pin_of(&settings, &row.org_id),
                    total_chunks: usize::try_from(row.total_chunks)
                        .ok()
                        .filter(|total| *total > 0),
                }),
            );
        }
    }

    // Restricted by the database now, whatever flag the unit was queued with.
    let restricted_documents: HashSet<(&str, &str)> = rows
        .iter()
        .filter(|row| row.restricted)
        .map(|row| (row.document_id.as_str(), row.org_id.as_str()))
        .collect();

    // Decide, per unit.
    let mut decisions: Vec<Decision> = Vec::with_capacity(items.len());
    for item in &items {
        let unit = &stored[&item.knowledge_id];
        let key = (item.document_id.as_str(), item.org_id.as_str());
        let document = prepared.get(&key);
        let chunk = ChunkInput {
            index: item.chunk_index,
            text: &item.text,
        };
        let hash = match (engine, document) {
            (Some(engine), Some(document)) => Some(engine.input_hash(document, &chunk)),
            _ => None,
        };
        let reading = match (engine, document, hash.as_deref()) {
            (None, _, _) => Reading::Off,
            (Some(engine), Some(document), Some(hash)) => Reading::Current(Current {
                version: document.version(),
                model: engine.model_name(),
                hash,
                refresh: pipeline.pass == PassKind::RefreshPrompt,
            }),
            (Some(_), _, _) => Reading::DocumentMissing,
        };
        // The organization's operator policy comes before what the document says: an organization
        // that is off is never generated for, and one that cannot be determined is not either.
        let reading = match settings.get(item.org_id.as_str()) {
            Some(OrgSetting::Off) => Reading::PolicyOff,
            Some(OrgSetting::Unknown) => Reading::PolicyUnknown,
            _ => reading,
        };
        let restricted = item.zdr || restricted_documents.contains(&key);
        decisions.push(decide(restricted, unit, &reading));
    }

    // The budget gate (CR-T9b): for every organization that is about to be asked for a context,
    // what its background budget says. A stop turns its generations into marks, a throttle lets
    // one chunk through and defers the rest, and everything else (no figure, under the figure,
    // alert only, a budget that cannot be measured) changes nothing. Consulted only here, where a
    // provider call is about to be made, and only while generation is on at all: with the incident
    // kill switch thrown (`engine` is `None`) nothing is generated, so there is no spend to hold
    // and no decision to take.
    apply_budget(
        pipeline,
        &items,
        &stored,
        &settings,
        &|item| {
            item.zdr
                || restricted_documents.contains(&(item.document_id.as_str(), item.org_id.as_str()))
        },
        &mut decisions,
        &mut report,
    )
    .await;

    // What is to be generated, a document at a time.
    let mut jobs: BTreeMap<(&str, &str), Vec<(usize, ChunkInput<'_>)>> = BTreeMap::new();
    for (position, (item, decision)) in items.iter().zip(&decisions).enumerate() {
        if *decision == Decision::Generate {
            jobs.entry((item.document_id.as_str(), item.org_id.as_str()))
                .or_default()
                .push((
                    position,
                    ChunkInput {
                        index: item.chunk_index,
                        text: &item.text,
                    },
                ));
        }
    }

    // Generate, inside the time box.
    let mut results: HashMap<usize, ChunkResult> = HashMap::new();
    if let (Some(engine), false) = (engine, jobs.is_empty()) {
        let clock = engine.inline_budget().start(Instant::now());
        let jobs: Vec<generate::Job<'_>> = jobs
            .into_iter()
            .filter_map(|(key, chunks)| {
                prepared
                    .get(&key)
                    .map(|prepared| generate::Job { prepared, chunks })
            })
            .collect();
        results = generate::run(engine, jobs, clock)
            .await
            .into_iter()
            .collect();
    }

    // Turn every decision into at most one write.
    let mut ready: Vec<ReadyWrite<'_>> = Vec::new();
    let mut statuses: Vec<StatusWrite<'_>> = Vec::new();
    let mut metered: BTreeMap<(String, String), Vec<MeteredCall>> = BTreeMap::new();
    let model = engine.map(|engine| engine.model_name()).unwrap_or_default();
    for (position, (item, decision)) in items.iter().zip(&decisions).enumerate() {
        let unit = &stored[&item.knowledge_id];
        let reference = UnitRef {
            knowledge_id: &item.knowledge_id,
            org_id: &item.org_id,
        };
        match decision {
            Decision::Keep { classify_legacy } => {
                report.kept += 1;
                if *classify_legacy {
                    statuses.push(legacy_write(reference, unit));
                }
            }
            Decision::Leave => report.left += 1,
            Decision::Record(outcome) => {
                *report
                    .by_status
                    .entry(outcome.status().as_str())
                    .or_default() += 1;
                statuses.push(status_write(reference, unit, outcome));
            }
            Decision::Generate => {
                let Some(result) = results.get(&position) else {
                    // The contextualizer is infallible per chunk; a missing result is a
                    // defect, and the unit is left raw with a visible reason rather than
                    // silently.
                    let outcome = ContextOutcome::permanent(ErrorClass::Internal);
                    statuses.push(status_write(reference, unit, &outcome));
                    continue;
                };
                if !result.calls.is_empty() {
                    metered
                        .entry((item.org_id.clone(), item.document_id.clone()))
                        .or_default()
                        .extend(result.calls.iter().cloned());
                }
                *report
                    .by_status
                    .entry(result.outcome.status().as_str())
                    .or_default() += 1;
                match result.outcome.generated() {
                    Some(generated) => ready.push(ReadyWrite {
                        unit: reference,
                        expected_hash: unit.input_hash.as_deref(),
                        context: generated.text.trim(),
                        prompt_version: generated.prompt_version.as_str(),
                        model,
                        provider: &generated.provider_used,
                        residency: &generated.residency,
                        input_hash: &generated.input_hash,
                    }),
                    None => statuses.push(status_write(reference, unit, &result.outcome)),
                }
            }
        }
    }

    report.metered = metered
        .into_iter()
        .map(|((org_id, document_id), calls)| MeteredDocument {
            org_id,
            document_id,
            calls,
        })
        .collect();

    // What those calls cost is added to the organization's figure at once (CR-T9b), so that the
    // next batch, which may start before the usage ledger has the events, knows the budget it is
    // spending. A call answered from inference-core's response cache cost nothing.
    for document in &report.metered {
        let (mut input, mut output) = (0u64, 0u64);
        for call in document
            .calls
            .iter()
            .filter(|call| call.usage.is_billable())
        {
            input += u64::from(call.usage.input_tokens);
            output += u64::from(call.usage.output_tokens);
        }
        pipeline
            .budget
            .record_spend(&document.org_id, input, output);
    }

    // The calls above are paid for, so they are recorded here, before anything below can fail
    // the batch. A database that is gone by the write-ahead fails the batch, which is
    // redelivered, generates again and pays again; recorded after this function returned, the
    // first payment would be recorded nowhere. A redelivery that finds the contexts stored calls
    // nothing, so each call is recorded once, when it was paid for. Never fails the batch.
    if let Some(ledger) = pipeline.usage {
        report.usage_lost += usage::publish(ledger, &usage::context_events(&report.metered)).await;
    }

    // Write ahead.
    let mut conn = pipeline
        .pool
        .acquire()
        .await
        .context("acquire a connection to write the contexts")?;
    let written = write_ready_isolated(&mut conn, &ready, &stored, &mut statuses).await?;
    report.generated = written.len();
    report.lost_race = ready.len() - written.len();
    write_status_isolated(&mut conn, &statuses).await?;

    // What is in force now, whoever put it there.
    let refs: Vec<UnitRef<'_>> = items
        .iter()
        .map(|item| UnitRef {
            knowledge_id: &item.knowledge_id,
            org_id: &item.org_id,
        })
        .collect();
    let in_force: HashMap<String, Option<String>> = store::load(&mut conn, &refs)
        .await
        .context("read back the contexts in force")?
        .into_iter()
        .map(|unit| (unit.knowledge_id, unit.context))
        .collect();
    drop(conn);
    drop(statuses);
    drop(ready);
    drop(prepared);
    for item in &mut items {
        item.chunk_context = in_force.get(&item.knowledge_id).cloned().flatten();
        // The classification the database has now is the one the vector is embedded under
        // as well: a document that became restricted while its units waited goes through
        // the zero-retention route of the embedding client, not the one it was queued for.
        let key = (item.document_id.as_str(), item.org_id.as_str());
        if restricted_documents.contains(&key) {
            item.zdr = true;
        }
    }
    Ok((items, report))
}

/// What the budget gate does to the decisions of a batch (CR-T9b).
///
/// Only the organizations with a unit that is about to be generated are asked about (one gate
/// reading each, kept for 30 seconds), so a batch that generates nothing costs the gate nothing
/// and raises no alert for a budget that did not matter to it. Then, per verdict:
///
/// * `Stop`: each of the organization's generations is decided again as `Reading::BudgetStopped`,
///   which marks the unit `skipped_budget` (or leaves a mark already there, or keeps a context
///   the unit has). No provider call, no bearer.
/// * `Throttle`: the first [`Verdict::allowance`] generations (in the batch's order) go on and
///   every other is deferred ([`deferred`]): nothing is lost, nothing is skipped, and the
///   reconciler works the organization at the same pace.
/// * Anything else (`Proceed`, `Alert`, `Unmeasured`): untouched. The default mode never stops,
///   whatever the spend.
async fn apply_budget(
    pipeline: &Pipeline<'_>,
    items: &[BatchItem],
    stored: &HashMap<String, Stored>,
    settings: &HashMap<String, OrgSetting>,
    restricted: &dyn Fn(&BatchItem) -> bool,
    decisions: &mut [Decision],
    report: &mut ContextReport,
) {
    let orgs: BTreeSet<&str> = items
        .iter()
        .zip(decisions.iter())
        .filter(|(_, decision)| **decision == Decision::Generate)
        .map(|(item, _)| item.org_id.as_str())
        .collect();
    for org in orgs {
        let Some(OrgSetting::Generate { budget, .. }) = settings.get(org) else {
            continue;
        };
        let verdict = pipeline.budget.check(org, budget).await;
        let mut allowed = verdict.allowance();
        for (position, item) in items.iter().enumerate() {
            if item.org_id != org || decisions[position] != Decision::Generate {
                continue;
            }
            let unit = &stored[&item.knowledge_id];
            if verdict.withholds() {
                decisions[position] = decide(restricted(item), unit, &Reading::BudgetStopped);
                report.budget_stopped += 1;
            } else if let Some(left) = allowed.as_mut() {
                if *left > 0 {
                    *left -= 1;
                } else {
                    decisions[position] = deferred(unit);
                    report.budget_deferred += 1;
                }
            }
        }
    }
}

/// Write the generated contexts in one statement; if the database refuses it (a
/// constraint, a trigger, a deadlock), write each unit on its own so that one unit's
/// refusal cannot fail the others, and give the refused unit a retryable status. A
/// failure that is not a refusal (the connection is gone) is the batch's.
async fn write_ready_isolated<'a>(
    conn: &mut PgConnection,
    writes: &[ReadyWrite<'a>],
    stored: &'a HashMap<String, Stored>,
    statuses: &mut Vec<StatusWrite<'a>>,
) -> anyhow::Result<HashSet<String>> {
    match store::write_ready(conn, writes).await {
        Ok(written) => Ok(written),
        Err(sqlx::Error::Database(refused)) => {
            tracing::warn!(
                code = ?refused.code(),
                units = writes.len(),
                "the database refused the contexts; writing each unit on its own"
            );
            let mut written = HashSet::new();
            for write in writes {
                match store::write_ready(conn, std::slice::from_ref(write)).await {
                    Ok(done) => written.extend(done),
                    Err(sqlx::Error::Database(refused)) => {
                        tracing::warn!(
                            knowledge_id = write.unit.knowledge_id,
                            code = ?refused.code(),
                            "the database refused one unit's context; it is embedded from what it has"
                        );
                        let outcome = ContextOutcome::transient(ErrorClass::Internal, None);
                        if let Some(unit) = stored.get(write.unit.knowledge_id) {
                            statuses.push(status_write(write.unit, unit, &outcome));
                        }
                    }
                    Err(other) => return Err(other).context("write a generated context"),
                }
            }
            Ok(written)
        }
        Err(other) => Err(other).context("write the generated contexts"),
    }
}

/// The same for statuses: a refusal of one unit's status is logged, never fatal.
async fn write_status_isolated(
    conn: &mut PgConnection,
    writes: &[StatusWrite<'_>],
) -> anyhow::Result<()> {
    match store::write_status(conn, writes).await {
        Ok(_) => Ok(()),
        Err(sqlx::Error::Database(refused)) => {
            tracing::warn!(
                code = ?refused.code(),
                units = writes.len(),
                "the database refused the statuses; writing each unit on its own"
            );
            for write in writes {
                match store::write_status(conn, std::slice::from_ref(write)).await {
                    Ok(_) => {}
                    Err(sqlx::Error::Database(refused)) => tracing::warn!(
                        knowledge_id = write.unit.knowledge_id,
                        code = ?refused.code(),
                        "the database refused one unit's status; it is embedded from what it has"
                    ),
                    Err(other) => return Err(other).context("write a context status"),
                }
            }
            Ok(())
        }
        Err(other) => Err(other).context("write the context statuses"),
    }
}
