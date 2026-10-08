use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    pub entity_id: String,
    pub org_id: String,
    pub entity_type: String,
    pub entity_text: String,
    pub confidence: f64,
    pub provenance: String,
    pub source_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relationship {
    pub rel_id: String,
    pub org_id: String,
    pub entity_a_id: String,
    pub entity_b_id: String,
    pub relation_type: String,
    pub confidence: f64,
    pub provenance: String,
    pub source_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claim {
    pub claim_id: String,
    pub org_id: String,
    pub claim_text: String,
    pub entity_ids: Vec<String>,
    pub confidence: f64,
    pub provenance: String,
    pub source_refs: Vec<String>,
    pub contradicted_by: Vec<String>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)] // shape produced by detect_communities; persisted via save_community
pub struct Community {
    pub community_id: String,
    pub org_id: String,
    pub entity_ids: Vec<String>,
    pub summary: Option<String>,
    pub level: i32,
}

/// What the model answered. Each list defaults to empty: a model that has no relationships to
/// report and leaves the key out has answered, and an answer is not a failure to parse (a
/// failure here counts toward the chunk's poison cap).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionResult {
    #[serde(default)]
    pub entities: Vec<ExtractedEntity>,
    #[serde(default)]
    pub relationships: Vec<ExtractedRelationship>,
    #[serde(default)]
    pub claims: Vec<ExtractedClaim>,
}

/// An entity row shaped for the Neo4j graph read-model mirror. Built alongside
/// the canonical Postgres insert in `GraphStore::persist_extraction` so the
/// mirror carries the SAME `entity_id` (Postgres PK) — the join key that lets
/// the two stores cross-reference and makes `MERGE` idempotent.
#[derive(Debug, Clone)]
pub struct MirrorEntity {
    pub entity_id: String,
    pub entity_type: String,
    pub entity_text: String,
    pub confidence: f64,
}

/// A relationship row shaped for the Neo4j read-model mirror (Postgres PKs).
#[derive(Debug, Clone)]
pub struct MirrorRelationship {
    pub rel_id: String,
    pub entity_a_id: String,
    pub entity_b_id: String,
    pub relation_type: String,
    pub confidence: f64,
}

/// Result of a canonical Postgres extraction persist. The `*_ids` back the
/// `graph_text_units` mappings (as before); the `mirror_*` rows feed the
/// optional Neo4j read-model. Empty (default) when the knowledge unit is not
/// org-visible — so the mirror inherits the org-visibility + ZDR gate for free.
#[derive(Debug, Clone, Default)]
pub struct PersistedExtraction {
    /// False only when the chunk's document was not visible to the whole organization, so
    /// nothing was written. An extraction that found nothing is `visible` with no ids.
    pub visible: bool,
    pub entity_ids: Vec<String>,
    pub rel_ids: Vec<String>,
    pub claim_ids: Vec<String>,
    pub mirror_entities: Vec<MirrorEntity>,
    pub mirror_relationships: Vec<MirrorRelationship>,
}

/// Per-table row counts from one GDPR org-erasure purge run (see
/// `crate::gdpr_nats` and `crate::store::GraphStore::
/// purge_organization_data`). Every table here is one graph-index-rs itself
/// writes to — it never touches `documents` or `knowledge_units`, which are
/// owned by sibling Data Plane v2 services and are read-only in this crate.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct GdprPurgeSummary {
    pub graph_text_units: u64,
    pub graph_relationships: u64,
    pub graph_claims: u64,
    pub graph_communities: u64,
    pub graph_entities: u64,
    /// What was done to each chunk (CR-T12): ids and counters only, but it is the
    /// organization's, so it goes with the rest.
    pub graph_extraction_state: u64,
}

impl GdprPurgeSummary {
    /// Total rows deleted across every table in one purge run.
    #[must_use]
    pub fn total(&self) -> u64 {
        self.graph_text_units
            + self.graph_relationships
            + self.graph_claims
            + self.graph_communities
            + self.graph_entities
            + self.graph_extraction_state
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedEntity {
    pub entity_type: String,
    pub entity_text: String,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedRelationship {
    pub source_entity: String,
    pub target_entity: String,
    pub relation_type: String,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedClaim {
    pub claim_text: String,
    pub related_entities: Vec<String>,
    pub confidence: f64,
}
