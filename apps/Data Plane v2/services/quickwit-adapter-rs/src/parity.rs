//! Does the keyword index hold what Postgres says it should? (CR-T9a)
//!
//! The lexical half of contextual retrieval lives here, in Quickwit: each knowledge unit is
//! indexed with its `context_body`, and until now nothing measured whether the index and the
//! database agreed. On 2026-10-06 the live index held 52 of 207 units, 155 missing, and nothing
//! said so for a week. This module is the measurement, and `GET /admin/index-parity` is how it is
//! read. It measures; it changes nothing, and it is not a repair (a repair is a re-announce of the
//! documents it names, `GET /admin/index-parity/documents`, through the same durable path every
//! document takes: the adapter re-indexes a document from Postgres when it is announced).
//!
//! ## The contract
//!
//! `index-parity-v1` is owned by the consumer, `data-quality-go` (the release gate
//! `sparse_index_in_sync`, `internal/gates/parity.go`), which has no access to Quickwit: the
//! adapter owns the index, so the adapter measures it and the quality service judges the
//! measurement. The wire shape is the golden directory vendored byte for byte under
//! `tests/fixtures/contracts/index-parity-v1/` (its README is the field-by-field definition),
//! pinned by `SHA256SUMS`; `scripts/check-parity-contract.sh` is the repo-level drift check, and a
//! test here makes the golden's own numbers out of seeded units and compares the bytes.
//!
//! ## What "expected" means
//!
//! Exactly the population this adapter indexes (`rebuild::index_document_knowledge_units`) and the
//! quality service counts for itself: units with `embedding_status = 'done'`, whose document is
//! not deleted and is classified internal, public or sensitive. The gate refuses a report whose
//! expected counts differ from its own, so a predicate that drifted would not read as parity.
//!
//! ## What "indexed" means
//!
//! The distinct units the index holds for the organization, de-duplicated by `knowledge_id`: this
//! Quickwit refuses the `cardinality` aggregation, and a delete is eventual (a task applied at the
//! next merge: 5,215 documents for 1,164 chunks were measured), so the index can hold two
//! generations of a unit until a merge. `documents` is the index's own count and `distinct_units`
//! the de-duplicated one; the report is correct whenever it is read, and a `documents` above
//! `distinct_units` says a merge is still due. Ids are compared lower-cased, because the index
//! reads the lower-cased fast field.
//!
//! The organization is the one in the verified bearer, never a parameter.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::Context as _;
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{PgPool, Row};

use crate::quickwit::IndexedUnits;
use crate::rebuild::RebuildContext;

/// The wire contract of the report.
pub const CONTRACT: &str = "index-parity-v1";

/// The contract of the list of documents that differ.
pub const DOCUMENTS_CONTRACT: &str = "index-parity-documents-v1";

/// The scope the routes require: the quality service forwards its own caller's bearer, which
/// carries it, so no principal needs a new grant.
pub const SCOPE: &str = "data:quality:admin";

/// The most documents a drift listing names. An operator repairs in batches; the listing says when
/// there are more.
pub const DOCUMENT_LIMIT: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParityReport {
    pub contract: String,
    pub org_id: String,
    pub checked_at: String,
    pub postgres: PostgresSide,
    pub quickwit: QuickwitSide,
    pub difference: Difference,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostgresSide {
    pub eligible_units: u64,
    pub eligible_with_context: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuickwitSide {
    pub documents: u64,
    pub distinct_units: u64,
    pub with_context_body: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Difference {
    pub missing_units: u64,
    pub unexpected_units: u64,
    pub context_body_missing: u64,
    pub context_body_unexpected: u64,
}

/// A unit Postgres says the index must hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpectedUnit {
    pub document_id: String,
    /// Its stored context is not blank.
    pub has_context: bool,
}

/// The units Postgres says the index must hold, by lower-cased `knowledge_id`.
pub type Expected = BTreeMap<String, ExpectedUnit>;

/// What differs in one document.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocumentDrift {
    pub missing_units: u64,
    pub context_body_missing: u64,
    pub context_body_unexpected: u64,
}

/// A measurement: the report, and the documents that differ.
#[derive(Debug, Clone)]
pub struct Comparison {
    pub report: ParityReport,
    /// Documents with at least one unit that is missing from the index or whose context body
    /// differs from the stored context, by document id. A unit the index holds that Postgres does
    /// not expect (`unexpected_units`) has no document here: Postgres does not know it.
    pub drifted: BTreeMap<String, DocumentDrift>,
}

/// The units Postgres says the organization's index must hold. One statement, the same predicate
/// the adapter indexes by and the quality service counts by, run in an organization-scoped
/// transaction as the restricted application role (the predicate also names the organization:
/// the service connects as a superuser, for which row-level security is inert).
pub async fn expected(pool: &PgPool, org_id: &str) -> anyhow::Result<Expected> {
    let mut tx = pg_org_scope::begin_org_scoped(pool, org_id).await?;
    let rows = sqlx::query(
        r#"
        SELECT ku.knowledge_id,
               ku.document_id,
               (ku.chunk_context IS NOT NULL AND btrim(ku.chunk_context) <> '') AS has_context
          FROM knowledge_units ku
          JOIN documents d ON d.document_id = ku.document_id
         WHERE ku.org_id = $1
           AND ku.embedding_status = 'done'
           AND d.deleted_at IS NULL
           AND lower(btrim(d.zdr_classification)) IN ('internal', 'public', 'sensitive')
        "#,
    )
    .bind(org_id)
    .fetch_all(&mut *tx)
    .await
    .context("read the units the index must hold")?;
    tx.commit().await?;
    Ok(rows
        .iter()
        .map(|row| {
            (
                row.get::<String, _>("knowledge_id").to_lowercase(),
                ExpectedUnit {
                    document_id: row.get("document_id"),
                    has_context: row.get("has_context"),
                },
            )
        })
        .collect())
}

/// Compare what Postgres expects with what the index holds. Pure.
pub fn compare(
    org_id: &str,
    checked_at: &str,
    expected: &Expected,
    indexed: &IndexedUnits,
) -> Comparison {
    // The two reads of the index are two requests: a unit committed between them is in the second
    // and not the first. A unit that has a context body is a unit that is indexed.
    let mut distinct: BTreeSet<&String> = indexed.distinct.iter().collect();
    distinct.extend(indexed.with_context.iter());

    let mut difference = Difference {
        missing_units: 0,
        unexpected_units: 0,
        context_body_missing: 0,
        context_body_unexpected: 0,
    };
    let mut drifted: BTreeMap<String, DocumentDrift> = BTreeMap::new();
    for (id, unit) in expected {
        if !distinct.contains(id) {
            difference.missing_units += 1;
            drifted
                .entry(unit.document_id.clone())
                .or_default()
                .missing_units += 1;
            continue;
        }
        let has_body = indexed.with_context.contains(id);
        if unit.has_context && !has_body {
            difference.context_body_missing += 1;
            drifted
                .entry(unit.document_id.clone())
                .or_default()
                .context_body_missing += 1;
        } else if !unit.has_context && has_body {
            difference.context_body_unexpected += 1;
            drifted
                .entry(unit.document_id.clone())
                .or_default()
                .context_body_unexpected += 1;
        }
    }
    difference.unexpected_units = distinct
        .iter()
        .filter(|id| !expected.contains_key(**id))
        .count() as u64;

    Comparison {
        report: ParityReport {
            contract: CONTRACT.to_owned(),
            org_id: org_id.to_owned(),
            checked_at: checked_at.to_owned(),
            postgres: PostgresSide {
                eligible_units: expected.len() as u64,
                eligible_with_context: expected.values().filter(|unit| unit.has_context).count()
                    as u64,
            },
            quickwit: QuickwitSide {
                documents: indexed.documents,
                distinct_units: distinct.len() as u64,
                with_context_body: indexed.with_context.len() as u64,
            },
            difference,
        },
        drifted,
    }
}

/// Measure the organization: the database, then the index.
///
/// The database is read first. A unit indexed between the two reads is then in the index and not in
/// the expectation, which reads as `unexpected` for as long as it takes to run again, and never as
/// a unit that is missing; the other order would report a freshly indexed unit as present when it
/// had just been deleted. Either way a measurement taken while ingestion runs is a measurement of
/// a moving thing, and the gate says so when the two services' counts of what is eligible differ.
pub async fn measure(ctx: &RebuildContext, org_id: &str) -> anyhow::Result<Comparison> {
    let expected = expected(&ctx.pool, org_id).await?;
    let indexed = ctx
        .quickwit
        .indexed_units(org_id)
        .await
        .context("read the index")?;
    Ok(compare(
        org_id,
        &Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        &expected,
        &indexed,
    ))
}

/// The documents that differ, as the operator reads them: ids and counts. Content-free.
pub fn documents_report(comparison: &Comparison) -> Value {
    let total = comparison.drifted.len();
    let documents: Vec<Value> = comparison
        .drifted
        .iter()
        .take(DOCUMENT_LIMIT)
        .map(|(document_id, drift)| {
            json!({
                "document_id": document_id,
                "missing_units": drift.missing_units,
                "context_body_missing": drift.context_body_missing,
                "context_body_unexpected": drift.context_body_unexpected,
            })
        })
        .collect();
    json!({
        "contract": DOCUMENTS_CONTRACT,
        "org_id": comparison.report.org_id,
        "checked_at": comparison.report.checked_at,
        "documents_with_drift": total,
        "truncated": total > DOCUMENT_LIMIT,
        "unexpected_units": comparison.report.difference.unexpected_units,
        "documents": documents,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DRIFT: &str =
        include_str!("../tests/fixtures/contracts/index-parity-v1/response.drift.golden.json");
    const IN_SYNC: &str =
        include_str!("../tests/fixtures/contracts/index-parity-v1/response.in-sync.golden.json");
    const ORG: &str = "org-parity-example";
    const AT: &str = "2026-10-07T09:30:00Z";

    fn id(n: usize) -> String {
        format!("{n:08x}-0000-4000-8000-000000000000")
    }

    /// The live corpus of 2026-10-06 as the golden records it: 207 eligible units in 100
    /// documents, 206 with a stored context (unit 51 has none).
    fn live() -> Expected {
        (0..207)
            .map(|n| {
                (
                    id(n),
                    ExpectedUnit {
                        document_id: format!("doc-{}", n / 2),
                        has_context: n != 51,
                    },
                )
            })
            .collect()
    }

    fn indexed(distinct: usize, with_context: &[usize], documents: u64) -> IndexedUnits {
        IndexedUnits {
            documents,
            distinct: (0..distinct).map(id).collect(),
            with_context: with_context.iter().map(|n| id(*n)).collect(),
        }
    }

    fn pretty(report: &ParityReport) -> String {
        format!("{}\n", serde_json::to_string_pretty(report).expect("json"))
    }

    fn lf(text: &str) -> String {
        text.replace("\r\n", "\n")
    }

    /// The report this module produces for the golden's own numbers is the golden, byte for byte
    /// (key order and formatting included): the consumer's contract and the producer's output are
    /// one file.
    #[test]
    fn the_live_drift_of_2026_10_06_produces_the_drift_golden_byte_for_byte() {
        let with_context: Vec<usize> = (0..52).filter(|n| *n != 51).collect();
        let comparison = compare(ORG, AT, &live(), &indexed(52, &with_context, 52));
        assert_eq!(pretty(&comparison.report), lf(DRIFT));
        // 155 units missing, in documents that still hold other units or none of them.
        assert_eq!(comparison.report.difference.missing_units, 155);
        assert_eq!(comparison.drifted.len(), 78);
    }

    #[test]
    fn an_index_that_holds_everything_produces_the_in_sync_golden_byte_for_byte() {
        let with_context: Vec<usize> = (0..207).filter(|n| *n != 51).collect();
        let comparison = compare(ORG, AT, &live(), &indexed(207, &with_context, 207));
        assert_eq!(pretty(&comparison.report), lf(IN_SYNC));
        assert!(comparison.drifted.is_empty());
    }

    /// What the gate checks of any report it reads: it adds up.
    fn adds_up(report: &ParityReport) -> bool {
        report.postgres.eligible_units - report.difference.missing_units
            + report.difference.unexpected_units
            == report.quickwit.distinct_units
    }

    #[test]
    fn a_unit_the_index_lacks_and_one_it_should_not_have_are_counted_apart() {
        let mut expected = live();
        expected.retain(|_, unit| unit.document_id.as_str() < "doc-9");
        let kept = expected.len();
        // The index holds the first 10 units, and 3 that Postgres does not know.
        let mut index = indexed(10, &(0..10).filter(|n| *n != 51).collect::<Vec<_>>(), 13);
        index.distinct.extend((900..903).map(id));
        let comparison = compare(ORG, AT, &expected, &index);
        let report = &comparison.report;
        assert_eq!(report.postgres.eligible_units as usize, kept);
        assert_eq!(report.quickwit.distinct_units, 13);
        assert_eq!(report.difference.unexpected_units, 3);
        assert_eq!(report.difference.missing_units as usize, kept - 10);
        assert!(adds_up(report));
    }

    #[test]
    fn a_context_the_index_lacks_and_one_it_should_not_have_are_counted_apart() {
        let expected: Expected = (0..4)
            .map(|n| {
                (
                    id(n),
                    ExpectedUnit {
                        document_id: format!("doc-{n}"),
                        has_context: n != 3,
                    },
                )
            })
            .collect();
        // Unit 1 is indexed without the context Postgres has; unit 3 is indexed with a context
        // Postgres has not; units 0 and 2 are right.
        let index = IndexedUnits {
            documents: 4,
            distinct: (0..4).map(id).collect(),
            with_context: [0, 2, 3].iter().map(|n| id(*n)).collect(),
        };
        let comparison = compare(ORG, AT, &expected, &index);
        let difference = &comparison.report.difference;
        assert_eq!(
            (
                difference.missing_units,
                difference.unexpected_units,
                difference.context_body_missing,
                difference.context_body_unexpected
            ),
            (0, 0, 1, 1)
        );
        assert_eq!(
            comparison.drifted.keys().collect::<Vec<_>>(),
            ["doc-1", "doc-3"]
        );
        assert_eq!(comparison.drifted["doc-1"].context_body_missing, 1);
        assert_eq!(comparison.drifted["doc-3"].context_body_unexpected, 1);
        assert_eq!(comparison.report.quickwit.with_context_body, 3);
        assert!(adds_up(&comparison.report));
    }

    /// A delete that waits for a merge leaves two generations of a unit: `documents` says so, and
    /// the distinct count and the report are right all the same.
    #[test]
    fn duplicates_awaiting_a_merge_are_reported_and_do_not_make_a_unit_present_twice() {
        let expected: Expected = (0..3)
            .map(|n| {
                (
                    id(n),
                    ExpectedUnit {
                        document_id: "doc".to_owned(),
                        has_context: true,
                    },
                )
            })
            .collect();
        let index = IndexedUnits {
            documents: 9,
            distinct: (0..3).map(id).collect(),
            with_context: (0..3).map(id).collect(),
        };
        let comparison = compare(ORG, AT, &expected, &index);
        assert_eq!(comparison.report.quickwit.documents, 9);
        assert_eq!(comparison.report.quickwit.distinct_units, 3);
        assert_eq!(comparison.report.difference.missing_units, 0);
        assert!(comparison.drifted.is_empty());
    }

    /// The index and Postgres write the same id in different cases (the index reads the
    /// lower-cased field): they are the same unit.
    #[test]
    fn ids_are_compared_lower_cased() {
        let expected: Expected = [(
            "ABCDEF00-0000-4000-8000-000000000001".to_lowercase(),
            ExpectedUnit {
                document_id: "doc".to_owned(),
                has_context: false,
            },
        )]
        .into();
        let index = IndexedUnits {
            documents: 1,
            distinct: ["abcdef00-0000-4000-8000-000000000001".to_owned()].into(),
            with_context: BTreeSet::new(),
        };
        assert_eq!(
            compare(ORG, AT, &expected, &index).report.difference,
            Difference {
                missing_units: 0,
                unexpected_units: 0,
                context_body_missing: 0,
                context_body_unexpected: 0
            }
        );
    }

    /// A unit committed between the two reads of the index is in the second and not the first: it
    /// counts as indexed, and the report still adds up.
    #[test]
    fn a_unit_seen_by_the_second_read_only_is_indexed() {
        let expected: Expected = (0..2)
            .map(|n| {
                (
                    id(n),
                    ExpectedUnit {
                        document_id: "doc".to_owned(),
                        has_context: true,
                    },
                )
            })
            .collect();
        let index = IndexedUnits {
            documents: 1,
            distinct: [id(0)].into(),
            with_context: [id(0), id(1)].into(),
        };
        let comparison = compare(ORG, AT, &expected, &index);
        assert_eq!(comparison.report.quickwit.distinct_units, 2);
        assert_eq!(comparison.report.difference.missing_units, 0);
        assert!(adds_up(&comparison.report));
    }

    #[test]
    fn an_organization_with_nothing_eligible_and_nothing_indexed_is_in_sync() {
        let comparison = compare(ORG, AT, &Expected::new(), &IndexedUnits::default());
        assert_eq!(comparison.report.postgres.eligible_units, 0);
        assert_eq!(comparison.report.difference.missing_units, 0);
        assert!(adds_up(&comparison.report));
    }

    /// The listing of documents that differ holds ids and counts and nothing else, says when it
    /// is cut, and names the documents in order.
    #[test]
    fn the_drift_listing_is_content_free_ordered_and_bounded() {
        let expected: Expected = (0..(DOCUMENT_LIMIT + 5))
            .map(|n| {
                (
                    id(n),
                    ExpectedUnit {
                        document_id: format!("doc-{n:05}"),
                        has_context: true,
                    },
                )
            })
            .collect();
        let comparison = compare(ORG, AT, &expected, &IndexedUnits::default());
        let report = documents_report(&comparison);
        assert_eq!(report["contract"], DOCUMENTS_CONTRACT);
        assert_eq!(report["documents_with_drift"], DOCUMENT_LIMIT + 5);
        assert_eq!(report["truncated"], true);
        let documents = report["documents"].as_array().expect("documents");
        assert_eq!(documents.len(), DOCUMENT_LIMIT);
        assert_eq!(documents[0]["document_id"], "doc-00000");
        assert_eq!(documents[0]["missing_units"], 1);
        let mut keys: Vec<&str> = documents[0]
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "context_body_missing",
                "context_body_unexpected",
                "document_id",
                "missing_units"
            ]
        );
        let small = documents_report(&compare(ORG, AT, &live(), &indexed(207, &[], 207)));
        assert_eq!(small["truncated"], false);
    }
}
