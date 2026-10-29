-- Eval golden sets — human/agent-judged relevant ids per query, so retrieval
-- evals compute REAL recall@10/nDCG@10/MRR instead of candidate-count proxies.
CREATE TABLE IF NOT EXISTS eval_golden_judgments (
    org_id       TEXT         NOT NULL,
    query_norm   TEXT         NOT NULL,
    relevant_ids JSONB        NOT NULL,
    created_at   TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    updated_at   TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    PRIMARY KEY (org_id, query_norm)
);
