-- Org-scoped skill bundles (WS19-S6): the files of a skill package, stored by digest.
--
-- One bundle per (org, skill): a new upload replaces the bundle in one transaction. The
-- bundle row holds the manifest digest and the totals; skill_bundle_files holds one row per
-- file, and skill_bundle_blobs holds each distinct file body once per org, keyed by its
-- sha256. Blobs are never global: two orgs that upload the same bytes store two rows.
--
-- Nothing here touches agent_skills. A bundle is keyed by the skill id it was uploaded for
-- and carries no foreign key into the skills table, which lives in another database's
-- schema. Reads are gated by SKILL_BUNDLES_ENABLED in capability-core, so applying this
-- migration changes no decision anywhere.
--
-- RE-RUN SAFETY. capability-core re-applies every *.up.sql at every boot, so everything
-- here is idempotent.

CREATE TABLE IF NOT EXISTS skill_bundles (
    id              TEXT PRIMARY KEY,
    org_id          TEXT NOT NULL,
    skill_id        TEXT NOT NULL,
    manifest_sha256 TEXT NOT NULL,
    file_count      INTEGER NOT NULL,
    total_bytes     BIGINT NOT NULL,
    created_by      TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT skill_bundles_manifest_check CHECK (manifest_sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT skill_bundles_file_count_check CHECK (file_count BETWEEN 1 AND 256),
    CONSTRAINT skill_bundles_total_bytes_check CHECK (total_bytes BETWEEN 0 AND 8388608)
);

CREATE UNIQUE INDEX IF NOT EXISTS skill_bundles_org_skill_uq
    ON skill_bundles (org_id, skill_id);

CREATE TABLE IF NOT EXISTS skill_bundle_files (
    bundle_id   TEXT NOT NULL REFERENCES skill_bundles (id) ON DELETE CASCADE,
    path        TEXT NOT NULL,
    sha256      TEXT NOT NULL,
    size_bytes  INTEGER NOT NULL,
    executable  BOOLEAN NOT NULL DEFAULT FALSE,
    -- is_binary, not binary: BINARY is a reserved word in PostgreSQL and cannot name a column.
    is_binary   BOOLEAN NOT NULL DEFAULT FALSE,
    PRIMARY KEY (bundle_id, path),
    CONSTRAINT skill_bundle_files_sha256_check CHECK (sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT skill_bundle_files_size_check CHECK (size_bytes BETWEEN 0 AND 1048576)
);

CREATE INDEX IF NOT EXISTS skill_bundle_files_sha256_idx
    ON skill_bundle_files (sha256);

CREATE TABLE IF NOT EXISTS skill_bundle_blobs (
    org_id      TEXT NOT NULL,
    sha256      TEXT NOT NULL,
    content     BYTEA NOT NULL,
    size_bytes  INTEGER NOT NULL,
    PRIMARY KEY (org_id, sha256),
    CONSTRAINT skill_bundle_blobs_sha256_check CHECK (sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT skill_bundle_blobs_size_check CHECK (size_bytes BETWEEN 0 AND 1048576)
);
