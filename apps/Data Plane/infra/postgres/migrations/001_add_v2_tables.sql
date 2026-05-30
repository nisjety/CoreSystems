-- Migration: Add Data Plane V2 Tables for Traces, Graph, Wiki
-- Description: Adds retrieval trace audit trail, entity graph storage, and wiki/knowledge base tables
-- Version: 1.0
-- Created: 2026-05-06

-- ────────────────────────────────────────────────────────────────────────────
-- RETRIEVAL TRACE TABLES (for audit trail and analytics)
-- ────────────────────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS retrieval_runs (
  trace_id UUID PRIMARY KEY,
  org_id VARCHAR(255) NOT NULL,
  query TEXT NOT NULL,
  query_embedding_model VARCHAR(255),
  index_version VARCHAR(255),
  filters_json JSONB,
  
  reranker_name VARCHAR(255),
  reranker_model VARCHAR(255),
  zdr_mode VARCHAR(50),  -- "disabled" | "ephemeral_only"
  
  created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
  
  -- Timing metrics
  dense_retrieval_ms INT,
  sparse_retrieval_ms INT,
  rerank_ms INT,
  total_ms INT,
  
  -- Pagination metadata
  top_k INT,
  
  CONSTRAINT fk_retrieval_runs_org FOREIGN KEY (org_id) REFERENCES organizations(id) ON DELETE CASCADE
);

CREATE INDEX idx_retrieval_runs_org_id ON retrieval_runs(org_id);
CREATE INDEX idx_retrieval_runs_created_at ON retrieval_runs(created_at DESC);
CREATE INDEX idx_retrieval_runs_zdr ON retrieval_runs(org_id, zdr_mode);

-- ────────────────────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS retrieval_candidates (
  id BIGSERIAL PRIMARY KEY,
  trace_id UUID NOT NULL,
  rank INT NOT NULL,
  
  knowledge_id VARCHAR(255),
  document_id VARCHAR(255),
  dense_score FLOAT,
  sparse_score FLOAT,
  rerank_score FLOAT,
  final_score FLOAT,
  
  source_chunk_ref VARCHAR(255),
  
  CONSTRAINT fk_retrieval_candidates_trace FOREIGN KEY (trace_id) REFERENCES retrieval_runs(trace_id) ON DELETE CASCADE,
  CONSTRAINT fk_retrieval_candidates_doc FOREIGN KEY (document_id) REFERENCES documents(id) ON DELETE SET NULL
);

CREATE INDEX idx_retrieval_candidates_trace ON retrieval_candidates(trace_id);
CREATE INDEX idx_retrieval_candidates_rank ON retrieval_candidates(trace_id, rank);

-- ────────────────────────────────────────────────────────────────────────────
-- GRAPH TABLES (entities, relationships, claims)
-- ────────────────────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS graph_entities (
  entity_id UUID PRIMARY KEY,
  org_id VARCHAR(255) NOT NULL,
  entity_type VARCHAR(100) NOT NULL,  -- "person" | "organization" | "location" | etc.
  entity_text TEXT NOT NULL,
  
  confidence FLOAT,
  provenance VARCHAR(50),  -- "extracted" | "inferred" | "ambiguous"
  source_refs JSONB,  -- array of chunk IDs
  
  metadata JSONB,
  created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
  
  CONSTRAINT fk_graph_entities_org FOREIGN KEY (org_id) REFERENCES organizations(id) ON DELETE CASCADE
);

CREATE INDEX idx_graph_entities_org ON graph_entities(org_id);
CREATE INDEX idx_graph_entities_type ON graph_entities(org_id, entity_type);
CREATE INDEX idx_graph_entities_text ON graph_entities USING GIN(to_tsvector('english', entity_text));

-- ────────────────────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS graph_relationships (
  rel_id UUID PRIMARY KEY,
  org_id VARCHAR(255) NOT NULL,
  
  entity_a_id UUID NOT NULL,
  entity_b_id UUID NOT NULL,
  relation_type VARCHAR(100) NOT NULL,  -- "is_part_of" | "works_for" | "mentions" | etc.
  
  confidence FLOAT,
  provenance VARCHAR(50),  -- "extracted" | "inferred"
  source_refs JSONB,  -- array of chunk IDs
  
  metadata JSONB,
  created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
  
  CONSTRAINT fk_graph_rel_org FOREIGN KEY (org_id) REFERENCES organizations(id) ON DELETE CASCADE,
  CONSTRAINT fk_graph_rel_entity_a FOREIGN KEY (entity_a_id) REFERENCES graph_entities(entity_id) ON DELETE CASCADE,
  CONSTRAINT fk_graph_rel_entity_b FOREIGN KEY (entity_b_id) REFERENCES graph_entities(entity_id) ON DELETE CASCADE
);

CREATE INDEX idx_graph_relationships_org ON graph_relationships(org_id);
CREATE INDEX idx_graph_relationships_entity_a ON graph_relationships(entity_a_id);
CREATE INDEX idx_graph_relationships_entity_b ON graph_relationships(entity_b_id);
CREATE INDEX idx_graph_relationships_type ON graph_relationships(org_id, relation_type);

-- ────────────────────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS graph_claims (
  claim_id UUID PRIMARY KEY,
  org_id VARCHAR(255) NOT NULL,
  
  claim_text TEXT NOT NULL,
  entity_ids JSONB,  -- array of entity_id UUIDs
  
  confidence FLOAT,
  provenance VARCHAR(50),  -- "extracted" | "inferred"
  source_refs JSONB,  -- array of chunk IDs
  
  contradicted_by_claim_ids JSONB,  -- array of claim_id UUIDs
  claim_status VARCHAR(50),  -- "active" | "disputed" | "resolved"
  
  metadata JSONB,
  created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
  
  CONSTRAINT fk_graph_claims_org FOREIGN KEY (org_id) REFERENCES organizations(id) ON DELETE CASCADE
);

CREATE INDEX idx_graph_claims_org ON graph_claims(org_id);
CREATE INDEX idx_graph_claims_status ON graph_claims(org_id, claim_status);
CREATE INDEX idx_graph_claims_text ON graph_claims USING GIN(to_tsvector('english', claim_text));

-- ────────────────────────────────────────────────────────────────────────────
-- WIKI TABLES (knowledge base pages and versions)
-- ────────────────────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS wiki_pages (
  page_id UUID PRIMARY KEY,
  org_id VARCHAR(255) NOT NULL,
  workspace_id VARCHAR(255) NOT NULL,
  
  title VARCHAR(500) NOT NULL,
  path VARCHAR(500) NOT NULL,  -- /engineering/onboarding
  
  current_version_id UUID,
  page_status VARCHAR(50),  -- "draft" | "published" | "deprecated"
  
  backlinks JSONB,  -- array of page_ids
  metadata JSONB,
  
  created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
  updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
  
  CONSTRAINT fk_wiki_pages_org FOREIGN KEY (org_id) REFERENCES organizations(id) ON DELETE CASCADE
);

CREATE INDEX idx_wiki_pages_org ON wiki_pages(org_id);
CREATE INDEX idx_wiki_pages_workspace ON wiki_pages(org_id, workspace_id);
CREATE INDEX idx_wiki_pages_path ON wiki_pages(org_id, path);
CREATE INDEX idx_wiki_pages_status ON wiki_pages(org_id, page_status);

-- ────────────────────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS wiki_page_versions (
  version_id UUID PRIMARY KEY,
  page_id UUID NOT NULL,
  
  content TEXT,  -- markdown
  source_refs JSONB,  -- array of chunk IDs
  
  proposed_by_agent VARCHAR(255),
  proposed_by_user VARCHAR(255),
  approved_by VARCHAR(255),
  
  edit_reason VARCHAR(100),  -- "initial_synthesis" | "manual_edit" | "refresh" | "correction"
  version_status VARCHAR(50),  -- "draft" | "approved" | "published"
  
  metadata JSONB,
  created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
  published_at TIMESTAMP WITH TIME ZONE,
  
  CONSTRAINT fk_wiki_versions_page FOREIGN KEY (page_id) REFERENCES wiki_pages(page_id) ON DELETE CASCADE
);

CREATE INDEX idx_wiki_versions_page ON wiki_page_versions(page_id);
CREATE INDEX idx_wiki_versions_created ON wiki_page_versions(page_id, created_at DESC);
CREATE INDEX idx_wiki_versions_status ON wiki_page_versions(page_id, version_status);

-- ────────────────────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS wiki_source_logs (
  log_id UUID PRIMARY KEY,
  page_id UUID NOT NULL,
  
  original_chunks JSONB,  -- array of chunk IDs/refs
  processing_model VARCHAR(255),  -- model used for synthesis
  synthesis_prompt_hash VARCHAR(64),  -- SHA256 of prompt for reproducibility
  
  metadata JSONB,
  created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
  
  CONSTRAINT fk_wiki_source_logs_page FOREIGN KEY (page_id) REFERENCES wiki_pages(page_id) ON DELETE CASCADE
);

CREATE INDEX idx_wiki_source_logs_page ON wiki_source_logs(page_id);

-- ────────────────────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS wiki_maintenance_logs (
  log_id UUID PRIMARY KEY,
  page_id UUID NOT NULL,
  
  issue_type VARCHAR(100),  -- "stale_content" | "missing_section" | "broken_reference" | "conflicting_info"
  issue_details JSONB,
  proposed_fix TEXT,
  
  issue_status VARCHAR(50),  -- "open" | "in_progress" | "resolved" | "ignored"
  
  metadata JSONB,
  created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
  resolved_at TIMESTAMP WITH TIME ZONE,
  
  CONSTRAINT fk_wiki_maintenance_page FOREIGN KEY (page_id) REFERENCES wiki_pages(page_id) ON DELETE CASCADE
);

CREATE INDEX idx_wiki_maintenance_page ON wiki_maintenance_logs(page_id);
CREATE INDEX idx_wiki_maintenance_status ON wiki_maintenance_logs(page_id, issue_status);

-- ────────────────────────────────────────────────────────────────────────────
-- AUDIT & MIGRATION TRACKING
-- ────────────────────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS v2_schema_migrations (
  migration_id SERIAL PRIMARY KEY,
  name VARCHAR(500) NOT NULL UNIQUE,
  applied_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
  description TEXT
);

INSERT INTO v2_schema_migrations (name, description) VALUES
  ('001_add_v2_tables', 'Add retrieval traces, graph entities, and wiki tables for Data Plane V2')
ON CONFLICT (name) DO NOTHING;
