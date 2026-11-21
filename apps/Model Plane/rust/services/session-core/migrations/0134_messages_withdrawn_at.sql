-- 0134: a message the person took back before the run read it (build plan O-01, DEC-F01-03).
--
-- A message sent while a run is working is stored under its client_message_id before it is delivered (WS04 S6,
-- the 2026-08-24 "persist before deliver" ledger entry). With MODEL_GATEWAY_QUEUE_RETRACT on, the person can take
-- it back while the gateway still holds it, and the WithdrawMessage call stamps this column on the stored row.
-- The same statement clears the row's text and metadata (as a deleted Space message is cleared) and takes it off
-- the active path, so every reader that already leaves an inactive turn out leaves a withdrawn one out too. The
-- thread view alone asks for the withdrawn rows (ListConversation include_withdrawn_notes) and shows one text-free
-- "retracted" note in each one's place.
--
-- Content: one timestamp, no text. Additive: a nullable column with no default and no backfill, so every existing
-- row reads as not withdrawn and an older binary keeps working on it. Erasure: it lives on the message row, so
-- deleting the message or the org's threads (gdpr.rs) deletes it with the row. The reversal is in
-- revert/0134_messages_withdrawn_at.down.sql.

ALTER TABLE messages ADD COLUMN IF NOT EXISTS withdrawn_at TIMESTAMPTZ;
