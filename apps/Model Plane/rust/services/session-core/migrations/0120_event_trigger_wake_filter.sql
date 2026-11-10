-- 0120: the wake filter of an event trigger (Wave 4 EP-7, F-t3-047, slice W4-F T5).
--
-- A trigger may carry one wake filter: a closed, deterministic predicate over the webhook event a fire is for. Session
-- Core checks the filter against its closed version 1 schema and stores it here in canonical form (event_triggers.rs).
-- capability-core's sweeper reads the column only when EVENT_TRIGGER_WAKE_GATE_ENABLED is on. Then a fire whose event
-- the filter does not match is refused as wake_gate_closed before Control is asked, and a fire whose payload cannot be
-- read stays pending for a later sweep. The filter grants no authority: Control still authorises every fire that passes,
-- and no model reads the payload.
--
-- Zero Data Retention: the column holds the member's own filter, which is closed tokens, dotted field paths and short
-- literals the member typed. It never holds webhook content. A ZDR or waived caller is refused on every RPC before the
-- column is read or written, so no ZDR content reaches it. Erasure: rows still go with their org.
--
-- Additive only: one nullable column with no default (a metadata-only change, no rewrite). Every existing trigger stays
-- valid with NULL, which means no filter, so every fire is decided as before. The check accepts only a JSON object and
-- bounds its stored text at 16 KiB: Session Core writes at most 4 KiB, and Postgres re-spacing the stored JSON at most
-- doubles it. The reversal is in revert/0120_event_trigger_wake_filter.down.sql.

ALTER TABLE event_triggers
    ADD COLUMN IF NOT EXISTS wake_filter JSONB NULL
        CHECK (wake_filter IS NULL
               OR (jsonb_typeof(wake_filter) = 'object' AND octet_length(wake_filter::text) <= 16384));
