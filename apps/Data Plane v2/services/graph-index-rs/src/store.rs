use sqlx::PgPool;
use uuid::Uuid;

use crate::model::{
    Claim, Community, Entity, ExtractionResult, GdprPurgeSummary, MirrorEntity, MirrorRelationship,
    PersistedExtraction, Relationship,
};

/// Stable namespace for every deterministic graph identifier in this plane.
///
/// **Fixed forever.** Changing these bytes re-keys every entity, relationship
/// and claim, orphaning `graph_text_units` / `graph_communities` references and
/// the Neo4j mirror's `entity_id` unique constraint.
const GRAPH_ID_NAMESPACE: Uuid = Uuid::from_bytes([
    0xd9, 0x2f, 0x41, 0x6c, 0x7a, 0x8b, 0x4e, 0x51, 0x9c, 0x0d, 0x1e, 0x7f, 0x3a, 0x62, 0xb4, 0x08,
]);

/// Field separator for composite identity keys.
///
/// U+001F (unit separator) cannot survive [`normalize_identity`] — it is
/// whitespace-stripped — so it can never appear inside a field. That keeps the
/// boundary unambiguous: without it, `("ab", "c")` and `("a", "bc")` would hash
/// to the same identifier.
const KEY_SEP: char = '\u{1f}';

/// Canonical form used for *identity only* — never for display.
///
/// Trims, collapses internal whitespace, and lowercases, so `"Triodelab"`,
/// `"  triodelab "` and `"Triodelab\n"` all resolve to a single node. The
/// first-seen spelling is kept as the stored `entity_text`.
///
/// Deliberately conservative: this is exact-match-after-normalisation, not
/// alias resolution. Merging `"Sarah Chen"` with `"SC"` needs the embedding
/// clustering stage (plan P1-2 step 2) and must not be faked here.
fn normalize_identity(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn deterministic_id(parts: &[&str]) -> String {
    let key = parts.join(&KEY_SEP.to_string());
    Uuid::new_v5(&GRAPH_ID_NAMESPACE, key.as_bytes()).to_string()
}

/// Removes duplicates while keeping the first occurrence's position.
/// `Vec::dedup` only collapses *adjacent* equals, which is not enough here.
fn dedupe_preserving_order(ids: &mut Vec<String>) {
    let mut seen = std::collections::HashSet::new();
    ids.retain(|id| seen.insert(id.clone()));
}

/// The closed entity-type ontology (P1-3).
///
/// Measured 2026-08-05: the free-text `entity_type` field had produced **61
/// distinct types for 547 entities** — the extractor invents a new one almost
/// per call. The cost is not cosmetic: `Aquatiq` existed 13× as `Organization`
/// and 9× as `Company`, so deterministic identity alone (P1-2) could not merge
/// it, and multi-hop traversal saw two unrelated nodes.
///
/// `Concept` is a deliberate explicit catch-all: without a documented escape
/// hatch the model invents a new label whenever nothing fits, which is exactly
/// the failure being removed.
pub const ENTITY_TYPES: [&str; 18] = [
    "Organization",
    "Person",
    "Location",
    "Product",
    "Service",
    "Process",
    "Substance",
    "Organism",
    "Standard",
    "Document",
    "Technology",
    "Training",
    "Event",
    "ContactPoint",
    "Date",
    "Metric",
    "Industry",
    "Concept",
];

/// Reduces a free-text type to its lookup form: lowercased, punctuation and
/// separators dropped, so `"Phone Number"`, `"phone_number"` and
/// `"phone-number"` all collapse to `phonenumber`.
fn type_lookup_key(raw: &str) -> String {
    raw.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// Maps an extractor-supplied type onto [`ENTITY_TYPES`].
///
/// Authoritative and applied at persistence rather than in the extractor, for
/// two reasons: the extraction "schema" is only prompt text (no provider-side
/// enum is enforced, and the Anthropic path drops structured-output schemas
/// entirely), and both the `model_plane` and `azure_openai` backends must be
/// constrained identically. Anything unrecognised becomes `Concept` — never
/// passed through, or the ontology would silently reopen.
pub fn canonical_entity_type(raw: &str) -> &'static str {
    match type_lookup_key(raw).as_str() {
        "organization" | "organisation" | "company" | "companies" | "corporation" | "team"
        | "group" | "institution" | "department" | "partner" | "supplier" | "vendor"
        | "customer" | "client" | "brand" | "business" | "employer" => "Organization",

        "person" | "people" | "individual" | "employee" | "staff" | "author" | "contactperson"
        | "role" | "jobtitle" | "position" => "Person",

        "location" | "country" | "city" | "region" | "place" | "site" | "address" | "facility"
        | "area" | "venue" | "premises" => "Location",

        "product" | "products" | "goods" | "item" | "equipment" | "machine" | "machinery"
        | "device" | "hardware" | "component" | "part" => "Product",

        "service" | "services" | "offering" | "solution" | "solutions" | "consulting" => "Service",

        "process" | "procedure" | "method" | "methodology" | "action" | "function" | "activity"
        | "workflow" | "operation" | "practice" | "task" | "step" | "treatment" => "Process",

        "substance" | "chemical" | "chemicals" | "material" | "materials" | "gas" | "liquid"
        | "compound" | "ingredient" | "agent" | "detergent" | "disinfectant" | "sample" => {
            "Substance"
        }

        "organism" | "microorganism" | "microorganisms" | "pathogen" | "pathogens" | "bacteria"
        | "bacterium" | "virus" | "fungus" | "fungi" | "mould" | "mold" | "yeast" | "animal"
        | "species" | "allergen" | "insect" | "pest" => "Organism",

        "standard" | "standards" | "guideline" | "guidelines" | "regulation" | "regulations"
        | "certification" | "accreditation" | "membership" | "requirement" | "policy" | "law"
        | "directive" | "legislation" | "compliance" | "specification" => "Standard",

        "document" | "documents" | "file" | "files" | "image" | "images" | "page" | "report"
        | "article" | "form" | "record" | "template" | "contract" | "agreement" | "declaration"
        | "certificate" | "manual" | "publication" => "Document",

        "technology" | "software" | "platform" | "application" | "app" | "website" | "websites"
        | "system" | "tool" | "database" | "integration" | "api" | "socialmedia" | "channel" => {
            "Technology"
        }

        "training" | "course" | "courses" | "workshop" | "seminar" | "education" | "programme"
        | "program" | "curriculum" | "lesson" | "webinar" => "Training",

        "event" | "events" | "expo" | "exhibition" | "conference" | "fair" | "meeting"
        | "campaign" | "launch" | "milestone" => "Event",

        "contactpoint" | "contact" | "contactinformation" | "contactinfo" | "contactdetails"
        | "phone" | "phonenumber" | "telephone" | "mobile" | "email" | "emailaddress" | "fax"
        | "url" | "link" => "ContactPoint",

        "date" | "dates" | "time" | "datetime" | "timestamp" | "period" | "year" | "month"
        | "deadline" | "duration" | "schedule" => "Date",

        "metric" | "measurement" | "value" | "property" | "quantity" | "number" | "price"
        | "cost" | "percentage" | "amount" | "rate" | "score" | "result" | "outcome"
        | "statistic" | "kpi" | "temperature" | "threshold" | "limit" => "Metric",

        // `Industry` earns its own type rather than falling into `Concept`: it
        // was the 4th most common observed label (32 rows) and is a useful
        // retrieval facet for this domain (aquaculture, food processing…).
        // Without it, `Concept` absorbed 25% of all entities — a catch-all that
        // large stops being an escape hatch and becomes an untyped bucket.
        "industry" | "industries" | "sector" | "market" | "field" | "domain" | "vertical"
        | "valuechain" | "segment" => "Industry",

        // Everything else — including the observed `Claim`, which was a modelling
        // error (claims are their own table, not entities), plus `Risk`,
        // `Warning`, `Surface`, `Experience`, `Collaboration`.
        _ => "Concept",
    }
}

/// Identity of an entity: organisation + normalised text + **canonical** type.
///
/// Canonicalising the type here is what lets `Aquatiq/Organization` and
/// `Aquatiq/Company` resolve to one node.
pub fn entity_identity(org_id: &str, entity_text: &str, entity_type: &str) -> String {
    deterministic_id(&[
        org_id,
        &normalize_identity(entity_text),
        &normalize_identity(canonical_entity_type(entity_type)),
    ])
}

/// Identity of a relationship: its two (already deterministic) endpoints plus
/// the normalised relation type. Direction is significant.
pub fn relationship_identity(
    org_id: &str,
    entity_a_id: &str,
    entity_b_id: &str,
    relation_type: &str,
) -> String {
    deterministic_id(&[
        org_id,
        entity_a_id,
        entity_b_id,
        &normalize_identity(relation_type),
    ])
}

/// Identity of a claim: organisation + normalised claim text.
///
/// Org-wide rather than per-chunk on purpose — the same assertion found in two
/// chunks becomes one claim accumulating both `source_refs`, which is what
/// contradiction detection needs to compare claims rather than duplicates.
pub fn claim_identity(org_id: &str, claim_text: &str) -> String {
    deterministic_id(&[org_id, &normalize_identity(claim_text)])
}

/// Identity of a community: organisation + its **member set**.
///
/// Communities are derived (connected components over the visible relationship
/// graph) and were previously keyed by `Uuid::new_v4()`, so every recompute —
/// which happens on **every ingest that adds a relationship** — minted brand-new
/// ids. Combined with `replace_communities`' DELETE-then-INSERT that made a
/// community's identity, and anything hung off it, disposable.
///
/// That is the blocker for summarisation (P1-5): an LLM-generated summary keyed
/// to a v4 id is destroyed on the next ingest and has to be paid for again.
/// Deriving the id from the member set means an unchanged community keeps its
/// id, so its summary survives.
///
/// Members are **sorted and deduplicated** first: `connected_components` yields
/// no stable ordering, so an unsorted key would hash differently run to run and
/// defeat the whole point.
pub fn community_identity(org_id: &str, entity_ids: &[String]) -> String {
    let mut members: Vec<&str> = entity_ids.iter().map(String::as_str).collect();
    members.sort_unstable();
    members.dedup();
    // Members joined with the same separator; entity ids are UUIDs so they can
    // never contain it.
    let joined = members.join(&KEY_SEP.to_string());
    deterministic_id(&[org_id, &joined])
}

pub struct GraphStore {
    pool: PgPool,
}

impl GraphStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Returns chunks only when their canonical document is live and visible
    /// to the whole verified organization. Private/shared grants are not a
    /// graph authorization source in the secure MVP.
    pub async fn load_org_visible_chunks(
        &self,
        org_id: &str,
        document_id: &str,
    ) -> anyhow::Result<Vec<(String, String)>> {
        // Phase 1 RLS: this path serves exactly one org (taken from the
        // verified caller claims), so it reads through an org-scoped
        // transaction. The SQL still binds `org_id` itself — the database
        // policy is a backstop against that filter being dropped or mis-edited
        // later, not a replacement for it.
        let mut tx = pg_org_scope::begin_org_scoped(&self.pool, org_id).await?;
        let rows = sqlx::query_as(
            "SELECT ku.knowledge_id, ku.text
             FROM knowledge_units AS ku
             JOIN documents AS d
               ON d.document_id = ku.document_id AND d.org_id = ku.org_id
             WHERE d.document_id = $1 AND d.org_id = $2
               AND d.visibility = 'org' AND d.deleted_at IS NULL",
        )
        .bind(document_id)
        .bind(org_id)
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(rows)
    }

    async fn knowledge_unit_is_org_visible(
        &self,
        org_id: &str,
        knowledge_id: &str,
    ) -> anyhow::Result<bool> {
        // Phase 1 RLS: single-org visibility probe, same rationale as
        // `load_org_visible_chunks` above.
        let mut tx = pg_org_scope::begin_org_scoped(&self.pool, org_id).await?;
        let (visible,): (bool,) = sqlx::query_as(
            "SELECT EXISTS (
                SELECT 1 FROM knowledge_units AS ku
                JOIN documents AS d
                  ON d.document_id = ku.document_id AND d.org_id = ku.org_id
                WHERE ku.knowledge_id = $1 AND ku.org_id = $2
                  AND d.visibility = 'org' AND d.deleted_at IS NULL
             )",
        )
        .bind(knowledge_id)
        .bind(org_id)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(visible)
    }

    pub async fn persist_extraction(
        &self,
        org_id: &str,
        knowledge_id: &str,
        result: &ExtractionResult,
    ) -> anyhow::Result<PersistedExtraction> {
        if !self
            .knowledge_unit_is_org_visible(org_id, knowledge_id)
            .await?
        {
            // Not org-visible → persist nothing. The empty result means the
            // Neo4j mirror is a no-op too, so the read-model inherits this gate
            // (and the upstream restrictive-ZDR drop) with no extra code.
            return Ok(PersistedExtraction::default());
        }
        let source_ref = serde_json::json!([knowledge_id]);

        // Phase 1 RLS: every write below belongs to the single org this
        // extraction was produced for, so they all share ONE scoped
        // transaction rather than paying the set_config/SET LOCAL ROLE round
        // trip per entity, relationship and claim. Each statement still binds
        // `org_id` itself — the database policy is a backstop, not a
        // replacement for the explicit column.
        let mut tx = pg_org_scope::begin_org_scoped(&self.pool, org_id).await?;

        let mut entity_ids = Vec::new();
        let mut mirror_entities = Vec::new();
        for e in &result.entities {
            // Deterministic identity: re-extracting the same text resolves onto
            // the existing node instead of minting a new one. Previously this
            // was `Uuid::new_v4()`, so the `ON CONFLICT` below could never fire
            // and every re-ingest duplicated the whole entity set (live: 547
            // entities over 276 text units).
            let id = entity_identity(org_id, &e.entity_text, &e.entity_type);
            // Persist the canonical type, not the extractor's free-text label —
            // otherwise the identity would merge correctly while the stored
            // `entity_type` kept whichever of 61 invented spellings arrived
            // first, and every type-filtered query stayed unreliable.
            let entity_type = canonical_entity_type(&e.entity_type);
            sqlx::query(
                // On conflict we now MERGE rather than discard: union the
                // source_refs so provenance accumulates across chunks, and keep
                // the highest confidence seen. `entity_text` is deliberately
                // NOT overwritten — the first-seen spelling stays the display
                // form, while identity is the normalised key.
                "INSERT INTO graph_entities (entity_id, org_id, entity_type, entity_text, confidence, provenance, source_refs)
                 VALUES ($1, $2, $3, $4, $5, 'extracted', $6)
                 ON CONFLICT (entity_id) DO UPDATE SET
                     confidence = GREATEST(graph_entities.confidence, EXCLUDED.confidence),
                     source_refs = COALESCE((
                         SELECT jsonb_agg(DISTINCT ref)
                         FROM jsonb_array_elements(
                             COALESCE(graph_entities.source_refs, '[]'::jsonb)
                             || COALESCE(EXCLUDED.source_refs, '[]'::jsonb)
                         ) AS ref
                     ), graph_entities.source_refs)"
            )
            .bind(&id)
            .bind(org_id)
            .bind(entity_type)
            .bind(&e.entity_text)
            .bind(e.confidence)
            .bind(&source_ref)
            .execute(&mut *tx)
            .await?;
            mirror_entities.push(MirrorEntity {
                entity_id: id.clone(),
                // Canonical here too, so the Neo4j read-model cannot drift from
                // the canonical Postgres graph on type.
                entity_type: entity_type.to_string(),
                entity_text: e.entity_text.clone(),
                confidence: e.confidence,
            });
            entity_ids.push(id);
        }

        let entity_map: std::collections::HashMap<&str, &str> = result
            .entities
            .iter()
            .zip(entity_ids.iter())
            .map(|(e, id)| (e.entity_text.as_str(), id.as_str()))
            .collect();

        let mut rel_ids = Vec::new();
        let mut mirror_relationships = Vec::new();
        for r in &result.relationships {
            let a_id = entity_map
                .get(r.source_entity.as_str())
                .copied()
                .unwrap_or("");
            let b_id = entity_map
                .get(r.target_entity.as_str())
                .copied()
                .unwrap_or("");
            if a_id.is_empty() || b_id.is_empty() {
                continue;
            }
            // Deterministic on both (already deterministic) endpoints plus the
            // normalised relation type, so the same edge re-extracted merges
            // instead of duplicating. Direction is significant.
            let id = relationship_identity(org_id, a_id, b_id, &r.relation_type);
            sqlx::query(
                "INSERT INTO graph_relationships (rel_id, org_id, entity_a_id, entity_b_id, relation_type, confidence, provenance, source_refs)
                 VALUES ($1, $2, $3, $4, $5, $6, 'extracted', $7)
                 ON CONFLICT (rel_id) DO UPDATE SET
                     confidence = GREATEST(graph_relationships.confidence, EXCLUDED.confidence),
                     source_refs = COALESCE((
                         SELECT jsonb_agg(DISTINCT ref)
                         FROM jsonb_array_elements(
                             COALESCE(graph_relationships.source_refs, '[]'::jsonb)
                             || COALESCE(EXCLUDED.source_refs, '[]'::jsonb)
                         ) AS ref
                     ), graph_relationships.source_refs)"
            )
            .bind(&id)
            .bind(org_id)
            .bind(a_id)
            .bind(b_id)
            .bind(&r.relation_type)
            .bind(r.confidence)
            .bind(&source_ref)
            .execute(&mut *tx)
            .await?;
            mirror_relationships.push(MirrorRelationship {
                rel_id: id.clone(),
                entity_a_id: a_id.to_string(),
                entity_b_id: b_id.to_string(),
                relation_type: r.relation_type.clone(),
                confidence: r.confidence,
            });
            rel_ids.push(id);
        }

        let mut claim_ids = Vec::new();
        for c in &result.claims {
            let linked: Vec<&str> = c
                .related_entities
                .iter()
                .filter_map(|name| entity_map.get(name.as_str()).copied())
                .collect();
            // Deterministic on the normalised claim text, org-wide: the same
            // assertion found in two chunks becomes ONE claim accumulating both
            // source_refs and both entity links, which is what contradiction
            // detection needs to compare claims rather than duplicates.
            //
            // `claim_status` is intentionally left untouched on conflict — once
            // something marks a claim superseded, re-extraction must not
            // silently resurrect it as 'active'.
            let id = claim_identity(org_id, &c.claim_text);
            sqlx::query(
                "INSERT INTO graph_claims (claim_id, org_id, claim_text, entity_ids, confidence, provenance, source_refs, claim_status)
                 VALUES ($1, $2, $3, $4, $5, 'extracted', $6, 'active')
                 ON CONFLICT (claim_id) DO UPDATE SET
                     confidence = GREATEST(graph_claims.confidence, EXCLUDED.confidence),
                     entity_ids = COALESCE((
                         SELECT jsonb_agg(DISTINCT eid)
                         FROM jsonb_array_elements(
                             COALESCE(graph_claims.entity_ids, '[]'::jsonb)
                             || COALESCE(EXCLUDED.entity_ids, '[]'::jsonb)
                         ) AS eid
                     ), graph_claims.entity_ids),
                     source_refs = COALESCE((
                         SELECT jsonb_agg(DISTINCT ref)
                         FROM jsonb_array_elements(
                             COALESCE(graph_claims.source_refs, '[]'::jsonb)
                             || COALESCE(EXCLUDED.source_refs, '[]'::jsonb)
                         ) AS ref
                     ), graph_claims.source_refs)"
            )
            .bind(&id)
            .bind(org_id)
            .bind(&c.claim_text)
            .bind(serde_json::json!(linked))
            .bind(c.confidence)
            .bind(&source_ref)
            .execute(&mut *tx)
            .await?;
            claim_ids.push(id);
        }

        tx.commit().await?;

        // Deterministic identity means one extraction can now yield the same id
        // twice — e.g. an LLM returning both "Sarah" and "sarah" normalises to a
        // single entity. The downstream writes are all idempotent
        // (`ON CONFLICT DO NOTHING` in `persist_text_unit_mappings`, `MERGE` in
        // Neo4j), so duplicates are harmless, but they cost a round-trip each.
        // Collapse them here, preserving first-seen order.
        dedupe_preserving_order(&mut entity_ids);
        dedupe_preserving_order(&mut rel_ids);
        dedupe_preserving_order(&mut claim_ids);
        mirror_entities.dedup_by(|a, b| a.entity_id == b.entity_id);
        mirror_relationships.dedup_by(|a, b| a.rel_id == b.rel_id);

        Ok(PersistedExtraction {
            entity_ids,
            rel_ids,
            claim_ids,
            mirror_entities,
            mirror_relationships,
        })
    }

    pub async fn persist_text_unit_mappings(
        &self,
        org_id: &str,
        knowledge_id: &str,
        entity_ids: &[String],
        relationship_ids: &[String],
        claim_ids: &[String],
    ) -> anyhow::Result<()> {
        if !self
            .knowledge_unit_is_org_visible(org_id, knowledge_id)
            .await?
        {
            return Ok(());
        }
        // Phase 1 RLS: one scoped transaction for all three mapping loops —
        // they write the same org's rows, so there is no reason to re-enter
        // the scope per id. Same backstop rationale as `persist_extraction`.
        let mut tx = pg_org_scope::begin_org_scoped(&self.pool, org_id).await?;
        for eid in entity_ids {
            sqlx::query(
                "INSERT INTO graph_text_units (org_id, knowledge_id, entity_id, rel_id, claim_id)
                 VALUES ($1, $2, $3, NULL, NULL)
                 ON CONFLICT DO NOTHING",
            )
            .bind(org_id)
            .bind(knowledge_id)
            .bind(eid)
            .execute(&mut *tx)
            .await?;
        }
        for rid in relationship_ids {
            sqlx::query(
                "INSERT INTO graph_text_units (org_id, knowledge_id, entity_id, rel_id, claim_id)
                 VALUES ($1, $2, NULL, $3, NULL)
                 ON CONFLICT DO NOTHING",
            )
            .bind(org_id)
            .bind(knowledge_id)
            .bind(rid)
            .execute(&mut *tx)
            .await?;
        }
        for cid in claim_ids {
            sqlx::query(
                "INSERT INTO graph_text_units (org_id, knowledge_id, entity_id, rel_id, claim_id)
                 VALUES ($1, $2, NULL, NULL, $3)
                 ON CONFLICT DO NOTHING",
            )
            .bind(org_id)
            .bind(knowledge_id)
            .bind(cid)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Removes the chunk→graph mappings for knowledge IDs orphaned by a content
    /// re-chunk (carried on knowledge.units.deleted). Graph retrieval joins
    /// through graph_text_units to live chunks, so dropping these mappings is
    /// enough to keep superseded content out of results; any now-unreferenced
    /// entities/relationships/claims are pruned by a separate GC pass.
    pub async fn delete_text_unit_mappings(
        &self,
        org_id: &str,
        knowledge_ids: &[String],
    ) -> anyhow::Result<u64> {
        if knowledge_ids.is_empty() {
            return Ok(0);
        }
        // Phase 1 RLS: a re-chunk event carries exactly one org, so this
        // targeted mapping delete runs inside an org-scoped transaction. Note
        // the contrast with `purge_organization_data` below, which stays
        // unscoped on purpose — see its doc comment.
        let mut tx = pg_org_scope::begin_org_scoped(&self.pool, org_id).await?;
        let result = sqlx::query(
            "DELETE FROM graph_text_units WHERE org_id = $1 AND knowledge_id = ANY($2)",
        )
        .bind(org_id)
        .bind(knowledge_ids)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(result.rows_affected())
    }

    /// GDPR organization-erasure hard-purge (see `crate::gdpr_nats`, which
    /// gates this to `subject_type == "organization"` events only). Deletes
    /// every org-scoped row THIS service owns for `org_id`, in one
    /// transaction, in FK-safe child-before-parent order. Does NOT touch
    /// `documents` or `knowledge_units` — those tables belong to
    /// documents-api-go / embedding-engine-rs / index-engine-rs and are
    /// read-only here (see `load_org_visible_chunks` above). There is no
    /// `graph_exports` table write path in this crate (the `/v1/graph/
    /// exports` endpoint in `api.rs` streams a snapshot back to the caller
    /// without persisting it), so it is intentionally excluded.
    ///
    /// Idempotent: every statement is `DELETE ... WHERE org_id = $1`, so a
    /// redelivered event (NATS at-least-once) matches zero rows the second
    /// time — not an error. Every statement binds `org_id` as a parameter;
    /// none is ever string-interpolated, so a purge for one org can never
    /// touch another org's rows.
    ///
    /// Phase 1 RLS: this purge deliberately keeps running on the plain
    /// (unscoped, superuser) pool while the rest of this file moved onto
    /// org-scoped transactions. **Failure here would be silent.** Under a
    /// scoped transaction a mis-scoped `DELETE` matches zero rows and reports
    /// success — indistinguishable from "nothing left to purge", which the
    /// idempotency contract above says is the *normal* outcome of a
    /// redelivered event. An erasure that quietly deletes nothing and returns
    /// `Ok` is a GDPR compliance failure no caller would notice, so converting
    /// it earns its own change with dedicated verification against a real
    /// database rather than riding along in a sweep. This mirrors the same
    /// decision, for the same reason, in
    /// `retrieval-engine-rs/src/gdpr/purge.rs`.
    pub async fn purge_organization_data(&self, org_id: &str) -> anyhow::Result<GdprPurgeSummary> {
        // Phase 1 RLS: unscoped on purpose — see the doc comment above.
        let mut tx = self.pool.begin().await?;

        // Bound to locals (rather than assigned onto a `default()` struct) so
        // the FK-driven delete order below stays explicit and cannot be
        // reordered by accident.
        //
        // graph_text_units references graph_entities/graph_relationships/
        // graph_claims via FK (ON DELETE CASCADE) — purging it first keeps
        // this scoped purge independent of that cascade rather than
        // relying on it, and matches the org-isolation safety requirement
        // that every statement scope strictly by the event's own org_id.
        let graph_text_units = sqlx::query("DELETE FROM graph_text_units WHERE org_id = $1")
            .bind(org_id)
            .execute(&mut *tx)
            .await?
            .rows_affected();

        let graph_relationships = sqlx::query("DELETE FROM graph_relationships WHERE org_id = $1")
            .bind(org_id)
            .execute(&mut *tx)
            .await?
            .rows_affected();

        let graph_claims = sqlx::query("DELETE FROM graph_claims WHERE org_id = $1")
            .bind(org_id)
            .execute(&mut *tx)
            .await?
            .rows_affected();

        let graph_communities = sqlx::query("DELETE FROM graph_communities WHERE org_id = $1")
            .bind(org_id)
            .execute(&mut *tx)
            .await?
            .rows_affected();

        // graph_entities last: graph_relationships.entity_a_id/entity_b_id
        // and graph_text_units.entity_id both reference it with ON DELETE
        // CASCADE, so it must be purged after both of its dependents above.
        let graph_entities = sqlx::query("DELETE FROM graph_entities WHERE org_id = $1")
            .bind(org_id)
            .execute(&mut *tx)
            .await?
            .rows_affected();

        tx.commit().await?;
        Ok(GdprPurgeSummary {
            graph_text_units,
            graph_relationships,
            graph_claims,
            graph_communities,
            graph_entities,
        })
    }

    pub async fn get_entity(
        &self,
        org_id: &str,
        entity_id: &str,
    ) -> anyhow::Result<Option<Entity>> {
        // Phase 1 RLS: single-org read, same rationale as
        // `load_org_visible_chunks` above.
        let mut tx = pg_org_scope::begin_org_scoped(&self.pool, org_id).await?;
        let row = sqlx::query_as::<_, (String, String, String, String, f64, String, serde_json::Value)>(
            "SELECT ge.entity_id, ge.org_id, ge.entity_type, ge.entity_text, COALESCE(ge.confidence, 0), COALESCE(ge.provenance, ''), COALESCE(ge.source_refs, '[]')
             FROM graph_entities AS ge WHERE ge.entity_id = $1 AND ge.org_id = $2
               AND EXISTS (
                 SELECT 1 FROM graph_text_units AS gtu
                 JOIN knowledge_units AS ku ON ku.knowledge_id = gtu.knowledge_id AND ku.org_id = gtu.org_id
                 JOIN documents AS d ON d.document_id = ku.document_id AND d.org_id = ku.org_id
                 WHERE gtu.entity_id = ge.entity_id AND gtu.org_id = ge.org_id
                   AND d.visibility = 'org' AND d.deleted_at IS NULL
               )"
        )
        .bind(entity_id)
        .bind(org_id)
        .fetch_optional(&mut *tx)
        .await?;
        tx.commit().await?;

        Ok(
            row.map(|(eid, oid, etype, etext, conf, prov, refs)| Entity {
                entity_id: eid,
                org_id: oid,
                entity_type: etype,
                entity_text: etext,
                confidence: conf,
                provenance: prov,
                source_refs: serde_json::from_value(refs).unwrap_or_default(),
            }),
        )
    }

    /// D4+D5 spec §3.1 — aggregate graph snapshot for one org.
    /// Returns `(nodes, edges, total_nodes, total_edges)`. Handler uses the
    /// totals vs limits to decide whether to return `200` or `413` so the
    /// caller knows to fall back to paginated endpoints.
    pub async fn snapshot_org_graph(
        &self,
        org_id: &str,
        limit_nodes: i32,
        limit_edges: i32,
    ) -> anyhow::Result<(Vec<Entity>, Vec<Relationship>, i64, i64)> {
        // Phase 1 RLS: one snapshot is one org's graph. All four statements
        // (node count, edge count, node page, edge page) share ONE scoped
        // transaction, which also means the totals and the returned pages are
        // read from a single consistent view instead of four separate ones.
        let mut tx = pg_org_scope::begin_org_scoped(&self.pool, org_id).await?;

        let (n_total,): (i64,) =
            sqlx::query_as(
                "SELECT COUNT(*) FROM graph_entities AS ge WHERE ge.org_id = $1
                 AND EXISTS (
                   SELECT 1 FROM graph_text_units AS gtu
                   JOIN knowledge_units AS ku ON ku.knowledge_id = gtu.knowledge_id AND ku.org_id = gtu.org_id
                   JOIN documents AS d ON d.document_id = ku.document_id AND d.org_id = ku.org_id
                   WHERE gtu.entity_id = ge.entity_id AND gtu.org_id = ge.org_id
                     AND d.visibility = 'org' AND d.deleted_at IS NULL
                 )",
            )
                .bind(org_id)
                .fetch_one(&mut *tx)
                .await?;

        let (e_total,): (i64,) =
            sqlx::query_as(
                "SELECT COUNT(*) FROM graph_relationships AS gr WHERE gr.org_id = $1
                 AND EXISTS (
                   SELECT 1 FROM graph_text_units AS gtu
                   JOIN knowledge_units AS ku ON ku.knowledge_id = gtu.knowledge_id AND ku.org_id = gtu.org_id
                   JOIN documents AS d ON d.document_id = ku.document_id AND d.org_id = ku.org_id
                   WHERE gtu.rel_id = gr.rel_id AND gtu.org_id = gr.org_id
                     AND d.visibility = 'org' AND d.deleted_at IS NULL
                 )",
            )
                .bind(org_id)
                .fetch_one(&mut *tx)
                .await?;

        let node_rows = sqlx::query_as::<_, (String, String, String, String, f64, String, serde_json::Value)>(
            "SELECT ge.entity_id, ge.org_id, ge.entity_type, ge.entity_text, COALESCE(ge.confidence, 0), COALESCE(ge.provenance, ''), COALESCE(ge.source_refs, '[]')
             FROM graph_entities AS ge WHERE ge.org_id = $1
               AND EXISTS (
                 SELECT 1 FROM graph_text_units AS gtu
                 JOIN knowledge_units AS ku ON ku.knowledge_id = gtu.knowledge_id AND ku.org_id = gtu.org_id
                 JOIN documents AS d ON d.document_id = ku.document_id AND d.org_id = ku.org_id
                 WHERE gtu.entity_id = ge.entity_id AND gtu.org_id = ge.org_id
                   AND d.visibility = 'org' AND d.deleted_at IS NULL
               )
             ORDER BY ge.created_at DESC LIMIT $2"
        )
        .bind(org_id)
        .bind(limit_nodes)
        .fetch_all(&mut *tx)
        .await?;

        let edge_rows = sqlx::query_as::<_, (String, String, String, String, String, f64, String, serde_json::Value)>(
            "SELECT gr.rel_id, gr.org_id, gr.entity_a_id, gr.entity_b_id, gr.relation_type, COALESCE(gr.confidence, 0), COALESCE(gr.provenance, ''), COALESCE(gr.source_refs, '[]')
             FROM graph_relationships AS gr WHERE gr.org_id = $1
               AND EXISTS (
                 SELECT 1 FROM graph_text_units AS gtu
                 JOIN knowledge_units AS ku ON ku.knowledge_id = gtu.knowledge_id AND ku.org_id = gtu.org_id
                 JOIN documents AS d ON d.document_id = ku.document_id AND d.org_id = ku.org_id
                 WHERE gtu.rel_id = gr.rel_id AND gtu.org_id = gr.org_id
                   AND d.visibility = 'org' AND d.deleted_at IS NULL
               )
             ORDER BY gr.created_at DESC LIMIT $2"
        )
        .bind(org_id)
        .bind(limit_edges)
        .fetch_all(&mut *tx)
        .await?;

        tx.commit().await?;

        let nodes = node_rows
            .into_iter()
            .map(|(eid, oid, etype, etext, conf, prov, refs)| Entity {
                entity_id: eid,
                org_id: oid,
                entity_type: etype,
                entity_text: etext,
                confidence: conf,
                provenance: prov,
                source_refs: serde_json::from_value(refs).unwrap_or_default(),
            })
            .collect();

        let edges = edge_rows
            .into_iter()
            .map(|(rid, oid, a, b, rt, conf, prov, refs)| Relationship {
                rel_id: rid,
                org_id: oid,
                entity_a_id: a,
                entity_b_id: b,
                relation_type: rt,
                confidence: conf,
                provenance: prov,
                source_refs: serde_json::from_value(refs).unwrap_or_default(),
            })
            .collect();

        Ok((nodes, edges, n_total, e_total))
    }

    pub async fn list_entities_by_type(
        &self,
        org_id: &str,
        entity_type: &str,
        limit: i32,
        offset: i32,
    ) -> anyhow::Result<(Vec<Entity>, i64)> {
        // Phase 1 RLS: single-org listing. The count and the page it describes
        // share ONE scoped transaction, so the reported total cannot come from
        // a different snapshot than the rows.
        let mut tx = pg_org_scope::begin_org_scoped(&self.pool, org_id).await?;

        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM graph_entities AS ge WHERE ge.org_id = $1 AND ge.entity_type = $2
             AND EXISTS (
               SELECT 1 FROM graph_text_units AS gtu
               JOIN knowledge_units AS ku ON ku.knowledge_id = gtu.knowledge_id AND ku.org_id = gtu.org_id
               JOIN documents AS d ON d.document_id = ku.document_id AND d.org_id = ku.org_id
               WHERE gtu.entity_id = ge.entity_id AND gtu.org_id = ge.org_id
                 AND d.visibility = 'org' AND d.deleted_at IS NULL
             )",
        )
        .bind(org_id)
        .bind(entity_type)
        .fetch_one(&mut *tx)
        .await?;

        let rows = sqlx::query_as::<_, (String, String, String, String, f64, String, serde_json::Value)>(
            "SELECT ge.entity_id, ge.org_id, ge.entity_type, ge.entity_text, COALESCE(ge.confidence, 0), COALESCE(ge.provenance, ''), COALESCE(ge.source_refs, '[]')
             FROM graph_entities AS ge WHERE ge.org_id = $1 AND ge.entity_type = $2
               AND EXISTS (
                 SELECT 1 FROM graph_text_units AS gtu
                 JOIN knowledge_units AS ku ON ku.knowledge_id = gtu.knowledge_id AND ku.org_id = gtu.org_id
                 JOIN documents AS d ON d.document_id = ku.document_id AND d.org_id = ku.org_id
                 WHERE gtu.entity_id = ge.entity_id AND gtu.org_id = ge.org_id
                   AND d.visibility = 'org' AND d.deleted_at IS NULL
               )
             ORDER BY ge.created_at DESC LIMIT $3 OFFSET $4"
        )
        .bind(org_id)
        .bind(entity_type)
        .bind(limit)
        .bind(offset)
        .fetch_all(&mut *tx)
        .await?;

        tx.commit().await?;

        let entities = rows
            .into_iter()
            .map(|(eid, oid, etype, etext, conf, prov, refs)| Entity {
                entity_id: eid,
                org_id: oid,
                entity_type: etype,
                entity_text: etext,
                confidence: conf,
                provenance: prov,
                source_refs: serde_json::from_value(refs).unwrap_or_default(),
            })
            .collect();

        Ok((entities, count))
    }

    pub async fn get_relationships(
        &self,
        org_id: &str,
        entity_id: &str,
        relation_type: Option<&str>,
    ) -> anyhow::Result<Vec<Relationship>> {
        // Phase 1 RLS: single-org read. The two branches below are the same
        // query with and without a relation-type filter, so exactly one of
        // them runs inside this scoped transaction.
        let mut tx = pg_org_scope::begin_org_scoped(&self.pool, org_id).await?;
        let rows = if let Some(rt) = relation_type {
            sqlx::query_as::<_, (String, String, String, String, String, f64, String, serde_json::Value)>(
                "SELECT gr.rel_id, gr.org_id, gr.entity_a_id, gr.entity_b_id, gr.relation_type, COALESCE(gr.confidence, 0), COALESCE(gr.provenance, ''), COALESCE(gr.source_refs, '[]')
                 FROM graph_relationships AS gr
                 WHERE gr.org_id = $1 AND (gr.entity_a_id = $2 OR gr.entity_b_id = $2) AND gr.relation_type = $3
                   AND EXISTS (
                     SELECT 1 FROM graph_text_units AS gtu
                     JOIN knowledge_units AS ku ON ku.knowledge_id = gtu.knowledge_id AND ku.org_id = gtu.org_id
                     JOIN documents AS d ON d.document_id = ku.document_id AND d.org_id = ku.org_id
                     WHERE gtu.rel_id = gr.rel_id AND gtu.org_id = gr.org_id
                       AND d.visibility = 'org' AND d.deleted_at IS NULL
                   )"
            )
            .bind(org_id)
            .bind(entity_id)
            .bind(rt)
            .fetch_all(&mut *tx)
            .await?
        } else {
            sqlx::query_as::<_, (String, String, String, String, String, f64, String, serde_json::Value)>(
                "SELECT gr.rel_id, gr.org_id, gr.entity_a_id, gr.entity_b_id, gr.relation_type, COALESCE(gr.confidence, 0), COALESCE(gr.provenance, ''), COALESCE(gr.source_refs, '[]')
                 FROM graph_relationships AS gr
                 WHERE gr.org_id = $1 AND (gr.entity_a_id = $2 OR gr.entity_b_id = $2)
                   AND EXISTS (
                     SELECT 1 FROM graph_text_units AS gtu
                     JOIN knowledge_units AS ku ON ku.knowledge_id = gtu.knowledge_id AND ku.org_id = gtu.org_id
                     JOIN documents AS d ON d.document_id = ku.document_id AND d.org_id = ku.org_id
                     WHERE gtu.rel_id = gr.rel_id AND gtu.org_id = gr.org_id
                       AND d.visibility = 'org' AND d.deleted_at IS NULL
                   )"
            )
            .bind(org_id)
            .bind(entity_id)
            .fetch_all(&mut *tx)
            .await?
        };
        tx.commit().await?;

        Ok(rows
            .into_iter()
            .map(|(rid, oid, a, b, rt, conf, prov, refs)| Relationship {
                rel_id: rid,
                org_id: oid,
                entity_a_id: a,
                entity_b_id: b,
                relation_type: rt,
                confidence: conf,
                provenance: prov,
                source_refs: serde_json::from_value(refs).unwrap_or_default(),
            })
            .collect())
    }

    pub async fn get_claims(
        &self,
        org_id: &str,
        entity_id: Option<&str>,
        status: Option<&str>,
    ) -> anyhow::Result<Vec<Claim>> {
        let mut q = String::from(
            "SELECT gc.claim_id, gc.org_id, gc.claim_text, COALESCE(gc.entity_ids, '[]'), COALESCE(gc.confidence, 0), COALESCE(gc.provenance, ''), COALESCE(gc.source_refs, '[]'), COALESCE(gc.contradicted_by_claim_ids, '[]'), COALESCE(gc.claim_status, 'active')
             FROM graph_claims AS gc WHERE gc.org_id = $1
               AND EXISTS (
                 SELECT 1 FROM graph_text_units AS gtu
                 JOIN knowledge_units AS ku ON ku.knowledge_id = gtu.knowledge_id AND ku.org_id = gtu.org_id
                 JOIN documents AS d ON d.document_id = ku.document_id AND d.org_id = ku.org_id
                 WHERE gtu.claim_id = gc.claim_id AND gtu.org_id = gc.org_id
                   AND d.visibility = 'org' AND d.deleted_at IS NULL
               )"
        );
        let mut params: Vec<String> = vec![org_id.to_string()];

        if let Some(eid) = entity_id {
            params.push(eid.to_string());
            q.push_str(&format!(" AND gc.entity_ids @> ${}::jsonb", params.len()));
        }
        if let Some(s) = status {
            params.push(s.to_string());
            q.push_str(&format!(" AND gc.claim_status = ${}", params.len()));
        }

        let mut query = sqlx::query_as::<
            _,
            (
                String,
                String,
                String,
                serde_json::Value,
                f64,
                String,
                serde_json::Value,
                serde_json::Value,
                String,
            ),
        >(&q);
        for p in &params {
            query = query.bind(p);
        }
        // Phase 1 RLS: single-org read. The optional entity/status filters are
        // appended as bind parameters above; `org_id` is always `$1`, and the
        // database policy backstops it either way.
        let mut tx = pg_org_scope::begin_org_scoped(&self.pool, org_id).await?;
        let rows = query.fetch_all(&mut *tx).await?;
        tx.commit().await?;

        Ok(rows
            .into_iter()
            .map(
                |(cid, oid, text, eids, conf, prov, refs, contra, st)| Claim {
                    claim_id: cid,
                    org_id: oid,
                    claim_text: text,
                    entity_ids: serde_json::from_value(eids).unwrap_or_default(),
                    confidence: conf,
                    provenance: prov,
                    source_refs: serde_json::from_value(refs).unwrap_or_default(),
                    contradicted_by: serde_json::from_value(contra).unwrap_or_default(),
                    status: st,
                },
            )
            .collect())
    }

    /// Detects and records contradictions among the claims just persisted
    /// (plan P1-4) — the missing writer for `contradicted_by_claim_ids` /
    /// `claim_status`.
    ///
    /// Candidate generation is what keeps this off an O(n²) scan: a claim is
    /// only ever compared against claims that **share at least one entity** with
    /// it, mirroring the `(entity, property)` key in the reference
    /// implementation. `MAX_CANDIDATES_PER_CLAIM` bounds the fan-out so a
    /// hub entity ("Aquatiq", linked to most claims) cannot make one ingest
    /// quadratic.
    ///
    /// Org-scoped throughout, and candidates are restricted to org-visible
    /// documents using the same join the read path applies — a claim sourced
    /// from a deleted or private document must not become evidence against a
    /// visible one.
    ///
    /// Writes are **symmetric and additive**: both claims gain the other in
    /// `contradicted_by_claim_ids` (unioned, never clobbered) and move to
    /// `claim_status = 'contradicted'`. Neither is superseded — see
    /// `contradiction.rs` for why auto-demotion needs a confidence signal this
    /// detector does not have.
    ///
    /// Returns the number of pairs recorded.
    pub async fn detect_claim_contradictions(
        &self,
        org_id: &str,
        claim_ids: &[String],
    ) -> anyhow::Result<usize> {
        /// Per-claim candidate cap. Hub entities link to most claims in a
        /// corpus, so this bounds worst-case comparisons per ingest.
        const MAX_CANDIDATES_PER_CLAIM: i64 = 200;

        if claim_ids.is_empty() {
            return Ok(0);
        }

        // Phase 1 RLS: the candidate-generation loop below is all reads for a
        // single org, so it shares ONE scoped transaction instead of
        // re-entering the scope twice per claim. It is committed before the
        // write transaction opens rather than merged into it: the existing
        // shape only takes a write transaction when there is actually
        // something to record, and folding the two together would hold a
        // read-write transaction open across the whole detection pass.
        let mut read_tx = pg_org_scope::begin_org_scoped(&self.pool, org_id).await?;

        let mut pairs: Vec<crate::contradiction::DetectedPair> = Vec::new();
        for claim_id in claim_ids {
            // The claim under test, with its entity links.
            let Some((claim_text, entity_ids)) = sqlx::query_as::<_, (String, serde_json::Value)>(
                "SELECT claim_text, COALESCE(entity_ids, '[]')
                 FROM graph_claims WHERE claim_id = $1 AND org_id = $2",
            )
            .bind(claim_id)
            .bind(org_id)
            .fetch_optional(&mut *read_tx)
            .await?
            else {
                continue;
            };

            let entity_list: Vec<String> = serde_json::from_value(entity_ids).unwrap_or_default();
            if entity_list.is_empty() {
                // No shared-entity anchor → no trustworthy candidate set. Better
                // to skip than to compare against the whole org's claims.
                continue;
            }

            // Candidates: other claims in this org sharing >=1 entity, sourced
            // from an org-visible live document.
            let candidates = sqlx::query_as::<_, (String, String)>(
                "SELECT DISTINCT gc.claim_id, gc.claim_text
                 FROM graph_claims AS gc
                 WHERE gc.org_id = $1
                   AND gc.claim_id <> $2
                   AND gc.entity_ids ?| $3
                   AND EXISTS (
                     SELECT 1 FROM graph_text_units AS gtu
                     JOIN knowledge_units AS ku
                       ON ku.knowledge_id = gtu.knowledge_id AND ku.org_id = gtu.org_id
                     JOIN documents AS d
                       ON d.document_id = ku.document_id AND d.org_id = ku.org_id
                     WHERE gtu.claim_id = gc.claim_id AND gtu.org_id = gc.org_id
                       AND d.visibility = 'org' AND d.deleted_at IS NULL
                   )
                 LIMIT $4",
            )
            .bind(org_id)
            .bind(claim_id)
            .bind(&entity_list)
            .bind(MAX_CANDIDATES_PER_CLAIM)
            .fetch_all(&mut *read_tx)
            .await?;

            pairs.extend(crate::contradiction::detect_against_candidates(
                &crate::contradiction::StructuralAdjudicator,
                claim_id,
                &claim_text,
                &candidates,
            ));
        }

        read_tx.commit().await?;

        if pairs.is_empty() {
            return Ok(0);
        }

        // Phase 1 RLS: the symmetric contradiction writes below are all for
        // this one org, so the transaction they already shared is now the
        // scoped one.
        let mut tx = pg_org_scope::begin_org_scoped(&self.pool, org_id).await?;
        for pair in &pairs {
            for (subject, other) in [
                (&pair.claim_a, &pair.claim_b),
                (&pair.claim_b, &pair.claim_a),
            ] {
                sqlx::query(
                    "UPDATE graph_claims SET
                         contradicted_by_claim_ids = COALESCE((
                             SELECT jsonb_agg(DISTINCT cid)
                             FROM jsonb_array_elements(
                                 COALESCE(contradicted_by_claim_ids, '[]'::jsonb)
                                 || jsonb_build_array($2::text)
                             ) AS cid
                         ), jsonb_build_array($2::text)),
                         claim_status = 'contradicted'
                     WHERE claim_id = $1 AND org_id = $3",
                )
                .bind(subject)
                .bind(other)
                .bind(org_id)
                .execute(&mut *tx)
                .await?;
            }
        }
        tx.commit().await?;

        Ok(pairs.len())
    }

    pub async fn get_contradictions(
        &self,
        org_id: &str,
        limit: i32,
        offset: i32,
    ) -> anyhow::Result<(Vec<Claim>, i64)> {
        // Phase 1 RLS: single-org listing. Count and page share ONE scoped
        // transaction, same rationale as `list_entities_by_type` above.
        let mut tx = pg_org_scope::begin_org_scoped(&self.pool, org_id).await?;

        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM graph_claims AS gc
             WHERE gc.org_id = $1 AND jsonb_array_length(COALESCE(gc.contradicted_by_claim_ids, '[]')) > 0
               AND EXISTS (
                 SELECT 1 FROM graph_text_units AS gtu
                 JOIN knowledge_units AS ku ON ku.knowledge_id = gtu.knowledge_id AND ku.org_id = gtu.org_id
                 JOIN documents AS d ON d.document_id = ku.document_id AND d.org_id = ku.org_id
                 WHERE gtu.claim_id = gc.claim_id AND gtu.org_id = gc.org_id
                   AND d.visibility = 'org' AND d.deleted_at IS NULL
               )"
        )
        .bind(org_id)
        .fetch_one(&mut *tx)
        .await?;

        let rows = sqlx::query_as::<_, (String, String, String, serde_json::Value, f64, String, serde_json::Value, serde_json::Value, String)>(
            "SELECT gc.claim_id, gc.org_id, gc.claim_text, COALESCE(gc.entity_ids, '[]'), COALESCE(gc.confidence, 0), COALESCE(gc.provenance, ''), COALESCE(gc.source_refs, '[]'), COALESCE(gc.contradicted_by_claim_ids, '[]'), COALESCE(gc.claim_status, 'active')
             FROM graph_claims AS gc
             WHERE gc.org_id = $1 AND jsonb_array_length(COALESCE(gc.contradicted_by_claim_ids, '[]')) > 0
               AND EXISTS (
                 SELECT 1 FROM graph_text_units AS gtu
                 JOIN knowledge_units AS ku ON ku.knowledge_id = gtu.knowledge_id AND ku.org_id = gtu.org_id
                 JOIN documents AS d ON d.document_id = ku.document_id AND d.org_id = ku.org_id
                 WHERE gtu.claim_id = gc.claim_id AND gtu.org_id = gc.org_id
                   AND d.visibility = 'org' AND d.deleted_at IS NULL
               )
             ORDER BY gc.created_at DESC LIMIT $2 OFFSET $3"
        )
        .bind(org_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&mut *tx)
        .await?;

        tx.commit().await?;

        let claims = rows
            .into_iter()
            .map(
                |(cid, oid, text, eids, conf, prov, refs, contra, st)| Claim {
                    claim_id: cid,
                    org_id: oid,
                    claim_text: text,
                    entity_ids: serde_json::from_value(eids).unwrap_or_default(),
                    confidence: conf,
                    provenance: prov,
                    source_refs: serde_json::from_value(refs).unwrap_or_default(),
                    contradicted_by: serde_json::from_value(contra).unwrap_or_default(),
                    status: st,
                },
            )
            .collect();

        Ok((claims, count))
    }

    /// All org entity ids whose provenance is live + org-visible (the same
    /// `graph_text_units` → `documents` gate every read uses). One bulk query —
    /// community detection must not do a per-entity N+1.
    pub async fn list_visible_entity_ids(&self, org_id: &str) -> anyhow::Result<Vec<String>> {
        // Phase 1 RLS: single-org bulk read backing community detection.
        let mut tx = pg_org_scope::begin_org_scoped(&self.pool, org_id).await?;
        let rows = sqlx::query_as::<_, (String,)>(
            "SELECT ge.entity_id FROM graph_entities AS ge
             WHERE ge.org_id = $1
               AND EXISTS (
                 SELECT 1 FROM graph_text_units AS gtu
                 JOIN knowledge_units AS ku ON ku.knowledge_id = gtu.knowledge_id AND ku.org_id = gtu.org_id
                 JOIN documents AS d ON d.document_id = ku.document_id AND d.org_id = ku.org_id
                 WHERE gtu.entity_id = ge.entity_id AND gtu.org_id = ge.org_id
                   AND d.visibility = 'org' AND d.deleted_at IS NULL
               )
             LIMIT 10000",
        )
        .bind(org_id)
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(rows.into_iter().map(|(id,)| id).collect())
    }

    /// All org relationship endpoint pairs with live + org-visible provenance.
    /// One bulk query backing the community adjacency build.
    pub async fn list_visible_relationship_pairs(
        &self,
        org_id: &str,
    ) -> anyhow::Result<Vec<(String, String)>> {
        // Phase 1 RLS: single-org bulk read, paired with
        // `list_visible_entity_ids` above.
        let mut tx = pg_org_scope::begin_org_scoped(&self.pool, org_id).await?;
        let rows = sqlx::query_as::<_, (String, String)>(
            "SELECT gr.entity_a_id, gr.entity_b_id FROM graph_relationships AS gr
             WHERE gr.org_id = $1
               AND EXISTS (
                 SELECT 1 FROM graph_text_units AS gtu
                 JOIN knowledge_units AS ku ON ku.knowledge_id = gtu.knowledge_id AND ku.org_id = gtu.org_id
                 JOIN documents AS d ON d.document_id = ku.document_id AND d.org_id = ku.org_id
                 WHERE gtu.rel_id = gr.rel_id AND gtu.org_id = gr.org_id
                   AND d.visibility = 'org' AND d.deleted_at IS NULL
               )
             LIMIT 50000",
        )
        .bind(org_id)
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(rows)
    }

    /// Atomically replaces the org's derived communities with a fresh detection
    /// run (communities are a derived artifact — delete-and-replace inside one
    /// transaction keeps re-detection idempotent at the set level and never
    /// leaves a half-written state). Membership provenance is inherited: every
    /// member entity id comes from the visibility-gated listing above, and the
    /// read side additionally requires the queried entity set to cover the
    /// community (`retrieval-engine` COMMUNITY_SUMMARY_SQL subset check).
    pub async fn replace_communities(
        &self,
        org_id: &str,
        communities: &[Community],
    ) -> anyhow::Result<()> {
        // Phase 1 RLS: this method already ran everything in one transaction
        // for atomicity; it is now the org-scoped one. The advisory lock, the
        // upserts and the prune below are all this org's rows, and each
        // statement still binds `org_id` itself — the database policy is a
        // backstop, not a replacement.
        let mut tx = pg_org_scope::begin_org_scoped(&self.pool, org_id).await?;
        // Per-org transaction advisory lock: serializes concurrent recomputes
        // (post-extraction consumer + rebuild endpoint, possibly across
        // replicas). Still required with deterministic ids — two overlapping
        // runs would otherwise interleave the upsert and the prune below, and a
        // run whose snapshot predates a concurrent commit could prune rows the
        // other just wrote. The lock releases on commit/rollback.
        // hashtextextended keeps the key stable across sessions.
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(org_id)
            .execute(&mut *tx)
            .await?;

        // Upsert rather than DELETE-then-INSERT (P1-5). Community ids are now
        // derived from the member set, so an unchanged community lands on the
        // same row — and `summary` is deliberately NOT in the UPDATE list, so a
        // generated summary survives every subsequent recompute. Overwriting it
        // with the incoming `None` is precisely the bug that made
        // summarisation unaffordable: detection runs on every ingest that adds
        // a relationship.
        let mut keep: Vec<String> = Vec::with_capacity(communities.len());
        for c in communities {
            keep.push(c.community_id.clone());
            sqlx::query(
                "INSERT INTO graph_communities (community_id, org_id, entity_ids, summary, level)
                 VALUES ($1, $2, $3, $4, $5)
                 ON CONFLICT (community_id) DO UPDATE SET
                     entity_ids = EXCLUDED.entity_ids,
                     level = EXCLUDED.level",
            )
            .bind(&c.community_id)
            .bind(org_id)
            .bind(serde_json::json!(c.entity_ids))
            .bind(&c.summary)
            .bind(c.level)
            .execute(&mut *tx)
            .await?;
        }

        // Prune communities that no longer exist for this org. An empty `keep`
        // correctly removes them all (`= ANY('{}')` is false for every row, so
        // the NOT matches everything) — that is the honest result when detection
        // finds no qualifying component.
        sqlx::query(
            "DELETE FROM graph_communities
             WHERE org_id = $1 AND NOT (community_id = ANY($2))",
        )
        .bind(org_id)
        .bind(&keep)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }

    /// Communities for `org_id` that have no summary yet, with their member
    /// entity labels (plan P1-5).
    ///
    /// Only `summary IS NULL` rows are returned, which is what makes
    /// summarisation **incremental**: the first pass covers the backlog, and
    /// every later pass only pays for communities that are genuinely new or whose
    /// membership changed (a membership change mints a new
    /// [`community_identity`], so it arrives without a summary).
    ///
    /// Labels are ordered by confidence descending so that
    /// `MAX_SUMMARY_LABELS` truncation in the summariser drops the weakest
    /// members rather than an arbitrary slice.
    pub async fn list_communities_needing_summary(
        &self,
        org_id: &str,
        limit: i64,
    ) -> anyhow::Result<Vec<(String, Vec<String>)>> {
        // Phase 1 RLS: single-org read. The correlated member-label subquery
        // joins `graph_entities` on `c.org_id`, which is the same org the
        // scope pins, so the policy narrows both sides consistently.
        let mut tx = pg_org_scope::begin_org_scoped(&self.pool, org_id).await?;
        let rows = sqlx::query_as::<_, (String, Vec<String>)>(
            "SELECT c.community_id,
                    COALESCE(
                        ARRAY(
                            SELECT e.entity_text
                            FROM graph_entities AS e
                            WHERE e.org_id = c.org_id
                              AND e.entity_id IN (
                                  SELECT jsonb_array_elements_text(c.entity_ids)
                              )
                            ORDER BY e.confidence DESC NULLS LAST, e.entity_text
                        ),
                        ARRAY[]::text[]
                    ) AS labels
             FROM graph_communities AS c
             WHERE c.org_id = $1 AND c.summary IS NULL
             ORDER BY jsonb_array_length(c.entity_ids) DESC
             LIMIT $2",
        )
        .bind(org_id)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        // A community whose members have all been deleted has nothing to
        // summarise; skip rather than sending an empty prompt.
        Ok(rows.into_iter().filter(|(_, l)| !l.is_empty()).collect())
    }

    /// Stores a generated community summary.
    ///
    /// Org-scoped and idempotent. Deliberately does **not** overwrite a summary
    /// that already exists (`AND summary IS NULL`): two overlapping recompute
    /// runs must not both pay for, and then race on, the same summary.
    pub async fn set_community_summary(
        &self,
        org_id: &str,
        community_id: &str,
        summary: &str,
    ) -> anyhow::Result<bool> {
        // Phase 1 RLS: single-org write. The `rows_affected() > 0` result stays
        // meaningful under the scope — the row is this org's or it does not
        // exist, which is exactly what the `summary IS NULL` guard already
        // treats as "someone else got there first".
        let mut tx = pg_org_scope::begin_org_scoped(&self.pool, org_id).await?;
        let result = sqlx::query(
            "UPDATE graph_communities SET summary = $3
             WHERE community_id = $2 AND org_id = $1 AND summary IS NULL",
        )
        .bind(org_id)
        .bind(community_id)
        .bind(summary)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn get_graph_expansion(
        &self,
        org_id: &str,
        seed_ids: &[String],
        max_hops: i32,
        max_entities: i32,
    ) -> anyhow::Result<(Vec<Entity>, Vec<Relationship>)> {
        let mut visited: std::collections::HashSet<String> = seed_ids.iter().cloned().collect();
        let mut frontier: Vec<String> = seed_ids.to_vec();
        let mut all_entities = Vec::new();
        let mut all_rels = Vec::new();

        for _ in 0..max_hops {
            if frontier.is_empty() || all_entities.len() >= max_entities as usize {
                break;
            }
            let mut next_frontier = Vec::new();
            for eid in &frontier {
                let rels = self.get_relationships(org_id, eid, None).await?;
                for rel in &rels {
                    let neighbor = if rel.entity_a_id == *eid {
                        &rel.entity_b_id
                    } else {
                        &rel.entity_a_id
                    };
                    if !visited.contains(neighbor) {
                        visited.insert(neighbor.clone());
                        next_frontier.push(neighbor.clone());
                        if let Some(entity) = self.get_entity(org_id, neighbor).await? {
                            all_entities.push(entity);
                        }
                    }
                }
                all_rels.extend(rels);
            }
            frontier = next_frontier;
        }

        Ok((all_entities, all_rels))
    }

    /// Re-joins a set of entity ids (e.g. from a Neo4j traversal) against the
    /// canonical Postgres graph, returning only entities that are live and
    /// org-visible, plus the relationships whose BOTH endpoints are in the
    /// visible set. This is the security-critical gate that makes Neo4j a pure
    /// topology accelerator: even a stale or over-broad read-model cannot leak,
    /// because provenance/visibility is enforced here from canonical Postgres.
    pub async fn get_subgraph_visible(
        &self,
        org_id: &str,
        entity_ids: &[String],
    ) -> anyhow::Result<(Vec<Entity>, Vec<Relationship>)> {
        if entity_ids.is_empty() {
            return Ok((Vec::new(), Vec::new()));
        }

        // Phase 1 RLS: this is the gate that keeps Neo4j a pure topology
        // accelerator — the entity and relationship re-joins below both read
        // canonical Postgres for one org, so they share ONE scoped transaction
        // and the visibility set the edges are filtered against comes from the
        // same snapshot as the entities themselves.
        let mut tx = pg_org_scope::begin_org_scoped(&self.pool, org_id).await?;

        let entity_rows = sqlx::query_as::<_, (String, String, String, String, f64, String, serde_json::Value)>(
            "SELECT ge.entity_id, ge.org_id, ge.entity_type, ge.entity_text, COALESCE(ge.confidence, 0), COALESCE(ge.provenance, ''), COALESCE(ge.source_refs, '[]')
             FROM graph_entities AS ge
             WHERE ge.org_id = $1 AND ge.entity_id = ANY($2)
               AND EXISTS (
                 SELECT 1 FROM graph_text_units AS gtu
                 JOIN knowledge_units AS ku ON ku.knowledge_id = gtu.knowledge_id AND ku.org_id = gtu.org_id
                 JOIN documents AS d ON d.document_id = ku.document_id AND d.org_id = ku.org_id
                 WHERE gtu.entity_id = ge.entity_id AND gtu.org_id = ge.org_id
                   AND d.visibility = 'org' AND d.deleted_at IS NULL
               )"
        )
        .bind(org_id)
        .bind(entity_ids)
        .fetch_all(&mut *tx)
        .await?;

        let entities: Vec<Entity> = entity_rows
            .into_iter()
            .map(|(eid, oid, etype, etext, conf, prov, refs)| Entity {
                entity_id: eid,
                org_id: oid,
                entity_type: etype,
                entity_text: etext,
                confidence: conf,
                provenance: prov,
                source_refs: serde_json::from_value(refs).unwrap_or_default(),
            })
            .collect();

        // Only entities that survived the visibility gate may anchor an edge.
        let visible_ids: std::collections::HashSet<&str> =
            entities.iter().map(|e| e.entity_id.as_str()).collect();

        let rel_rows = sqlx::query_as::<_, (String, String, String, String, String, f64, String, serde_json::Value)>(
            "SELECT gr.rel_id, gr.org_id, gr.entity_a_id, gr.entity_b_id, gr.relation_type, COALESCE(gr.confidence, 0), COALESCE(gr.provenance, ''), COALESCE(gr.source_refs, '[]')
             FROM graph_relationships AS gr
             WHERE gr.org_id = $1 AND gr.entity_a_id = ANY($2) AND gr.entity_b_id = ANY($2)
               AND EXISTS (
                 SELECT 1 FROM graph_text_units AS gtu
                 JOIN knowledge_units AS ku ON ku.knowledge_id = gtu.knowledge_id AND ku.org_id = gtu.org_id
                 JOIN documents AS d ON d.document_id = ku.document_id AND d.org_id = ku.org_id
                 WHERE gtu.rel_id = gr.rel_id AND gtu.org_id = gr.org_id
                   AND d.visibility = 'org' AND d.deleted_at IS NULL
               )"
        )
        .bind(org_id)
        .bind(entity_ids)
        .fetch_all(&mut *tx)
        .await?;

        tx.commit().await?;

        let relationships: Vec<Relationship> = rel_rows
            .into_iter()
            .map(|(rid, oid, a, b, rt, conf, prov, refs)| Relationship {
                rel_id: rid,
                org_id: oid,
                entity_a_id: a,
                entity_b_id: b,
                relation_type: rt,
                confidence: conf,
                provenance: prov,
                source_refs: serde_json::from_value(refs).unwrap_or_default(),
            })
            .filter(|r| {
                visible_ids.contains(r.entity_a_id.as_str())
                    && visible_ids.contains(r.entity_b_id.as_str())
            })
            .collect();

        Ok((entities, relationships))
    }
}

#[cfg(test)]
mod identity_tests {
    use super::*;

    #[test]
    fn entity_identity_is_stable_across_runs() {
        // The whole point of P1-2: the same input must always yield the same id,
        // so re-extraction resolves onto the existing node.
        let a = entity_identity("org1", "Triodelab", "Organization");
        let b = entity_identity("org1", "Triodelab", "Organization");
        assert_eq!(a, b);
        assert_eq!(a.len(), 36, "expected a hyphenated UUID string");
    }

    #[test]
    fn entity_identity_normalises_case_and_whitespace() {
        let canonical = entity_identity("org1", "Triodelab", "Organization");
        for variant in [
            "triodelab",
            "  Triodelab  ",
            "TRIODELAB",
            "Triodelab\n",
            "Trio delab",
        ] {
            let id = entity_identity("org1", variant, "Organization");
            if variant == "Trio delab" {
                // Internal whitespace is collapsed, not removed — a different
                // word sequence stays a different entity.
                assert_ne!(canonical, id, "{variant} must not collapse into one word");
            } else {
                assert_eq!(canonical, id, "{variant} should normalise to canonical");
            }
        }
    }

    #[test]
    fn entity_identity_is_org_scoped_and_type_scoped() {
        let base = entity_identity("org1", "Acme", "Organization");
        assert_ne!(base, entity_identity("org2", "Acme", "Organization"));
        assert_ne!(base, entity_identity("org1", "Acme", "Person"));
    }

    #[test]
    fn field_separator_prevents_boundary_collisions() {
        // Without a separator that cannot occur inside a field, ("ab","c") and
        // ("a","bc") would hash identically.
        assert_ne!(
            entity_identity("org1", "ab", "c"),
            entity_identity("org1", "a", "bc")
        );
    }

    #[test]
    fn relationship_identity_is_directional_and_stable() {
        let a = entity_identity("org1", "Alice", "Person");
        let b = entity_identity("org1", "Acme", "Organization");
        let forward = relationship_identity("org1", &a, &b, "works_at");
        assert_eq!(forward, relationship_identity("org1", &a, &b, "WORKS_AT"));
        assert_ne!(
            forward,
            relationship_identity("org1", &b, &a, "works_at"),
            "direction must be significant"
        );
    }

    #[test]
    fn claim_identity_is_org_scoped_and_normalised() {
        let a = claim_identity("org1", "Coresystem uses HACCP");
        assert_eq!(a, claim_identity("org1", "  coresystem uses haccp "));
        assert_ne!(a, claim_identity("org2", "Coresystem uses HACCP"));
    }

    #[test]
    fn dedupe_preserves_first_seen_order() {
        let mut ids = vec![
            "c".to_string(),
            "a".to_string(),
            "c".to_string(),
            "b".to_string(),
            "a".to_string(),
        ];
        dedupe_preserving_order(&mut ids);
        assert_eq!(ids, vec!["c", "a", "b"]);
    }

    #[test]
    fn community_identity_is_stable_and_order_independent() {
        // `connected_components` gives no stable member ordering, so identity
        // must not depend on it — otherwise the id changes every recompute and
        // the stored summary is orphaned.
        let a = community_identity("org1", &["e3".into(), "e1".into(), "e2".into()]);
        let b = community_identity("org1", &["e1".into(), "e2".into(), "e3".into()]);
        assert_eq!(a, b, "member order must not affect identity");
        // Duplicates collapse.
        let c = community_identity(
            "org1",
            &["e1".into(), "e2".into(), "e2".into(), "e3".into()],
        );
        assert_eq!(a, c, "duplicate members must not affect identity");
    }

    #[test]
    fn community_identity_changes_with_membership_and_org() {
        let base = community_identity("org1", &["e1".into(), "e2".into()]);
        assert_ne!(
            base,
            community_identity("org1", &["e1".into(), "e2".into(), "e3".into()]),
            "a community that gained a member is a different community"
        );
        assert_ne!(
            base,
            community_identity("org1", &["e1".into()]),
            "a community that lost a member is a different community"
        );
        assert_ne!(
            base,
            community_identity("org2", &["e1".into(), "e2".into()]),
            "communities are org-scoped"
        );
    }

    #[test]
    fn ontology_collapses_the_observed_synonym_clusters() {
        // Every cluster below is a real grouping from the live 61-type set.
        for (types, expected) in [
            (
                vec!["Organization", "Company", "Team", "Group", "companies"],
                "Organization",
            ),
            (
                vec!["Chemical", "Chemicals", "Substance", "Gas", "Material"],
                "Substance",
            ),
            (
                vec!["Microorganism", "Pathogen", "Bacteria", "Animal"],
                "Organism",
            ),
            (
                vec![
                    "Contact",
                    "Contact Information",
                    "Phone",
                    "Phone Number",
                    "Email",
                ],
                "ContactPoint",
            ),
            // `Page` deliberately sits with Document, not Technology: in a
            // web-crawl corpus a page is *content*, while `Website` is the
            // system hosting it.
            (
                vec!["Website", "Platform", "Application", "Software"],
                "Technology",
            ),
            (
                vec!["Process", "Procedure", "Method", "Action", "Function"],
                "Process",
            ),
            (vec!["Course", "Training"], "Training"),
            (
                vec!["Standard", "Guideline", "Certification", "Membership"],
                "Standard",
            ),
            (vec!["Document", "File", "Image", "Page"], "Document"),
            (vec!["Date", "Time"], "Date"),
            (vec!["Location", "Country"], "Location"),
            (vec!["Property", "Outcome", "Risk"], "Metric"),
        ] {
            for t in types {
                let got = canonical_entity_type(t);
                if expected == "Metric" && (t == "Risk") {
                    // Risk is abstract -> Concept, not a measurement.
                    assert_eq!(got, "Concept", "{t}");
                } else {
                    assert_eq!(got, expected, "{t} should canonicalise to {expected}");
                }
            }
        }
    }

    #[test]
    fn ontology_is_closed_and_defaults_to_concept() {
        // Unrecognised labels must never pass through, or the ontology reopens.
        for invented in ["Experience", "Collaboration", "Warning", "Surface", "Zorp"] {
            assert_eq!(canonical_entity_type(invented), "Concept", "{invented}");
        }
        // Industry-family labels get their own facet, not the catch-all.
        for industry in ["Industry", "Field", "Sector", "Value Chain", "vertical"] {
            assert_eq!(canonical_entity_type(industry), "Industry", "{industry}");
        }
        // `Claim` as an entity type was a modelling error (claims have their own table).
        assert_eq!(canonical_entity_type("Claim"), "Concept");
        // Every output is a member of the declared ontology.
        for t in ["Company", "Phone Number", "bacteria", "nonsense", ""] {
            assert!(
                ENTITY_TYPES.contains(&canonical_entity_type(t)),
                "{t} produced a type outside ENTITY_TYPES"
            );
        }
    }

    #[test]
    fn ontology_lookup_ignores_case_separators_and_punctuation() {
        for variant in [
            "Phone Number",
            "phone_number",
            "phone-number",
            "PHONENUMBER",
            " Phone  Number ",
        ] {
            assert_eq!(canonical_entity_type(variant), "ContactPoint", "{variant}");
        }
    }

    #[test]
    fn canonical_type_merges_the_coresystem_split() {
        // The concrete live defect: 13 rows as Organization + 9 as Company.
        assert_eq!(
            entity_identity("org1", "Aquatiq", "Organization"),
            entity_identity("org1", "Aquatiq", "Company"),
        );
    }

    #[test]
    fn normalisation_is_not_alias_resolution() {
        // Guard against over-claiming: "Sarah Chen" and "SC" are genuinely
        // different keys. Merging them needs the embedding stage (P1-2 step 2).
        assert_ne!(
            entity_identity("org1", "Sarah Chen", "Person"),
            entity_identity("org1", "SC", "Person")
        );
    }
}

#[cfg(test)]
mod visibility_tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    const FIXTURE_SQL: &str = r#"
        CREATE TABLE documents (
            document_id TEXT PRIMARY KEY, org_id TEXT NOT NULL, owner_id TEXT NOT NULL,
            visibility TEXT NOT NULL, deleted_at TIMESTAMPTZ
        );
        CREATE TABLE knowledge_units (
            knowledge_id TEXT PRIMARY KEY, document_id TEXT NOT NULL, org_id TEXT NOT NULL,
            text TEXT NOT NULL
        );
        CREATE TABLE graph_entities (
            entity_id TEXT PRIMARY KEY, org_id TEXT NOT NULL, entity_type TEXT NOT NULL,
            entity_text TEXT NOT NULL, confidence DOUBLE PRECISION, provenance TEXT,
            source_refs JSONB, created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );
        CREATE TABLE graph_relationships (
            rel_id TEXT PRIMARY KEY, org_id TEXT NOT NULL, entity_a_id TEXT NOT NULL,
            entity_b_id TEXT NOT NULL, relation_type TEXT NOT NULL,
            confidence DOUBLE PRECISION, provenance TEXT, source_refs JSONB,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );
        CREATE TABLE graph_claims (
            claim_id TEXT PRIMARY KEY, org_id TEXT NOT NULL, claim_text TEXT NOT NULL,
            entity_ids JSONB, confidence DOUBLE PRECISION, provenance TEXT,
            source_refs JSONB, contradicted_by_claim_ids JSONB, claim_status TEXT,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );
        CREATE TABLE graph_text_units (
            id BIGSERIAL PRIMARY KEY, org_id TEXT NOT NULL, knowledge_id TEXT NOT NULL,
            entity_id TEXT, rel_id TEXT, claim_id TEXT
        );

        INSERT INTO documents VALUES
            ('doc-org', 'org-a', 'user-a', 'org', NULL),
            ('doc-private', 'org-a', 'user-a', 'private', NULL),
            ('doc-shared', 'org-a', 'user-a', 'shared', NULL),
            ('doc-deleted', 'org-a', 'user-a', 'org', NOW()),
            ('doc-other', 'org-b', 'user-c', 'org', NULL);
        INSERT INTO knowledge_units VALUES
            ('k-org', 'doc-org', 'org-a', 'org visible'),
            ('k-private', 'doc-private', 'org-a', 'private'),
            ('k-shared', 'doc-shared', 'org-a', 'shared'),
            ('k-deleted', 'doc-deleted', 'org-a', 'deleted'),
            ('k-other', 'doc-other', 'org-b', 'other org');
        INSERT INTO graph_entities VALUES
            ('e-org-a', 'org-a', 'Person', 'Org A', 1, 'test', '[]', NOW()),
            ('e-org-b', 'org-a', 'Person', 'Org B', 1, 'test', '[]', NOW()),
            ('e-private', 'org-a', 'Person', 'Private', 1, 'test', '[]', NOW()),
            ('e-shared', 'org-a', 'Person', 'Shared', 1, 'test', '[]', NOW()),
            ('e-deleted', 'org-a', 'Person', 'Deleted', 1, 'test', '[]', NOW()),
            ('e-other', 'org-b', 'Person', 'Other', 1, 'test', '[]', NOW());
        INSERT INTO graph_relationships VALUES
            ('r-org', 'org-a', 'e-org-a', 'e-org-b', 'knows', 1, 'test', '[]', NOW()),
            ('r-private', 'org-a', 'e-private', 'e-private', 'knows', 1, 'test', '[]', NOW()),
            ('r-shared', 'org-a', 'e-shared', 'e-shared', 'knows', 1, 'test', '[]', NOW()),
            ('r-deleted', 'org-a', 'e-deleted', 'e-deleted', 'knows', 1, 'test', '[]', NOW()),
            ('r-other', 'org-b', 'e-other', 'e-other', 'knows', 1, 'test', '[]', NOW());
        INSERT INTO graph_claims VALUES
            ('c-org', 'org-a', 'org claim', '["e-org-a"]', 1, 'test', '[]', '["c-x"]', 'active', NOW()),
            ('c-private', 'org-a', 'private claim', '["e-private"]', 1, 'test', '[]', '["c-x"]', 'active', NOW()),
            ('c-shared', 'org-a', 'shared claim', '["e-shared"]', 1, 'test', '[]', '["c-x"]', 'active', NOW()),
            ('c-deleted', 'org-a', 'deleted claim', '["e-deleted"]', 1, 'test', '[]', '["c-x"]', 'active', NOW()),
            ('c-other', 'org-b', 'other claim', '["e-other"]', 1, 'test', '[]', '["c-x"]', 'active', NOW());
        INSERT INTO graph_text_units (org_id, knowledge_id, entity_id) VALUES
            ('org-a', 'k-org', 'e-org-a'), ('org-a', 'k-org', 'e-org-b'),
            ('org-a', 'k-private', 'e-private'), ('org-a', 'k-shared', 'e-shared'),
            ('org-a', 'k-deleted', 'e-deleted'), ('org-b', 'k-other', 'e-other');
        INSERT INTO graph_text_units (org_id, knowledge_id, rel_id) VALUES
            ('org-a', 'k-org', 'r-org'), ('org-a', 'k-private', 'r-private'),
            ('org-a', 'k-shared', 'r-shared'), ('org-a', 'k-deleted', 'r-deleted'),
            ('org-b', 'k-other', 'r-other');
        INSERT INTO graph_text_units (org_id, knowledge_id, claim_id) VALUES
            ('org-a', 'k-org', 'c-org'), ('org-a', 'k-private', 'c-private'),
            ('org-a', 'k-shared', 'c-shared'), ('org-a', 'k-deleted', 'c-deleted'),
            ('org-b', 'k-other', 'c-other');
    "#;

    /// Creates the RLS runtime role and grants it the fixture's schema.
    ///
    /// # Why a test fixture needs a database role at all
    ///
    /// Production code in this file opens org-scoped transactions through
    /// `pg_org_scope::begin_org_scoped` (see `services/pg-org-scope-rs`), which
    /// issues `SET LOCAL ROLE dataplane_app` on every scoped path — which is now
    /// every `GraphStore` method except `purge_organization_data`.
    ///
    /// In production that role is created by
    /// `infra/postgres/migrations/20260809120000_org_rls_isolation.sql`. These
    /// tests, however, build their own minimal schema with `CREATE TABLE` on a
    /// bare disposable database and never run the migrations — so without this
    /// helper every scoped path fails at runtime with:
    ///
    /// ```text
    /// error returned from database: role "dataplane_app" does not exist
    /// ```
    ///
    /// That failure is invisible to a normal `cargo test` run, because every
    /// test that would hit it is `#[ignore]`d behind `GRAPH_TEST_DATABASE_URL`.
    /// Do not delete this as boilerplate: removing it silently disables the
    /// integration tests that cover graph provenance and visibility.
    ///
    /// # Grants only — deliberately
    ///
    /// This creates the role and grants it access, but does **not** enable
    /// row-level security or install any policy. That is the point: these
    /// fixtures keep asserting exactly what they asserted before RLS existed —
    /// that each query's own `org_id` predicate does the filtering. Having them
    /// enforce RLS too would be a strictly stronger test, but it changes what
    /// the suite covers, so it belongs in its own deliberate change.
    ///
    /// # Two deviations from `retrieval-engine-rs/tests/common/mod.rs`
    ///
    /// 1. **Grants target `schema`, not `public`.** That fixture builds its
    ///    tables in `public`; this one creates a per-test
    ///    `graph_visibility_<uuid>` schema and `SET search_path` to it, so
    ///    granting `public` would leave every scoped path failing with
    ///    `permission denied for schema graph_visibility_…` instead.
    /// 2. **`ALTER DEFAULT PRIVILEGES` as well as `ON ALL TABLES`.** The latter
    ///    only covers tables that exist when it runs, and
    ///    `community_summaries_survive_recompute_and_stale_ones_are_pruned`
    ///    creates `graph_communities` itself *after* `fixture()` returns. The
    ///    default-privileges grants cover that later table; the `ON ALL TABLES`
    ///    grants cover the six `FIXTURE_SQL` creates above.
    async fn grant_rls_runtime_role(pool: &PgPool, schema: &str) {
        sqlx::raw_sql(&format!(
            r#"
            -- Roles are cluster-wide, so a concurrent test in the same binary
            -- may win the race to create it, and a plain IF NOT EXISTS check
            -- has a TOCTOU window. BOTH handlers are required, and this was
            -- verified against a real database rather than assumed:
            --   * duplicate_object (42710) is what a *sequential* re-run
            --     raises, once the role is already committed.
            --   * unique_violation (23505) is what an actual *concurrent*
            --     race raises — the losing backend faults on
            --     pg_authid_rolname_index before the duplicate_object check
            --     is ever reached. Catching only duplicate_object leaves the
            --     exact race this guard exists for unhandled; with the role
            --     absent and tests running multi-threaded it fails with
            --     `duplicate key value violates unique constraint
            --     "pg_authid_rolname_index"`.
            DO $role$
            BEGIN
                CREATE ROLE dataplane_app NOLOGIN NOSUPERUSER NOBYPASSRLS;
            EXCEPTION WHEN duplicate_object OR unique_violation THEN
                NULL;
            END
            $role$;

            GRANT USAGE ON SCHEMA {schema} TO dataplane_app;
            GRANT SELECT, INSERT, UPDATE, DELETE
                ON ALL TABLES IN SCHEMA {schema} TO dataplane_app;
            GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA {schema} TO dataplane_app;
            ALTER DEFAULT PRIVILEGES IN SCHEMA {schema}
                GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO dataplane_app;
            ALTER DEFAULT PRIVILEGES IN SCHEMA {schema}
                GRANT USAGE, SELECT ON SEQUENCES TO dataplane_app;
            "#
        ))
        .execute(pool)
        .await
        .expect("create and grant the dataplane_app RLS runtime role");
    }

    async fn fixture() -> (GraphStore, PgPool, String) {
        let database_url = std::env::var("GRAPH_TEST_DATABASE_URL")
            .expect("GRAPH_TEST_DATABASE_URL must point to disposable PostgreSQL");
        assert!(
            (database_url.contains("localhost") || database_url.contains("127.0.0.1"))
                && database_url.contains("/graph_test"),
            "refusing non-local or non-graph_test database"
        );
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&database_url)
            .await
            .expect("connect disposable admin database");
        let schema = format!("graph_visibility_{}", Uuid::new_v4().simple());
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .expect("create isolated graph schema");
        admin.close().await;

        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&database_url)
            .await
            .expect("connect isolated graph pool");
        sqlx::query(&format!("SET search_path TO {schema}"))
            .execute(&pool)
            .await
            .expect("select isolated graph schema");
        sqlx::raw_sql(FIXTURE_SQL)
            .execute(&pool)
            .await
            .expect("create graph visibility fixture");
        // Must come after the CREATE TABLEs above — see the helper's doc.
        grant_rls_runtime_role(&pool, &schema).await;
        (GraphStore::new(pool.clone()), pool, schema)
    }

    #[tokio::test]
    #[ignore = "requires GRAPH_TEST_DATABASE_URL pointing to disposable PostgreSQL"]
    async fn only_live_org_visible_provenance_is_queryable_for_same_org_users() {
        let (store, pool, schema) = fixture().await;

        for _conceptual_user in ["user-a", "user-b"] {
            let (entities, relationships, entity_total, relationship_total) =
                store.snapshot_org_graph("org-a", 100, 100).await.unwrap();
            assert_eq!(entity_total, 2);
            assert_eq!(relationship_total, 1);
            assert_eq!(entities.len(), 2);
            assert_eq!(relationships.len(), 1);
            assert!(store
                .get_entity("org-a", "e-org-a")
                .await
                .unwrap()
                .is_some());
            for hidden in ["e-private", "e-shared", "e-deleted", "e-other"] {
                assert!(store.get_entity("org-a", hidden).await.unwrap().is_none());
            }
            let (listed, total) = store
                .list_entities_by_type("org-a", "Person", 100, 0)
                .await
                .unwrap();
            assert_eq!((listed.len(), total), (2, 2));
            assert_eq!(
                store
                    .get_relationships("org-a", "e-org-a", None)
                    .await
                    .unwrap()
                    .len(),
                1
            );
            assert!(store
                .get_relationships("org-a", "e-private", None)
                .await
                .unwrap()
                .is_empty());
            let claims = store.get_claims("org-a", None, None).await.unwrap();
            assert_eq!(
                claims
                    .iter()
                    .map(|c| c.claim_id.as_str())
                    .collect::<Vec<_>>(),
                ["c-org"]
            );
            let (contradictions, total) = store.get_contradictions("org-a", 100, 0).await.unwrap();
            assert_eq!(total, 1);
            assert_eq!(contradictions[0].claim_id, "c-org");
            let (expanded, rels) = store
                .get_graph_expansion("org-a", &["e-org-a".into()], 1, 100)
                .await
                .unwrap();
            assert_eq!(expanded[0].entity_id, "e-org-b");
            assert_eq!(rels[0].rel_id, "r-org");
        }

        assert_eq!(
            store
                .load_org_visible_chunks("org-a", "doc-org")
                .await
                .unwrap(),
            vec![("k-org".into(), "org visible".into())]
        );
        for hidden_doc in ["doc-private", "doc-shared", "doc-deleted", "doc-other"] {
            assert!(store
                .load_org_visible_chunks("org-a", hidden_doc)
                .await
                .unwrap()
                .is_empty());
        }

        sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
            .execute(&pool)
            .await
            .expect("drop isolated graph schema");
        pool.close().await;
    }

    /// P1-5a: a generated community summary must survive re-detection.
    ///
    /// This is the property that makes summarisation affordable. Detection runs
    /// on every ingest that adds a relationship; before this, each run minted
    /// fresh v4 ids and DELETE-then-INSERTed, so every summary was thrown away
    /// and would have to be paid for again. Also pins the prune, so stale
    /// communities still disappear.
    #[tokio::test]
    #[ignore = "requires GRAPH_TEST_DATABASE_URL pointing to disposable PostgreSQL"]
    async fn community_summaries_survive_recompute_and_stale_ones_are_pruned() {
        let (store, pool, schema) = fixture().await;

        // `graph_communities` is not part of the shared visibility fixture;
        // create it locally rather than widening a fixture other tests rely on.
        sqlx::raw_sql(
            "CREATE TABLE graph_communities (
                 community_id TEXT PRIMARY KEY, org_id TEXT NOT NULL,
                 entity_ids JSONB NOT NULL, summary TEXT, level INTEGER NOT NULL DEFAULT 0,
                 created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
             );",
        )
        .execute(&pool)
        .await
        .expect("create graph_communities");

        let members = vec!["e-a".to_string(), "e-b".to_string()];
        let id = community_identity("org-a", &members);

        let initial = vec![crate::model::Community {
            community_id: id.clone(),
            org_id: "org-a".to_string(),
            entity_ids: members.clone(),
            summary: None,
            level: 0,
        }];
        store
            .replace_communities("org-a", &initial)
            .await
            .expect("first detection");

        // Stand in for the summariser writing its result.
        sqlx::query("UPDATE graph_communities SET summary = $2 WHERE community_id = $1")
            .bind(&id)
            .bind("Hygiene systems supplied to Norwegian food processors.")
            .execute(&pool)
            .await
            .expect("seed summary");

        // Re-detect the SAME community (member order deliberately reversed, to
        // prove identity is order-independent end-to-end, not just in the unit test).
        let recomputed = vec![crate::model::Community {
            community_id: community_identity("org-a", &["e-b".to_string(), "e-a".to_string()]),
            org_id: "org-a".to_string(),
            entity_ids: vec!["e-b".to_string(), "e-a".to_string()],
            summary: None, // detection never computes summaries
            level: 0,
        }];
        assert_eq!(recomputed[0].community_id, id, "identity must be stable");
        store
            .replace_communities("org-a", &recomputed)
            .await
            .expect("second detection");

        let (summary,): (Option<String>,) =
            sqlx::query_as("SELECT summary FROM graph_communities WHERE community_id = $1")
                .bind(&id)
                .fetch_one(&pool)
                .await
                .expect("read after recompute");
        assert_eq!(
            summary.as_deref(),
            Some("Hygiene systems supplied to Norwegian food processors."),
            "recompute must NOT overwrite the stored summary with detection's None"
        );

        // A community whose membership changed is a different community, and the
        // old row must be pruned rather than linger with a now-wrong summary.
        let changed = vec![crate::model::Community {
            community_id: community_identity(
                "org-a",
                &["e-a".to_string(), "e-b".to_string(), "e-c".to_string()],
            ),
            org_id: "org-a".to_string(),
            entity_ids: vec!["e-a".to_string(), "e-b".to_string(), "e-c".to_string()],
            summary: None,
            level: 0,
        }];
        store
            .replace_communities("org-a", &changed)
            .await
            .expect("third detection");

        let (remaining,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM graph_communities WHERE org_id = 'org-a'")
                .fetch_one(&pool)
                .await
                .expect("count after membership change");
        assert_eq!(remaining, 1, "the superseded community must be pruned");
        let (old_gone,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM graph_communities WHERE community_id = $1")
                .bind(&id)
                .fetch_one(&pool)
                .await
                .expect("check old id");
        assert_eq!(
            old_gone, 0,
            "stale summary must not survive a membership change"
        );

        // Empty detection prunes everything for the org.
        store
            .replace_communities("org-a", &[])
            .await
            .expect("empty detection");
        let (none_left,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM graph_communities WHERE org_id = 'org-a'")
                .fetch_one(&pool)
                .await
                .expect("count after empty detection");
        assert_eq!(none_left, 0);

        sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
            .execute(&pool)
            .await
            .expect("drop isolated graph schema");
        pool.close().await;
    }

    /// P1-4 end-to-end: detect → write → the pre-existing read path returns it.
    ///
    /// The live corpus contains **no** structurally-detectable contradiction
    /// (validated read-only: 107 shared-entity candidate pairs, 0 hits), so the
    /// write path cannot be proven against production data. This constructs the
    /// positive case explicitly, and also pins the two properties most likely to
    /// regress: org isolation, and that a claim sourced only from a non-visible
    /// document is never used as evidence.
    #[tokio::test]
    #[ignore = "requires GRAPH_TEST_DATABASE_URL pointing to disposable PostgreSQL"]
    async fn contradiction_writer_flags_pairs_and_read_path_returns_them() {
        let (store, pool, schema) = fixture().await;

        // Shared entity is the candidate-selection anchor.
        sqlx::raw_sql(
            "INSERT INTO graph_entities (entity_id, org_id, entity_type, entity_text, confidence, provenance, source_refs)
             VALUES ('e-plant', 'org-a', 'Organization', 'Processing Plant', 0.9, 'extracted', '[]'),
                    ('e-plant-b', 'org-b', 'Organization', 'Processing Plant', 0.9, 'extracted', '[]');

             INSERT INTO graph_claims (claim_id, org_id, claim_text, entity_ids, confidence, provenance, source_refs, claim_status)
             VALUES
               ('c-yes',    'org-a', 'The processing plant is certified for export', '[\"e-plant\"]',   0.9, 'extracted', '[]', 'active'),
               ('c-no',     'org-a', 'The processing plant is not certified for export', '[\"e-plant\"]', 0.9, 'extracted', '[]', 'active'),
               ('c-unrel',  'org-a', 'Bergen hosted the annual seafood conference', '[\"e-plant\"]',   0.9, 'extracted', '[]', 'active'),
               ('c-hidden', 'org-a', 'The processing plant is never certified for export', '[\"e-plant\"]', 0.9, 'extracted', '[]', 'active'),
               ('c-otherorg','org-b','The processing plant is not certified for export', '[\"e-plant-b\"]', 0.9, 'extracted', '[]', 'active');

             -- Provenance: c-yes/c-no/c-unrel are org-visible; c-hidden is only
             -- reachable through a private document and must be excluded.
             INSERT INTO graph_text_units (org_id, knowledge_id, entity_id, rel_id, claim_id) VALUES
               ('org-a', 'k-org',     NULL, NULL, 'c-yes'),
               ('org-a', 'k-org',     NULL, NULL, 'c-no'),
               ('org-a', 'k-org',     NULL, NULL, 'c-unrel'),
               ('org-a', 'k-private', NULL, NULL, 'c-hidden');",
        )
        .execute(&pool)
        .await
        .expect("seed contradiction fixture");

        // The shared fixture already seeds claims carrying
        // `contradicted_by_claim_ids = '["c-x"]'`, one of which is org-visible,
        // so the read path has a non-zero baseline. Assert the delta, not an
        // absolute count, or this test breaks whenever the fixture changes.
        let (baseline, _) = store
            .get_contradictions("org-a", 100, 0)
            .await
            .expect("baseline read must succeed");
        let baseline_total = baseline.len() as i64;

        let pairs = store
            .detect_claim_contradictions("org-a", &["c-yes".to_string()])
            .await
            .expect("detection must succeed");
        assert_eq!(
            pairs, 1,
            "exactly one visible contradiction (c-no); c-unrel is unrelated and \
             c-hidden is not org-visible"
        );

        // Symmetric write: both sides flagged, both moved off 'active'.
        for (subject, other) in [("c-yes", "c-no"), ("c-no", "c-yes")] {
            let (contra, status): (serde_json::Value, String) = sqlx::query_as(
                "SELECT COALESCE(contradicted_by_claim_ids, '[]'), COALESCE(claim_status, 'active')
                 FROM graph_claims WHERE claim_id = $1",
            )
            .bind(subject)
            .fetch_one(&pool)
            .await
            .expect("read flagged claim");
            let ids: Vec<String> = serde_json::from_value(contra).unwrap_or_default();
            assert_eq!(ids, vec![other.to_string()], "{subject} must cite {other}");
            assert_eq!(status, "contradicted", "{subject} status");
        }

        // Untouched claims keep their status.
        for untouched in ["c-unrel", "c-hidden"] {
            let (status,): (String,) = sqlx::query_as(
                "SELECT COALESCE(claim_status, 'active') FROM graph_claims WHERE claim_id = $1",
            )
            .bind(untouched)
            .fetch_one(&pool)
            .await
            .expect("read untouched claim");
            assert_eq!(status, "active", "{untouched} must not be flagged");
        }

        // Org isolation: org-b's identical claim is never involved.
        let (other_org_status,): (String,) = sqlx::query_as(
            "SELECT COALESCE(claim_status, 'active') FROM graph_claims WHERE claim_id = 'c-otherorg'",
        )
        .fetch_one(&pool)
        .await
        .expect("read other-org claim");
        assert_eq!(other_org_status, "active", "cross-tenant leak");

        // The pre-existing read path — empty before this writer existed — now returns the pair.
        let (claims, total) = store
            .get_contradictions("org-a", 100, 0)
            .await
            .expect("get_contradictions must succeed");
        assert_eq!(
            total,
            baseline_total + 2,
            "both newly flagged claims must appear in the read path"
        );
        let ids: Vec<String> = claims.into_iter().map(|c| c.claim_id).collect();
        for expected in ["c-yes", "c-no"] {
            assert!(
                ids.iter().any(|id| id == expected),
                "{expected} missing from get_contradictions; got {ids:?}"
            );
        }
        assert!(
            !ids.iter().any(|id| id == "c-hidden"),
            "a claim sourced only from a private document must never surface"
        );

        // Idempotent: a re-run must not duplicate ids or change status.
        let again = store
            .detect_claim_contradictions("org-a", &["c-yes".to_string()])
            .await
            .expect("re-run must succeed");
        assert_eq!(again, 1, "same pair re-detected");
        let (contra,): (serde_json::Value,) = sqlx::query_as(
            "SELECT COALESCE(contradicted_by_claim_ids, '[]') FROM graph_claims WHERE claim_id = 'c-yes'",
        )
        .fetch_one(&pool)
        .await
        .expect("re-read");
        let ids: Vec<String> = serde_json::from_value(contra).unwrap_or_default();
        assert_eq!(ids, vec!["c-no".to_string()], "union must not duplicate");

        sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
            .execute(&pool)
            .await
            .expect("drop isolated graph schema");
        pool.close().await;
    }
}

#[cfg(test)]
mod gdpr_purge_tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;
    use sqlx::Row;

    // Every table graph-index-rs owns and purges, seeded for TWO
    // organizations so the isolation assertion below is meaningful.
    // `documents`/`knowledge_units` are intentionally NOT seeded here: this
    // service reads but never writes them, and `purge_organization_data`
    // must not (and does not) reference either table.
    const FIXTURE_SQL: &str = r#"
        CREATE TABLE graph_entities (
            entity_id TEXT PRIMARY KEY, org_id TEXT NOT NULL, entity_type TEXT NOT NULL,
            entity_text TEXT NOT NULL, confidence DOUBLE PRECISION, provenance TEXT,
            source_refs JSONB, created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );
        CREATE TABLE graph_relationships (
            rel_id TEXT PRIMARY KEY, org_id TEXT NOT NULL,
            entity_a_id TEXT NOT NULL REFERENCES graph_entities(entity_id) ON DELETE CASCADE,
            entity_b_id TEXT NOT NULL REFERENCES graph_entities(entity_id) ON DELETE CASCADE,
            relation_type TEXT NOT NULL, confidence DOUBLE PRECISION, provenance TEXT,
            source_refs JSONB, created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );
        CREATE TABLE graph_claims (
            claim_id TEXT PRIMARY KEY, org_id TEXT NOT NULL, claim_text TEXT NOT NULL,
            entity_ids JSONB, confidence DOUBLE PRECISION, provenance TEXT,
            source_refs JSONB, contradicted_by_claim_ids JSONB, claim_status TEXT,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );
        CREATE TABLE graph_text_units (
            id BIGSERIAL PRIMARY KEY, org_id TEXT NOT NULL, knowledge_id TEXT NOT NULL,
            entity_id TEXT REFERENCES graph_entities(entity_id) ON DELETE CASCADE,
            rel_id TEXT REFERENCES graph_relationships(rel_id) ON DELETE CASCADE,
            claim_id TEXT REFERENCES graph_claims(claim_id) ON DELETE CASCADE
        );
        CREATE TABLE graph_communities (
            community_id TEXT PRIMARY KEY, org_id TEXT NOT NULL,
            entity_ids JSONB NOT NULL, summary TEXT, level INTEGER NOT NULL DEFAULT 0,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );

        INSERT INTO graph_entities VALUES
            ('e-a1', 'org-a', 'Person', 'A One', 1, 'test', '[]', NOW()),
            ('e-a2', 'org-a', 'Person', 'A Two', 1, 'test', '[]', NOW()),
            ('e-b1', 'org-b', 'Person', 'B One', 1, 'test', '[]', NOW()),
            ('e-b2', 'org-b', 'Person', 'B Two', 1, 'test', '[]', NOW());
        INSERT INTO graph_relationships VALUES
            ('r-a', 'org-a', 'e-a1', 'e-a2', 'knows', 1, 'test', '[]', NOW()),
            ('r-b', 'org-b', 'e-b1', 'e-b2', 'knows', 1, 'test', '[]', NOW());
        INSERT INTO graph_claims VALUES
            ('c-a', 'org-a', 'a claim', '["e-a1"]', 1, 'test', '[]', '[]', 'active', NOW()),
            ('c-b', 'org-b', 'b claim', '["e-b1"]', 1, 'test', '[]', '[]', 'active', NOW());
        INSERT INTO graph_text_units (org_id, knowledge_id, entity_id) VALUES
            ('org-a', 'k-a', 'e-a1'), ('org-b', 'k-b', 'e-b1');
        INSERT INTO graph_text_units (org_id, knowledge_id, rel_id) VALUES
            ('org-a', 'k-a', 'r-a'), ('org-b', 'k-b', 'r-b');
        INSERT INTO graph_text_units (org_id, knowledge_id, claim_id) VALUES
            ('org-a', 'k-a', 'c-a'), ('org-b', 'k-b', 'c-b');
        INSERT INTO graph_communities VALUES
            ('comm-a', 'org-a', '["e-a1","e-a2"]', 'org a community', 0, NOW()),
            ('comm-b', 'org-b', '["e-b1","e-b2"]', 'org b community', 0, NOW());
    "#;

    async fn fixture() -> (GraphStore, PgPool, String) {
        let database_url = std::env::var("GRAPH_TEST_DATABASE_URL")
            .expect("GRAPH_TEST_DATABASE_URL must point to disposable PostgreSQL");
        assert!(
            (database_url.contains("localhost") || database_url.contains("127.0.0.1"))
                && database_url.contains("/graph_test"),
            "refusing non-local or non-graph_test database"
        );
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&database_url)
            .await
            .expect("connect disposable admin database");
        let schema = format!("graph_gdpr_{}", Uuid::new_v4().simple());
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .expect("create isolated graph schema");
        admin.close().await;

        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&database_url)
            .await
            .expect("connect isolated graph pool");
        sqlx::query(&format!("SET search_path TO {schema}"))
            .execute(&pool)
            .await
            .expect("select isolated graph schema");
        sqlx::raw_sql(FIXTURE_SQL)
            .execute(&pool)
            .await
            .expect("create graph gdpr-purge fixture");
        (GraphStore::new(pool.clone()), pool, schema)
    }

    async fn org_row_counts(pool: &PgPool, org_id: &str) -> [i64; 5] {
        let mut counts = [0i64; 5];
        for (idx, table) in [
            "graph_text_units",
            "graph_relationships",
            "graph_claims",
            "graph_communities",
            "graph_entities",
        ]
        .into_iter()
        .enumerate()
        {
            let row = sqlx::query(&format!(
                "SELECT COUNT(*) AS n FROM {table} WHERE org_id = $1"
            ))
            .bind(org_id)
            .fetch_one(pool)
            .await
            .expect("count query");
            counts[idx] = row.get::<i64, _>("n");
        }
        counts
    }

    /// Safety-critical isolation test (per the GDPR purge safety contract):
    /// purging org-a must remove every graph-index-owned row for org-a and
    /// must NOT touch a single row belonging to org-b.
    #[tokio::test]
    #[ignore = "requires GRAPH_TEST_DATABASE_URL pointing to disposable PostgreSQL"]
    async fn purge_is_scoped_strictly_by_org_id_and_leaves_other_orgs_untouched() {
        let (store, pool, schema) = fixture().await;

        // graph_text_units carries THREE rows per org: one mapping row each
        // for the entity_id-only, rel_id-only, and claim_id-only inserts
        // above — not one row per source table.
        assert_eq!(org_row_counts(&pool, "org-a").await, [3, 1, 1, 1, 2]);
        assert_eq!(org_row_counts(&pool, "org-b").await, [3, 1, 1, 1, 2]);

        let summary = store
            .purge_organization_data("org-a")
            .await
            .expect("purge org-a");
        assert_eq!(summary.graph_text_units, 3);
        assert_eq!(summary.graph_relationships, 1);
        assert_eq!(summary.graph_claims, 1);
        assert_eq!(summary.graph_communities, 1);
        assert_eq!(summary.graph_entities, 2);
        assert_eq!(summary.total(), 8);

        // org-a is now fully purged across every owned table...
        assert_eq!(org_row_counts(&pool, "org-a").await, [0, 0, 0, 0, 0]);
        // ...and org-b's rows are byte-for-byte untouched.
        assert_eq!(org_row_counts(&pool, "org-b").await, [3, 1, 1, 1, 2]);

        // Idempotency: NATS is at-least-once delivery, so a redelivered
        // erasure event must be safe to run twice — the second purge must
        // match zero rows everywhere and must not error.
        let replay = store
            .purge_organization_data("org-a")
            .await
            .expect("replayed purge of already-purged org-a must not error");
        assert_eq!(replay.total(), 0);
        assert_eq!(org_row_counts(&pool, "org-b").await, [3, 1, 1, 1, 2]);

        sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
            .execute(&pool)
            .await
            .expect("drop isolated graph schema");
        pool.close().await;
    }
}
