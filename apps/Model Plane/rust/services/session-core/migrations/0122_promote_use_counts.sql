-- 0122: durable promote-on-use counter (Wave 5, W5-093-b, F-t3-093).
--
-- execution-core counts a grounded use of a web page per (org, user, url) and, at a threshold, promotes the page
-- into the knowledge base. The count was process-local; with PROMOTE_ON_USE_DURABLE on, it is kept here so a
-- restart does not reset it. One row per (org, user, URL hash). `promoted_at` is set in the same statement that
-- takes the count to the threshold, which is what makes the promote fire exactly once.
--
-- Zero Data Retention: the row holds ids, the SHA-256 of the normalised URL, a count and two times. The URL is
-- never sent to this table, so nothing here can be read back as a page address. A zero-retention or waived
-- credential is refused on the RPC (`zdr_durable_mutation_forbidden`), so no ZDR run writes a row. Erasure: the
-- org purge deletes the org's rows (gdpr.rs).
--
-- Additive only: a new, empty table. The reversal is in revert/0122_promote_use_counts.down.sql.

CREATE TABLE IF NOT EXISTS promote_use_counts (
    org_id TEXT NOT NULL CHECK (length(org_id) BETWEEN 1 AND 128),
    user_id TEXT NOT NULL CHECK (length(user_id) BETWEEN 1 AND 256),
    -- SHA-256 of the normalised URL: always 32 bytes.
    url_hash BYTEA NOT NULL CHECK (octet_length(url_hash) = 32),
    count INT NOT NULL DEFAULT 0 CHECK (count >= 0),
    -- Set once, when the count first reaches the threshold; null until then.
    promoted_at TIMESTAMPTZ NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (org_id, user_id, url_hash)
);
