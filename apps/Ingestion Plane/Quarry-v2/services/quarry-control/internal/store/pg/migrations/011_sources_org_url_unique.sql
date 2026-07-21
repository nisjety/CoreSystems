-- 011: enforce one durable Source row per (org_id, url).
--
-- Backs `sourcesStore.UpsertByOrgAndURL`'s `ON CONFLICT (org_id, url)`
-- target (cycle23.go `createSourceHandler` / crawl-completion tracked-source
-- materialization — 2026-07-20 Aquatiq crawl-to-KB audit). Before this,
-- repeat POST /v1/sources calls for the same website — one per crawled
-- page, since quarry-runtime's PageRunner now registers on every successful
-- ingest — would have inserted a duplicate row per page instead of
-- collapsing into one "tracked website" entry.
--
-- Partial (WHERE deleted_at IS NULL) so a soft-deleted row never blocks
-- re-registering the same URL later.
CREATE UNIQUE INDEX IF NOT EXISTS quarry_sources_org_url_uniq
    ON quarry_sources (org_id, url)
    WHERE deleted_at IS NULL;
