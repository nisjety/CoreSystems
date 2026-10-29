//! gRPC server for `graph_v1.GraphService` (HARNESS_PHASE1 §7).
//!
//! Mirrors the HTTP surface in `api.rs` 1:1 — both delegate to the same
//! `GraphStore`, so adding gRPC is *pure surface* and doesn't change the
//! storage layer. The Model Plane gateway expects gRPC on `:50053` (per its
//! `DATAPLANE_GRAPH_ADDR` default), so this completes the wire surface the
//! gateway already declared but couldn't reach.

use std::sync::Arc;

use tonic::{Request, Response, Status};

use crate::auth::Principal;
use crate::model::{Claim, Entity, Relationship};
use crate::store::GraphStore;

pub mod pb {
    tonic::include_proto!("dataplane.graph.v1");
}

pub use pb::graph_service_server::GraphServiceServer;

pub struct GraphGrpc {
    store: Arc<GraphStore>,
}

impl GraphGrpc {
    pub fn new(store: Arc<GraphStore>) -> Self {
        Self { store }
    }
}

fn verified_principal<T>(request: &Request<T>) -> Option<&Principal> {
    request.extensions().get::<Principal>()
}

fn claimed_org<'a>(principal: &'a Principal, requested_org_id: &str) -> Option<&'a str> {
    if principal.authorizes_org(requested_org_id) {
        Some(&principal.org_id)
    } else {
        None
    }
}

// ── Model → proto mapping ─────────────────────────────────────────────────

fn entity_to_pb(e: Entity) -> pb::GraphEntity {
    pb::GraphEntity {
        entity_id: e.entity_id,
        org_id: e.org_id,
        r#type: e.entity_type,
        text: e.entity_text,
        confidence: e.confidence as f32,
        provenance: e.provenance,
        source_refs: e.source_refs,
        // `created_at` is not on the internal Entity row yet (store doesn't
        // surface it). Leave None until the store adds it.
        created_at: None,
        metadata: None,
    }
}

fn relationship_to_pb(r: Relationship) -> pb::GraphRelationship {
    pb::GraphRelationship {
        rel_id: r.rel_id,
        org_id: r.org_id,
        entity_a_id: r.entity_a_id,
        entity_b_id: r.entity_b_id,
        relation_type: r.relation_type,
        confidence: r.confidence as f32,
        provenance: r.provenance,
        source_refs: r.source_refs,
        created_at: None,
        metadata: None,
    }
}

fn claim_to_pb(c: Claim) -> pb::GraphClaim {
    pb::GraphClaim {
        claim_id: c.claim_id,
        org_id: c.org_id,
        text: c.claim_text,
        entity_ids: c.entity_ids,
        confidence: c.confidence as f32,
        provenance: c.provenance,
        source_refs: c.source_refs,
        contradicted_by_claim_ids: c.contradicted_by,
        status: c.status,
        created_at: None,
        metadata: None,
    }
}

// ── Service implementation ────────────────────────────────────────────────

#[tonic::async_trait]
impl pb::graph_service_server::GraphService for GraphGrpc {
    async fn get_entity(
        &self,
        request: Request<pb::GetEntityRequest>,
    ) -> Result<Response<pb::GetEntityResponse>, Status> {
        let principal = verified_principal(&request)
            .cloned()
            .ok_or_else(|| Status::unauthenticated("missing verified principal"))?;
        let req = request.into_inner();
        let org_id = claimed_org(&principal, &req.org_id)
            .ok_or_else(|| Status::permission_denied("tenant mismatch"))?;
        let entity = self
            .store
            .get_entity(org_id, &req.entity_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(pb::GetEntityResponse {
            entity: entity.map(entity_to_pb),
        }))
    }

    async fn list_entities_by_type(
        &self,
        request: Request<pb::ListEntitiesByTypeRequest>,
    ) -> Result<Response<pb::ListEntitiesByTypeResponse>, Status> {
        let principal = verified_principal(&request)
            .cloned()
            .ok_or_else(|| Status::unauthenticated("missing verified principal"))?;
        let req = request.into_inner();
        let org_id = claimed_org(&principal, &req.org_id)
            .ok_or_else(|| Status::permission_denied("tenant mismatch"))?;
        let limit = if req.limit > 0 { req.limit } else { 50 };
        let (rows, total) = self
            .store
            .list_entities_by_type(org_id, &req.entity_type, limit, req.offset)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(pb::ListEntitiesByTypeResponse {
            entities: rows.into_iter().map(entity_to_pb).collect(),
            total: i32::try_from(total).unwrap_or(i32::MAX),
        }))
    }

    async fn get_relationships(
        &self,
        request: Request<pb::GetRelationshipsRequest>,
    ) -> Result<Response<pb::GetRelationshipsResponse>, Status> {
        let principal = verified_principal(&request)
            .cloned()
            .ok_or_else(|| Status::unauthenticated("missing verified principal"))?;
        let req = request.into_inner();
        let org_id = claimed_org(&principal, &req.org_id)
            .ok_or_else(|| Status::permission_denied("tenant mismatch"))?;
        let filter = if req.relation_type.is_empty() {
            None
        } else {
            Some(req.relation_type.as_str())
        };
        let rows = self
            .store
            .get_relationships(org_id, &req.entity_id, filter)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(pb::GetRelationshipsResponse {
            relationships: rows.into_iter().map(relationship_to_pb).collect(),
        }))
    }

    async fn get_claims(
        &self,
        request: Request<pb::GetClaimsRequest>,
    ) -> Result<Response<pb::GetClaimsResponse>, Status> {
        let principal = verified_principal(&request)
            .cloned()
            .ok_or_else(|| Status::unauthenticated("missing verified principal"))?;
        let req = request.into_inner();
        let org_id = claimed_org(&principal, &req.org_id)
            .ok_or_else(|| Status::permission_denied("tenant mismatch"))?;
        let entity = if req.entity_id.is_empty() {
            None
        } else {
            Some(req.entity_id.as_str())
        };
        let status = if req.status.is_empty() {
            None
        } else {
            Some(req.status.as_str())
        };
        let rows = self
            .store
            .get_claims(org_id, entity, status)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(pb::GetClaimsResponse {
            claims: rows.into_iter().map(claim_to_pb).collect(),
        }))
    }

    async fn expand_graph(
        &self,
        request: Request<pb::GraphExpansionRequest>,
    ) -> Result<Response<pb::GraphExpansionResponse>, Status> {
        let principal = verified_principal(&request)
            .cloned()
            .ok_or_else(|| Status::unauthenticated("missing verified principal"))?;
        let req = request.into_inner();
        let org_id = claimed_org(&principal, &req.org_id)
            .ok_or_else(|| Status::permission_denied("tenant mismatch"))?;
        let max_hops = if req.max_hops > 0 { req.max_hops } else { 2 };
        let max_entities = if req.max_entities > 0 {
            req.max_entities
        } else {
            50
        };
        let (entities, relationships) = self
            .store
            .get_graph_expansion(org_id, &req.entity_ids, max_hops, max_entities)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        let new_entities = i32::try_from(
            entities
                .iter()
                .filter(|e| !req.entity_ids.contains(&e.entity_id))
                .count(),
        )
        .unwrap_or(i32::MAX);
        let hops_traversed = max_hops; // store doesn't surface actual depth yet.
        Ok(Response::new(pb::GraphExpansionResponse {
            graph: Some(pb::ExpandedGraph {
                entities: entities.into_iter().map(entity_to_pb).collect(),
                relationships: relationships.into_iter().map(relationship_to_pb).collect(),
                // store::get_graph_expansion doesn't surface claims/communities;
                // a future enhancement could; for now the expansion is
                // entity-relationship only (parity with HTTP).
                claims: Vec::new(),
                communities: Vec::new(),
            }),
            hops_traversed,
            new_entities_found: new_entities,
        }))
    }

    async fn get_contradictions(
        &self,
        request: Request<pb::GetContradictionsRequest>,
    ) -> Result<Response<pb::GetContradictionsResponse>, Status> {
        let principal = verified_principal(&request)
            .cloned()
            .ok_or_else(|| Status::unauthenticated("missing verified principal"))?;
        let req = request.into_inner();
        let org_id = claimed_org(&principal, &req.org_id)
            .ok_or_else(|| Status::permission_denied("tenant mismatch"))?;
        let limit = if req.limit > 0 { req.limit } else { 50 };
        let (rows, total) = self
            .store
            .get_contradictions(org_id, limit, req.offset)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        // Proto carries optional entity_id; the store doesn't filter by it
        // yet, so callers asking for entity-scoped contradictions get the
        // org-wide list and filter client-side. Same behaviour as HTTP.
        let filtered: Vec<_> = match req.entity_id.as_deref() {
            Some(id) if !id.is_empty() => rows
                .into_iter()
                .filter(|c| c.entity_ids.iter().any(|e| e == id))
                .collect(),
            _ => rows,
        };
        Ok(Response::new(pb::GetContradictionsResponse {
            contradictions: filtered.into_iter().map(claim_to_pb).collect(),
            total: i32::try_from(total).unwrap_or(i32::MAX),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::{Principal, PrincipalKind};

    fn principal(org_id: &str) -> Principal {
        Principal {
            subject_id: "user-1".into(),
            org_id: org_id.into(),
            kind: PrincipalKind::User,
        }
    }

    #[test]
    fn tenant_guard_accepts_claim_org_and_denies_spoofed_org() {
        let principal = principal("org-a");
        assert_eq!(claimed_org(&principal, "org-a"), Some("org-a"));
        assert_eq!(claimed_org(&principal, "org-b"), None);
    }

    #[test]
    fn request_without_interceptor_principal_is_unauthenticated() {
        let request = Request::new(pb::GetEntityRequest::default());
        assert!(verified_principal(&request).is_none());
    }

    #[test]
    fn entity_to_pb_maps_fields() {
        let e = Entity {
            entity_id: "ent_1".into(),
            org_id: "org_1".into(),
            entity_type: "Person".into(),
            entity_text: "Alice".into(),
            confidence: 0.95,
            provenance: "doc_1".into(),
            source_refs: vec!["chunk_1".into()],
        };
        let pb = entity_to_pb(e);
        assert_eq!(pb.entity_id, "ent_1");
        assert_eq!(pb.r#type, "Person");
        assert_eq!(pb.text, "Alice");
        assert!((pb.confidence - 0.95).abs() < 0.001);
    }

    #[test]
    fn claim_to_pb_carries_contradictions() {
        let c = Claim {
            claim_id: "c_1".into(),
            org_id: "org_1".into(),
            claim_text: "X is Y".into(),
            entity_ids: vec!["e_1".into()],
            confidence: 0.8,
            provenance: "doc_1".into(),
            source_refs: vec![],
            contradicted_by: vec!["c_2".into()],
            status: "active".into(),
        };
        let pb = claim_to_pb(c);
        assert_eq!(pb.contradicted_by_claim_ids, vec!["c_2"]);
        assert_eq!(pb.text, "X is Y");
    }
}
