-- Cycle 30 / cluster #9 — Versioned change history persistence.
--
-- Backs `PostgresBaselineStore`. Tables:
--
--   * `quarry_baselines`      — one row per captured version of a URL.
--   * `quarry_change_diffs`   — computed diff between two baselines.
--
-- ## Per-URL chain
--
-- For a given `(org_id, source_url)`, the most-recent baseline is the
-- one with the largest `captured_at`. The `prev_baseline_id` field
-- forms a singly-linked chain so walking history is a series of PK
-- probes rather than a `WHERE source_url = $1` scan.
--
-- ## Tenant isolation
--
-- Every read MUST filter `WHERE org_id = $verified`. The index
-- `quarry_baselines_org_url_idx` is org-prefixed so cross-tenant
-- scans are statically impossible.

CREATE TABLE IF NOT EXISTS quarry_baselines (
    baseline_id        TEXT         PRIMARY KEY,
    org_id             TEXT         NOT NULL,
    source_url         TEXT         NOT NULL,
    fingerprint        TEXT         NOT NULL,
    artifact_id        TEXT,
    prev_baseline_id   TEXT REFERENCES quarry_baselines(baseline_id),
    run_id             TEXT,
    captured_at        TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    CHECK (org_id <> '')
);

-- "Latest baseline for this URL" — the planner picks the index for an
-- ORDER BY captured_at DESC LIMIT 1 lookup.
CREATE INDEX IF NOT EXISTS quarry_baselines_org_url_idx
    ON quarry_baselines (org_id, source_url, captured_at DESC);

-- History walks: `SELECT * FROM quarry_baselines WHERE prev_baseline_id = $1`.
CREATE INDEX IF NOT EXISTS quarry_baselines_prev_idx
    ON quarry_baselines (prev_baseline_id)
    WHERE prev_baseline_id IS NOT NULL;

CREATE TABLE IF NOT EXISTS quarry_change_diffs (
    diff_id            TEXT         PRIMARY KEY,
    org_id             TEXT         NOT NULL,
    from_baseline_id   TEXT         NOT NULL REFERENCES quarry_baselines(baseline_id),
    to_baseline_id     TEXT         NOT NULL REFERENCES quarry_baselines(baseline_id),
    source_url         TEXT         NOT NULL,
    format             TEXT         NOT NULL,   -- "text" | "markdown" | "html" | "json-patch"
    artifact_id        TEXT         NOT NULL,
    summary            TEXT,
    created_at         TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    CHECK (org_id <> ''),
    CHECK (from_baseline_id <> to_baseline_id)
);

CREATE INDEX IF NOT EXISTS quarry_change_diffs_org_url_idx
    ON quarry_change_diffs (org_id, source_url, created_at DESC);
