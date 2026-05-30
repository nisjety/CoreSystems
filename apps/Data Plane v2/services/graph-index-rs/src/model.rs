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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionResult {
    pub entities: Vec<ExtractedEntity>,
    pub relationships: Vec<ExtractedRelationship>,
    pub claims: Vec<ExtractedClaim>,
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
