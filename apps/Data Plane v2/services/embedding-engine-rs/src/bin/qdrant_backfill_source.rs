//! Operator-invoked backfill of the `source` payload key on points that were written before
//! the embedding engine stamped it (QB-M1b).
//!
//! ## Why this exists
//!
//! The retrieval engine scopes a dense search to a corpus by filtering Qdrant on the payload
//! key `source` (`filters.sources`). Every point written by the batch pipeline from now on
//! carries it, copied from `documents.source`. The points that exist today do not, and a
//! scoped search finds nothing among them. This tool stamps them, in place, with
//! Qdrant's `set_payload`: no re-embedding, no provider call, no change to a vector, to any
//! other payload key, or to `org_id`.
//!
//! ## What it does, and what it never does
//!
//! `stamp` (the default) scrolls the points whose payload has no `source`, finds each one's
//! document in Postgres by `(document_id, org_id)` TOGETHER (both taken from the point itself;
//! the organisation predicate is written out, the connection is a superuser's and
//! row-level security does not apply), and writes `{ "source": <documents.source> }` to those
//! points by id. It never writes any other key, never overwrites a `source` that is
//! already there (a stamped point is not even scrolled), and leaves a point alone, counted and
//! reported, when it has no document id or no organisation, when its document is not in that
//! organisation, or when the document's source is blank. It does not invent a source.
//!
//! It is idempotent: a second run finds nothing to do. It is bounded: `--max-points` (default
//! 10000) is the most points one run examines, and a run that stops at the bound prints the
//! `resume_offset` to pass as `--offset` so the next run continues past points it already
//! looked at (a point that cannot be stamped is examined again by every run that starts
//! before it).
//!
//! `--report` is read-only. It prints, for the collection, the points with each source
//! next to the number of finished units Postgres has for documents of that source, the points
//! with no source, and the points carrying a source that no document has. It exits 0 when
//! every number agrees and 3 when they do not. It is meant for the text collection
//! (`QDRANT_COLLECTION`, one point per finished unit); on the visual, audio and video
//! collections the Postgres side is not comparable.
//!
//! A stamp is a copy. `documents.source` can change when the same document key is re-ingested
//! from another source, and a chunk that did not change is not re-embedded, so its point is
//! not rewritten: `--report` shows that as a count that stops agreeing, and nothing here
//! rewrites a stamp that exists. The retrieval engine keeps its own check against
//! `documents.source` after fusion for the same reason.
//!
//! ## Usage
//!
//! ```text
//! qdrant-backfill-source --report                                   (read-only counts)
//! qdrant-backfill-source --dry-run [--org-id <ORG>]... [--max-points N] [--offset ID]
//! qdrant-backfill-source           [--org-id <ORG>]... [--max-points N] [--offset ID]
//! qdrant-backfill-source --report  [--org-id <ORG>]...
//!     [--collection NAME]   default: $QDRANT_COLLECTION, else dataplane_knowledge_embedv4
//! ```
//!
//! Env: `DATABASE_URL`, `QDRANT_URL` (the gRPC endpoint): the variables the `embedding-engine`
//! service itself runs with. One JSON line goes to stdout; logs go to stderr.
//!
//! Run order: `--report`, `--dry-run`, the real run, `--report` again (see
//! `docs/qdrant-source-stamp-runbook.md`).

// The key and the lookup are the service's own, compiled in here so there is one definition.
#[path = "../source_stamp.rs"]
mod source_stamp;

use std::collections::{BTreeMap, BTreeSet, HashMap};

use anyhow::{anyhow, bail, Context};
use qdrant_client::qdrant::{
    point_id::PointIdOptions, value::Kind as QdrantKind, with_payload_selector::SelectorOptions,
    Condition, CountPointsBuilder, Filter, PayloadIncludeSelector, PointId, PointsIdsList,
    ScrollPointsBuilder, SetPayloadPoints, SetPayloadPointsBuilder, Value as QdrantValue,
};
use qdrant_client::Qdrant;

use source_stamp::{stampable, PAYLOAD_KEY};

const DEFAULT_COLLECTION: &str = "dataplane_knowledge_embedv4";
const DEFAULT_MAX_POINTS: usize = 10_000;
/// Points per scroll page and per `set_payload` call.
const PAGE: usize = 256;

// ── arguments ───────────────────────────────────────────────────────────────

#[derive(Debug, PartialEq)]
enum Mode {
    Stamp {
        dry_run: bool,
        max_points: usize,
        offset: Option<PointId>,
    },
    Report,
}

#[derive(Debug, PartialEq)]
struct Args {
    collection: String,
    org_ids: Vec<String>,
    mode: Mode,
}

#[derive(Debug, PartialEq)]
enum Parsed {
    Help,
    Run(Args),
}

const USAGE: &str =
    "qdrant-backfill-source [--report | [--dry-run] [--max-points N] [--offset ID]] \
[--org-id <ORG>]... [--collection NAME]";

fn parse_point_id(text: &str) -> anyhow::Result<PointId> {
    if let Ok(number) = text.parse::<u64>() {
        return Ok(PointId::from(number));
    }
    let uuid = uuid::Uuid::parse_str(text)
        .map_err(|_| anyhow!("--offset must be a point id (a UUID or an integer): {text}"))?;
    Ok(PointId::from(uuid.to_string()))
}

fn parse_args(
    args: impl IntoIterator<Item = String>,
    default_collection: &str,
) -> anyhow::Result<Parsed> {
    let mut collection: Option<String> = None;
    let mut org_ids: Vec<String> = Vec::new();
    let mut dry_run = false;
    let mut report = false;
    let mut max_points: Option<usize> = None;
    let mut offset: Option<PointId> = None;

    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--collection" => {
                collection = Some(
                    it.next()
                        .filter(|value| !value.trim().is_empty())
                        .ok_or_else(|| anyhow!("--collection needs a value"))?,
                );
            }
            "--org-id" => org_ids.push(
                it.next()
                    .filter(|value| !value.trim().is_empty())
                    .ok_or_else(|| anyhow!("--org-id needs a value"))?,
            ),
            "--dry-run" => dry_run = true,
            "--report" => report = true,
            "--max-points" => {
                let value = it
                    .next()
                    .ok_or_else(|| anyhow!("--max-points needs a value"))?;
                let parsed: usize = value
                    .parse()
                    .map_err(|_| anyhow!("--max-points must be a whole number: {value}"))?;
                if parsed == 0 {
                    bail!("--max-points must be at least 1");
                }
                max_points = Some(parsed);
            }
            "--offset" => {
                let value = it.next().ok_or_else(|| anyhow!("--offset needs a value"))?;
                offset = Some(parse_point_id(&value)?);
            }
            "-h" | "--help" => return Ok(Parsed::Help),
            other => bail!("unknown arg: {other}"),
        }
    }

    let mode = if report {
        if dry_run || max_points.is_some() || offset.is_some() {
            bail!("--report only reads counts: it takes no --dry-run, --max-points or --offset");
        }
        Mode::Report
    } else {
        Mode::Stamp {
            dry_run,
            max_points: max_points.unwrap_or(DEFAULT_MAX_POINTS),
            offset,
        }
    };
    org_ids.sort();
    org_ids.dedup();
    Ok(Parsed::Run(Args {
        collection: collection.unwrap_or_else(|| default_collection.to_owned()),
        org_ids,
        mode,
    }))
}

// ── what the tool talks to ──────────────────────────────────────────────────

/// A point the scroll returned: its id, and the two payload values that identify its
/// document. Anything else about a point is none of this tool's business.
#[derive(Debug, Clone, PartialEq)]
struct Candidate {
    id: PointId,
    document_id: Option<String>,
    org_id: Option<String>,
}

struct Page {
    candidates: Vec<Candidate>,
    /// Where the next page starts (inclusive), or `None` at the end.
    next: Option<PointId>,
}

/// Which points a count is over, always within the requested organisations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Count<'a> {
    All,
    Unstamped,
    Source(&'a str),
}

/// The vector store, one collection of it.
trait Points {
    /// Up to `limit` points that have no `source`, in id order, from `offset` (inclusive).
    async fn unstamped_page(
        &self,
        org_ids: &[String],
        offset: Option<&PointId>,
        limit: usize,
    ) -> anyhow::Result<Page>;
    /// Set the payload key `source` on exactly these points, and write nothing else.
    async fn stamp(&self, ids: &[PointId], source: &str) -> anyhow::Result<()>;
    async fn count(&self, org_ids: &[String], scope: Count<'_>) -> anyhow::Result<u64>;
}

/// The documents table.
trait Documents {
    /// `documents.source` of each `(document_id, org_id)` pair that exists.
    async fn sources(
        &self,
        pairs: &[(String, String)],
    ) -> anyhow::Result<HashMap<(String, String), String>>;
    /// Finished units of live documents, by the source of their document.
    async fn done_units_by_source(
        &self,
        org_ids: &[String],
    ) -> anyhow::Result<BTreeMap<String, u64>>;
}

fn id_key(id: &PointId) -> String {
    match &id.point_id_options {
        Some(PointIdOptions::Uuid(uuid)) => uuid.clone(),
        Some(PointIdOptions::Num(number)) => number.to_string(),
        None => String::new(),
    }
}

// ── the stamping run ────────────────────────────────────────────────────────

#[derive(Debug, Default, PartialEq)]
struct Summary {
    examined: usize,
    /// Points stamped, or that would be in a dry run.
    stamped: usize,
    by_source: BTreeMap<String, usize>,
    skipped_without_identity: usize,
    skipped_document_not_found: usize,
    skipped_blank_source: usize,
    /// The run stopped at `--max-points` with points left to look at.
    truncated: bool,
    /// Where to continue (`--offset`), when `truncated`.
    resume_offset: Option<String>,
}

impl Summary {
    fn skipped(&self) -> usize {
        self.skipped_without_identity + self.skipped_document_not_found + self.skipped_blank_source
    }

    fn to_json(&self, collection: &str, dry_run: bool) -> serde_json::Value {
        // What a dry run reports is what it would do, under a name that cannot be read as
        // done.
        let stamped_key = if dry_run { "would_stamp" } else { "stamped" };
        serde_json::json!({
            "mode": "stamp",
            "collection": collection,
            "dry_run": dry_run,
            "examined": self.examined,
            (stamped_key): self.stamped,
            "by_source": self.by_source,
            "skipped": {
                "without_identity": self.skipped_without_identity,
                "document_not_found": self.skipped_document_not_found,
                "blank_source": self.skipped_blank_source,
            },
            "truncated": self.truncated,
            "resume_offset": self.resume_offset,
        })
    }
}

struct StampOptions<'a> {
    org_ids: &'a [String],
    dry_run: bool,
    max_points: usize,
    offset: Option<PointId>,
}

async fn stamp_missing(
    points: &impl Points,
    documents: &impl Documents,
    options: &StampOptions<'_>,
) -> anyhow::Result<Summary> {
    let mut summary = Summary::default();
    let mut offset = options.offset.clone();

    loop {
        let remaining = options.max_points.saturating_sub(summary.examined);
        if remaining == 0 {
            // The bound, not the end: say where to pick up.
            summary.truncated = offset.is_some();
            summary.resume_offset = offset.as_ref().map(id_key);
            break;
        }
        let page = points
            .unstamped_page(options.org_ids, offset.as_ref(), remaining.min(PAGE))
            .await
            .context("scroll the points that have no source")?;
        if page.candidates.is_empty() && page.next.is_some() {
            bail!("the scroll returned no points but says there are more: refusing to loop");
        }
        summary.examined += page.candidates.len();

        // Identity first: a point that does not say which document and organisation it
        // belongs to is not looked up under a guess.
        let mut with_identity: Vec<(&Candidate, (String, String))> = Vec::new();
        for candidate in &page.candidates {
            match (&candidate.document_id, &candidate.org_id) {
                (Some(document_id), Some(org_id))
                    if !document_id.is_empty() && !org_id.is_empty() =>
                {
                    with_identity.push((candidate, (document_id.clone(), org_id.clone())));
                }
                _ => summary.skipped_without_identity += 1,
            }
        }
        let pairs: Vec<(String, String)> = with_identity
            .iter()
            .map(|(_, pair)| pair.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let found = documents
            .sources(&pairs)
            .await
            .context("read the documents of the points")?;

        let mut by_source: BTreeMap<&str, Vec<PointId>> = BTreeMap::new();
        for (candidate, pair) in &with_identity {
            match found.get(pair) {
                None => summary.skipped_document_not_found += 1,
                Some(source) => match stampable(source) {
                    None => summary.skipped_blank_source += 1,
                    Some(source) => by_source
                        .entry(source)
                        .or_default()
                        .push(candidate.id.clone()),
                },
            }
        }

        for (source, ids) in &by_source {
            if !options.dry_run {
                for batch in ids.chunks(PAGE) {
                    points
                        .stamp(batch, source)
                        .await
                        .with_context(|| format!("stamp {} points with {source:?}", batch.len()))?;
                }
            }
            summary.stamped += ids.len();
            *summary.by_source.entry((*source).to_owned()).or_default() += ids.len();
        }
        tracing::info!(
            examined = summary.examined,
            stamped = summary.stamped,
            skipped = summary.skipped(),
            dry_run = options.dry_run,
            "page done"
        );

        match page.next {
            Some(next) => offset = Some(next),
            None => break,
        }
    }
    Ok(summary)
}

// ── the report ──────────────────────────────────────────────────────────────

#[derive(Debug, PartialEq)]
struct Report {
    total_points: u64,
    unstamped_points: u64,
    /// Per source Postgres knows: `(points in Qdrant, finished units in Postgres)`.
    by_source: BTreeMap<String, (u64, u64)>,
    /// Points that carry a source no document of Postgres has: a stamp gone stale, or a
    /// document that is gone.
    stamped_with_unknown_source: u64,
}

impl Report {
    fn consistent(&self) -> bool {
        self.unstamped_points == 0
            && self.stamped_with_unknown_source == 0
            && self
                .by_source
                .values()
                .all(|(points, units)| points == units)
    }

    fn to_json(&self, collection: &str) -> serde_json::Value {
        let by_source: serde_json::Map<String, serde_json::Value> = self
            .by_source
            .iter()
            .map(|(source, (points, units))| {
                (
                    source.clone(),
                    serde_json::json!({
                        "qdrant_points": points,
                        "postgres_done_units": units,
                        "agree": points == units,
                    }),
                )
            })
            .collect();
        serde_json::json!({
            "mode": "report",
            "collection": collection,
            "total_points": self.total_points,
            "unstamped_points": self.unstamped_points,
            "stamped_with_unknown_source": self.stamped_with_unknown_source,
            "by_source": by_source,
            "consistent": self.consistent(),
        })
    }
}

async fn report(
    points: &impl Points,
    documents: &impl Documents,
    org_ids: &[String],
) -> anyhow::Result<Report> {
    let units = documents
        .done_units_by_source(org_ids)
        .await
        .context("count the finished units by source")?;
    let total_points = points.count(org_ids, Count::All).await?;
    let unstamped_points = points.count(org_ids, Count::Unstamped).await?;
    let mut by_source = BTreeMap::new();
    let mut stamped_known = 0u64;
    for (source, in_postgres) in units {
        let in_qdrant = points.count(org_ids, Count::Source(&source)).await?;
        stamped_known += in_qdrant;
        by_source.insert(source, (in_qdrant, in_postgres));
    }
    Ok(Report {
        total_points,
        unstamped_points,
        by_source,
        stamped_with_unknown_source: total_points
            .saturating_sub(unstamped_points)
            .saturating_sub(stamped_known),
    })
}

// ── Qdrant ──────────────────────────────────────────────────────────────────

fn org_condition(org_ids: &[String]) -> Option<Condition> {
    (!org_ids.is_empty()).then(|| Condition::matches("org_id", org_ids.to_vec()))
}

/// The points that have no `source`: absent, null or an empty list (Qdrant's `is_empty`). A point whose `source` is the empty STRING is not matched (verified on Qdrant 1.17): it counts as stamped here, and `--report` shows it under `stamped_with_unknown_source`. The writer never writes one.
fn unstamped_filter(org_ids: &[String]) -> Filter {
    let mut must = vec![Condition::is_empty(PAYLOAD_KEY)];
    must.extend(org_condition(org_ids));
    Filter::must(must)
}

fn count_filter(org_ids: &[String], scope: Count<'_>) -> Option<Filter> {
    let mut must = Vec::new();
    match scope {
        Count::All => {}
        Count::Unstamped => must.push(Condition::is_empty(PAYLOAD_KEY)),
        Count::Source(source) => must.push(Condition::matches(PAYLOAD_KEY, source.to_owned())),
    }
    must.extend(org_condition(org_ids));
    (!must.is_empty()).then(|| Filter::must(must))
}

/// The request that stamps points: the one key `source`, to exactly these ids. Qdrant's
/// `set_payload` merges the given keys into the point's payload and leaves every other key
/// where it is, so `org_id` and the rest cannot change.
fn stamp_request(collection: &str, ids: &[PointId], source: &str) -> SetPayloadPoints {
    let payload: HashMap<String, QdrantValue> = HashMap::from([(
        PAYLOAD_KEY.to_owned(),
        QdrantValue {
            kind: Some(QdrantKind::StringValue(source.to_owned())),
        },
    )]);
    SetPayloadPointsBuilder::new(collection, payload)
        .points_selector(PointsIdsList { ids: ids.to_vec() })
        .wait(true)
        .build()
}

struct QdrantCollection<'a> {
    client: &'a Qdrant,
    name: &'a str,
}

impl Points for QdrantCollection<'_> {
    async fn unstamped_page(
        &self,
        org_ids: &[String],
        offset: Option<&PointId>,
        limit: usize,
    ) -> anyhow::Result<Page> {
        let mut request = ScrollPointsBuilder::new(self.name)
            .filter(unstamped_filter(org_ids))
            .limit(u32::try_from(limit).context("page size")?)
            .with_payload(SelectorOptions::Include(PayloadIncludeSelector {
                fields: vec!["document_id".to_owned(), "org_id".to_owned()],
            }));
        if let Some(offset) = offset {
            request = request.offset(offset.clone());
        }
        let response = self.client.scroll(request).await?;
        let candidates = response
            .result
            .into_iter()
            .map(|point| {
                let id = point.id.context("a scrolled point has no id")?;
                let text = |key: &str| {
                    point
                        .payload
                        .get(key)
                        .and_then(QdrantValue::as_str)
                        .cloned()
                };
                Ok(Candidate {
                    document_id: text("document_id"),
                    org_id: text("org_id"),
                    id,
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok(Page {
            candidates,
            next: response.next_page_offset,
        })
    }

    async fn stamp(&self, ids: &[PointId], source: &str) -> anyhow::Result<()> {
        self.client
            .set_payload(stamp_request(self.name, ids, source))
            .await?;
        Ok(())
    }

    async fn count(&self, org_ids: &[String], scope: Count<'_>) -> anyhow::Result<u64> {
        let mut request = CountPointsBuilder::new(self.name).exact(true);
        if let Some(filter) = count_filter(org_ids, scope) {
            request = request.filter(filter);
        }
        let response = self.client.count(request).await?;
        Ok(response.result.map_or(0, |result| result.count))
    }
}

// ── Postgres ────────────────────────────────────────────────────────────────

struct Postgres(sqlx::PgPool);

impl Documents for Postgres {
    async fn sources(
        &self,
        pairs: &[(String, String)],
    ) -> anyhow::Result<HashMap<(String, String), String>> {
        let borrowed: Vec<(&str, &str)> = pairs
            .iter()
            .map(|(document_id, org_id)| (document_id.as_str(), org_id.as_str()))
            .collect();
        let mut conn = self.0.acquire().await.context("acquire a connection")?;
        Ok(source_stamp::load(&mut conn, &borrowed).await?)
    }

    async fn done_units_by_source(
        &self,
        org_ids: &[String],
    ) -> anyhow::Result<BTreeMap<String, u64>> {
        let orgs: Option<Vec<String>> = (!org_ids.is_empty()).then(|| org_ids.to_vec());
        let rows: Vec<(String, i64)> = sqlx::query_as(
            "SELECT d.source, count(*) \
               FROM knowledge_units AS ku \
               JOIN documents AS d ON d.document_id = ku.document_id AND d.org_id = ku.org_id \
              WHERE ku.embedding_status = 'done' AND d.deleted_at IS NULL \
                AND ($1::text[] IS NULL OR ku.org_id = ANY($1)) \
              GROUP BY d.source",
        )
        .bind(orgs)
        .fetch_all(&self.0)
        .await?;
        Ok(rows
            .into_iter()
            .filter(|(source, _)| stampable(source).is_some())
            .map(|(source, count)| (source, u64::try_from(count).unwrap_or(0)))
            .collect())
    }
}

// ── main ────────────────────────────────────────────────────────────────────

fn env_var(name: &str) -> anyhow::Result<String> {
    std::env::var(name).with_context(|| format!("{name} must be set"))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Logs on stderr: stdout carries the one JSON line a script reads.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();

    let default_collection =
        std::env::var("QDRANT_COLLECTION").unwrap_or_else(|_| DEFAULT_COLLECTION.to_owned());
    let args = match parse_args(std::env::args().skip(1), &default_collection)? {
        Parsed::Help => {
            println!("{USAGE}");
            return Ok(());
        }
        Parsed::Run(args) => args,
    };
    tracing::info!(?args, "qdrant-backfill-source starting");

    let database_url = env_var("DATABASE_URL")?;
    let qdrant_url = env_var("QDRANT_URL")?;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .connect(&database_url)
        .await
        .context("connect to postgres")?;
    let client = Qdrant::from_url(&qdrant_url)
        .build()
        .map_err(|e| anyhow!("qdrant connect: {e}"))?;
    if !client
        .collection_exists(&args.collection)
        .await
        .context("ask qdrant whether the collection exists")?
    {
        bail!("collection {} does not exist", args.collection);
    }
    let points = QdrantCollection {
        client: &client,
        name: &args.collection,
    };
    let documents = Postgres(pool);

    match args.mode {
        Mode::Report => {
            let report = report(&points, &documents, &args.org_ids).await?;
            println!("{}", report.to_json(&args.collection));
            if !report.consistent() {
                std::process::exit(3);
            }
        }
        Mode::Stamp {
            dry_run,
            max_points,
            offset,
        } => {
            let summary = stamp_missing(
                &points,
                &documents,
                &StampOptions {
                    org_ids: &args.org_ids,
                    dry_run,
                    max_points,
                    offset,
                },
            )
            .await?;
            println!("{}", summary.to_json(&args.collection, dry_run));
        }
    }
    Ok(())
}

// ── tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use qdrant_client::qdrant::{points_selector::PointsSelectorOneOf, r#match::MatchValue};

    use super::*;

    // The fakes record what they are asked and hold the points; they decide nothing about
    // what the tool should do. The stored points keep a plain string payload, which is all
    // the tool reads or writes.

    #[derive(Clone, Debug, PartialEq)]
    struct Stored {
        id: String,
        payload: BTreeMap<String, String>,
    }

    #[derive(Clone, Debug, PartialEq)]
    struct Write {
        ids: Vec<String>,
        source: String,
        /// The payload keys of the request: what Qdrant would be told to set.
        keys: Vec<String>,
    }

    /// One scroll as asked: the organisations, the offset and the page size.
    type Scroll = (Vec<String>, Option<String>, usize);

    #[derive(Default)]
    struct FakePoints {
        stored: Mutex<Vec<Stored>>,
        writes: Mutex<Vec<Write>>,
        scrolls: Mutex<Vec<Scroll>>,
    }

    impl FakePoints {
        fn with(points: Vec<Stored>) -> Self {
            let mut points = points;
            points.sort_by(|a, b| a.id.cmp(&b.id));
            Self {
                stored: Mutex::new(points),
                ..Self::default()
            }
        }

        fn stored(&self) -> Vec<Stored> {
            self.stored.lock().unwrap().clone()
        }

        fn writes(&self) -> Vec<Write> {
            self.writes.lock().unwrap().clone()
        }

        fn scrolls(&self) -> Vec<Scroll> {
            self.scrolls.lock().unwrap().clone()
        }

        fn source_of(&self, id: &str) -> Option<String> {
            self.stored()
                .into_iter()
                .find(|point| point.id == id)
                .and_then(|point| point.payload.get("source").cloned())
        }
    }

    fn in_orgs(point: &Stored, org_ids: &[String]) -> bool {
        org_ids.is_empty()
            || point
                .payload
                .get("org_id")
                .is_some_and(|org| org_ids.contains(org))
    }

    impl Points for FakePoints {
        async fn unstamped_page(
            &self,
            org_ids: &[String],
            offset: Option<&PointId>,
            limit: usize,
        ) -> anyhow::Result<Page> {
            self.scrolls
                .lock()
                .unwrap()
                .push((org_ids.to_vec(), offset.map(id_key), limit));
            let from = offset.map(id_key);
            let matching: Vec<Stored> = self
                .stored()
                .into_iter()
                .filter(|point| !point.payload.contains_key("source"))
                .filter(|point| in_orgs(point, org_ids))
                .filter(|point| from.as_ref().is_none_or(|from| &point.id >= from))
                .collect();
            let candidates = matching
                .iter()
                .take(limit)
                .map(|point| Candidate {
                    id: PointId::from(point.id.as_str()),
                    document_id: point.payload.get("document_id").cloned(),
                    org_id: point.payload.get("org_id").cloned(),
                })
                .collect();
            Ok(Page {
                candidates,
                next: matching
                    .get(limit)
                    .map(|point| PointId::from(point.id.as_str())),
            })
        }

        async fn stamp(&self, ids: &[PointId], source: &str) -> anyhow::Result<()> {
            let request = stamp_request("fake", ids, source);
            let mut keys: Vec<String> = request.payload.keys().cloned().collect();
            keys.sort();
            let ids: Vec<String> = ids.iter().map(id_key).collect();
            self.writes.lock().unwrap().push(Write {
                ids: ids.clone(),
                source: source.to_owned(),
                keys: keys.clone(),
            });
            // Qdrant's set_payload: merge the request's keys into each named point.
            let mut stored = self.stored.lock().unwrap();
            for point in stored.iter_mut().filter(|point| ids.contains(&point.id)) {
                for (key, value) in &request.payload {
                    if let Some(text) = value.as_str() {
                        point.payload.insert(key.clone(), text.clone());
                    }
                }
            }
            Ok(())
        }

        async fn count(&self, org_ids: &[String], scope: Count<'_>) -> anyhow::Result<u64> {
            let count = self
                .stored()
                .iter()
                .filter(|point| in_orgs(point, org_ids))
                .filter(|point| match scope {
                    Count::All => true,
                    Count::Unstamped => !point.payload.contains_key("source"),
                    Count::Source(source) => point
                        .payload
                        .get("source")
                        .is_some_and(|have| have == source),
                })
                .count();
            Ok(count as u64)
        }
    }

    #[derive(Default)]
    struct FakeDocuments {
        rows: HashMap<(String, String), String>,
        done_units: BTreeMap<String, u64>,
        lookups: Mutex<Vec<Vec<(String, String)>>>,
    }

    impl FakeDocuments {
        fn with(rows: &[(&str, &str, &str)]) -> Self {
            Self {
                rows: rows
                    .iter()
                    .map(|(doc, org, source)| {
                        (((*doc).to_owned(), (*org).to_owned()), (*source).to_owned())
                    })
                    .collect(),
                ..Self::default()
            }
        }

        fn lookups(&self) -> Vec<Vec<(String, String)>> {
            self.lookups.lock().unwrap().clone()
        }
    }

    impl Documents for FakeDocuments {
        async fn sources(
            &self,
            pairs: &[(String, String)],
        ) -> anyhow::Result<HashMap<(String, String), String>> {
            self.lookups.lock().unwrap().push(pairs.to_vec());
            // The real lookup matches the pair together: a document id under another
            // organisation is not found.
            Ok(pairs
                .iter()
                .filter_map(|pair| {
                    self.rows
                        .get(pair)
                        .map(|source| (pair.clone(), source.clone()))
                })
                .collect())
        }

        async fn done_units_by_source(
            &self,
            _org_ids: &[String],
        ) -> anyhow::Result<BTreeMap<String, u64>> {
            Ok(self.done_units.clone())
        }
    }

    fn point(id: &str, document_id: &str, org_id: &str) -> Stored {
        Stored {
            id: id.to_owned(),
            payload: BTreeMap::from([
                ("document_id".to_owned(), document_id.to_owned()),
                ("org_id".to_owned(), org_id.to_owned()),
                ("knowledge_id".to_owned(), id.to_owned()),
                ("text".to_owned(), "never read by the tool".to_owned()),
            ]),
        }
    }

    fn stamped(mut point: Stored, source: &str) -> Stored {
        point.payload.insert("source".to_owned(), source.to_owned());
        point
    }

    fn options(max_points: usize) -> StampOptions<'static> {
        StampOptions {
            org_ids: &[],
            dry_run: false,
            max_points,
            offset: None,
        }
    }

    /// The corpus the tests start from: four documents of three sources in one organisation,
    /// and a fifth in another.
    fn corpus() -> (FakePoints, FakeDocuments) {
        let points = FakePoints::with(vec![
            point("p01", "doc-s1", "org-1"),
            point("p02", "doc-s1", "org-1"),
            point("p03", "doc-s2", "org-1"),
            point("p04", "doc-q1", "org-1"),
            point("p05", "doc-u1", "org-1"),
            point("p06", "doc-o1", "org-2"),
        ]);
        let documents = FakeDocuments::with(&[
            ("doc-s1", "org-1", "sharepoint"),
            ("doc-s2", "org-1", "sharepoint"),
            ("doc-q1", "org-1", "quarry"),
            ("doc-u1", "org-1", "chat-upload"),
            ("doc-o1", "org-2", "sharepoint"),
        ]);
        (points, documents)
    }

    // ── the run ──

    #[tokio::test]
    async fn every_unstamped_point_gets_the_source_of_its_document() {
        let (points, documents) = corpus();

        let summary = stamp_missing(&points, &documents, &options(1000))
            .await
            .expect("the run succeeds");

        for (id, source) in [
            ("p01", "sharepoint"),
            ("p02", "sharepoint"),
            ("p03", "sharepoint"),
            ("p04", "quarry"),
            ("p05", "chat-upload"),
            ("p06", "sharepoint"),
        ] {
            assert_eq!(points.source_of(id).as_deref(), Some(source), "{id}");
        }
        assert_eq!(summary.examined, 6);
        assert_eq!(summary.stamped, 6);
        assert_eq!(
            summary.by_source,
            BTreeMap::from([
                ("chat-upload".to_owned(), 1),
                ("quarry".to_owned(), 1),
                ("sharepoint".to_owned(), 4),
            ])
        );
        assert_eq!(summary.skipped(), 0);
        assert!(!summary.truncated);
        assert_eq!(summary.resume_offset, None);
    }

    /// The tool writes the key `source` and nothing else, and no point's `org_id` (or any
    /// other value) differs afterwards.
    #[tokio::test]
    async fn only_the_source_key_is_written_and_nothing_else_about_a_point_changes() {
        let (points, documents) = corpus();
        let before = points.stored();

        stamp_missing(&points, &documents, &options(1000))
            .await
            .expect("the run succeeds");

        let writes = points.writes();
        assert!(!writes.is_empty());
        assert!(
            writes.iter().all(|write| write.keys == ["source"]),
            "a request named another key: {writes:?}"
        );
        for (was, now) in before.iter().zip(points.stored()) {
            assert_eq!(was.id, now.id);
            let mut without_source = now.payload.clone();
            let added = without_source.remove("source");
            assert!(added.is_some(), "{} was not stamped", now.id);
            assert_eq!(
                was.payload, without_source,
                "{}: more than the source changed",
                now.id
            );
            assert_eq!(was.payload["org_id"], now.payload["org_id"]);
        }
    }

    /// Points that already have a source are not scrolled and not written, whatever
    /// Postgres says now.
    #[tokio::test]
    async fn a_point_that_already_has_a_source_is_never_touched() {
        let points = FakePoints::with(vec![
            stamped(point("p01", "doc-a", "org-1"), "sharepoint"),
            point("p02", "doc-a", "org-1"),
        ]);
        // Postgres now says the document is a quarry one; the stamp that is there stays.
        let documents = FakeDocuments::with(&[("doc-a", "org-1", "quarry")]);

        let summary = stamp_missing(&points, &documents, &options(1000))
            .await
            .expect("the run succeeds");

        assert_eq!(points.source_of("p01").as_deref(), Some("sharepoint"));
        assert_eq!(points.source_of("p02").as_deref(), Some("quarry"));
        assert_eq!(
            summary.examined, 1,
            "the stamped point was not even scrolled"
        );
        assert_eq!(points.writes().len(), 1);
        assert_eq!(points.writes()[0].ids, ["p02"]);
    }

    #[tokio::test]
    async fn a_second_run_writes_nothing() {
        let (points, documents) = corpus();
        stamp_missing(&points, &documents, &options(1000))
            .await
            .expect("the first run succeeds");
        let writes_after_first = points.writes().len();
        let stored_after_first = points.stored();

        let second = stamp_missing(&points, &documents, &options(1000))
            .await
            .expect("the second run succeeds");

        assert_eq!(second.examined, 0);
        assert_eq!(second.stamped, 0);
        assert_eq!(
            points.writes().len(),
            writes_after_first,
            "no request at all"
        );
        assert_eq!(points.stored(), stored_after_first);
    }

    #[tokio::test]
    async fn a_dry_run_reads_and_reports_but_never_writes() {
        let (points, documents) = corpus();
        let before = points.stored();

        let summary = stamp_missing(
            &points,
            &documents,
            &StampOptions {
                dry_run: true,
                ..options(1000)
            },
        )
        .await
        .expect("the dry run succeeds");

        assert!(points.writes().is_empty(), "a dry run wrote");
        assert_eq!(points.stored(), before);
        assert_eq!(summary.examined, 6);
        assert_eq!(summary.stamped, 6, "what it would stamp");
        assert_eq!(summary.by_source["sharepoint"], 4);
        // And it reports it as what it would do.
        let json = summary.to_json("c", true);
        assert_eq!(json["would_stamp"], 6);
        assert!(json.get("stamped").is_none());
        assert_eq!(summary.to_json("c", false)["stamped"], 6);
    }

    // ── what it does not stamp ──

    #[tokio::test]
    async fn a_point_whose_document_is_not_in_its_organisation_is_left_alone() {
        let points = FakePoints::with(vec![
            point("p01", "doc-gone", "org-1"),
            // The document exists, but under another organisation: not this point's.
            point("p02", "doc-theirs", "org-1"),
            point("p03", "doc-mine", "org-1"),
        ]);
        let documents = FakeDocuments::with(&[
            ("doc-theirs", "org-2", "quarry"),
            ("doc-mine", "org-1", "sharepoint"),
        ]);

        let summary = stamp_missing(&points, &documents, &options(1000))
            .await
            .expect("the run succeeds");

        assert_eq!(points.source_of("p01"), None);
        assert_eq!(
            points.source_of("p02"),
            None,
            "another organisation's source"
        );
        assert_eq!(points.source_of("p03").as_deref(), Some("sharepoint"));
        assert_eq!(summary.skipped_document_not_found, 2);
        assert_eq!(summary.stamped, 1);
        // The lookup was by document AND organisation, both from the point.
        assert!(documents
            .lookups()
            .iter()
            .flatten()
            .any(|pair| pair == &("doc-theirs".to_owned(), "org-1".to_owned())));
    }

    #[tokio::test]
    async fn a_point_that_does_not_name_its_document_and_organisation_is_left_alone() {
        let mut no_document = point("p01", "doc-a", "org-1");
        no_document.payload.remove("document_id");
        let mut no_org = point("p02", "doc-a", "org-1");
        no_org.payload.remove("org_id");
        let mut empty_ids = point("p03", "", "org-1");
        empty_ids.payload.insert("org_id".to_owned(), String::new());
        let points = FakePoints::with(vec![no_document, no_org, empty_ids]);
        // Even a document that could be guessed at must not be.
        let documents = FakeDocuments::with(&[("doc-a", "org-1", "quarry")]);

        let summary = stamp_missing(&points, &documents, &options(1000))
            .await
            .expect("the run succeeds");

        assert!(points.writes().is_empty());
        assert_eq!(summary.skipped_without_identity, 3);
        assert_eq!(summary.stamped, 0);
        assert!(
            documents.lookups().iter().all(Vec::is_empty),
            "no lookup was made for a point with no identity"
        );
    }

    #[tokio::test]
    async fn a_blank_source_is_never_written() {
        let points = FakePoints::with(vec![
            point("p01", "doc-blank", "org-1"),
            point("p02", "doc-space", "org-1"),
        ]);
        let documents =
            FakeDocuments::with(&[("doc-blank", "org-1", ""), ("doc-space", "org-1", "   ")]);

        let summary = stamp_missing(&points, &documents, &options(1000))
            .await
            .expect("the run succeeds");

        assert!(points.writes().is_empty());
        assert_eq!(points.source_of("p01"), None);
        assert_eq!(summary.skipped_blank_source, 2);
    }

    #[tokio::test]
    async fn the_source_is_written_exactly_as_the_document_has_it() {
        let points = FakePoints::with(vec![point("p01", "doc-a", "org-1")]);
        let documents = FakeDocuments::with(&[("doc-a", "org-1", "Chat-Upload")]);

        stamp_missing(&points, &documents, &options(1000))
            .await
            .expect("the run succeeds");

        assert_eq!(points.source_of("p01").as_deref(), Some("Chat-Upload"));
    }

    // ── bounds, paging, scope ──

    #[tokio::test]
    async fn max_points_bounds_a_run_and_the_next_run_continues_where_it_stopped() {
        let ids: Vec<String> = (1..=7).map(|n| format!("p{n:02}")).collect();
        let points = FakePoints::with(ids.iter().map(|id| point(id, "doc-a", "org-1")).collect());
        let documents = FakeDocuments::with(&[("doc-a", "org-1", "quarry")]);

        let first = stamp_missing(&points, &documents, &options(3))
            .await
            .expect("the first run succeeds");

        assert_eq!(first.examined, 3);
        assert_eq!(first.stamped, 3);
        assert!(first.truncated);
        assert_eq!(first.resume_offset.as_deref(), Some("p04"));
        assert!(
            points.scrolls().iter().all(|(_, _, limit)| *limit <= 3),
            "a page larger than what was left of the bound: {:?}",
            points.scrolls()
        );

        // The next run starts from the offset it was given and takes the rest.
        let second = stamp_missing(
            &points,
            &documents,
            &StampOptions {
                offset: Some(PointId::from("p04")),
                ..options(3)
            },
        )
        .await
        .expect("the second run succeeds");
        assert_eq!(second.examined, 3);
        assert!(second.truncated);
        assert_eq!(second.resume_offset.as_deref(), Some("p07"));

        let third = stamp_missing(&points, &documents, &options(3))
            .await
            .expect("the third run succeeds");
        assert_eq!(
            third.examined, 1,
            "only the one left; the stamped ones are not scrolled"
        );
        assert!(!third.truncated, "it reached the end");
        for id in &ids {
            assert_eq!(points.source_of(id).as_deref(), Some("quarry"), "{id}");
        }
    }

    #[tokio::test]
    async fn a_run_that_ends_exactly_at_the_bound_is_not_reported_as_truncated() {
        let points = FakePoints::with(vec![
            point("p01", "doc-a", "org-1"),
            point("p02", "doc-a", "org-1"),
        ]);
        let documents = FakeDocuments::with(&[("doc-a", "org-1", "quarry")]);

        let summary = stamp_missing(&points, &documents, &options(2))
            .await
            .expect("the run succeeds");

        assert_eq!(summary.examined, 2);
        assert!(!summary.truncated);
        assert_eq!(summary.resume_offset, None);
    }

    #[tokio::test]
    async fn a_large_collection_is_paged_and_stamped_in_bounded_requests() {
        let count = PAGE * 2 + 40;
        let points = FakePoints::with(
            (0..count)
                .map(|n| point(&format!("p{n:05}"), "doc-a", "org-1"))
                .collect(),
        );
        let documents = FakeDocuments::with(&[("doc-a", "org-1", "quarry")]);

        let summary = stamp_missing(&points, &documents, &options(10_000))
            .await
            .expect("the run succeeds");

        assert_eq!(summary.examined, count);
        assert_eq!(summary.stamped, count);
        assert!(points.scrolls().len() >= 3);
        assert!(points.scrolls().iter().all(|(_, _, limit)| *limit <= PAGE));
        assert!(points.writes().iter().all(|write| write.ids.len() <= PAGE));
        assert!(
            (0..count).all(|n| points.source_of(&format!("p{n:05}")).as_deref() == Some("quarry"))
        );
    }

    #[tokio::test]
    async fn an_organisation_filter_restricts_what_is_scrolled_and_stamped() {
        let (points, documents) = corpus();

        let summary = stamp_missing(
            &points,
            &documents,
            &StampOptions {
                org_ids: &["org-2".to_owned()],
                ..options(1000)
            },
        )
        .await
        .expect("the run succeeds");

        assert_eq!(summary.examined, 1);
        assert_eq!(points.source_of("p06").as_deref(), Some("sharepoint"));
        for id in ["p01", "p02", "p03", "p04", "p05"] {
            assert_eq!(points.source_of(id), None, "{id} is another organisation's");
        }
        assert!(points
            .scrolls()
            .iter()
            .all(|(orgs, _, _)| orgs == &["org-2".to_owned()]));
    }

    // ── the report ──

    fn counted(by_source: &[(&str, u64)]) -> FakeDocuments {
        FakeDocuments {
            done_units: by_source
                .iter()
                .map(|(source, count)| ((*source).to_owned(), *count))
                .collect(),
            ..FakeDocuments::default()
        }
    }

    #[tokio::test]
    async fn the_report_is_consistent_after_the_backfill_and_not_before() {
        let (points, documents) = corpus();
        let postgres = counted(&[("sharepoint", 4), ("quarry", 1), ("chat-upload", 1)]);

        let before = report(&points, &postgres, &[]).await.expect("the report");
        assert_eq!(before.total_points, 6);
        assert_eq!(before.unstamped_points, 6);
        assert!(!before.consistent());

        stamp_missing(&points, &documents, &options(1000))
            .await
            .expect("the run succeeds");

        let after = report(&points, &postgres, &[]).await.expect("the report");
        assert_eq!(after.unstamped_points, 0);
        assert_eq!(after.stamped_with_unknown_source, 0);
        assert_eq!(after.by_source["sharepoint"], (4, 4));
        assert_eq!(after.by_source["quarry"], (1, 1));
        assert_eq!(after.by_source["chat-upload"], (1, 1));
        assert!(after.consistent());
        assert_eq!(after.to_json("c")["consistent"], true);
    }

    #[tokio::test]
    async fn the_report_shows_a_stamp_that_no_longer_matches_postgres() {
        // A point stamped `sharepoint` whose document has since become `quarry`.
        let points = FakePoints::with(vec![
            stamped(point("p01", "doc-a", "org-1"), "sharepoint"),
            stamped(point("p02", "doc-b", "org-1"), "retired-source"),
        ]);
        let postgres = counted(&[("quarry", 1), ("sharepoint", 0)]);

        let report = report(&points, &postgres, &[]).await.expect("the report");

        assert_eq!(report.by_source["quarry"], (0, 1));
        assert_eq!(report.by_source["sharepoint"], (1, 0));
        assert_eq!(
            report.stamped_with_unknown_source, 1,
            "a source no document has is counted, not lost"
        );
        assert!(!report.consistent());
    }

    // ── arguments ──

    fn parse(args: &[&str]) -> anyhow::Result<Parsed> {
        parse_args(
            args.iter().map(|arg| (*arg).to_owned()),
            "default_collection",
        )
    }

    #[test]
    fn the_defaults_are_a_bounded_real_run_on_the_default_collection() {
        assert_eq!(
            parse(&[]).unwrap(),
            Parsed::Run(Args {
                collection: "default_collection".to_owned(),
                org_ids: vec![],
                mode: Mode::Stamp {
                    dry_run: false,
                    max_points: DEFAULT_MAX_POINTS,
                    offset: None,
                },
            })
        );
    }

    #[test]
    fn the_options_are_read() {
        let Parsed::Run(args) = parse(&[
            "--dry-run",
            "--max-points",
            "50",
            "--org-id",
            "org-b",
            "--org-id",
            "org-a",
            "--org-id",
            "org-a",
            "--collection",
            "c",
            "--offset",
            "00d10181-6b85-1944-b1af-d3473bfff4cd",
        ])
        .unwrap() else {
            panic!("expected a run");
        };
        assert_eq!(args.collection, "c");
        assert_eq!(args.org_ids, ["org-a", "org-b"], "sorted and de-duplicated");
        assert_eq!(
            args.mode,
            Mode::Stamp {
                dry_run: true,
                max_points: 50,
                offset: Some(PointId::from("00d10181-6b85-1944-b1af-d3473bfff4cd")),
            }
        );
        assert_eq!(parse(&["--report"]).unwrap(), {
            Parsed::Run(Args {
                collection: "default_collection".to_owned(),
                org_ids: vec![],
                mode: Mode::Report,
            })
        });
        assert_eq!(parse(&["--help"]).unwrap(), Parsed::Help);
    }

    #[test]
    fn nonsense_is_refused() {
        for args in [
            &["--max-points", "0"][..],
            &["--max-points", "many"],
            &["--max-points"],
            &["--org-id"],
            &["--org-id", " "],
            &["--collection", ""],
            &["--offset", "not-an-id"],
            &["--bogus"],
            &["--report", "--dry-run"],
            &["--report", "--max-points", "5"],
            &["--report", "--offset", "7"],
        ] {
            assert!(parse(args).is_err(), "{args:?} was accepted");
        }
        // A numeric point id is a point id.
        assert!(parse(&["--offset", "7"]).is_ok());
    }

    // ── the requests ──

    fn must_of(filter: &Filter) -> &[Condition] {
        &filter.must
    }

    #[test]
    fn the_scroll_asks_for_points_with_no_source_in_the_requested_organisations() {
        use qdrant_client::qdrant::condition::ConditionOneOf;

        let filter = unstamped_filter(&["org-a".to_owned(), "org-b".to_owned()]);
        let must = must_of(&filter);
        assert_eq!(must.len(), 2);
        assert!(matches!(
            &must[0].condition_one_of,
            Some(ConditionOneOf::IsEmpty(empty)) if empty.key == "source"
        ));
        match &must[1].condition_one_of {
            Some(ConditionOneOf::Field(field)) => {
                assert_eq!(field.key, "org_id");
                assert!(matches!(
                    field.r#match.as_ref().and_then(|m| m.match_value.as_ref()),
                    Some(MatchValue::Keywords(keywords)) if keywords.strings == ["org-a", "org-b"]
                ));
            }
            other => panic!("expected an org condition, got {other:?}"),
        }
        // No organisation filter: only the source condition.
        assert_eq!(must_of(&unstamped_filter(&[])).len(), 1);
    }

    #[test]
    fn the_stamp_request_sets_one_key_on_exactly_the_named_points() {
        let ids = [PointId::from("p01"), PointId::from("p02")];
        let request = stamp_request("coll", &ids, "quarry");

        assert_eq!(request.collection_name, "coll");
        assert_eq!(
            request.wait,
            Some(true),
            "the write is confirmed before the next"
        );
        assert_eq!(request.payload.len(), 1);
        assert_eq!(
            request
                .payload
                .get("source")
                .and_then(QdrantValue::as_str)
                .map(String::as_str),
            Some("quarry")
        );
        // By id: not by a filter that could match more than was examined.
        match request
            .points_selector
            .and_then(|s| s.points_selector_one_of)
        {
            Some(PointsSelectorOneOf::Points(list)) => {
                assert_eq!(
                    list.ids.iter().map(id_key).collect::<Vec<_>>(),
                    ["p01", "p02"]
                );
            }
            other => panic!("expected an id selector, got {other:?}"),
        }
        assert_eq!(
            request.key, None,
            "the whole payload object, not a nested path"
        );
    }

    #[test]
    fn the_counts_are_over_the_requested_organisations() {
        assert!(count_filter(&[], Count::All).is_none());
        let orgs = ["org-a".to_owned()];
        assert_eq!(count_filter(&orgs, Count::All).unwrap().must.len(), 1);
        assert_eq!(count_filter(&orgs, Count::Unstamped).unwrap().must.len(), 2);
        assert_eq!(
            count_filter(&[], Count::Source("quarry"))
                .unwrap()
                .must
                .len(),
            1
        );
    }

    // ── against a real Qdrant ──

    /// The real adapter, against a real Qdrant: the filter, the scroll offsets, the
    /// `set_payload` request and the counts do what the fakes above assume.
    ///
    /// Gated behind `TEST_QDRANT_URL` (a disposable Qdrant's gRPC endpoint, e.g.
    /// `http://localhost:16334`). The collection is created and dropped by the test and
    /// carries a unique name, so it never touches a real collection.
    #[tokio::test]
    #[ignore = "requires TEST_QDRANT_URL pointing to a disposable Qdrant instance"]
    async fn a_real_qdrant_is_stamped_only_where_a_source_is_missing_and_org_id_is_kept() {
        use qdrant_client::qdrant::{
            CreateCollectionBuilder, Distance, PointStruct, UpsertPointsBuilder,
            VectorParamsBuilder,
        };

        let url = std::env::var("TEST_QDRANT_URL")
            .expect("TEST_QDRANT_URL must point to a disposable Qdrant instance");
        let client = Qdrant::from_url(&url)
            .build()
            .expect("connect disposable qdrant");
        let name = format!("source_backfill_test_{}", uuid::Uuid::new_v4().simple());
        client
            .create_collection(
                CreateCollectionBuilder::new(&name)
                    .vectors_config(VectorParamsBuilder::new(4, Distance::Cosine)),
            )
            .await
            .expect("create the scratch collection");

        let text = |value: &str| QdrantValue {
            kind: Some(QdrantKind::StringValue(value.to_owned())),
        };
        let mut seeded = Vec::new();
        for (n, document, org, source) in [
            (1u64, "doc-q", "org-a", None),
            (2, "doc-q", "org-a", None),
            (3, "doc-s", "org-a", Some("sharepoint")),
            (4, "doc-u", "org-b", None),
            (5, "doc-gone", "org-a", None),
        ] {
            let mut payload: HashMap<String, QdrantValue> = HashMap::from([
                ("document_id".to_owned(), text(document)),
                ("org_id".to_owned(), text(org)),
                ("text".to_owned(), text("kept")),
            ]);
            if let Some(source) = source {
                payload.insert("source".to_owned(), text(source));
            }
            seeded.push(PointStruct::new(n, vec![0.1, 0.2, 0.3, 0.4], payload));
        }
        client
            .upsert_points(UpsertPointsBuilder::new(&name, seeded).wait(true))
            .await
            .expect("seed the scratch collection");

        let points = QdrantCollection {
            client: &client,
            name: &name,
        };
        let documents = FakeDocuments::with(&[
            ("doc-q", "org-a", "quarry"),
            // The stamp that is there says sharepoint; it must survive.
            ("doc-s", "org-a", "quarry"),
            ("doc-u", "org-b", "chat-upload"),
        ]);

        let outcome = async {
            let first = stamp_missing(&points, &documents, &options(100)).await?;
            assert_eq!(first.examined, 4, "the stamped point is not scrolled");
            assert_eq!(first.stamped, 3);
            assert_eq!(first.skipped_document_not_found, 1);

            let second = stamp_missing(&points, &documents, &options(100)).await?;
            assert_eq!(second.stamped, 0);

            assert_eq!(points.count(&[], Count::Source("quarry")).await?, 2);
            assert_eq!(points.count(&[], Count::Source("sharepoint")).await?, 1);
            assert_eq!(points.count(&[], Count::Source("chat-upload")).await?, 1);
            assert_eq!(points.count(&[], Count::Unstamped).await?, 1);
            assert_eq!(points.count(&["org-b".to_owned()], Count::All).await?, 1);

            // Everything else about a stamped point is as it was seeded.
            let page = client
                .scroll(
                    ScrollPointsBuilder::new(&name)
                        .filter(Filter::must([Condition::matches(
                            "document_id",
                            "doc-u".to_owned(),
                        )]))
                        .with_payload(true),
                )
                .await?;
            let payload = &page.result[0].payload;
            assert_eq!(
                payload["org_id"].as_str().map(String::as_str),
                Some("org-b")
            );
            assert_eq!(payload["text"].as_str().map(String::as_str), Some("kept"));
            assert_eq!(
                payload["source"].as_str().map(String::as_str),
                Some("chat-upload")
            );
            anyhow::Ok(())
        }
        .await;

        client
            .delete_collection(&name)
            .await
            .expect("drop the scratch collection");
        outcome.expect("the real-Qdrant run");
    }
}
