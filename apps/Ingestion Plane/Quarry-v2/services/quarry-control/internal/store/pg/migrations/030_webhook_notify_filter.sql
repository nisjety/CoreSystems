-- E3.5-7: notifyOnChange filter on a webhook subscription (control migration 030).
--
-- A subscription may carry one filter: the change types it takes (new, changed, removed) and, for a
-- changed page, the fewest paragraphs that must have changed. The control plane normalises the filter
-- before it writes it (resources.normalizeChangeNotifyFilter), so the document is stored as JSONB and
-- this column does not repeat that check. NULL is no filter: the subscription takes every change, as it
-- did before this migration, and every row written before it reads back NULL.
--
-- The column sits on webhooks, the org-scoped subscription table of migration 025, so an org erasure
-- already removes it with the row. Numbering: 029 is the job webhook deliveries of W3-E3.5-3c.
--
-- Idempotent: ADD COLUMN IF NOT EXISTS, so a re-run changes nothing.

ALTER TABLE webhooks ADD COLUMN IF NOT EXISTS notify_on_change JSONB NULL;
