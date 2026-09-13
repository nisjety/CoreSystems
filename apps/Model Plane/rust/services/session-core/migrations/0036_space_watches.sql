-- 0036_space_watches.sql
--
-- S4.3 step 1: the general Watch primitive's record and its event log.
--
-- A Watch is a standing intent to be told when something changes. It is
-- deliberately NOT a delivery mechanism (S4.4 owns that), NOT a live tail
-- (a watch has lag by construction), and NOT a topic subscription — it is
-- bound to ONE resource in ONE Space under ONE member's authority.
--
-- WHY THESE TABLES LIVE HERE AND NOT IN capability-core/migrations
--
-- Same split cron_schedules already uses: the schema is a session-core
-- migration (this file), the rows are operated by capability-core's Go
-- sweeper. Both services point DATABASE_URL at the same `session_core`
-- database, so this is one plane's schema shared inside that plane, not a
-- cross-plane database crossing. Putting it in capability-core's own
-- migrations directory would create a second migrator against the same
-- database, which is how two services end up disagreeing about whether a
-- column exists.
--
-- WHY capability-core OWNS WATCHES AT ALL
--
-- A Watch is structurally a Schedule. A cron_schedules row is created by a
-- Space member under a Control decision, stores the Space ref and all five
-- authority revisions, is advanced by a sweeper claiming rows FOR UPDATE SKIP
-- LOCKED, and is reauthorized freshly per fire because a long-lived record is
-- not authority by itself. Every one of those sentences is true of a watch.
-- The only difference is the trigger: a clock, versus a source's cursor
-- moving. See the 2026-09-14 ledger entry and the S4.3 design doc.
--
-- NOTHING READS THESE TABLES YET. Step 1 ships the record, the state machine
-- and the store; step 2 adds the first adapter, step 3 the create path.

-- ---------------------------------------------------------------------------
-- space_watches
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS space_watches (
    id                          TEXT PRIMARY KEY,
    org_id                      TEXT NOT NULL,
    space_ref                   TEXT NOT NULL,
    creator_subject_id          TEXT NOT NULL,

    -- WHAT is being watched. `source_kind` selects the adapter; `source_ref`
    -- is that adapter's own identifier for the thing.
    --
    -- Two columns rather than one opaque URI on purpose: a URI invites
    -- parsing, and an adapter that parses its own identifier out of a string
    -- is an adapter that can be handed someone else's. The Space check the
    -- sweeper performs compares source_ref against the resource's OWN recorded
    -- Space, so source_ref is caller-supplied and never trusted.
    source_kind                 TEXT NOT NULL,
    source_ref                  TEXT NOT NULL,

    -- The bounded predicate. One per watch: "any of these five things" is five
    -- watches, which keeps evaluation, authority and cancellation per-answer
    -- rather than per-set.
    --
    -- NO REGULAR EXPRESSIONS, and the constraint below is what keeps it that
    -- way. A predicate runs against output a model wrote, on a shared sweeper,
    -- where a model-created watch could supply both the pattern and the
    -- subject — an attacker on both ends of a ReDoS. A literal substring is not
    -- interesting enough to be dangerous and covers what people actually want.
    predicate_kind              TEXT NOT NULL,
    predicate_value             TEXT NOT NULL DEFAULT '',
    predicate_stream            TEXT NOT NULL DEFAULT '',

    -- WHERE the cursor is. Opaque to the core, meaningful to the adapter; for
    -- process output it is the registry's own seq. The cursor IS the contract:
    -- an adapter that cannot say "I have consumed up to here, durably" cannot
    -- be a watch source, because crash-before-commit, duplicate-event and
    -- unwatch-race are all statements about a cursor.
    cursor_value                BIGINT NOT NULL DEFAULT 0,
    cursor_committed_at         TIMESTAMPTZ,

    -- 1 ACTIVE, 2 TRIGGERED, 3 EXPIRED, 4 CANCELLED, 5 SOURCE_GONE.
    -- Every terminal state is terminal; re-watching is a NEW row, because the
    -- authority that justified the first is not the authority that justifies
    -- the second.
    state                       SMALLINT NOT NULL DEFAULT 1,
    trigger_mode                TEXT NOT NULL DEFAULT 'once',

    -- Scheduling and backoff. next_poll_at is BOTH the due-time and the claim:
    -- the sweeper advances it inside the short claiming transaction, before it
    -- does any network work, so a second replica will not re-pick the row and
    -- no transaction is held open across an RPC. A sweeper that dies after
    -- claiming costs that watch one interval, which is the right trade against
    -- holding row locks across a remote call.
    --
    -- idle_polls is the backoff LEVEL, not a stored duration: the interval is
    -- derived as min(base * 2^idle_polls, ceiling), so there is one source of
    -- truth rather than a counter and a duration that can disagree.
    next_poll_at                TIMESTAMPTZ NOT NULL DEFAULT now(),
    idle_polls                  INT NOT NULL DEFAULT 0,
    last_polled_at              TIMESTAMPTZ,

    -- Liveness. last_event_summary is redacted and bounded — it is a label for
    -- a human scanning a list, never the raw matched payload.
    last_event_at               TIMESTAMPTZ,
    last_event_summary          TEXT NOT NULL DEFAULT '',
    consecutive_failures        INT NOT NULL DEFAULT 0,
    expires_at                  TIMESTAMPTZ NOT NULL,

    -- WHERE an event should go. A reference S4.4 will resolve; empty until
    -- then, which is the honest state rather than a placeholder. A watch that
    -- notified today would promise at-least-once delivery with no outbox,
    -- claim or receipt.
    delivery_target_ref         TEXT NOT NULL DEFAULT '',

    -- The authority binding, identical in shape to cron_schedules' 0028
    -- columns and for the identical reason: the record is not the authority,
    -- and re-deriving it at emission time needs something to compare against.
    -- A changed recipient_audience_revision means the Space's disclosure set
    -- moved under the watch, and the watch stops rather than emitting.
    recipient_audience_ref      TEXT NOT NULL DEFAULT '',
    recipient_audience_hash     TEXT NOT NULL DEFAULT '',
    resource_authorization_ref  TEXT NOT NULL DEFAULT '',
    privacy_policy_ref          TEXT NOT NULL DEFAULT '',
    authority_revision          BIGINT NOT NULL DEFAULT 0,
    membership_revision         BIGINT NOT NULL DEFAULT 0,
    privacy_revision            BIGINT NOT NULL DEFAULT 0,
    recipient_audience_revision BIGINT NOT NULL DEFAULT 0,
    entitlement_revision        BIGINT NOT NULL DEFAULT 0,

    created_at                  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at                  TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at                  TIMESTAMPTZ,

    CONSTRAINT space_watches_state_check
        CHECK (state BETWEEN 1 AND 5),
    CONSTRAINT space_watches_trigger_mode_check
        CHECK (trigger_mode IN ('once', 'continuous')),
    -- The closed predicate vocabulary, enforced by the database and not only
    -- by the Go that writes it. A future adapter that wants a new predicate
    -- has to change this line, which is the review it deserves.
    CONSTRAINT space_watches_predicate_kind_check
        CHECK (predicate_kind IN ('any', 'contains', 'state_change')),
    -- A `contains` predicate needs a value and nothing else may carry one:
    -- a value on an `any` watch would look like a filter that is not applied.
    CONSTRAINT space_watches_predicate_shape_check
        CHECK (
            (predicate_kind = 'contains' AND predicate_value <> '')
            OR (predicate_kind <> 'contains' AND predicate_value = '')
        ),
    CONSTRAINT space_watches_predicate_value_bounded_check
        CHECK (length(predicate_value) <= 200),
    CONSTRAINT space_watches_predicate_stream_check
        CHECK (predicate_stream IN ('', 'stdout', 'stderr')),
    -- A Space-bound record with no Space is the shape every "it worked in dev"
    -- authority bug starts from.
    CONSTRAINT space_watches_space_scoped_check
        CHECK (space_ref <> '' AND org_id <> '' AND creator_subject_id <> ''),
    CONSTRAINT space_watches_source_check
        CHECK (source_kind <> '' AND source_ref <> '')
);

-- The sweeper's only hot query: ACTIVE watches whose next_poll_at has passed.
-- Partial on state and deleted_at so a Space with a thousand finished watches
-- costs nothing to sweep.
CREATE INDEX IF NOT EXISTS space_watches_due_idx
    ON space_watches (next_poll_at)
    WHERE state = 1 AND deleted_at IS NULL;

-- The read path (step 4) lists a Space's watches newest-first.
CREATE INDEX IF NOT EXISTS space_watches_space_idx
    ON space_watches (org_id, space_ref, id DESC)
    WHERE deleted_at IS NULL;

-- One ACTIVE watch per (Space, source, predicate, creator). Watching the same
-- thing for the same reason twice is a duplicate, not two answers — and
-- without this a UI retry silently doubles the events a person receives.
-- Terminal watches fall outside it, so re-watching after one ends is allowed.
CREATE UNIQUE INDEX IF NOT EXISTS space_watches_active_uq
    ON space_watches (org_id, space_ref, source_kind, source_ref,
                      predicate_kind, predicate_value, predicate_stream,
                      creator_subject_id)
    WHERE state = 1 AND deleted_at IS NULL;

-- ---------------------------------------------------------------------------
-- space_watch_events
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS space_watch_events (
    id             TEXT PRIMARY KEY,
    watch_id       TEXT NOT NULL REFERENCES space_watches(id),
    org_id         TEXT NOT NULL,

    -- The cursor position this event was emitted AT. With watch_id and kind
    -- this is unique, which is what turns a redelivered poll into a no-op
    -- instead of a duplicate event — the same ON CONFLICT DO NOTHING move
    -- S4.2's output append uses, for the same reason.
    cursor_value   BIGINT NOT NULL,
    kind           TEXT NOT NULL,

    -- Redacted and bounded. The source already scrubbed it at a line boundary
    -- (S4.2), and this caps what a single event can carry into a list.
    summary        TEXT NOT NULL,

    -- Content provenance, as a COLUMN rather than a convention.
    --
    -- 'unscreened_source_payload' is anything derived from watched content:
    -- bytes a model chose, from a program a model wrote. Redaction is not
    -- trust — a scrubbed line is still attacker-chosen text. 'owner_metadata'
    -- is a fact the owning plane asserts (a state change, an exit code).
    --
    -- A consumer that cannot tell these apart will eventually render one as
    -- the other, and a watch is the first thing that carries process output
    -- OUT of the run that produced it.
    trust          TEXT NOT NULL,

    -- S4.4 owns everything past 'recorded'.
    delivery_state TEXT NOT NULL DEFAULT 'recorded',
    emitted_at     TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT space_watch_events_kind_check
        CHECK (kind IN ('match', 'state_change', 'gap', 'source_gone', 'expired')),
    CONSTRAINT space_watch_events_trust_check
        CHECK (trust IN ('unscreened_source_payload', 'owner_metadata')),
    CONSTRAINT space_watch_events_summary_bounded_check
        CHECK (length(summary) <= 256),
    CONSTRAINT space_watch_events_delivery_state_check
        CHECK (delivery_state = 'recorded')
);

CREATE UNIQUE INDEX IF NOT EXISTS space_watch_events_cursor_uq
    ON space_watch_events (watch_id, cursor_value, kind);

CREATE INDEX IF NOT EXISTS space_watch_events_watch_idx
    ON space_watch_events (watch_id, emitted_at DESC);
