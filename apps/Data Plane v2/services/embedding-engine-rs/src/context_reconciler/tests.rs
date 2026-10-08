//! The reconciler against the rules it really runs under.
//!
//! Every test here that touches the database is gated on `MIGRATED_TEST_DATABASE_URL` and FAILS,
//! rather than skips, when it is not set (`tests/scripts/run-postgres-contract-tests.sh` provides
//! it): a contract that does not run proves nothing. They run on the same rig as the batch
//! pipeline's tests (`batch::testing`): the real migrated schema as the superuser every Data
//! Plane service connects as, the real token client reading what Auth Core's real mint produced,
//! the real operator policy table, the real contextualizer over a model that answers per tenant,
//! and recording fakes at the only three places that cannot be asked what they were sent (the
//! embedding model, the vector store, the message bus). What is the reconciler's own, the
//! selection, the lease, the pacing and the decisions about what to announce, is the real code.
//!
//! The database is shared with the pipeline's tests, which leave work behind, so every pass here
//! is restricted to the organizations the test made ([`Scope`]); the daemon's own scope (every
//! organization) is the default and is what the lease tests exercise.

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;

use super::pacing::{ManualClock, PassHealth};
use super::select::{self, Kind, LeaseRequest};
use super::*;
use crate::batch::testing::{Doc, Posture, Rig, UnitRow, COLLECTION, EMBEDDING_MODEL, TITLE};
use crate::config::Config;
use crate::context::prompt::{compose_contextualized, stored_context_hash};
use crate::provider::contextualize::testing::{context_for, Behaviour, ModelPort};
use crate::provider::contextualize::Contextualizer;

const CHUNKS: [&str; 3] = [
    "Driftsmarginen økte til 31 prosent.",
    "Røyking av laks i Tromsø ble utvidet til to nye anlegg.",
    "Fjordfisk AS åpner et nytt anlegg i Hammerfest i 2026.",
];
const ANNOUNCED: &str = "dataplane.documents.indexed";
const WITHIN: Duration = Duration::from_secs(60);

// ── the fixture ─────────────────────────────────────────────────────────────

fn defaults() -> Settings {
    let cfg: Config = envy::from_iter(vec![
        (
            "DATABASE_URL".to_owned(),
            "postgres://test.invalid/t".to_owned(),
        ),
        ("NATS_URL".to_owned(), "nats://test.invalid:4222".to_owned()),
        (
            "QDRANT_URL".to_owned(),
            "http://test.invalid:6334".to_owned(),
        ),
    ])
    .expect("a minimal configuration");
    let mut settings = Settings::from_config(&cfg).expect("the defaults are valid");
    // The tests do not depend on a file in /tmp of the machine they run on.
    settings.pause_file = None;
    settings
}

struct Fixture {
    reconciler: Reconciler,
    /// What the reconciler reads: moved by [`Fixture::advance`]. The token client keeps the
    /// frozen clock of the golden it replays (a captured token cannot be minted again at a later
    /// time), so a bearer, once minted, is reused for as long as the test runs; what a test can
    /// and does assert about time is the reconciler's own schedule.
    clock: Arc<ManualClock>,
}

impl Fixture {
    fn new(rig: &Rig, tweak: impl FnOnce(&mut Settings)) -> Self {
        Self::build(rig, true, |_| {}, tweak)
    }

    /// A deployment that generates with this prompt version.
    fn on_prompt(
        rig: &Rig,
        version: crate::context::PromptVersion,
        tweak: impl FnOnce(&mut Settings),
    ) -> Self {
        Self::build(rig, true, |engine| engine.prompt_version = version, tweak)
    }

    /// The reconciler while the incident kill switch is on: no model at all.
    fn without_model(rig: &Rig) -> Self {
        Self::build(rig, false, |_| {}, |_| {})
    }

    fn build(
        rig: &Rig,
        model: bool,
        engine_tweak: impl FnOnce(&mut crate::provider::contextualize::Settings),
        tweak: impl FnOnce(&mut Settings),
    ) -> Self {
        let mut settings = defaults();
        tweak(&mut settings);
        let clock = Arc::new(ManualClock::new());
        let mut engine_settings = crate::batch::testing::settings();
        engine_tweak(&mut engine_settings);
        let engine = Contextualizer::new(
            Arc::new(ModelPort(rig.model.clone())),
            rig.tenants.client(),
            engine_settings,
        );
        let reconciler = Reconciler::new(
            Deps {
                pool: rig.pool.clone(),
                provider: rig.embedder.clone(),
                vectors: rig.vectors.clone(),
                events: rig.events.clone(),
                signer: None,
                collection: COLLECTION.to_owned(),
                engine: model.then_some(engine),
                policy: Arc::new(PolicyReader::with_ttl(
                    rig.policy_source.clone(),
                    Duration::ZERO,
                )),
                budget: rig.budget.clone(),
                usage: None,
            },
            settings,
            clock.clone(),
        );
        Self { reconciler, clock }
    }

    /// Let time pass for the reconciler.
    fn advance(&self, by: Duration) {
        self.clock.advance(by);
    }

    /// One pass over one organization, looking at everything now.
    async fn pass(&self, org: &str) -> PassReport {
        tokio::time::timeout(WITHIN, self.reconciler.pass_scoped(&Scope::only(org)))
            .await
            .expect("the pass was not finished in time")
    }

    /// One pass as the daemon runs it (the sweep's schedule), over the organizations named.
    async fn daemon_pass(&self, orgs: &[&str]) -> PassReport {
        let scope = Scope {
            orgs: Some(orgs.iter().map(|org| (*org).to_owned()).collect()),
            ..Scope::default()
        };
        tokio::time::timeout(WITHIN, self.reconciler.pass_scoped(&scope))
            .await
            .expect("the pass was not finished in time")
    }
}

// ── seeding: the states a unit can be in ────────────────────────────────────

/// A document whose units are embedded already, raw: the state of the live corpus's one raw
/// chunk, and of every unit whose inline phase ran out of time.
async fn embedded_doc(rig: &Rig, org: &str, chunks: &[&str]) -> Doc {
    let doc = rig.plain_document(org, chunks).await;
    mark_embedded(rig, &doc).await;
    doc
}

/// Embedded as the pipeline marks a unit: `embedded_at` and the trigger's `updated_at` are one
/// instant, so that the stale-embedding detector sees no change.
async fn mark_embedded(rig: &Rig, doc: &Doc) {
    sqlx::query(
        "UPDATE knowledge_units SET embedding_status = 'done', embedded_at = now(), \
                embedding_model = $3 WHERE document_id = $1 AND org_id = $2",
    )
    .bind(&doc.id)
    .bind(&doc.org)
    .bind(EMBEDDING_MODEL)
    .execute(&rig.pool)
    .await
    .expect("mark the document's units embedded");
}

#[derive(Clone, Default)]
struct Seed {
    context: Option<&'static str>,
    status: Option<&'static str>,
    class: Option<&'static str>,
    attempts: i32,
    /// `embedded_context_hash`.
    vector: Option<String>,
    /// Seconds from now until the unit is due; negative is past.
    due_in: Option<i64>,
    /// Seconds from now until the lease ends; negative is past.
    lease_in: Option<i64>,
    prompt_version: Option<&'static str>,
}

async fn seed(rig: &Rig, knowledge_id: &str, seed: Seed) {
    sqlx::query(
        "UPDATE knowledge_units SET chunk_context = $2, context_status = $3, \
                context_error_class = $4, context_attempts = $5, embedded_context_hash = $6, \
                context_next_attempt_at = CASE WHEN $7::float8 IS NULL THEN NULL \
                    ELSE now() + make_interval(secs => $7::float8) END, \
                context_lease_until = CASE WHEN $8::float8 IS NULL THEN NULL \
                    ELSE now() + make_interval(secs => $8::float8) END, \
                context_prompt_version = $9 \
          WHERE knowledge_id = $1",
    )
    .bind(knowledge_id)
    .bind(seed.context)
    .bind(seed.status)
    .bind(seed.class)
    .bind(seed.attempts)
    .bind(seed.vector)
    .bind(seed.due_in.map(|secs| secs as f64))
    .bind(seed.lease_in.map(|secs| secs as f64))
    .bind(seed.prompt_version)
    .execute(&rig.pool)
    .await
    .expect("seed a unit");
}

fn pending() -> Seed {
    Seed {
        status: Some("pending"),
        ..Seed::default()
    }
}

fn legacy(context: &'static str) -> Seed {
    Seed {
        context: Some(context),
        status: Some("legacy"),
        prompt_version: Some("ctx-v1"),
        ..Seed::default()
    }
}

fn ready_with(context: &'static str, vector: Option<String>) -> Seed {
    Seed {
        context: Some(context),
        status: Some("ready"),
        vector,
        prompt_version: Some("ctx-v1"),
        ..Seed::default()
    }
}

/// Make time pass for the unit's schedule (its due time and its lease), as a test cannot wait.
async fn let_time_pass(rig: &Rig, doc: &Doc) {
    sqlx::query(
        "UPDATE knowledge_units SET context_next_attempt_at = now() - interval '1 second', \
                context_lease_until = NULL WHERE document_id = $1 AND org_id = $2 \
                AND context_next_attempt_at IS NOT NULL",
    )
    .bind(&doc.id)
    .bind(&doc.org)
    .execute(&rig.pool)
    .await
    .expect("let time pass");
    sqlx::query(
        "UPDATE knowledge_units SET context_lease_until = NULL \
          WHERE document_id = $1 AND org_id = $2",
    )
    .bind(&doc.id)
    .bind(&doc.org)
    .execute(&rig.pool)
    .await
    .expect("end the leases");
}

fn composed(text: &str) -> String {
    compose_contextualized(&context_for(text), text)
}

fn announcements(rig: &Rig) -> usize {
    rig.events.on(ANNOUNCED).len()
}

/// The stale-embedding detector's own test: a unit that is done and was written after it was
/// embedded is reported as changed after the embed.
fn assert_quiet(rows: &[UnitRow]) {
    for row in rows {
        assert_eq!(
            row.embedded_at,
            Some(row.updated_at),
            "{}: written after it was embedded: the stale-embedding detector would report it",
            row.knowledge_id
        );
    }
}

fn ids(leased: &[select::Leased]) -> std::collections::BTreeSet<String> {
    leased
        .iter()
        .map(|unit| unit.knowledge_id.clone())
        .collect()
}

fn lease_request<'a>(org: &'a str, kind: Kind, limit: i64) -> LeaseRequest<'a> {
    LeaseRequest {
        org_id: org,
        kind,
        limit,
        lease_secs: 300.0,
        excluded: &[],
        max_attempts: 8,
        include_skipped_policy: false,
        include_skipped_budget: false,
        refresh_target: "ctx-v2",
    }
}

// ── the selector ────────────────────────────────────────────────────────────

/// What needs work is decided by the database's own columns, and by nothing else: for every
/// status, classification, embedding state, schedule, lease and organization, the rows that are
/// leased are exactly the right ones.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn the_selector_returns_exactly_the_rows_that_need_work() {
    let rig = Rig::new().await;
    let org = rig.org("sel");
    let other = rig.org("oth");
    let right = stored_context_hash("Kontekst A.");
    let wrong = stored_context_hash("Kontekst B.");

    struct Case {
        name: &'static str,
        classification: &'static str,
        embedded: bool,
        deleted: bool,
        org: &'static str, // "me" or "other"
        seed: Seed,
        /// The kind that leases it, if any.
        kind: Option<Kind>,
    }
    let me = "me";
    let cases = vec![
        // ── no usable context: Generate ──
        Case {
            name: "pending",
            classification: "internal",
            embedded: true,
            deleted: false,
            org: me,
            seed: pending(),
            kind: Some(Kind::Generate),
        },
        Case {
            name: "no status",
            classification: "internal",
            embedded: true,
            deleted: false,
            org: me,
            seed: Seed::default(),
            kind: Some(Kind::Generate),
        },
        Case {
            name: "blank context",
            classification: "internal",
            embedded: true,
            deleted: false,
            org: me,
            seed: Seed {
                context: Some("   "),
                status: Some("pending"),
                ..Seed::default()
            },
            kind: Some(Kind::Generate),
        },
        Case {
            name: "failed, due",
            classification: "internal",
            embedded: true,
            deleted: false,
            org: me,
            seed: Seed {
                status: Some("failed_transient"),
                class: Some("timeout"),
                attempts: 2,
                due_in: Some(-5),
                ..Seed::default()
            },
            kind: Some(Kind::Generate),
        },
        Case {
            name: "failed, not due",
            classification: "internal",
            embedded: true,
            deleted: false,
            org: me,
            seed: Seed {
                status: Some("failed_transient"),
                class: Some("timeout"),
                attempts: 2,
                due_in: Some(3_600),
                ..Seed::default()
            },
            kind: None,
        },
        Case {
            name: "failed, every attempt used",
            classification: "internal",
            embedded: true,
            deleted: false,
            org: me,
            seed: Seed {
                status: Some("failed_transient"),
                class: Some("timeout"),
                attempts: 8,
                due_in: Some(-5),
                ..Seed::default()
            },
            kind: None,
        },
        Case {
            name: "failed for good",
            classification: "internal",
            embedded: true,
            deleted: false,
            org: me,
            seed: Seed {
                status: Some("failed_permanent"),
                class: Some("content_filter"),
                attempts: 1,
                ..Seed::default()
            },
            kind: None,
        },
        // A unit the budget gate holds has no schedule of its own: it is work only when the gate
        // lets the organization through (`include_skipped_budget`, which this request does not
        // set), whatever time the row carries. The test below turns the gate off.
        Case {
            name: "held by the budget, time past",
            classification: "internal",
            embedded: true,
            deleted: false,
            org: me,
            seed: Seed {
                status: Some("skipped_budget"),
                due_in: Some(-5),
                ..Seed::default()
            },
            kind: None,
        },
        Case {
            name: "held by the budget, no time",
            classification: "internal",
            embedded: true,
            deleted: false,
            org: me,
            seed: Seed {
                status: Some("skipped_budget"),
                ..Seed::default()
            },
            kind: None,
        },
        Case {
            name: "zero retention (organization)",
            classification: "internal",
            embedded: true,
            deleted: false,
            org: me,
            seed: Seed {
                status: Some("skipped_zdr"),
                class: Some("zdr_org"),
                ..Seed::default()
            },
            kind: None,
        },
        Case {
            name: "zero retention (document)",
            classification: "internal",
            embedded: true,
            deleted: false,
            org: me,
            seed: Seed {
                status: Some("skipped_zdr"),
                class: Some("zdr_document"),
                ..Seed::default()
            },
            kind: None,
        },
        // ── what the unit's document and embedding say ──
        Case {
            name: "restricted document",
            classification: "restricted",
            embedded: true,
            deleted: false,
            org: me,
            seed: pending(),
            kind: None,
        },
        Case {
            name: "public document",
            classification: "public",
            embedded: true,
            deleted: false,
            org: me,
            seed: pending(),
            kind: Some(Kind::Generate),
        },
        Case {
            name: "sensitive document",
            classification: "sensitive",
            embedded: true,
            deleted: false,
            org: me,
            seed: pending(),
            kind: Some(Kind::Generate),
        },
        Case {
            name: "not embedded yet",
            classification: "internal",
            embedded: false,
            deleted: false,
            org: me,
            seed: pending(),
            kind: None,
        },
        Case {
            name: "deleted document",
            classification: "internal",
            embedded: true,
            deleted: true,
            org: me,
            seed: pending(),
            kind: None,
        },
        Case {
            name: "another organization",
            classification: "internal",
            embedded: true,
            deleted: false,
            org: "other",
            seed: pending(),
            kind: None,
        },
        // ── leases ──
        Case {
            name: "leased by a live worker",
            classification: "internal",
            embedded: true,
            deleted: false,
            org: me,
            seed: Seed {
                status: Some("pending"),
                lease_in: Some(600),
                ..Seed::default()
            },
            kind: None,
        },
        Case {
            name: "lease ended",
            classification: "internal",
            embedded: true,
            deleted: false,
            org: me,
            seed: Seed {
                status: Some("pending"),
                lease_in: Some(-5),
                ..Seed::default()
            },
            kind: Some(Kind::Generate),
        },
        // ── a context of unknown provenance: Verify ──
        Case {
            name: "legacy",
            classification: "internal",
            embedded: true,
            deleted: false,
            org: me,
            seed: legacy("Kontekst A."),
            kind: Some(Kind::Verify),
        },
        Case {
            name: "legacy, vector already matches",
            classification: "internal",
            embedded: true,
            deleted: false,
            org: me,
            seed: Seed {
                vector: Some(right.clone()),
                ..legacy("Kontekst A.")
            },
            kind: Some(Kind::Verify),
        },
        Case {
            name: "a context and no status",
            classification: "internal",
            embedded: true,
            deleted: false,
            org: me,
            seed: Seed {
                context: Some("Kontekst A."),
                ..Seed::default()
            },
            kind: Some(Kind::Verify),
        },
        Case {
            name: "legacy, restricted",
            classification: "restricted",
            embedded: true,
            deleted: false,
            org: me,
            seed: legacy("Kontekst A."),
            kind: None,
        },
        // ── a vector that is not the one the stored context makes: Repair ──
        Case {
            name: "ready, vector never recorded",
            classification: "internal",
            embedded: true,
            deleted: false,
            org: me,
            seed: ready_with("Kontekst A.", None),
            kind: Some(Kind::Repair),
        },
        Case {
            name: "ready, vector of another context",
            classification: "internal",
            embedded: true,
            deleted: false,
            org: me,
            seed: ready_with("Kontekst A.", Some(wrong.clone())),
            kind: Some(Kind::Repair),
        },
        Case {
            name: "ready and coherent",
            classification: "internal",
            embedded: true,
            deleted: false,
            org: me,
            seed: ready_with("Kontekst A.", Some(right.clone())),
            kind: None,
        },
        Case {
            name: "ready, restricted",
            classification: "restricted",
            embedded: true,
            deleted: false,
            org: me,
            seed: ready_with("Kontekst A.", None),
            kind: None,
        },
    ];

    let mut expected: std::collections::BTreeMap<Kind, std::collections::BTreeSet<String>> =
        Default::default();
    let mut everything = Vec::new();
    for case in &cases {
        let owner = if case.org == me { &org } else { &other };
        let doc = rig
            .document(owner, case.classification, &["en tekst"])
            .await;
        let kid = doc.units[0].knowledge_id.clone();
        if case.embedded {
            mark_embedded(&rig, &doc).await;
        }
        seed(&rig, &kid, case.seed.clone()).await;
        if case.deleted {
            sqlx::query("UPDATE documents SET deleted_at = now() WHERE document_id = $1")
                .bind(&doc.id)
                .execute(&rig.pool)
                .await
                .expect("delete the document");
        }
        if let Some(kind) = case.kind {
            expected.entry(kind).or_default().insert(kid.clone());
        }
        everything.push((case.name, kid));
    }

    for kind in [Kind::Generate, Kind::Verify, Kind::Repair] {
        let leased = select::lease(&rig.pool, &lease_request(&org, kind, 100))
            .await
            .expect("lease");
        let found = ids(&leased);
        let want = expected.get(&kind).cloned().unwrap_or_default();
        let name = |kid: &String| {
            everything
                .iter()
                .find(|(_, id)| id == kid)
                .map_or("?", |(name, _)| *name)
        };
        assert_eq!(
            found,
            want,
            "{}: got {:?}, wanted {:?}",
            kind.as_str(),
            found.iter().map(name).collect::<Vec<_>>(),
            want.iter().map(name).collect::<Vec<_>>()
        );
        assert!(leased
            .iter()
            .all(|unit| unit.org_id == org && unit.kind == kind));
    }

    // Everything leased is leased now, and a second request finds nothing: a lease is not given
    // twice.
    for kind in [Kind::Generate, Kind::Verify, Kind::Repair] {
        let again = select::lease(&rig.pool, &lease_request(&org, kind, 100))
            .await
            .expect("lease again");
        assert!(again.is_empty(), "{}: leased twice", kind.as_str());
    }
    let (live,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM knowledge_units WHERE org_id = $1 AND context_lease_until > now()",
    )
    .bind(&org)
    .fetch_one(&rig.pool)
    .await
    .expect("count leases");
    let leased_total: usize = expected.values().map(|set| set.len()).sum();
    assert_eq!(
        live as usize,
        leased_total + 1,
        "the unit held by a live worker is still held"
    );
    let (foreign,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM knowledge_units WHERE org_id = $1 AND context_lease_until IS NOT NULL",
    )
    .bind(&other)
    .fetch_one(&rig.pool)
    .await
    .expect("count the other organization's leases");
    assert_eq!(foreign, 0, "another organization's unit was leased");
}

/// A `skipped_policy` unit is work only while the organization's policy is not off (the policy
/// is read now, not when the unit was marked), and the stale and refresh kinds are decided by
/// other means than a predicate.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_unit_skipped_by_policy_is_work_only_when_the_policy_is_not_off() {
    let rig = Rig::new().await;
    let org = rig.org("skp");
    let doc = embedded_doc(&rig, &org, &["en tekst"]).await;
    let kid = &doc.units[0].knowledge_id;
    seed(
        &rig,
        kid,
        Seed {
            status: Some("skipped_policy"),
            ..Seed::default()
        },
    )
    .await;

    let mut request = lease_request(&org, Kind::Generate, 10);
    assert!(select::lease(&rig.pool, &request)
        .await
        .expect("lease")
        .is_empty());
    request.include_skipped_policy = true;
    assert_eq!(
        select::lease(&rig.pool, &request)
            .await
            .expect("lease")
            .len(),
        1
    );

    // Stale needs no lease request of its own, and a zero limit leases nothing.
    assert!(
        select::lease(&rig.pool, &lease_request(&org, Kind::Stale, 10))
            .await
            .expect("lease")
            .is_empty()
    );
    assert!(
        select::lease(&rig.pool, &lease_request(&org, Kind::Generate, 0))
            .await
            .expect("lease")
            .is_empty()
    );
}

/// A `skipped_budget` unit is work only while the organization's background budget does not hold
/// it (CR-T9b): the gate is read now, not when the unit was marked, so a unit the gate held is
/// generated again the moment it lets the organization through, whatever its row says about time.
/// While the gate holds the organization, the units that have no status yet are still work (the
/// pipeline marks them, which is how a stop is never silent) and the ones already marked are not.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_unit_held_by_the_budget_is_work_only_when_the_budget_does_not_hold_it() {
    let rig = Rig::new().await;
    let org = rig.org("skb");
    let held = embedded_doc(&rig, &org, &["held"]).await;
    let fresh = embedded_doc(&rig, &org, &["fresh"]).await;
    let (held_id, fresh_id) = (&held.units[0].knowledge_id, &fresh.units[0].knowledge_id);
    seed(
        &rig,
        held_id,
        Seed {
            status: Some("skipped_budget"),
            ..Seed::default()
        },
    )
    .await;
    seed(&rig, fresh_id, pending()).await;

    // The gate holds the organization: the marked unit stays where it is, the new one is leased
    // so that it can be marked.
    let request = lease_request(&org, Kind::Generate, 10);
    assert!(!request.include_skipped_budget);
    let leased = select::lease(&rig.pool, &request).await.expect("lease");
    assert_eq!(ids(&leased), std::iter::once(fresh_id.clone()).collect());

    // The gate lets it through: the marked unit is work again, with no time to wait for.
    let mut open = lease_request(&org, Kind::Generate, 10);
    open.include_skipped_budget = true;
    let leased = select::lease(&rig.pool, &open).await.expect("lease");
    assert_eq!(
        ids(&leased),
        std::iter::once(held_id.clone()).collect(),
        "the unit that was leased a moment ago is under its lease; the marked one is work"
    );
    // And it is work whatever time the row carries.
    sqlx::query(
        "UPDATE knowledge_units SET context_lease_until = NULL, \
                context_next_attempt_at = now() + interval '1 day' WHERE knowledge_id = $1",
    )
    .bind(held_id)
    .execute(&rig.pool)
    .await
    .expect("move the unit's time");
    let leased = select::lease(&rig.pool, &open).await.expect("lease");
    assert_eq!(ids(&leased), std::iter::once(held_id.clone()).collect());
}

/// A superseded prompt is work for the refresh only, and only for the contexts the organization
/// does not have at the version in force.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn the_refresh_selects_the_contexts_of_another_prompt_version_only() {
    let rig = Rig::new().await;
    let org = rig.org("rfs");
    let doc = embedded_doc(&rig, &org, &["a", "b", "c", "d"]).await;
    let right = stored_context_hash("Kontekst A.");
    let seeds = [
        ready_with("Kontekst A.", Some(right.clone())), // ctx-v1: behind
        Seed {
            prompt_version: Some("ctx-v2"),
            ..ready_with("Kontekst A.", Some(right.clone()))
        },
        legacy("Kontekst A."), // ctx-v1: behind
        Seed {
            prompt_version: None,
            ..ready_with("Kontekst A.", Some(right.clone()))
        }, // never recorded
    ];
    for (unit, seeded) in doc.units.iter().zip(seeds) {
        seed(&rig, &unit.knowledge_id, seeded).await;
    }
    let leased = select::lease(&rig.pool, &lease_request(&org, Kind::Refresh, 10))
        .await
        .expect("lease");
    let want: std::collections::BTreeSet<String> = [0, 2, 3]
        .iter()
        .map(|i| doc.units[*i].knowledge_id.clone())
        .collect();
    assert_eq!(ids(&leased), want);
}

// ── leases ──────────────────────────────────────────────────────────────────

/// Two reconcilers never process the same unit. Four workers lease ten units each from forty at
/// the same instant: every unit goes to exactly one, and no worker comes away short, which is
/// what a worker that waited on another's locks and then found its rows taken would.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn two_reconcilers_never_lease_the_same_unit() {
    let rig = Rig::new().await;
    let org = rig.org("con");
    let texts: Vec<String> = (0..40).map(|i| format!("avsnitt nummer {i:02}")).collect();
    let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
    let doc = embedded_doc(&rig, &org, &refs).await;
    for unit in &doc.units {
        seed(&rig, &unit.knowledge_id, pending()).await;
    }

    let workers: Vec<_> = (0..4)
        .map(|_| {
            let pool = rig.pool.clone();
            let org = org.clone();
            tokio::spawn(async move {
                select::lease(&pool, &lease_request(&org, Kind::Generate, 10))
                    .await
                    .expect("lease")
            })
        })
        .collect();
    let mut all = Vec::new();
    for worker in workers {
        let leased = worker.await.expect("worker");
        assert_eq!(
            leased.len(),
            10,
            "a worker was left short: it waited for another's rows"
        );
        all.extend(leased);
    }
    let distinct = ids(&all);
    assert_eq!(all.len(), 40);
    assert_eq!(distinct.len(), 40, "a unit was leased to two workers");
}

/// A worker never waits for rows another worker holds: it takes the next ones. That is what
/// `FOR UPDATE SKIP LOCKED` gives and nothing else does (a statement that waited would find the
/// rows taken once the other worker was done, and come away short or late). The test holds the
/// first four rows itself, in a transaction of its own, so that it does not depend on two workers
/// happening to overlap.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_lease_skips_the_rows_another_worker_holds_and_does_not_wait_for_them() {
    let rig = Rig::new().await;
    let org = rig.org("skp");
    let texts: Vec<String> = (0..12).map(|i| format!("avsnitt nummer {i:02}")).collect();
    let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
    let doc = embedded_doc(&rig, &org, &refs).await;
    for unit in &doc.units {
        seed(&rig, &unit.knowledge_id, pending()).await;
    }

    // Another worker is part-way through leasing: its transaction holds the first four rows in
    // the order the lease reads them.
    let held: Vec<String> = doc
        .units
        .iter()
        .take(4)
        .map(|unit| unit.knowledge_id.clone())
        .collect();
    let mut other = rig.pool.begin().await.expect("a transaction");
    sqlx::query("SELECT knowledge_id FROM knowledge_units WHERE knowledge_id = ANY($1) FOR UPDATE")
        .bind(&held)
        .fetch_all(&mut *other)
        .await
        .expect("hold the rows");

    let request = lease_request(&org, Kind::Generate, 8);
    let leased = tokio::time::timeout(Duration::from_secs(10), select::lease(&rig.pool, &request))
        .await
        .expect("the lease waited for rows another worker holds")
        .expect("lease");
    assert_eq!(leased.len(), 8, "the lease did not take the next rows");
    let taken = ids(&leased);
    assert!(
        held.iter().all(|id| !taken.contains(id)),
        "the lease took rows another worker holds"
    );

    // The other worker lets go: its rows are free, and they are the only ones left.
    other.rollback().await.expect("release the rows");
    let rest = select::lease(&rig.pool, &request).await.expect("lease");
    assert_eq!(ids(&rest), held.iter().cloned().collect());
}

/// A crashed worker's units come back when its lease runs out, and not before.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_lease_holds_the_unit_until_its_time_is_up() {
    let rig = Rig::new().await;
    let org = rig.org("lse");
    let doc = embedded_doc(&rig, &org, &["en tekst"]).await;
    seed(&rig, &doc.units[0].knowledge_id, pending()).await;

    let request = LeaseRequest {
        lease_secs: 120.0,
        ..lease_request(&org, Kind::Generate, 10)
    };
    assert_eq!(
        select::lease(&rig.pool, &request)
            .await
            .expect("lease")
            .len(),
        1
    );
    assert!(select::lease(&rig.pool, &request)
        .await
        .expect("lease")
        .is_empty());
    let (secs,): (f64,) = sqlx::query_as(
        "SELECT extract(epoch FROM (context_lease_until - now()))::float8 \
           FROM knowledge_units WHERE knowledge_id = $1",
    )
    .bind(&doc.units[0].knowledge_id)
    .fetch_one(&rig.pool)
    .await
    .expect("lease time");
    assert!(
        (100.0..=121.0).contains(&secs),
        "the lease is {secs} seconds"
    );

    // The worker dies; its lease runs out.
    sqlx::query("UPDATE knowledge_units SET context_lease_until = now() - interval '1 second' WHERE knowledge_id = $1")
        .bind(&doc.units[0].knowledge_id)
        .execute(&rig.pool)
        .await
        .expect("end the lease");
    assert_eq!(
        select::lease(&rig.pool, &request)
            .await
            .expect("lease")
            .len(),
        1
    );
}

/// A unit whose cooldown has not ended is not asked for.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_unit_that_is_cooling_is_not_leased() {
    let rig = Rig::new().await;
    let org = rig.org("cool");
    let doc = embedded_doc(&rig, &org, &["a", "b"]).await;
    for unit in &doc.units {
        seed(&rig, &unit.knowledge_id, pending()).await;
    }
    let cooling = [doc.units[0].knowledge_id.clone()];
    let request = LeaseRequest {
        excluded: &cooling,
        ..lease_request(&org, Kind::Generate, 10)
    };
    let leased = select::lease(&rig.pool, &request).await.expect("lease");
    assert_eq!(ids(&leased), [doc.units[1].knowledge_id.clone()].into());
}

// ── the pipeline, run by the reconciler ─────────────────────────────────────

/// The live case: one raw chunk with `pending` and a vector built without a context. One pass
/// writes its context, builds the vector from it, announces the document once and leaves the unit
/// as the pipeline leaves any unit. A second pass makes no provider call of any kind.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_raw_unit_is_healed_and_a_second_run_costs_nothing() {
    let rig = Rig::new().await;
    let org = rig.org("raw");
    let doc = embedded_doc(&rig, &org, &CHUNKS[..1]).await;
    seed(&rig, &doc.units[0].knowledge_id, pending()).await;
    let fx = Fixture::new(&rig, |_| {});

    let report = fx.pass(&org).await;

    assert_eq!(report.leased.get("generate"), Some(&1));
    assert_eq!(
        (report.generated, report.announced, report.withheld),
        (1, 1, 0)
    );
    assert_eq!(report.health, Some(PassHealth::Healthy));
    let row = rig.row(&doc.units[0].knowledge_id).await;
    assert_eq!(row.context_status.as_deref(), Some("ready"));
    assert_eq!(
        row.chunk_context.as_deref(),
        Some(context_for(CHUNKS[0]).as_str())
    );
    assert_eq!(
        row.embedded_context_hash.as_deref(),
        Some(stored_context_hash(&context_for(CHUNKS[0])).as_str()),
        "the vector says which context it was built from"
    );
    assert_eq!(
        rig.vectors
            .stored(&doc.units[0].knowledge_id)
            .unwrap()
            .vector,
        crate::batch::testing::vector_for(&composed(CHUNKS[0]))
    );
    assert_eq!(rig.model.call_count(), 1);
    assert_eq!(rig.embedder.texts(), [composed(CHUNKS[0])]);
    assert_eq!(announcements(&rig), 1);
    assert_eq!(
        rig.events.on(ANNOUNCED)[0].payload["document_id"],
        doc.id.as_str()
    );
    assert_quiet(&[row]);

    // A second full run: nothing is selected, so nothing is asked of anyone.
    let again = fx.pass(&org).await;
    assert_eq!(again.leased_total(), 0);
    assert_eq!(again.health, Some(PassHealth::Idle));
    assert_eq!(rig.model.call_count(), 1, "a second run asked the model");
    assert_eq!(
        rig.embedder.calls().len(),
        1,
        "a second run asked the embedding model"
    );
    assert_eq!(rig.vectors.upsert_count(), 1);
    assert_eq!(announcements(&rig), 1);
}

/// The legacy verification pass: no model is asked (none is given to the pipeline), the unit is
/// `ready` because its vector is now PROVEN to be built from its stored context, the model that
/// embedded it is recorded for the first time, the provenance the context never had is not made
/// up, and nothing is announced (the keyword index holds the same context as before, and an
/// announcement would make graph extraction run on every document of the organization).
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn legacy_units_are_verified_without_a_model_and_without_an_announcement() {
    let rig = Rig::new().await;
    let org = rig.org("leg");
    let first = embedded_doc(&rig, &org, &CHUNKS[..2]).await;
    let second = embedded_doc(&rig, &org, &CHUNKS[2..]).await;
    let old = [
        "Gammel kontekst 1.",
        "Gammel kontekst 2.",
        "Gammel kontekst 3.",
    ];
    let units: Vec<_> = first.units.iter().chain(&second.units).collect();
    for (index, unit) in units.iter().enumerate() {
        let seeded = if index < 2 {
            legacy(old[index])
        } else {
            // A context a pre-provenance writer left with no status.
            Seed {
                context: Some(old[index]),
                prompt_version: Some("ctx-v1"),
                ..Seed::default()
            }
        };
        seed(&rig, &unit.knowledge_id, seeded).await;
    }
    // The live corpus: the embedding model was never recorded.
    sqlx::query("UPDATE knowledge_units SET embedding_model = NULL WHERE org_id = $1")
        .bind(&org)
        .execute(&rig.pool)
        .await
        .expect("forget the model");
    let fx = Fixture::new(&rig, |_| {});

    let report = fx.pass(&org).await;

    assert_eq!(report.leased.get("verify"), Some(&3));
    assert_eq!((report.verified, report.generated), (3, 0));
    assert_eq!(
        report.health,
        Some(PassHealth::Idle),
        "no provider was asked"
    );
    assert_eq!(rig.model.call_count(), 0, "the verification asked a model");
    assert_eq!(
        announcements(&rig),
        0,
        "a verification changes nothing the keyword index holds"
    );
    assert_eq!(
        report.withheld, 2,
        "one per document, held back and dropped"
    );
    let mut texts = rig.embedder.texts();
    texts.sort();
    let mut want: Vec<String> = units
        .iter()
        .zip(old)
        .map(|(unit, context)| compose_contextualized(context, &unit.text))
        .collect();
    want.sort();
    assert_eq!(texts, want, "the stored context is what was embedded");
    let rows = [rig.rows(&first).await, rig.rows(&second).await].concat();
    for (row, context) in rows.iter().zip(old) {
        assert_eq!(row.context_status.as_deref(), Some("ready"));
        assert_eq!(row.chunk_context.as_deref(), Some(context));
        assert_eq!(
            row.embedded_context_hash.as_deref(),
            Some(stored_context_hash(context).as_str())
        );
        assert_eq!(row.embedding_model.as_deref(), Some(EMBEDDING_MODEL));
        assert_eq!(row.context_input_hash, None, "provenance is not invented");
        assert_eq!(row.context_model, None);
    }
    assert_quiet(&rows);

    let again = fx.pass(&org).await;
    assert_eq!(again.leased_total(), 0);
    assert_eq!(rig.embedder.calls().len(), 2, "a second run embedded again");
}

/// An interrupted run resumes without generating twice. The embedding fails after the contexts
/// were written ahead: the contexts are in the database, the vectors are not built from them. The
/// next run (after the lease and the cooldown) only embeds, makes no model call, and, because the
/// batch that wrote the contexts died before it could announce them, announces the document.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn an_interrupted_run_resumes_without_repeating_generation() {
    let rig = Rig::new().await;
    let org = rig.org("int");
    let doc = embedded_doc(&rig, &org, &CHUNKS).await;
    for unit in &doc.units {
        seed(&rig, &unit.knowledge_id, pending()).await;
    }
    let fx = Fixture::new(&rig, |_| {});
    rig.embedder.fail_next(1);

    let first = fx.pass(&org).await;

    assert_eq!(first.errors, 1, "the batch failed");
    assert_eq!(first.generated, 3, "the contexts were written ahead");
    assert_eq!(rig.model.call_count(), 3);
    for row in rig.rows(&doc).await {
        assert_eq!(row.context_status.as_deref(), Some("ready"));
        assert_eq!(
            row.embedded_context_hash, None,
            "no vector was built from it yet"
        );
    }
    assert!(rig.vectors.points().is_empty());
    assert_eq!(
        announcements(&rig),
        0,
        "nothing was announced by the batch that died"
    );

    // The same worker's units are still leased: nobody takes them over early.
    let early = fx.pass(&org).await;
    assert_eq!(early.leased_total(), 0, "the lease holds");

    // Time passes: the lease ends and the cooldown with it.
    let_time_pass(&rig, &doc).await;
    fx.advance(Duration::from_secs(301));
    let second = fx.pass(&org).await;

    assert_eq!(second.leased.get("repair"), Some(&3));
    assert_eq!((second.repaired, second.generated), (3, 0));
    assert_eq!(rig.model.call_count(), 3, "the generation was repeated");
    assert_eq!(
        second.announced, 1,
        "the document is complete now: it is announced"
    );
    let rows = rig.rows(&doc).await;
    for (unit, row) in doc.units.iter().zip(&rows) {
        assert_eq!(
            row.embedded_context_hash.as_deref(),
            Some(stored_context_hash(&context_for(&unit.text)).as_str())
        );
        assert_eq!(
            rig.vectors.stored(&unit.knowledge_id).unwrap().vector,
            crate::batch::testing::vector_for(&composed(&unit.text))
        );
    }
    assert_quiet(&rows);
}

/// A vector that cannot be built is asked for again on the retry ladder (a minute, five, thirty,
/// two hours, half a day), not every time its lease runs out: the re-embeds have no schedule in the
/// database (a write there would make the stale-embedding detector report the unit), so the
/// reconciler keeps it, and a unit that is cooling is not leased even when nothing else holds it.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_vector_that_cannot_be_built_is_asked_for_again_on_the_ladder_and_not_at_every_lease() {
    use crate::batch::ladder_step;

    let rig = Rig::new().await;
    let org = rig.org("cdn");
    let doc = embedded_doc(&rig, &org, &CHUNKS[..1]).await;
    seed(
        &rig,
        &doc.units[0].knowledge_id,
        ready_with("Gammel kontekst.", None),
    )
    .await;
    rig.embedder.fail_next(100);
    let fx = Fixture::new(&rig, |settings| settings.breaker_failures = 20);

    for attempt in 1..=4u32 {
        let asked = rig.embedder.calls().len();
        let report = fx.pass(&org).await;
        assert_eq!(report.leased.get("repair"), Some(&1), "attempt {attempt}");
        assert_eq!(rig.embedder.calls().len(), asked + 1);
        assert_eq!(report.errors, 1, "the embedding failed");

        // The lease runs out, but the ladder has not been climbed: not asked again.
        let_time_pass(&rig, &doc).await;
        fx.advance(ladder_step(attempt) - Duration::from_secs(1));
        assert_eq!(
            fx.pass(&org).await.leased_total(),
            0,
            "attempt {attempt}: asked again before its step of the ladder"
        );
        assert_eq!(rig.embedder.calls().len(), asked + 1);
        // The step is over: it is asked again.
        fx.advance(Duration::from_secs(2));
    }
    // And it was never given up on by the database: the unit is as it was, only unresolved.
    let row = rig.row(&doc.units[0].knowledge_id).await;
    assert_eq!(row.context_status.as_deref(), Some("ready"));
    assert_eq!(row.embedded_context_hash, None);
    assert_eq!(
        row.context_attempts, 0,
        "a re-embed's failures are not the context's attempts"
    );
}

/// What the units' schedule does under a provider that is down: one provider call per attempt,
/// each failure due later by the ladder (a minute, five, thirty, two hours, half a day), and a
/// unit that has failed eight times is given up on, with an alert, and costs nothing afterwards.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_provider_that_stays_down_walks_the_ladder_and_the_unit_is_given_up_after_eight_attempts()
{
    let rig = Rig::new().await;
    let org = rig.org("lad");
    let doc = embedded_doc(&rig, &org, &CHUNKS[..1]).await;
    seed(&rig, &doc.units[0].knowledge_id, pending()).await;
    rig.model
        .set_default(Behaviour::Fail("provider_unavailable"));
    // The breaker would stop the walk after three passes: it has its own test.
    let fx = Fixture::new(&rig, |settings| settings.breaker_failures = 20);

    let ladder = [
        60.0, 300.0, 1_800.0, 7_200.0, 43_200.0, 43_200.0, 43_200.0, 43_200.0,
    ];
    for (index, wait) in ladder.iter().enumerate() {
        let report = fx.pass(&org).await;
        assert_eq!(report.leased_total(), 1, "attempt {}", index + 1);
        assert_eq!(report.health, Some(PassHealth::ProviderFailed));
        let row = rig.row(&doc.units[0].knowledge_id).await;
        assert_eq!(row.context_status.as_deref(), Some("failed_transient"));
        assert_eq!(
            row.context_error_class.as_deref(),
            Some("provider_unavailable")
        );
        assert_eq!(row.context_attempts, i32::try_from(index + 1).unwrap());
        let due = (row.context_next_attempt_at.unwrap() - chrono::Utc::now()).num_seconds() as f64;
        assert!(
            (wait - 5.0..=*wait).contains(&due),
            "attempt {}: due in {due} s, the ladder says {wait}",
            index + 1
        );
        assert_eq!(rig.model.call_count(), index + 1, "one call per attempt");
        // Not due yet: the next pass leaves it alone. (After the eighth failure it is not left
        // alone: it is given up on, in the first pass that looks.)
        let early = fx.pass(&org).await;
        assert_eq!(early.leased_total(), 0, "asked again before it was due");
        if index + 1 < ladder.len() {
            assert!(
                early.given_up.is_empty(),
                "given up on after {} attempts",
                index + 1
            );
            let_time_pass(&rig, &doc).await;
        } else {
            // Every attempt is used: the unit is given up on, with an alert, keeping the class it
            // failed with.
            assert_eq!(
                early.given_up,
                [(org.clone(), "provider_unavailable".to_owned(), 1)]
            );
        }
    }
    let calls = rig.model.call_count();
    let report = fx.pass(&org).await;
    assert!(report.given_up.is_empty(), "given up on twice");
    assert_eq!(
        report.leased_total(),
        0,
        "a unit that is given up on is not asked again"
    );
    let row = rig.row(&doc.units[0].knowledge_id).await;
    assert_eq!(row.context_status.as_deref(), Some("failed_permanent"));
    assert_eq!(
        row.context_error_class.as_deref(),
        Some("provider_unavailable")
    );
    assert_eq!(rig.model.call_count(), calls);
    let_time_pass(&rig, &doc).await;
    assert_eq!(fx.pass(&org).await.leased_total(), 0, "and never will be");
}

/// A posture problem is not the unit's fault: Auth Core could not say whether the organization is
/// zero-retention, so nothing is sent and nothing is skipped, the unit waits, and no attempt is
/// used however often it waits.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_posture_that_cannot_be_resolved_never_burns_an_attempt() {
    let rig = Rig::new().await;
    let org = rig.org("pos");
    let doc = embedded_doc(&rig, &org, &CHUNKS[..1]).await;
    seed(&rig, &doc.units[0].knowledge_id, pending()).await;
    rig.tenants.set(&org, Posture::Unresolved);
    let fx = Fixture::new(&rig, |_| {});

    for round in 0..4 {
        let report = fx.pass(&org).await;
        assert_eq!(report.leased_total(), 1, "round {round}");
        assert_eq!(
            report.health,
            Some(PassHealth::Idle),
            "an unresolved posture is not a provider failure"
        );
        let row = rig.row(&doc.units[0].knowledge_id).await;
        assert_eq!(row.context_status.as_deref(), Some("pending"));
        assert_eq!(
            row.context_error_class.as_deref(),
            Some("posture_unavailable")
        );
        assert_eq!(
            row.context_attempts, 0,
            "round {round}: a posture burned an attempt"
        );
        assert_eq!(
            rig.model.call_count(),
            0,
            "content went to a model without a posture"
        );
        let_time_pass(&rig, &doc).await;
    }
    // Auth Core answers again: the unit is generated, with its attempts untouched.
    rig.tenants.set(&org, Posture::Plain);
    let report = fx.pass(&org).await;
    assert_eq!(report.generated, 1);
    assert_eq!(
        rig.row(&doc.units[0].knowledge_id)
            .await
            .context_status
            .as_deref(),
        Some("ready")
    );
}

/// A refused credential is not a blip: it opens the breaker at once, the reconciler is quiet for
/// the pause and then lets one pass through, and the units' attempts are not touched.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_refused_credential_opens_the_breaker_and_the_pause_is_honoured() {
    let rig = Rig::new().await;
    let org = rig.org("crd");
    let doc = embedded_doc(&rig, &org, &CHUNKS[..2]).await;
    for unit in &doc.units {
        seed(&rig, &unit.knowledge_id, pending()).await;
    }
    rig.tenants.set(&org, Posture::Refused);
    let fx = Fixture::new(&rig, |_| {});

    let first = fx.pass(&org).await;
    assert_eq!(first.health, Some(PassHealth::CredentialRefused));
    for row in rig.rows(&doc).await {
        assert_eq!(
            row.context_attempts, 0,
            "a credential problem burned an attempt"
        );
        assert_eq!(row.context_error_class.as_deref(), Some("auth"));
    }
    assert_eq!(rig.model.call_count(), 0);

    let second = fx.pass(&org).await;
    assert!(
        matches!(second.paused, Some(Paused::Breaker(left)) if left > Duration::from_secs(290)),
        "{second:?}"
    );
    assert_eq!(second.leased_total(), 0);

    // The pause ends: one pass is let through, and the credential works now.
    fx.advance(Duration::from_secs(301));
    rig.tenants.set(&org, Posture::Plain);
    let_time_pass(&rig, &doc).await;
    let third = fx.pass(&org).await;
    assert_eq!(third.paused, None);
    assert_eq!(third.generated, 2);
    assert_eq!(third.health, Some(PassHealth::Healthy));
}

/// A refusal that arrives on the embedding call (and not on the model's) is read the same way:
/// the verification pass has no model, so this is the only way its credential can be refused, and
/// it opens the breaker at once. A provider that is merely down on the same call is one failed
/// pass, which counts toward the threshold and opens nothing yet.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_failure_on_the_embedding_call_is_read_as_the_health_of_the_provider() {
    let rig = Rig::new().await;
    let (refused_org, down_org) = (rig.org("ref"), rig.org("dwn"));
    let refused = embedded_doc(&rig, &refused_org, &CHUNKS[..1]).await;
    let down = embedded_doc(&rig, &down_org, &CHUNKS[..1]).await;
    seed(
        &rig,
        &refused.units[0].knowledge_id,
        legacy("Gammel kontekst."),
    )
    .await;
    seed(
        &rig,
        &down.units[0].knowledge_id,
        legacy("Gammel kontekst."),
    )
    .await;

    rig.embedder.refuse_next(
        100,
        tonic::Status::unauthenticated("the service token was refused"),
    );
    let fx = Fixture::new(&rig, |_| {});
    let first = fx.pass(&refused_org).await;
    assert_eq!(
        first.health,
        Some(PassHealth::CredentialRefused),
        "{first:?}"
    );
    assert_eq!(rig.model.call_count(), 0, "the verification asked a model");
    assert_eq!(
        rig.row(&refused.units[0].knowledge_id)
            .await
            .context_status
            .as_deref(),
        Some("legacy"),
        "a unit whose vector was not built was promoted"
    );
    let second = fx.pass(&refused_org).await;
    assert!(
        matches!(second.paused, Some(Paused::Breaker(left)) if left > Duration::from_secs(290)),
        "the breaker did not open on a refused credential: {second:?}"
    );

    // A fresh service (a restart forgets the breaker), and a provider that is down.
    let fx = Fixture::new(&rig, |_| {});
    rig.embedder.refuse_next(
        100,
        tonic::Status::unavailable("the embedding provider is down"),
    );
    let first = fx.pass(&down_org).await;
    assert_eq!(first.health, Some(PassHealth::ProviderFailed), "{first:?}");
    assert_eq!(first.paused, None);
    let second = fx.pass(&down_org).await;
    assert_eq!(
        second.paused, None,
        "one failed pass opened the breaker: {second:?}"
    );
}

/// Three passes in a row in which the provider failed everything open the breaker for five
/// minutes, during which no unit is leased and nothing is asked; then one pass is let through.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_provider_that_fails_every_pass_opens_the_breaker_for_five_minutes() {
    let rig = Rig::new().await;
    let org = rig.org("brk");
    let doc = embedded_doc(&rig, &org, &CHUNKS[..1]).await;
    seed(&rig, &doc.units[0].knowledge_id, pending()).await;
    rig.model
        .set_default(Behaviour::Fail("provider_unavailable"));
    let fx = Fixture::new(&rig, |_| {});

    for round in 1..=3 {
        let report = fx.pass(&org).await;
        assert_eq!(
            report.health,
            Some(PassHealth::ProviderFailed),
            "round {round}"
        );
        let_time_pass(&rig, &doc).await;
    }
    let calls = rig.model.call_count();
    let paused = fx.pass(&org).await;
    assert!(
        matches!(paused.paused, Some(Paused::Breaker(_))),
        "{paused:?}"
    );
    assert_eq!(paused.leased_total(), 0);
    assert_eq!(
        rig.model.call_count(),
        calls,
        "a call was made while the breaker was open"
    );

    fx.advance(Duration::from_secs(301));
    let probe = fx.pass(&org).await;
    assert_eq!(probe.paused, None);
    assert_eq!(
        rig.model.call_count(),
        calls + 1,
        "one probe is let through"
    );
    // It failed: open again at once.
    let again = fx.pass(&org).await;
    assert!(
        matches!(again.paused, Some(Paused::Breaker(_))),
        "{again:?}"
    );
}

/// A provider that asks to be left alone is: the unit is due when the provider said (capped at
/// five minutes), the pass says so, and the loop's pause is at least that.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_provider_that_asks_to_wait_is_not_asked_again_before_the_wait() {
    let rig = Rig::new().await;
    let org = rig.org("rtl");
    let doc = embedded_doc(&rig, &org, &CHUNKS[..2]).await;
    for unit in &doc.units {
        seed(&rig, &unit.knowledge_id, pending()).await;
    }
    rig.model
        .set_default(Behaviour::Fail("provider_rate_limited"));
    let fx = Fixture::new(&rig, |_| {});

    let report = fx.pass(&org).await;

    let wait = report
        .retry_after
        .expect("the pass says how long the provider asked");
    assert!(
        wait > Duration::ZERO && wait <= pacing::MAX_WAIT,
        "{wait:?}"
    );
    for row in rig.rows(&doc).await {
        assert_eq!(row.context_error_class.as_deref(), Some("rate_limited"));
        let due = (row.context_next_attempt_at.unwrap() - chrono::Utc::now())
            .to_std()
            .unwrap();
        assert!(due <= pacing::MAX_WAIT + Duration::from_secs(1), "{due:?}");
    }
    assert!(
        pacing::next_pause(
            true,
            fx.reconciler.settings().interval,
            report.retry_after,
            0.5
        ) >= wait,
        "the loop would ask again sooner than the provider said"
    );
    // The units are not due: nothing is asked until they are.
    let calls = rig.model.call_count();
    assert_eq!(fx.pass(&org).await.leased_total(), 0);
    assert_eq!(rig.model.call_count(), calls);
}

// ── zero retention and policy ───────────────────────────────────────────────

/// Units skipped for zero retention are rechecked at most hourly, by a bearer mint: while the
/// organization is zero-retention nothing is written and no model is asked, and the organization
/// is not asked about again before the hour is up; when it stops being zero-retention its
/// units are generated. A restricted document's skip is final and is never rechecked.
///
/// (A bearer lives for a minute in production, so every check that is made is a mint. The golden
/// this test replays cannot be minted at a later time, so here a bearer, once minted, is kept:
/// what is asserted about the hour is how often the reconciler asks, and the first check shows it
/// asks by minting.)
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn skipped_zero_retention_is_rechecked_at_most_hourly_by_a_mint() {
    let rig = Rig::new().await;
    let org = rig.org("zdr");
    let doc = embedded_doc(&rig, &org, &CHUNKS[..2]).await;
    let restricted = rig.document(&org, "restricted", &CHUNKS[2..]).await;
    for unit in &doc.units {
        seed(
            &rig,
            &unit.knowledge_id,
            Seed {
                status: Some("skipped_zdr"),
                class: Some("zdr_org"),
                ..Seed::default()
            },
        )
        .await;
    }
    seed(
        &rig,
        &restricted.units[0].knowledge_id,
        Seed {
            status: Some("skipped_zdr"),
            class: Some("zdr_document"),
            ..Seed::default()
        },
    )
    .await;
    rig.tenants.set(&org, Posture::Zdr);
    let fx = Fixture::new(&rig, |_| {});
    let before = rig.tenants.requests().len();
    let rows_before = rig.rows(&doc).await;

    let first = fx.pass(&org).await;
    assert_eq!(first.zero_retention_checked, 1);
    assert_eq!(
        rig.tenants.requests().len() - before,
        1,
        "the check is a mint"
    );
    assert_eq!(first.zero_retention_released, 0);
    assert_eq!(first.leased_total(), 0);
    assert_eq!(
        rig.model.call_count(),
        0,
        "zero-retention content reached a model"
    );
    for row in rig.rows(&doc).await {
        assert_eq!(row.context_status.as_deref(), Some("skipped_zdr"));
    }
    for (before, after) in rows_before.iter().zip(rig.rows(&doc).await) {
        assert_eq!(
            before.updated_at, after.updated_at,
            "a check that finds nothing writes nothing"
        );
    }

    // Within the hour: the organization is not asked about again.
    assert_eq!(fx.pass(&org).await.zero_retention_checked, 0);
    fx.advance(Duration::from_secs(59 * 60));
    assert_eq!(
        fx.pass(&org).await.zero_retention_checked,
        0,
        "rechecked more than once an hour"
    );

    // After it: asked again, and still zero-retention.
    fx.advance(Duration::from_secs(2 * 60));
    let again = fx.pass(&org).await;
    assert_eq!(again.zero_retention_checked, 1);
    assert_eq!(again.zero_retention_released, 0);
    assert_eq!(rig.model.call_count(), 0);
    assert_eq!(
        fx.pass(&org).await.zero_retention_checked,
        0,
        "and the hour starts again"
    );

    // The organization stops being zero-retention. The next reconciler that asks (a restart: a
    // bearer does not outlive its minute) finds out, and its units are work, done in the pass.
    rig.tenants.set(&org, Posture::Plain);
    let restarted = Fixture::new(&rig, |_| {});
    let report = restarted.pass(&org).await;
    assert_eq!(report.zero_retention_released, 2);
    assert_eq!(report.generated, 2);
    assert_eq!(rig.model.call_count(), 2);
    for row in rig.rows(&doc).await {
        assert_eq!(row.context_status.as_deref(), Some("ready"));
    }
    // The restricted document's unit is as it was.
    let restricted_row = rig.row(&restricted.units[0].knowledge_id).await;
    assert_eq!(
        restricted_row.context_status.as_deref(),
        Some("skipped_zdr")
    );
    assert_eq!(
        restricted_row.context_error_class.as_deref(),
        Some("zdr_document")
    );
}

/// When Auth Core cannot say, the answer is a wait: nothing is released, nothing is sent, and the
/// organization is asked again in five minutes, not in an hour and not at once.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn an_organization_whose_posture_cannot_be_read_is_asked_again_in_five_minutes() {
    let rig = Rig::new().await;
    let org = rig.org("zun");
    let doc = embedded_doc(&rig, &org, &CHUNKS[..1]).await;
    seed(
        &rig,
        &doc.units[0].knowledge_id,
        Seed {
            status: Some("skipped_zdr"),
            class: Some("zdr_org"),
            ..Seed::default()
        },
    )
    .await;
    rig.tenants.set(&org, Posture::Unresolved);
    let fx = Fixture::new(&rig, |_| {});
    let before = rig.tenants.requests().len();

    fx.pass(&org).await;
    assert_eq!(rig.tenants.requests().len() - before, 1);
    fx.advance(Duration::from_secs(4 * 60));
    fx.pass(&org).await;
    assert_eq!(
        rig.tenants.requests().len() - before,
        1,
        "asked again too soon"
    );
    fx.advance(Duration::from_secs(2 * 60));
    fx.pass(&org).await;
    assert_eq!(rig.tenants.requests().len() - before, 2);
    assert_eq!(
        rig.row(&doc.units[0].knowledge_id)
            .await
            .context_status
            .as_deref(),
        Some("skipped_zdr"),
        "an unresolved posture released the unit"
    );
    assert_eq!(rig.model.call_count(), 0);
}

/// An organization whose operator policy is off: its units are recorded `skipped_policy`, once,
/// at the cost of embedding them raw and nothing to the model; and when the policy is lifted they
/// are generated, without anyone requeueing them.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_policy_that_is_off_is_recorded_and_costs_nothing_and_a_lifted_one_is_generated() {
    let rig = Rig::new().await;
    let org = rig.org("pol");
    let doc = embedded_doc(&rig, &org, &CHUNKS[..2]).await;
    for unit in &doc.units {
        seed(&rig, &unit.knowledge_id, pending()).await;
    }
    rig.set_policy(&org, "off", None).await;
    let fx = Fixture::new(&rig, |_| {});

    let report = fx.pass(&org).await;
    assert_eq!(report.leased.get("generate"), Some(&2));
    assert_eq!(
        rig.model.call_count(),
        0,
        "an organization that is off was sent to a model"
    );
    for row in rig.rows(&doc).await {
        assert_eq!(
            row.context_status.as_deref(),
            Some("skipped_policy"),
            "never silent"
        );
        assert_eq!(row.chunk_context, None);
    }
    assert_eq!(announcements(&rig), 0);
    // Marked once: the next pass has nothing for an organization that is still off.
    assert_eq!(fx.pass(&org).await.leased_total(), 0);

    rig.clear_policy(&org).await;
    let still = fx.pass(&org).await;
    assert_eq!(
        still.leased.get("generate"),
        Some(&2),
        "a lifted policy is read at once"
    );
    assert_eq!(still.generated, 2);
    assert_eq!(rig.model.call_count(), 2);
    for row in rig.rows(&doc).await {
        assert_eq!(row.context_status.as_deref(), Some("ready"));
    }
}

/// A policy that cannot be read is a reason to do nothing for the organization, not a reason to
/// guess: its units are left as they are, and nothing is sent.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn an_organization_whose_policy_cannot_be_read_is_left_alone() {
    let rig = Rig::new().await;
    let org = rig.org("pun");
    let doc = embedded_doc(&rig, &org, &CHUNKS[..1]).await;
    seed(&rig, &doc.units[0].knowledge_id, pending()).await;
    rig.policy_source.fail_with(Some(|| {
        crate::policy::ReadError::Transient("timeout".to_owned())
    }));
    let fx = Fixture::new(&rig, |_| {});

    let report = fx.pass(&org).await;
    rig.policy_source.fail_with(None);

    assert_eq!(report.leased.get("generate"), None);
    assert_eq!(rig.model.call_count(), 0);
    let row = rig.row(&doc.units[0].knowledge_id).await;
    assert_eq!(row.context_status.as_deref(), Some("pending"));
    assert!(
        !lease_is_set(&rig, &doc.units[0].knowledge_id).await,
        "a unit was leased"
    );
}

// ── what is never touched ───────────────────────────────────────────────────

/// A restricted document is never read, windowed, sent, re-embedded or leased, whatever state its
/// units are in.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_restricted_document_is_never_touched() {
    let rig = Rig::new().await;
    let org = rig.org("rst");
    let doc = rig.document(&org, "restricted", &CHUNKS).await;
    mark_embedded(&rig, &doc).await;
    seed(&rig, &doc.units[0].knowledge_id, pending()).await;
    seed(&rig, &doc.units[1].knowledge_id, legacy("Gammel kontekst.")).await;
    seed(
        &rig,
        &doc.units[2].knowledge_id,
        ready_with("Gammel kontekst.", None),
    )
    .await;
    let before = rig.rows(&doc).await;
    let fx = Fixture::new(&rig, |_| {});

    let report = fx.pass(&org).await;

    assert_eq!(report.leased_total(), 0);
    assert_eq!(rig.model.call_count(), 0);
    assert!(
        rig.embedder.calls().is_empty(),
        "restricted text reached an embedding model"
    );
    assert!(rig.vectors.points().is_empty());
    assert!(
        rig.tenants.requests().is_empty(),
        "a bearer was minted for restricted content"
    );
    let after = rig.rows(&doc).await;
    for (before, after) in before.iter().zip(&after) {
        assert_eq!(
            before.updated_at, after.updated_at,
            "a restricted unit was written"
        );
        assert_eq!(before.context_status, after.context_status);
    }
    assert_eq!(announcements(&rig), 0);
}

/// The incident kill switch takes the model away and nothing else: no unit that needs a context
/// is touched and no policy is read, while the re-embeds that need no model go on.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn the_incident_kill_switch_takes_the_model_away_but_not_the_repairs() {
    let rig = Rig::new().await;
    let org = rig.org("kil");
    let doc = embedded_doc(&rig, &org, &CHUNKS).await;
    seed(&rig, &doc.units[0].knowledge_id, pending()).await;
    seed(&rig, &doc.units[1].knowledge_id, legacy("Gammel kontekst.")).await;
    seed(
        &rig,
        &doc.units[2].knowledge_id,
        ready_with("Gammel kontekst.", None),
    )
    .await;
    let fx = Fixture::without_model(&rig);

    let report = fx.pass(&org).await;

    assert_eq!(report.leased.get("generate"), None);
    assert_eq!((report.verified, report.repaired), (1, 1));
    assert_eq!(rig.model.call_count(), 0);
    assert_eq!(
        rig.policy_source.reads(),
        0,
        "the policy is not read without a model"
    );
    assert_eq!(rig.tenants.requests().len(), 0);
    let rows = rig.rows(&doc).await;
    assert_eq!(
        rows[0].context_status.as_deref(),
        Some("pending"),
        "untouched"
    );
    assert_eq!(rows[1].context_status.as_deref(), Some("ready"));
    assert_eq!(rows[2].context_status.as_deref(), Some("ready"));
    assert_eq!(
        rows[2].embedded_context_hash.as_deref(),
        Some(stored_context_hash("Gammel kontekst.").as_str())
    );
}

/// The pause file is the kill switch that needs no restart: while it exists no pass leases or
/// writes anything, and the next pass after it is removed works.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn the_pause_file_stops_every_pass_until_it_is_removed() {
    let rig = Rig::new().await;
    let org = rig.org("pau");
    let doc = embedded_doc(&rig, &org, &CHUNKS[..1]).await;
    seed(&rig, &doc.units[0].knowledge_id, pending()).await;
    let path = std::env::temp_dir().join(format!("context-reconciler-{}.pause", rig.org("f")));
    let fx = Fixture::new(&rig, |settings| settings.pause_file = Some(path.clone()));

    std::fs::write(&path, b"").expect("create the pause file");
    let paused = fx.pass(&org).await;
    assert_eq!(paused.paused, Some(Paused::File));
    assert_eq!(paused.leased_total(), 0);
    assert_eq!(rig.model.call_count(), 0);
    assert_eq!(
        rig.row(&doc.units[0].knowledge_id)
            .await
            .context_status
            .as_deref(),
        Some("pending")
    );

    std::fs::remove_file(&path).expect("remove the pause file");
    let resumed = fx.pass(&org).await;
    assert_eq!(resumed.paused, None);
    assert_eq!(resumed.generated, 1);
}

// ── what is announced ───────────────────────────────────────────────────────

/// Only a document whose keyword-index content changed is announced: a context was written. A
/// pass that failed to generate, and one that only re-embedded, announce nothing (the pipeline
/// announced the document and the reconciler held it back); and what is sent is the pipeline's own
/// announcement of the document.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn only_a_document_whose_context_changed_is_announced() {
    let rig = Rig::new().await;
    let (healed_org, failing_org, verified_org) = (rig.org("an1"), rig.org("an2"), rig.org("an3"));
    let healed = embedded_doc(&rig, &healed_org, &CHUNKS[..1]).await;
    let failing = embedded_doc(&rig, &failing_org, &CHUNKS[1..2]).await;
    let verified = embedded_doc(&rig, &verified_org, &CHUNKS[2..]).await;
    seed(&rig, &healed.units[0].knowledge_id, pending()).await;
    seed(&rig, &failing.units[0].knowledge_id, pending()).await;
    seed(
        &rig,
        &verified.units[0].knowledge_id,
        legacy("Gammel kontekst."),
    )
    .await;
    rig.model
        .set_for_org(&failing_org, Behaviour::Fail("provider_unavailable"));
    let fx = Fixture::new(&rig, |_| {});

    let report = fx.pass(&healed_org).await;
    assert_eq!(
        (report.generated, report.announced, report.withheld),
        (1, 1, 0)
    );
    assert_eq!(announcements(&rig), 1);
    assert_eq!(
        rig.events.on(ANNOUNCED)[0].payload["document_id"],
        healed.id.as_str()
    );
    assert_eq!(
        rig.events.on(ANNOUNCED)[0].payload["org_id"],
        healed_org.as_str()
    );
    // The document was complete when it was announced: every unit done, with its context.
    for seen in &rig.events.on(ANNOUNCED)[0].seen {
        assert_eq!(seen.embedding_status, "done");
    }

    // The provider fails for this organization: the pipeline would have announced the document
    // (every unit is embedded, raw), and the reconciler holds it back and drops it.
    let report = fx.pass(&failing_org).await;
    assert_eq!(
        (report.generated, report.announced, report.withheld),
        (0, 0, 1)
    );
    assert_eq!(
        announcements(&rig),
        1,
        "an announcement that changes nothing is a cost"
    );

    // A verification changes nothing the keyword index holds either.
    let report = fx.pass(&verified_org).await;
    assert_eq!(
        (report.verified, report.announced, report.withheld),
        (1, 0, 1)
    );
    assert_eq!(announcements(&rig), 1);
}

// ── fairness and limits ─────────────────────────────────────────────────────

/// Organizations take turns: with a cap on the pass, one organization's big backlog does not
/// leave another's small one waiting, in either order of the rotation.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn one_organizations_backlog_does_not_starve_anothers() {
    let rig = Rig::new().await;
    let big = rig.org("big");
    let small = rig.org("sml");
    let texts: Vec<String> = (0..12)
        .map(|i| format!("avsnitt {i:02} i den store"))
        .collect();
    let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
    let big_doc = embedded_doc(&rig, &big, &refs).await;
    let small_doc = embedded_doc(&rig, &small, &["en liten", "en til"]).await;
    for unit in big_doc.units.iter().chain(&small_doc.units) {
        seed(&rig, &unit.knowledge_id, pending()).await;
    }
    // Room for six units in all, four of them for any one organization.
    let fx = Fixture::new(&rig, |settings| {
        settings.units_per_pass = 6;
        settings.org_units_per_pass = 4;
        settings.concurrency = 2;
    });
    let leased_per_org = |report: &PassReport| report.leased_total();

    let first = fx.daemon_pass(&[big.as_str(), small.as_str()]).await;
    assert_eq!(leased_per_org(&first), 6);
    let ready = |org: &str| {
        let pool = rig.pool.clone();
        let org = org.to_owned();
        async move {
            let (count,): (i64,) = sqlx::query_as(
                "SELECT count(*) FROM knowledge_units WHERE org_id = $1 AND context_status = 'ready'",
            )
            .bind(org)
            .fetch_one(&pool)
            .await
            .expect("count");
            count
        }
    };
    let (big_ready, small_ready) = (ready(&big).await, ready(&small).await);
    // Four is the most one organization gets of a pass of six, so the small one has all of its
    // two: without the per-organization cap the big one would have taken the whole pass.
    assert_eq!(
        (big_ready, small_ready),
        (4, 2),
        "the pass is capped at four for one organization and the rest went to the other"
    );
}

/// When one organization's backlog can fill every pass, the organizations still take turns at
/// being first from one pass to the next: the second is not left waiting until the first has
/// nothing left (which, for a large first ingest, is hours).
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn organizations_take_turns_at_being_first_from_one_pass_to_the_next() {
    let rig = Rig::new().await;
    let (a, b) = (rig.org("ta"), rig.org("tb"));
    let texts: Vec<String> = (0..6)
        .map(|i| format!("avsnitt {i:02} i en lang rekke"))
        .collect();
    let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
    let doc_a = embedded_doc(&rig, &a, &refs).await;
    let doc_b = embedded_doc(&rig, &b, &refs).await;
    for unit in doc_a.units.iter().chain(&doc_b.units) {
        seed(&rig, &unit.knowledge_id, pending()).await;
    }
    // Room for two units a pass, and either organization could fill all of it.
    let fx = Fixture::new(&rig, |settings| {
        settings.units_per_pass = 2;
        settings.org_units_per_pass = 2;
        settings.concurrency = 1;
    });
    for pass in 1..=2 {
        let report = fx.daemon_pass(&[a.as_str(), b.as_str()]).await;
        assert_eq!(report.leased_total(), 2, "pass {pass}");
    }
    let ready = |rows: Vec<UnitRow>| {
        rows.iter()
            .filter(|row| row.context_status.as_deref() == Some("ready"))
            .count()
    };
    assert_eq!(
        (ready(rig.rows(&doc_a).await), ready(rig.rows(&doc_b).await)),
        (2, 2),
        "one organization had both passes"
    );
}

/// Documents are worked within the limits: one at a time for an organization, two at a time in
/// all, and each document with its own chunk concurrency.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn documents_are_worked_within_the_concurrency_limits() {
    let rig = Rig::new().await;
    let (a, b) = (rig.org("ca"), rig.org("cb"));
    let mut docs = Vec::new();
    for (index, org) in [&a, &a, &a, &b, &b].into_iter().enumerate() {
        let one = format!("fyrste avsnitt {index}");
        let two = format!("andre avsnitt {index}");
        let doc = embedded_doc(&rig, org, &[one.as_str(), two.as_str()]).await;
        for unit in &doc.units {
            seed(&rig, &unit.knowledge_id, pending()).await;
        }
        docs.push(doc);
    }
    rig.model
        .set_default(Behaviour::AnswerAfter(Duration::from_millis(120)));
    // The chunks of a document are asked two at a time (the rig's own default is four, and a
    // document here has two).

    // One organization alone, one document at a time: at most the two chunks of one document.
    let fx = Fixture::new(&rig, |settings| {
        settings.units_per_pass = 20;
        settings.org_units_per_pass = 20;
        settings.concurrency = 2;
        settings.org_concurrency = 1;
    });
    let report = fx.daemon_pass(&[a.as_str()]).await;
    assert_eq!(report.generated, 6);
    assert!(
        rig.model.peak_in_flight() <= 2,
        "{} calls at once for one organization with one document at a time",
        rig.model.peak_in_flight()
    );

    // Two organizations, two documents at a time: both are worked at once.
    let report = fx.daemon_pass(&[b.as_str()]).await;
    assert_eq!(report.generated, 4);
    let fx = Fixture::new(&rig, |settings| {
        settings.units_per_pass = 20;
        settings.org_units_per_pass = 20;
        settings.concurrency = 2;
        settings.org_concurrency = 1;
    });
    for doc in &docs {
        for unit in &doc.units {
            seed(&rig, &unit.knowledge_id, pending()).await;
        }
    }
    let before = rig.model.call_count();
    let report = fx.daemon_pass(&[a.as_str(), b.as_str()]).await;
    assert_eq!(report.generated, 10);
    assert_eq!(rig.model.call_count() - before, 10);
    assert!(
        (3..=4).contains(&rig.model.peak_in_flight()),
        "two documents of two chunks were not worked at once: peak {}",
        rig.model.peak_in_flight()
    );
}

// ── the stale pass ──────────────────────────────────────────────────────────

/// A document changed after its contexts were made: found by the document's timestamp, decided by
/// each chunk's input hash. A document whose timestamp moved for a reason that is not its text
/// (the pipeline stamps `indexed` on it after every batch) costs nothing: no model, no embed, no
/// announcement. An edit regenerates exactly the chunks whose inputs changed. And it is off unless
/// it is asked for.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn an_edited_document_regenerates_only_the_chunks_whose_inputs_changed() {
    let rig = Rig::new().await;
    let org = rig.org("stl");
    let doc = rig.plain_document(&org, &CHUNKS).await;
    rig.run(rig.items(&doc))
        .await
        .expect("the pipeline's own batch");
    let calls = rig.model.call_count();
    assert_eq!(calls, 3);
    let embeds = rig.embedder.calls().len();

    // Off by default: nothing.
    let off = Fixture::new(&rig, |_| {});
    let edit = |text: &'static str| {
        let pool = rig.pool.clone();
        let id = doc.id.clone();
        async move {
            sqlx::query("UPDATE documents SET content = content || $2 WHERE document_id = $1")
                .bind(id)
                .bind(text)
                .execute(&pool)
                .await
                .expect("edit the document");
        }
    };
    edit("\n\nEt tillegg som endrer vinduet.").await;
    assert_eq!(off.pass(&doc.org).await.leased_total(), 0);

    let fx = Fixture::new(&rig, |settings| settings.stale_documents = Some(8));
    // The document changed after its contexts: every chunk's window changed, so every context.
    let report = fx.pass(&org).await;
    assert_eq!(report.leased.get("stale"), Some(&3));
    assert_eq!((report.generated, report.announced), (3, 1));
    assert_eq!(rig.model.call_count() - calls, 3);
    // They are made from the current text now.
    let prepared_hash = |text: &str| {
        let content = format!("{}\n\nEt tillegg som endrer vinduet.", CHUNKS.join("\n\n"));
        let prepared = rig.engine.prepare(crate::context::DocumentInput {
            org_id: &org,
            document_id: &doc.id,
            meta: crate::context::DocumentMeta {
                title: TITLE,
                source: "test",
                date: "",
            },
            text: &content,
            restricted: false,
            prompt_version: None,
            total_chunks: Some(3),
        });
        rig.engine
            .input_hash(&prepared, &crate::context::ChunkInput { index: 0, text })
    };
    assert_eq!(
        rig.row(&doc.units[0].knowledge_id)
            .await
            .context_input_hash
            .as_deref(),
        Some(prepared_hash(CHUNKS[0]).as_str())
    );

    // The pipeline stamped `indexed` on the document after that batch, which moves its timestamp
    // past the contexts: a candidate, and an unchanged one. Time passes (the stale walk starts
    // again), and the pass finds it costs nothing.
    fx.advance(Duration::from_secs(301));
    let calls = rig.model.call_count();
    let embeds_now = rig.embedder.calls().len();
    let quiet = fx.pass(&org).await;
    assert_eq!(
        quiet.leased_total(),
        0,
        "a document whose text did not change was regenerated"
    );
    assert_eq!(rig.model.call_count(), calls);
    assert_eq!(
        rig.embedder.calls().len(),
        embeds_now,
        "an unchanged document was embedded again"
    );
    assert!(embeds_now > embeds);
}

// ── the operator's backfill ─────────────────────────────────────────────────

/// The dry run prints counts and estimated spend per organization, and no content: not a chunk,
/// a context, a title, a document id. It changes nothing: no lease, no write.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn the_dry_run_reports_counts_and_estimated_spend_and_no_content() {
    let rig = Rig::new().await;
    let org = rig.org("dry");
    let generate = embedded_doc(&rig, &org, &CHUNKS[..2]).await;
    let legacy_doc = embedded_doc(
        &rig,
        &org,
        &["Legacy-avsnitt om torsk.", "Legacy-avsnitt om sei."],
    )
    .await;
    let ready = embedded_doc(&rig, &org, &["Klart avsnitt om kveite."]).await;
    let exhausted = embedded_doc(&rig, &org, &["Oppgitt avsnitt."]).await;
    for unit in &generate.units {
        seed(&rig, &unit.knowledge_id, pending()).await;
    }
    for unit in &legacy_doc.units {
        seed(
            &rig,
            &unit.knowledge_id,
            legacy("Hemmelig gammel kontekst."),
        )
        .await;
    }
    seed(
        &rig,
        &ready.units[0].knowledge_id,
        ready_with("Hemmelig kontekst.", None),
    )
    .await;
    seed(
        &rig,
        &exhausted.units[0].knowledge_id,
        Seed {
            status: Some("failed_transient"),
            class: Some("timeout"),
            attempts: 8,
            due_in: Some(60),
            ..Seed::default()
        },
    )
    .await;
    let fx = Fixture::new(&rig, |_| {});

    let report = fx
        .reconciler
        .dry_run(Some(&org), None, &Prices::default(), 128)
        .await
        .expect("the dry run");

    let lines: Vec<Value> = report
        .lines()
        .map(|line| serde_json::from_str(line).expect("a JSON line"))
        .collect();
    assert_eq!(lines.len(), 2, "the organization and the total: {report}");
    let units = &lines[0]["units"];
    assert_eq!(lines[0]["org_id"], org.as_str());
    assert_eq!(units["generate"], 2);
    assert_eq!(units["verify_legacy"], 2);
    assert_eq!(units["repair_vector"], 1);
    assert_eq!(units["failed_exhausted"], 1);
    assert_eq!(lines[0]["estimate"]["llm_calls"], 2);
    assert!(lines[0]["estimate"]["llm_usd"].as_f64().unwrap() > 0.0);
    assert_eq!(lines[0]["estimate"]["embed_texts"], 5);
    assert_eq!(lines[0]["estimate"]["embed_usd"], Value::Null);
    assert_eq!(lines[1]["org_id"], "*");
    // No content, in any form.
    for secret in [
        CHUNKS[0],
        CHUNKS[1],
        "Legacy-avsnitt",
        "Hemmelig",
        "kveite",
        TITLE,
        generate.id.as_str(),
        &generate.units[0].knowledge_id,
    ] {
        assert!(
            !report.contains(secret),
            "the dry run leaked `{secret}`: {report}"
        );
    }
    // It changed nothing.
    let (touched,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM knowledge_units WHERE org_id = $1 AND context_lease_until IS NOT NULL",
    )
    .bind(&org)
    .fetch_one(&rig.pool)
    .await
    .expect("count");
    assert_eq!(touched, 0, "a dry run leased units");
    assert_eq!(rig.model.call_count(), 0);
}

/// A backfill stops at the operator's caps: a unit cap is exact, a spend cap (by the estimate)
/// stops it before the next pass could pass it, and with no cap it works the organization to its
/// end and says so.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_backfill_stops_at_its_caps_and_otherwise_at_the_end_of_the_work() {
    let rig = Rig::new().await;
    let org = rig.org("bkf");
    let texts: Vec<String> = (0..6)
        .map(|i| format!("avsnitt nummer {i} om fisk"))
        .collect();
    let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
    let doc = embedded_doc(&rig, &org, &refs).await;
    for unit in &doc.units {
        seed(&rig, &unit.knowledge_id, pending()).await;
    }
    let fx = Fixture::new(&rig, |settings| settings.units_per_pass = 2);
    let request = |max_units: Option<u64>, max_usd: Option<f64>| Backfill {
        org_id: org.clone(),
        max_units,
        max_usd,
        refresh_target: None,
        prices: Prices::default(),
    };

    let capped = fx
        .reconciler
        .backfill(&request(Some(3), None), 128)
        .await
        .expect("backfill");
    assert_eq!(
        capped.leased.get("generate"),
        Some(&3),
        "the unit cap is exact"
    );
    assert_eq!(capped.stopped, "unit cap reached");
    assert_eq!(rig.model.call_count(), 3);

    // A spend cap below one unit's estimate: one unit is let through and no more.
    let broke = fx
        .reconciler
        .backfill(&request(None, Some(0.000_000_1)), 128)
        .await
        .expect("backfill");
    assert_eq!(broke.leased.get("generate"), Some(&1), "{broke:?}");
    assert_eq!(broke.stopped, "spend cap reached");
    assert_eq!(rig.model.call_count(), 4);

    let all = fx
        .reconciler
        .backfill(&request(None, None), 128)
        .await
        .expect("backfill");
    assert_eq!(all.leased.get("generate"), Some(&2));
    assert_eq!(all.stopped, "nothing left to do");
    assert_eq!(rig.model.call_count(), 6, "every unit was generated once");
    for row in rig.rows(&doc).await {
        assert_eq!(row.context_status.as_deref(), Some("ready"));
    }
}

// ── the prompt refresh ──────────────────────────────────────────────────────

/// A prompt refresh is off unless it is asked for, and capped when it is: with the flag off a
/// unit made under another prompt version is left alone; with it on, exactly the cap is
/// regenerated, no more however many passes run, and a restart grants the cap again.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_prompt_refresh_is_off_unless_asked_and_stops_at_its_cap() {
    let rig = Rig::new().await;
    let org = rig.org("rfr");
    let doc = rig.plain_document(&org, &CHUNKS).await;
    rig.run(rig.items(&doc))
        .await
        .expect("generated under ctx-v1");
    assert_eq!(rig.model.call_count(), 3);

    // The deployment now generates with ctx-v2.
    let v2 = crate::context::PromptVersion::V2;
    let version = |rows: &[UnitRow]| -> Vec<Option<String>> {
        rows.iter()
            .map(|row| row.context_prompt_version.clone())
            .collect()
    };

    // Flag off: the units are left as they are, however often the pass runs.
    let off = Fixture::on_prompt(&rig, v2, |_| {});
    off.pass(&org).await;
    off.pass(&org).await;
    assert_eq!(
        rig.model.call_count(),
        3,
        "a prompt change paid for units without being asked"
    );
    assert!(version(&rig.rows(&doc).await)
        .iter()
        .all(|v| v.as_deref() == Some("ctx-v1")));

    // Flag on, cap of two.
    let capped = Fixture::on_prompt(&rig, v2, |settings| settings.refresh_cap = Some(2));
    let first = capped.pass(&org).await;
    assert_eq!(first.leased.get("refresh"), Some(&2));
    assert_eq!(rig.model.call_count(), 5);
    let rows = rig.rows(&doc).await;
    assert_eq!(
        version(&rows),
        [
            Some("ctx-v2".to_owned()),
            Some("ctx-v2".to_owned()),
            Some("ctx-v1".to_owned())
        ],
        "exactly the cap, in order"
    );
    assert!(rows
        .iter()
        .all(|row| row.context_status.as_deref() == Some("ready")));
    assert_quiet(&rows);
    let second = capped.pass(&org).await;
    assert_eq!(second.leased_total(), 0, "the cap is spent");
    assert_eq!(rig.model.call_count(), 5);

    // A restart grants the cap again, and one unit is all that is left.
    let restarted = Fixture::on_prompt(&rig, v2, |settings| settings.refresh_cap = Some(2));
    let third = restarted.pass(&org).await;
    assert_eq!(third.leased.get("refresh"), Some(&1));
    assert!(version(&rig.rows(&doc).await)
        .iter()
        .all(|v| v.as_deref() == Some("ctx-v2")));
    assert_eq!(rig.model.call_count(), 6);
    assert_eq!(restarted.pass(&org).await.leased_total(), 0);
}

// ── tenancy of the reconciler's own statements ──────────────────────────────

/// The statements the reconciler runs for one organization work as the restricted application
/// role inside the real org-scope helper, where the database and the explicit organization in the
/// statement must agree: another organization's units are neither visible nor writable. On the
/// connection every service really uses, the superuser, row-level security is inert and the
/// organization written into each statement is the only tenant check there is: given another
/// organization's units under the wrong one, they read and write nothing.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn the_reconcilers_own_statements_name_the_organization_and_work_under_row_level_security() {
    let rig = Rig::new().await;
    let (a, b) = (rig.org("ta"), rig.org("tb"));
    let mine = embedded_doc(&rig, &a, &["min"]).await;
    let theirs = embedded_doc(&rig, &b, &["deres"]).await;
    for (doc, class) in [(&mine, "timeout"), (&theirs, "timeout")] {
        seed(
            &rig,
            &doc.units[0].knowledge_id,
            Seed {
                status: Some("failed_transient"),
                class: Some(class),
                attempts: 8,
                ..Seed::default()
            },
        )
        .await;
    }
    let skipped = embedded_doc(&rig, &a, &["hoppet over"]).await;
    let their_skipped = embedded_doc(&rig, &b, &["deres hoppet over"]).await;
    for doc in [&skipped, &their_skipped] {
        seed(
            &rig,
            &doc.units[0].knowledge_id,
            Seed {
                status: Some("skipped_zdr"),
                class: Some("zdr_org"),
                ..Seed::default()
            },
        )
        .await;
    }
    let ids_of = |docs: &[&Doc]| -> Vec<String> {
        docs.iter()
            .map(|doc| doc.units[0].knowledge_id.clone())
            .collect()
    };

    // As the application role, in the scope of organization A.
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
    let states = select::states(&mut tx, &a, &ids_of(&[&mine, &theirs]))
        .await
        .expect("states");
    assert_eq!(
        states.len(),
        1,
        "the other organization's unit is not even visible"
    );
    let given_up = select::give_up_on_exhausted(&mut tx, &a, 8)
        .await
        .expect("give up");
    assert_eq!(given_up, [("timeout".to_owned(), 1)]);
    // Asked for the other organization's name inside this scope: row-level security says no.
    let foreign = select::give_up_on_exhausted(&mut tx, &b, 8)
        .await
        .expect("give up");
    assert!(
        foreign.is_empty(),
        "the policy let another organization's unit be written"
    );
    assert_eq!(
        select::release_zero_retention_skips(&mut tx, &a)
            .await
            .expect("release"),
        1
    );
    assert_eq!(
        select::release_zero_retention_skips(&mut tx, &b)
            .await
            .expect("release"),
        0
    );
    let documents = select::stale_documents(&mut tx, &a, "", 10)
        .await
        .expect("stale");
    assert!(documents
        .iter()
        .all(|document| document.document_id != theirs.id));
    tx.commit().await.expect("commit");
    assert_eq!(
        rig.row(&mine.units[0].knowledge_id)
            .await
            .context_status
            .as_deref(),
        Some("failed_permanent")
    );
    assert_eq!(
        rig.row(&theirs.units[0].knowledge_id)
            .await
            .context_status
            .as_deref(),
        Some("failed_transient")
    );
    assert_eq!(
        rig.row(&skipped.units[0].knowledge_id)
            .await
            .context_status
            .as_deref(),
        Some("pending")
    );
    assert_eq!(
        rig.row(&their_skipped.units[0].knowledge_id)
            .await
            .context_status
            .as_deref(),
        Some("skipped_zdr")
    );

    // On the superuser connection: organization B's units under organization A's name.
    let mut conn = rig.pool.acquire().await.expect("a connection");
    let (role,): (String,) = sqlx::query_as("SELECT current_user::text")
        .fetch_one(&mut *conn)
        .await
        .expect("role");
    assert_ne!(role, "dataplane_app", "this must run where RLS is inert");
    let states = select::states(&mut conn, &a, &ids_of(&[&theirs, &their_skipped]))
        .await
        .expect("states");
    assert!(
        states.is_empty(),
        "another organization's unit was read under this one's name"
    );
    assert!(select::give_up_on_exhausted(&mut conn, &a, 8)
        .await
        .expect("give up")
        .is_empty());
    assert_eq!(
        select::release_zero_retention_skips(&mut conn, &a)
            .await
            .expect("release"),
        0
    );
    drop(conn);
    assert_eq!(
        rig.row(&theirs.units[0].knowledge_id)
            .await
            .context_status
            .as_deref(),
        Some("failed_transient")
    );
    assert_eq!(
        rig.row(&their_skipped.units[0].knowledge_id)
            .await
            .context_status
            .as_deref(),
        Some("skipped_zdr")
    );
}

/// The leasing statement asks for one organization and joins its documents under the same one:
/// it never leases a unit through another organization's document.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_unit_is_never_leased_through_another_organizations_document() {
    let rig = Rig::new().await;
    let (a, b) = (rig.org("la"), rig.org("lb"));
    let theirs = embedded_doc(&rig, &b, &["deres"]).await;
    seed(&rig, &theirs.units[0].knowledge_id, pending()).await;
    // A corrupt row: the unit says organization A, its document is organization B's.
    sqlx::query("UPDATE knowledge_units SET org_id = $2 WHERE knowledge_id = $1")
        .bind(&theirs.units[0].knowledge_id)
        .bind(&a)
        .execute(&rig.pool)
        .await
        .expect("corrupt the row");
    let leased = select::lease(&rig.pool, &lease_request(&a, Kind::Generate, 10))
        .await
        .expect("lease");
    assert!(
        leased.is_empty(),
        "a unit was leased through another organization's document"
    );
}

// ── the image ───────────────────────────────────────────────────────────────

/// A binary the crate declares and the Dockerfile does not build and copy exists on a developer's
/// machine and nowhere an operator can run it. `context-backfill` is the one that was nearly
/// that: every `[[bin]]` of the crate is built, copied out of the builder and into the runtime
/// image.
#[test]
fn every_binary_of_the_crate_reaches_the_image() {
    let cargo = include_str!("../../Cargo.toml").replace('\r', "");
    let dockerfile = include_str!("../../Dockerfile").replace('\r', "");
    let mut binaries = Vec::new();
    let mut in_bin = false;
    for line in cargo.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_bin = line == "[[bin]]";
        } else if in_bin {
            if let Some(name) = line
                .strip_prefix("name = \"")
                .and_then(|rest| rest.strip_suffix('"'))
            {
                binaries.push(name.to_owned());
            }
        }
    }
    assert!(
        binaries.iter().any(|name| name == "context-backfill"),
        "the crate declares {binaries:?}"
    );
    for name in &binaries {
        for needed in [
            format!("--bin {name}"),
            format!("cp target/release/{name} /usr/local/bin/{name}"),
            format!("COPY --from=builder /usr/local/bin/{name} /usr/local/bin/{name}"),
        ] {
            assert!(
                dockerfile.contains(&needed),
                "the Dockerfile does not carry `{needed}`: the binary never reaches the image"
            );
        }
    }
}

// ── pure pieces of the reconciler ───────────────────────────────────────────

#[test]
fn a_failed_batch_is_read_through_the_table_that_reads_a_failed_call() {
    use crate::context::testing::golden_status;
    let typed = |id: &str| {
        anyhow::Error::new(
            InferenceError::from_status(&golden_status(id)).during("model-plane embedding"),
        )
        .context("embed the units")
    };
    // The statuses are the ones inference-core's contract pins (the golden), decoded by the
    // client, and read by the table the reconciler shares with the inline path.
    let throttled = classify_failure(&typed("provider_rate_limited"));
    assert_eq!(throttled.health, PassHealth::ProviderFailed);
    assert_eq!(
        throttled.wait,
        Some(Duration::from_millis(1500)),
        "the provider's own wait"
    );
    let down = classify_failure(&typed("provider_http"));
    assert_eq!(down.health, PassHealth::ProviderFailed);
    let refused = anyhow::Error::new(
        InferenceError::from_status(&tonic::Status::unauthenticated("refused"))
            .during("model-plane embedding"),
    );
    assert_eq!(
        classify_failure(&refused).health,
        PassHealth::CredentialRefused
    );
    // An untyped failure (the database, the vector store) says nothing about the provider and
    // moves nothing.
    let untyped = classify_failure(&anyhow::anyhow!("the vector store refused the points"));
    assert_eq!(untyped.health, PassHealth::Idle);
    assert_eq!(untyped.wait, None);
    let zdr = classify_failure(&typed("embedding_zdr_unavailable"));
    assert_eq!(
        zdr.health,
        PassHealth::Idle,
        "a zero-retention refusal is not the provider being unwell"
    );
}

#[test]
fn a_pass_is_as_healthy_as_its_best_job_unless_a_credential_was_refused() {
    use PassHealth::{CredentialRefused, Healthy, Idle, ProviderFailed};
    assert_eq!(worse_of(Idle, Idle), Idle);
    assert_eq!(worse_of(Idle, ProviderFailed), ProviderFailed);
    assert_eq!(
        worse_of(ProviderFailed, Healthy),
        Healthy,
        "the provider answered someone"
    );
    assert_eq!(worse_of(Healthy, CredentialRefused), CredentialRefused);
    assert_eq!(worse_of(CredentialRefused, Healthy), CredentialRefused);
}

#[test]
fn a_documents_units_are_one_job_in_kind_order() {
    let unit = |org: &str, doc: &str, kind: Kind, index: i32| select::Leased {
        kind,
        knowledge_id: format!("{org}-{doc}-{index}"),
        document_id: doc.to_owned(),
        org_id: org.to_owned(),
        chunk_index: index,
        text: String::new(),
        document_date: None,
        generated_at: None,
    };
    let jobs = group_into_jobs(vec![
        unit("b", "d1", Kind::Repair, 0),
        unit("a", "d2", Kind::Verify, 1),
        unit("a", "d1", Kind::Generate, 1),
        unit("a", "d1", Kind::Generate, 0),
        unit("a", "d2", Kind::Generate, 0),
    ]);
    let shape: Vec<(String, String, Vec<String>)> = jobs
        .iter()
        .map(|job| {
            (
                job.org_id.clone(),
                job.document_id.clone(),
                job.units
                    .iter()
                    .map(|unit| unit.knowledge_id.clone())
                    .collect(),
            )
        })
        .collect();
    assert_eq!(
        shape,
        [
            (
                "a".to_owned(),
                "d1".to_owned(),
                vec!["a-d1-0".to_owned(), "a-d1-1".to_owned()]
            ),
            (
                "a".to_owned(),
                "d2".to_owned(),
                vec!["a-d2-0".to_owned(), "a-d2-1".to_owned()]
            ),
            ("b".to_owned(), "d1".to_owned(), vec!["b-d1-0".to_owned()]),
        ]
    );
}

#[test]
fn every_kind_has_a_name_and_only_the_re_embeds_need_no_model() {
    let names: Vec<_> = Kind::ALL.iter().map(|kind| kind.as_str()).collect();
    assert_eq!(names, ["generate", "verify", "repair", "stale", "refresh"]);
    // The pipeline is given a model for exactly the groups that need one: only the re-embeds
    // are run without.
    let without_model: Vec<_> = Kind::ALL
        .iter()
        .filter(|kind| !Group::of(**kind).uses_model())
        .collect();
    assert_eq!(without_model, [&Kind::Verify, &Kind::Repair]);
    assert!(Group::of(Kind::Generate).uses_model() && Group::of(Kind::Stale).uses_model());
    assert!(Group::of(Kind::Refresh).uses_model());
    assert!(!Group::of(Kind::Verify).uses_model() && !Group::of(Kind::Repair).uses_model());
    assert_eq!(Group::of(Kind::Verify).pass(), PassKind::VerifyLegacy);
    assert_eq!(Group::of(Kind::Refresh).pass(), PassKind::RefreshPrompt);
    // Every batch the reconciler runs embeds units that were embedded before: none of them is the
    // stream's (the pass kind is what the version 1 cost event's key reads).
    assert_eq!(Group::of(Kind::Generate).pass(), PassKind::Reconcile);
    assert_eq!(Group::of(Kind::Stale).pass(), PassKind::Reconcile);
    assert_eq!(Group::of(Kind::Repair).pass(), PassKind::Reconcile);
    assert!(Kind::ALL
        .iter()
        .all(|kind| Group::of(*kind).pass() != PassKind::Inline));
}

/// While the usage ledger is off the embedding is recorded as a version 1 cost event, and the
/// consumer stores its key behind the organization and drops a key it already has. The key is made
/// of the units a batch embedded, so that the broker's redelivery of a batch is the same event. A
/// reconciler batch is never a redelivery: the second payment for the same units (a vector repaired
/// after its units were embedded) must not carry the first payment's key, or the consumer drops
/// it and the spend is lost.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_reconciler_batch_is_a_payment_of_its_own_and_not_a_duplicate_of_the_first() {
    let rig = Rig::new().await;
    let org = rig.org("key");
    let doc = embedded_doc(&rig, &org, &CHUNKS[..2]).await;
    for unit in &doc.units {
        seed(&rig, &unit.knowledge_id, pending()).await;
    }
    let fx = Fixture::new(&rig, |_| {});
    let keys = |rig: &Rig| -> Vec<String> {
        rig.events
            .on("dataplane.cost.ledger")
            .iter()
            .map(|event| {
                event.payload["idempotency_key"]
                    .as_str()
                    .expect("a key")
                    .to_owned()
            })
            .collect()
    };

    // The units are embedded again with their contexts: one payment.
    let first = fx.pass(&org).await;
    assert_eq!(first.leased.get("generate"), Some(&2));
    assert_eq!(keys(&rig).len(), 1);

    // A vector that is not the one its context makes, repaired: the same units, a second payment.
    sqlx::query("UPDATE knowledge_units SET embedded_context_hash = NULL WHERE org_id = $1")
        .bind(&org)
        .execute(&rig.pool)
        .await
        .expect("make the vectors wrong");
    let second = fx.pass(&org).await;
    assert_eq!(second.leased.get("repair"), Some(&2));
    let all = keys(&rig);
    assert_eq!(all.len(), 2, "each of the two payments is an event");
    assert_ne!(
        all[0], all[1],
        "the repair's cost event carries the first payment's key: the consumer drops it"
    );
    for event in rig.events.on("dataplane.cost.ledger") {
        assert_eq!(event.payload["org_id"], org.as_str());
    }
}

// ── the organization's background budget (CR-T9b) ───────────────────────────────

use crate::budget::BudgetMode;

/// Three embedded units that have no context yet, and what the reconciler is asked about them.
async fn pending_doc(rig: &Rig, org: &str) -> Doc {
    let doc = embedded_doc(rig, org, &CHUNKS).await;
    for unit in &doc.units {
        seed(rig, &unit.knowledge_id, pending()).await;
    }
    doc
}

/// A unit marked `skipped_budget` has no schedule of its own and nothing holds it while no budget
/// does: the gate is read now, so with no figure anywhere (the default) the unit is generated at
/// the very next pass. A status that nothing could lift would be a stop that never ends, and a
/// unit that was leased and embedded again at every pass for nothing.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_unit_marked_by_a_budget_that_nothing_holds_any_more_is_generated() {
    let rig = Rig::new().await;
    let org = rig.org("bud");
    let doc = embedded_doc(&rig, &org, &CHUNKS[..1]).await;
    let kid = &doc.units[0].knowledge_id;
    seed(
        &rig,
        kid,
        Seed {
            status: Some("skipped_budget"),
            ..Seed::default()
        },
    )
    .await;
    let fx = Fixture::new(&rig, |_| {});

    let report = fx.pass(&org).await;
    assert_eq!(report.leased.get("generate"), Some(&1));
    assert_eq!(report.budget_held, 0);
    assert_eq!(rig.model.call_count(), 1);
    let row = rig.row(kid).await;
    assert_eq!(row.context_status.as_deref(), Some("ready"));
    assert!(row.chunk_context.is_some());
    assert_eq!(fx.pass(&org).await.leased_total(), 0, "and then it is done");
    assert_eq!(rig.model.call_count(), 1);
}

/// A stop (explicit, on the organization's own row) leases the units that have no status ONCE,
/// which is how the pipeline marks them `skipped_budget`: no provider call, no bearer, an explicit
/// mark, and a vector from the raw text with nothing written after the embedding. Then nothing is
/// leased, nothing is embedded again and nothing is asked at any later pass: a unit the gate holds
/// is not work. The pass says the organization is held, so that an operator running
/// `context-backfill` is told why nothing happened.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_stopped_budget_marks_the_units_once_and_then_leaves_them_alone() {
    let mut rig = Rig::new().await;
    rig.deployment_budget(BudgetMode::AlertOnly, None);
    let org = rig.org("stp");
    let doc = pending_doc(&rig, &org).await;
    rig.set_budget(&org, "stop", Some(10.0)).await;
    rig.spend.set_usd(&org, 50.0);
    let fx = Fixture::new(&rig, |_| {});

    let first = fx.pass(&org).await;
    assert_eq!(first.budget_held, 1);
    assert_eq!(first.leased.get("generate"), Some(&CHUNKS.len()));
    assert_eq!(
        rig.model.call_count(),
        0,
        "a stop makes zero provider calls"
    );
    assert!(
        rig.tenants.requests().is_empty(),
        "not even a bearer was asked for"
    );
    let rows = rig.rows(&doc).await;
    for row in &rows {
        assert_eq!(row.context_status.as_deref(), Some("skipped_budget"));
        assert_eq!(row.context_error_class, None);
        assert_eq!(row.context_attempts, 0);
        assert_eq!(row.chunk_context, None);
        assert_eq!(row.embedding_status, "done");
    }
    assert_quiet(&rows);
    let embeds = rig.embedder.calls().len();
    assert_eq!(embeds, 1, "the three units are embedded raw, once");

    // Every later pass: held, and nothing leased, embedded or asked.
    for _ in 0..3 {
        let again = fx.pass(&org).await;
        assert_eq!(again.budget_held, 1);
        assert_eq!(
            again.leased_total(),
            0,
            "a unit the budget holds was leased again"
        );
    }
    assert_eq!(
        rig.embedder.calls().len(),
        embeds,
        "embedded again at a pass"
    );
    assert_eq!(rig.model.call_count(), 0);
    assert!(rig.budget.exceeded_total(BudgetMode::Stop) >= 1);
    assert_quiet(&rig.rows(&doc).await);

    // The operator raises the figure: the held units are work again at the next pass, with no
    // help, and become contexts.
    rig.set_budget(&org, "stop", Some(100.0)).await;
    let lifted = fx.pass(&org).await;
    assert_eq!(lifted.budget_held, 0);
    assert_eq!(lifted.leased.get("generate"), Some(&CHUNKS.len()));
    assert_eq!(rig.model.call_count(), CHUNKS.len());
    for row in rig.rows(&doc).await {
        assert_eq!(row.context_status.as_deref(), Some("ready"));
        assert!(row.chunk_context.is_some());
    }
}

/// A throttle is worked one unit at a time: each pass leases one unit of the organization for a
/// model and no more, whatever the pass has room for, and all of them are contextualized after as
/// many passes, never more than one provider call in one.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_throttled_organization_is_worked_one_unit_per_pass() {
    let mut rig = Rig::new().await;
    rig.deployment_budget(BudgetMode::AlertOnly, None);
    let org = rig.org("thr");
    let doc = pending_doc(&rig, &org).await;
    rig.set_budget(&org, "throttle", Some(10.0)).await;
    rig.spend.set_usd(&org, 50.0);
    let fx = Fixture::new(&rig, |_| {});

    for pass in 1..=CHUNKS.len() {
        let report = fx.pass(&org).await;
        assert_eq!(report.budget_held, 0, "a throttle holds nothing back");
        assert_eq!(report.leased.get("generate"), Some(&1), "pass {pass}");
        assert_eq!(rig.model.call_count(), pass, "pass {pass}");
    }
    assert_eq!(fx.pass(&org).await.leased_total(), 0);
    for row in rig.rows(&doc).await {
        assert_eq!(row.context_status.as_deref(), Some("ready"));
    }
    assert!(rig.budget.exceeded_total(BudgetMode::Throttle) >= 1);
}

/// An alert-only budget over its figure (the owner's default mode) counts and goes on: every unit
/// is generated in the one pass, nothing is held, and the alert is what an operator sees.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn an_alert_only_budget_over_its_figure_never_holds_the_reconciler_back() {
    let mut rig = Rig::new().await;
    rig.deployment_budget(BudgetMode::AlertOnly, Some(10.0));
    let org = rig.org("alr");
    let doc = pending_doc(&rig, &org).await;
    rig.spend.set_usd(&org, 50.0);
    let fx = Fixture::new(&rig, |_| {});

    let report = fx.pass(&org).await;
    assert_eq!(report.budget_held, 0);
    assert_eq!(report.leased.get("generate"), Some(&CHUNKS.len()));
    assert_eq!(rig.model.call_count(), CHUNKS.len());
    for row in rig.rows(&doc).await {
        assert_eq!(row.context_status.as_deref(), Some("ready"));
    }
    assert_eq!(rig.budget.exceeded_total(BudgetMode::AlertOnly), 1);
    assert_eq!(rig.budget.exceeded_total(BudgetMode::Stop), 0);
}

/// The default reads nothing and holds nothing back: no figure anywhere, so not even the usage
/// ledger is asked, whatever the organization has spent.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn with_no_figure_the_reconciler_never_reads_the_ledger() {
    let rig = Rig::new().await;
    let org = rig.org("dfl");
    let doc = pending_doc(&rig, &org).await;
    rig.spend.set_usd(&org, 1e9);
    let fx = Fixture::new(&rig, |_| {});

    let report = fx.pass(&org).await;
    assert_eq!(report.budget_held, 0);
    assert_eq!(rig.model.call_count(), CHUNKS.len());
    assert_eq!(rig.spend.reads(), 0);
    for row in rig.rows(&doc).await {
        assert_eq!(row.context_status.as_deref(), Some("ready"));
    }
}

/// A stop is the organization's own: another organization in the same pass is contextualized as
/// usual, and a deployment-wide stop is inherited by an organization with no row and refused by
/// one that opts out.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_stop_is_the_organizations_own_or_the_deployments_and_one_may_opt_out() {
    let mut rig = Rig::new().await;
    rig.deployment_budget(BudgetMode::Stop, Some(10.0));
    let inherits = rig.org("sin");
    let opts_out = rig.org("sou");
    rig.set_budget(&opts_out, "alert_only", None).await;
    let held_doc = pending_doc(&rig, &inherits).await;
    let free_doc = pending_doc(&rig, &opts_out).await;
    rig.spend.set_usd(&inherits, 50.0);
    rig.spend.set_usd(&opts_out, 50.0);
    let fx = Fixture::new(&rig, |_| {});

    let held = fx.pass(&inherits).await;
    assert_eq!(held.budget_held, 1, "the deployment's stop, inherited");
    let free = fx.pass(&opts_out).await;
    assert_eq!(free.budget_held, 0);
    assert!(rig.model.calls_for(&inherits).is_empty());
    assert_eq!(rig.model.calls_for(&opts_out).len(), CHUNKS.len());
    for row in rig.rows(&held_doc).await {
        assert_eq!(row.context_status.as_deref(), Some("skipped_budget"));
    }
    for row in rig.rows(&free_doc).await {
        assert_eq!(row.context_status.as_deref(), Some("ready"));
    }
}

/// The incident kill switch comes first: with no model nothing is generated, so no spend is held
/// and the gate is not asked. A stopped organization's units stay exactly as they were: nothing is
/// marked, nothing is read from the ledger.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn the_kill_switch_comes_before_the_budget_in_the_reconciler() {
    let mut rig = Rig::new().await;
    rig.deployment_budget(BudgetMode::AlertOnly, None);
    let org = rig.org("kil");
    let doc = pending_doc(&rig, &org).await;
    rig.set_budget(&org, "stop", Some(10.0)).await;
    rig.spend.set_usd(&org, 50.0);
    let fx = Fixture::without_model(&rig);

    let report = fx.pass(&org).await;
    assert_eq!(report.budget_held, 0);
    assert_eq!(report.leased_total(), 0);
    assert_eq!(rig.spend.reads(), 0);
    assert_eq!(rig.model.call_count(), 0);
    for row in rig.rows(&doc).await {
        assert_eq!(row.context_status.as_deref(), Some("pending"));
    }
}

/// The budget gate sits beside the organization's policy, behind it: an organization that is off
/// is marked `skipped_policy` whatever its budget says, and the gate is not asked for it.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn an_organization_that_is_off_is_marked_by_the_policy_and_not_the_budget() {
    let mut rig = Rig::new().await;
    rig.deployment_budget(BudgetMode::AlertOnly, None);
    let org = rig.org("pol");
    let doc = pending_doc(&rig, &org).await;
    rig.set_policy(&org, "off", None).await;
    rig.set_budget(&org, "stop", Some(10.0)).await;
    rig.spend.set_usd(&org, 50.0);
    let fx = Fixture::new(&rig, |_| {});

    let report = fx.pass(&org).await;
    assert_eq!(report.budget_held, 0);
    assert_eq!(
        rig.spend.reads(),
        0,
        "nothing is generated, so no spend is held"
    );
    for row in rig.rows(&doc).await {
        assert_eq!(row.context_status.as_deref(), Some("skipped_policy"));
    }
}

/// The sweep and the dry run count the units a stop holds, apart from the work.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn the_dry_run_counts_the_units_a_stop_holds_apart_from_the_work() {
    let rig = Rig::new().await;
    let org = rig.org("dry");
    let held = embedded_doc(&rig, &org, &CHUNKS[..2]).await;
    for unit in &held.units {
        seed(
            &rig,
            &unit.knowledge_id,
            Seed {
                status: Some("skipped_budget"),
                ..Seed::default()
            },
        )
        .await;
    }
    let work = embedded_doc(&rig, &org, &CHUNKS[2..]).await;
    seed(&rig, &work.units[0].knowledge_id, pending()).await;
    let fx = Fixture::new(&rig, |_| {});

    let report = fx
        .reconciler
        .dry_run(Some(&org), None, &Prices::default(), 128)
        .await
        .expect("the dry run");
    let lines: Vec<Value> = report
        .lines()
        .map(|line| serde_json::from_str(line).expect("a JSON line"))
        .collect();
    let units = &lines[0]["units"];
    assert_eq!(units["skipped_budget"], 2);
    assert_eq!(
        units["generate"], 1,
        "held units are not work the dry run prices"
    );
    assert_eq!(lines[1]["units"]["skipped_budget"], 2);
}

/// `context-backfill` of an organization whose budget is stopped says why it stopped, and does
/// not call it "nothing left to do".
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_backfill_of_a_stopped_organization_says_the_budget_is_why_it_stopped() {
    let mut rig = Rig::new().await;
    rig.deployment_budget(BudgetMode::AlertOnly, None);
    let org = rig.org("bkf");
    let doc = pending_doc(&rig, &org).await;
    rig.set_budget(&org, "stop", Some(10.0)).await;
    rig.spend.set_usd(&org, 50.0);
    let fx = Fixture::new(&rig, |_| {});

    let report = fx
        .reconciler
        .backfill(
            &Backfill {
                org_id: org.clone(),
                max_units: Some(100),
                max_usd: None,
                refresh_target: None,
                prices: Prices::default(),
            },
            128,
        )
        .await
        .expect("the backfill");

    assert!(
        report
            .stopped
            .starts_with("stopped: the organization's background budget is used up"),
        "{}",
        report.stopped
    );
    assert_eq!(report.generated, 0);
    assert_eq!(rig.model.call_count(), 0);
    for row in rig.rows(&doc).await {
        assert_eq!(row.context_status.as_deref(), Some("skipped_budget"));
    }
}

/// A stop holds something back only from an organization that has something to generate: one
/// whose units all have their contexts is not held, a pass says so, and a backfill of it is
/// "nothing left to do", not a report that the budget is why it did nothing.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_stopped_organization_with_nothing_to_generate_is_not_held_back() {
    let mut rig = Rig::new().await;
    rig.deployment_budget(BudgetMode::AlertOnly, None);
    let org = rig.org("don");
    let doc = embedded_doc(&rig, &org, &CHUNKS[..2]).await;
    let coherent = stored_context_hash("Kontekst A.");
    for unit in &doc.units {
        seed(
            &rig,
            &unit.knowledge_id,
            ready_with("Kontekst A.", Some(coherent.clone())),
        )
        .await;
    }
    rig.set_budget(&org, "stop", Some(10.0)).await;
    rig.spend.set_usd(&org, 50.0);
    let fx = Fixture::new(&rig, |_| {});

    let report = fx.pass(&org).await;
    assert_eq!(report.budget_held, 0, "nothing is held back from it");
    assert_eq!(report.leased_total(), 0);
    let backfill = fx
        .reconciler
        .backfill(
            &Backfill {
                org_id: org.clone(),
                max_units: Some(100),
                max_usd: None,
                refresh_target: None,
                prices: Prices::default(),
            },
            128,
        )
        .await
        .expect("the backfill");
    assert_eq!(backfill.stopped, "nothing left to do");
    assert_eq!(rig.model.call_count(), 0);
}

/// A refresh or a stale pass only ever regenerates, and a regeneration that cannot be made still
/// embeds the unit again (the pipeline embeds every unit of a batch). A zero-retention
/// organization's regeneration cannot be made, and its `ready` units stay selectable for as long as
/// their context is of another prompt version or their document is newer than it: without a check
/// of the organization the same units would be sent to the embedding model again and again, and a
/// bearer minted for each pass. They are left alone, and the moment the organization is not
/// zero-retention any more they are regenerated like anyone's.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_refresh_and_a_stale_pass_leave_a_zero_retention_organization_alone() {
    let rig = Rig::new().await;
    let org = rig.org("zre");
    let doc = rig.plain_document(&org, &CHUNKS).await;
    rig.run(rig.items(&doc))
        .await
        .expect("generated under ctx-v1");
    let (calls, embeds, upserts) = (
        rig.model.call_count(),
        rig.embedder.calls().len(),
        rig.vectors.upsert_count(),
    );
    rig.tenants.set(&org, Posture::Zdr);
    sqlx::query("UPDATE documents SET content = content || $2 WHERE document_id = $1")
        .bind(&doc.id)
        .bind("\n\nEt tillegg som endrer vinduet.")
        .execute(&rig.pool)
        .await
        .expect("edit the document");

    // The refresh (a deployment on ctx-v2) and the stale pass (a document edited under the prompt
    // its contexts were made with) are two ways in.
    let v2 = crate::context::PromptVersion::V2;
    let refresh = Fixture::on_prompt(&rig, v2, |settings| settings.refresh_cap = Some(100));
    let stale = Fixture::new(&rig, |settings| settings.stale_documents = Some(8));
    for _ in 0..3 {
        for fx in [&refresh, &stale] {
            let report = fx.pass(&org).await;
            assert_eq!(
                report.leased_total(),
                0,
                "a zero-retention organization's units were leased for regeneration"
            );
        }
    }
    assert_eq!(rig.model.call_count(), calls, "a model was asked");
    assert_eq!(
        rig.embedder.calls().len(),
        embeds,
        "a zero-retention organization's text was embedded again"
    );
    assert_eq!(rig.vectors.upsert_count(), upserts);

    // No longer zero-retention: the same units are regenerated, so the check is not a blanket stop.
    rig.tenants.set(&org, Posture::Plain);
    let stale = Fixture::new(&rig, |settings| settings.stale_documents = Some(8));
    let report = stale.pass(&org).await;
    assert_eq!(report.leased.get("stale"), Some(&3), "{:?}", report.leased);
    let refresh = Fixture::on_prompt(&rig, v2, |settings| settings.refresh_cap = Some(100));
    let report = refresh.pass(&org).await;
    assert_eq!(
        report.leased.get("refresh"),
        Some(&3),
        "{:?}",
        report.leased
    );
    assert!(rig.model.call_count() > calls);
}

/// A refresh is a rollout inside a cap, and the cap is spent by leasing. A regeneration that fails
/// leaves the unit as it was (`ready`, with its old context), and the pipeline writes the time it
/// may be tried again when the failure has one; a failure that is final (the content is too long)
/// has none. Either way the unit waits on the ladder: it is not asked again at the next pass, which
/// would spend the cap, and a model's time, on what has just been refused.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_refresh_that_could_not_be_made_waits_and_does_not_spend_the_cap_at_every_pass() {
    for failure in ["provider_unavailable", "provider_too_long"] {
        let rig = Rig::new().await;
        let org = rig.org("rfw");
        let doc = rig.plain_document(&org, &CHUNKS[..2]).await;
        rig.run(rig.items(&doc))
            .await
            .expect("generated under ctx-v1");
        let calls = rig.model.call_count();
        rig.model.set_default(Behaviour::Fail(failure));
        let v2 = crate::context::PromptVersion::V2;
        let fx = Fixture::on_prompt(&rig, v2, |settings| settings.refresh_cap = Some(100));

        let first = fx.pass(&org).await;
        assert_eq!(first.leased.get("refresh"), Some(&2), "{failure}");
        let asked = rig.model.call_count() - calls;
        assert!(asked >= 2, "{failure}: the model was asked {asked} times");
        for _ in 0..3 {
            let again = fx.pass(&org).await;
            assert_eq!(
                again.leased_total(),
                0,
                "{failure}: a refresh that was refused was asked again at once"
            );
        }
        assert_eq!(
            rig.model.call_count() - calls,
            asked,
            "{failure}: the model was asked again"
        );
        for row in rig.rows(&doc).await {
            assert_eq!(row.context_status.as_deref(), Some("ready"), "{failure}");
            assert_eq!(
                row.context_prompt_version.as_deref(),
                Some("ctx-v1"),
                "{failure}: a failed regeneration changed the unit"
            );
        }

        // After its wait it is asked again, once.
        let_time_pass(&rig, &doc).await;
        fx.advance(Duration::from_secs(61));
        assert_eq!(
            fx.pass(&org).await.leased.get("refresh"),
            Some(&2),
            "{failure}"
        );
        assert_eq!(fx.pass(&org).await.leased_total(), 0, "{failure}");
    }
}

/// The stale pass reads whole documents (to window them and hash each chunk's inputs), so the
/// predicate that keeps a restricted document out of it is the only thing that keeps its text out
/// of the process: no later step would lease its units. A document that changed after its
/// contexts is a candidate only when it is not restricted.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn the_stale_pass_never_reads_a_restricted_document() {
    let rig = Rig::new().await;
    let org = rig.org("srd");
    let open = embedded_doc(&rig, &org, &CHUNKS[..2]).await;
    let restricted = rig.document(&org, "restricted", &CHUNKS[2..]).await;
    mark_embedded(&rig, &restricted).await;
    for doc in [&open, &restricted] {
        sqlx::query(
            "UPDATE knowledge_units SET chunk_context = 'En kontekst.', context_status = 'ready', \
                    context_input_hash = repeat('a', 64), context_prompt_version = 'ctx-v1', \
                    context_generated_at = now() - interval '1 hour' \
              WHERE document_id = $1 AND org_id = $2",
        )
        .bind(&doc.id)
        .bind(&doc.org)
        .execute(&rig.pool)
        .await
        .expect("a context older than its document");
        sqlx::query("UPDATE documents SET updated_at = now() WHERE document_id = $1")
            .bind(&doc.id)
            .execute(&rig.pool)
            .await
            .expect("the document changed");
    }

    let mut tx = pg_org_scope::begin_org_scoped(&rig.pool, &org)
        .await
        .expect("scope");
    let candidates = select::stale_documents(&mut tx, &org, "", 10)
        .await
        .expect("the stale candidates");
    tx.commit().await.expect("commit");
    let found: Vec<&str> = candidates
        .iter()
        .map(|document| document.document_id.as_str())
        .collect();
    assert_eq!(found, [open.id.as_str()], "the candidates: {found:?}");
}

/// The operator's backfill names one organization, and a pass restricted to it does nothing for any
/// other: it does not give up on another organization's exhausted units, does not ask Auth Core
/// about its zero-retention (a bearer is minted for it) and does not release its skipped units. The
/// daemon's own scope is every organization, and the same housekeeping then covers both.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_pass_for_one_organization_gives_up_and_releases_nothing_of_another() {
    let rig = Rig::new().await;
    let (a, b) = (rig.org("sca"), rig.org("scb"));
    let mut exhausted = Vec::new();
    let mut skipped = Vec::new();
    for org in [&a, &b] {
        let worn = embedded_doc(&rig, org, &CHUNKS[..1]).await;
        seed(
            &rig,
            &worn.units[0].knowledge_id,
            Seed {
                status: Some("failed_transient"),
                class: Some("timeout"),
                attempts: 8,
                due_in: Some(3_600),
                ..Seed::default()
            },
        )
        .await;
        exhausted.push(worn.units[0].knowledge_id.clone());
        let held = embedded_doc(&rig, org, &CHUNKS[1..2]).await;
        seed(
            &rig,
            &held.units[0].knowledge_id,
            Seed {
                status: Some("skipped_zdr"),
                class: Some("zdr_org"),
                ..Seed::default()
            },
        )
        .await;
        skipped.push(held.units[0].knowledge_id.clone());
    }
    // Neither is zero-retention now: a check of either would release its skipped unit.
    let fx = Fixture::new(&rig, |_| {});
    let before = rig.tenants.requests().len();

    let report = fx.pass(&a).await;

    assert_eq!(
        report.zero_retention_checked, 1,
        "the organization it was asked about"
    );
    assert_eq!(
        rig.tenants.requests().len() - before,
        1,
        "a bearer was minted for another organization"
    );
    let status = |id: &str| {
        let rig = &rig;
        let id = id.to_owned();
        async move { rig.row(&id).await.context_status }
    };
    assert_eq!(
        status(&exhausted[0]).await.as_deref(),
        Some("failed_permanent")
    );
    // Released, and generated in the same pass.
    assert_eq!(report.zero_retention_released, 1);
    assert_eq!(status(&skipped[0]).await.as_deref(), Some("ready"));
    assert_eq!(
        status(&exhausted[1]).await.as_deref(),
        Some("failed_transient"),
        "another organization's exhausted unit was given up on"
    );
    assert_eq!(
        status(&skipped[1]).await.as_deref(),
        Some("skipped_zdr"),
        "another organization's skipped unit was released"
    );
}

/// The ladder a regeneration waits on is the reconciler's own memory, and a restart forgets it. The
/// database keeps the other half: a regeneration that failed writes the time it may be tried
/// again, and neither the refresh nor the stale pass selects a unit before it.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn a_regeneration_that_failed_waits_for_the_time_the_database_holds_across_a_restart() {
    for kind in ["refresh", "stale"] {
        let rig = Rig::new().await;
        let org = rig.org("wrs");
        let doc = rig.plain_document(&org, &CHUNKS[..2]).await;
        rig.run(rig.items(&doc))
            .await
            .expect("generated under ctx-v1");
        if kind == "stale" {
            sqlx::query("UPDATE documents SET content = content || $2 WHERE document_id = $1")
                .bind(&doc.id)
                .bind("\n\nEt tillegg som endrer vinduet.")
                .execute(&rig.pool)
                .await
                .expect("edit the document");
        }
        rig.model
            .set_default(Behaviour::Fail("provider_unavailable"));
        let v2 = crate::context::PromptVersion::V2;
        let boot = |rig: &Rig| match kind {
            "refresh" => Fixture::on_prompt(rig, v2, |settings| settings.refresh_cap = Some(100)),
            _ => Fixture::new(rig, |settings| settings.stale_documents = Some(8)),
        };

        let first = boot(&rig).pass(&org).await;
        assert_eq!(first.leased.get(kind), Some(&2), "{kind}");
        let asked = rig.model.call_count();

        // A new process: no memory of the failure, and the database says when to ask again.
        let restarted = boot(&rig).pass(&org).await;
        assert_eq!(
            restarted.leased_total(),
            0,
            "{kind}: a restart asked again before the time the failure wrote"
        );
        assert_eq!(rig.model.call_count(), asked, "{kind}");
    }
}

/// Whether a unit holds a lease.
async fn lease_is_set(rig: &Rig, knowledge_id: &str) -> bool {
    let (set,): (bool,) = sqlx::query_as(
        "SELECT context_lease_until IS NOT NULL FROM knowledge_units WHERE knowledge_id = $1",
    )
    .bind(knowledge_id)
    .fetch_one(&rig.pool)
    .await
    .expect("read the lease");
    set
}
