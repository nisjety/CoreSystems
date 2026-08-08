-- 014: bounded, trusted email threading provenance.
--
-- Gmail groups API-sent replies only when the provider thread identifier and
-- RFC message references agree. These fields are populated exclusively from
-- the signed email-ingest bridge; browser clients never write them.

ALTER TABLE conversation_channel_thread_refs
    ADD COLUMN IF NOT EXISTS reply_to_message_id TEXT NOT NULL DEFAULT '';

ALTER TABLE conversation_channel_thread_refs
    ADD COLUMN IF NOT EXISTS references_header TEXT NOT NULL DEFAULT '';

ALTER TABLE conversation_channel_thread_refs
    ADD CONSTRAINT conversation_channel_thread_refs_reply_to_message_id_length
    CHECK (char_length(reply_to_message_id) <= 4096);

ALTER TABLE conversation_channel_thread_refs
    ADD CONSTRAINT conversation_channel_thread_refs_references_header_length
    CHECK (char_length(references_header) <= 8192);
