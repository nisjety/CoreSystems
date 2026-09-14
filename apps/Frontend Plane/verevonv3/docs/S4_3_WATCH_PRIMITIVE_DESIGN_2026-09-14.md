# S4.3 — the general Watch primitive

**Status:** proposed, 2026-09-14. Successor to S4.2 (`S4_2_PROCESS_REGISTRY_DESIGN_2026-09-13.md`), which was its sole blocker.

**Scope of this document:** the Watch core and its first adapter (process output), plus the read path that makes a watch visible. The plan's remaining adapters — Model runs, Ingestion jobs, Data document/source changes, deployments, connector events — are specified only far enough to prove the core does not have to change to accept them. Each is its own slice, and none is in this pass.

---

## 1. What a Watch is, and what it is not

The adoption plan (`VEREVON_QM_COMPARISON_AND_ADOPTION_PLAN_2026-08-13.md:2200-2210`) asks for:

> Define the core Watch record/state machine with Space/run/source/resource authority, bounded predicate grammar, cursor, heartbeat, expiry, destination, last event, cancellation, and content trust/provenance.

A Watch is a **standing intent to be told when something changes**. That is the whole primitive. It is deliberately NOT:

- **a delivery mechanism.** S4.4 owns delivery, binding work to an Application-owned `DeliveryTarget` with an outbox, claim, receipt and an honest `unknown` outcome. A watch records *where* an event should go as a reference; it does not promise the event arrives. Building any part of that promise here would pre-empt a design that has not been made.
- **a poller with a callback.** The cursor is the contract. An adapter that cannot say "I have consumed up to here, durably" cannot be a watch source, because every verification case the plan lists — crash before cursor commit, duplicate event, unwatch race — is a statement about a cursor.
- **a subscription to a topic.** A watch is bound to one *resource* in one Space under one member's authority. "Tell me about everything in this org" is not expressible, and that is the point.

### 1.1 The thing S4.3 is actually for

S4.2 made a background process outlive the turn that started it. That immediately created a room where work is happening and **nobody is watching** — the exact failure the Work tab exists to avoid, now with a live process behind it. A watch is how a person (or the agent acting for them) says "that build — tell me when it breaks" and then stops holding the question open in their head.

That framing is why process output is the first adapter rather than an arbitrary choice: it is the source S4.2 just created, and the only one where "nobody is watching" is a new problem rather than an old one.

---

## 2. Ownership: capability-core, and the evidence for it

`docs/capability-ownership-matrix.md` has no watch row today, and `docs/decisions/ledger.md` records no prior ruling, so this is an open question this document has to answer rather than look up.

**A Watch is structurally a Schedule.** Compare what capability-core already owns:

| | `cron_schedules` (shipped) | `space_watches` (proposed) |
|---|---|---|
| created by | a Space member, under a Control decision | a Space member, under a Control decision |
| stores | Space ref, subject, five authority revisions, `delivery_target_ref`, `template_digest` | the same |
| advanced by | a sweeper claiming rows `FOR UPDATE SKIP LOCKED` | a sweeper claiming rows `FOR UPDATE SKIP LOCKED` |
| reauthorized | freshly, per fire, via `FireAuthorizer` — "a long-lived schedule record is not authority by itself" (`cron/sweeper.go:39-41`) | freshly, per emission, same rule |
| triggered by | a clock | a source's cursor moving |

The last row is the only difference. Everything else — the record shape, the authority binding, the sweeper, the "the record is not the authority" discipline — is the same, and capability-core already implements all of it (`internal/cron/{cron,sweeper,control_authorizer}.go`, session-core migrations `0004` and `0028`).

capability-core also already owns **the only watch that exists**: `run_watch_subscriptions` + `internal/runwatch`, the AUTO-2 "notify me when a run finishes" trigger. Putting the general primitive anywhere else would mean two services own watches.

**Decision: capability-core owns the Watch record and the sweeper. Rows live in the shared `session_core` Postgres alongside `cron_schedules`, created by a session-core migration — the same split that already exists for cron (schema in session-core's migrations, operated by capability-core).** No new service. This stays inside the Model Plane, so it does not cross a plane database boundary.

### 2.1 What happens to `run_watch_subscriptions`

It stays, and it is not silently duplicated.

`run_watch_subscriptions` is a **fire-once notification trigger**: one row per `(org, run, user)`, no Space, no cursor, no predicate, `pending → notified`, driven by a JetStream consumer on terminal run events. `space_watches` is a **cursor-bearing stream subscription**: Space-scoped, authority-bound, with a predicate and a resumable position. They answer different questions, and the second does not become a superset of the first until the Model-runs adapter exists.

So: build the general primitive alongside, and let the **Model-runs adapter (a later slice) be the retirement path** — at which point `run_watch_subscriptions` is replaced by a real equivalent rather than by an intention. Retiring it now would mean shipping a regression for AUTO-2's users in exchange for tidiness. The ownership matrix gets one row naming capability-core as the owner of both, with the retirement stated; this document is the record that the duplication is deliberate and bounded, not an oversight.

This follows the ledger's own precedent for the letta-bridge memory adapter and the sandbox-backend trait: *build the seam when the second real implementation lands, not before.*

---

## 3. The record

New table `space_watches`, in a session-core migration, modelled on `cron_schedules` + its `0028` Space binding.

```sql
CREATE TABLE IF NOT EXISTS space_watches (
    id                          TEXT PRIMARY KEY,        -- ULID
    org_id                      TEXT NOT NULL,
    space_ref                   TEXT NOT NULL,
    creator_subject_id          TEXT NOT NULL,

    -- WHAT is being watched. `source_kind` selects the adapter; `source_ref`
    -- is that adapter's own id for the thing. Deliberately two columns and not
    -- one opaque URI: a URI invites parsing, and an adapter that parses its own
    -- identifier out of a string is an adapter that can be handed someone
    -- else's.
    source_kind                 TEXT NOT NULL,           -- 'process_output' (slice 2); later: 'run', 'ingestion_job', ...
    source_ref                  TEXT NOT NULL,           -- e.g. a process_id

    -- The bounded predicate (§4). One row, not a table: a watch has exactly
    -- one predicate, and "any of these five things" is five watches.
    predicate_kind              TEXT NOT NULL,           -- 'any' | 'contains' | 'state_change'
    predicate_value             TEXT NOT NULL DEFAULT '',
    predicate_stream            TEXT NOT NULL DEFAULT '',-- 'stdout' | 'stderr' | '' (both)

    -- WHERE the cursor is. Opaque to the core, meaningful to the adapter; for
    -- process output it is the registry's own seq.
    cursor_value                BIGINT NOT NULL DEFAULT 0,
    cursor_committed_at         TIMESTAMPTZ,

    state                       SMALLINT NOT NULL,       -- 1 ACTIVE, 2 TRIGGERED, 3 EXPIRED, 4 CANCELLED, 5 SOURCE_GONE
    trigger_mode                TEXT NOT NULL DEFAULT 'once', -- 'once' | 'continuous'

    -- Liveness and bounds.
    last_polled_at              TIMESTAMPTZ,
    last_event_at               TIMESTAMPTZ,
    last_event_summary          TEXT NOT NULL DEFAULT '',-- redacted, bounded; never the raw matched payload
    consecutive_failures        INT NOT NULL DEFAULT 0,
    expires_at                  TIMESTAMPTZ NOT NULL,

    -- WHERE an event should go. A reference S4.4 will resolve; see §6.
    delivery_target_ref         TEXT NOT NULL DEFAULT '',

    -- The authority binding, identical in shape to cron_schedules' 0028
    -- columns, for the identical reason: the record is not the authority, and
    -- re-deriving it at emission time needs something to compare against.
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
    deleted_at                  TIMESTAMPTZ
);
```

And `space_watch_events`, the append-only record of what a watch actually emitted — the `cron_fires` equivalent, and the thing a human read path shows:

```sql
CREATE TABLE IF NOT EXISTS space_watch_events (
    id            TEXT PRIMARY KEY,
    watch_id      TEXT NOT NULL REFERENCES space_watches(id),
    org_id        TEXT NOT NULL,
    -- The cursor position this event was emitted AT. Together with watch_id
    -- this is a unique key, which is what makes a redelivered poll a no-op
    -- rather than a duplicate event — the same ON CONFLICT DO NOTHING move
    -- S4.2's output append uses, and for the same reason.
    cursor_value  BIGINT NOT NULL,
    kind          TEXT NOT NULL,          -- 'match' | 'state_change' | 'source_gone' | 'expired'
    summary       TEXT NOT NULL,          -- redacted, bounded (§5)
    trust         TEXT NOT NULL,          -- 'unscreened_source_payload' | 'owner_metadata' (§5)
    emitted_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    delivery_state TEXT NOT NULL DEFAULT 'recorded'  -- S4.4 owns anything past 'recorded'
);

CREATE UNIQUE INDEX IF NOT EXISTS space_watch_events_cursor_uq
    ON space_watch_events (watch_id, cursor_value, kind);
```

### 3.1 The state machine

```
                 ┌──────────────► CANCELLED  (a member unwatched)
                 │
   ACTIVE ───────┼──────────────► EXPIRED    (expires_at passed)
     │           │
     │           └──────────────► SOURCE_GONE (the source is terminal AND drained,
     │                                          or the resource was revoked)
     │
     └── emits ──► TRIGGERED     (trigger_mode='once' only; a 'continuous'
                                  watch emits and stays ACTIVE)
```

Every terminal state is terminal. A watch is never resurrected — re-watching is a new row, because the authority that justified the first one is not the authority that justifies the second.

`SOURCE_GONE` is its own state rather than folded into `EXPIRED`, because they mean different things to the person who set the watch: "the thing you were watching finished and you saw all of it" is an answer, and "we stopped looking" is not. The plan's verification list asks for both.

---

## 4. The bounded predicate grammar

Three kinds in this pass, and the bound is the design:

| kind | value | matches |
|---|---|---|
| `any` | — | any new output at all (a liveness watch) |
| `contains` | a literal substring, ≤ 200 bytes | a complete output line containing it |
| `state_change` | — | the source's own lifecycle state changing |

**No regular expressions, deliberately.** A watch predicate runs against output a model wrote and a process emitted — unscreened input by construction — on a shared sweeper. A regex there is a ReDoS surface where the attacker supplies *both* the pattern (via a model-created watch) and the subject (via process output). A literal substring is not interesting enough to be dangerous, and it covers the case people actually want ("tell me when it says ERROR"). If a later slice needs more, the honest additions are a fixed set of named predicates (`nonzero_exit`, `stderr_any`), not an expression language.

**`contains` matches only COMPLETE lines.** S4.2's output chunks carry `ends_with_newline` precisely so this is decidable, and its retention keeps a head and a tail so a cursor can land mid-stream. A predicate evaluated against a partial line would fire on a prefix that the next chunk completes into something else — the same class of error S4.2's scrubbing rule guards against ("a secret in half no longer matches the scrub patterns"). The adapter therefore buffers a trailing partial line and evaluates it only once it is complete or the process is terminal.

---

## 5. Content trust and provenance

The plan asks for "fail-closed/read-only handling of unscreened external payloads", and process output is exactly that: bytes a model chose, written by a program the model wrote.

Three rules:

1. **Every event is labelled.** `trust = 'unscreened_source_payload'` for anything derived from the watched content, `'owner_metadata'` for facts the owning plane asserts (a state change, an exit code). A consumer that cannot tell these apart will eventually render one as the other.
2. **The summary is bounded and already redacted.** The matched line is capped (256 bytes) and comes out of S4.2's registry, which the host scrubbed at a line boundary before it was ever stored. Redaction is not trust — a scrubbed line is still attacker-chosen text — which is why rule 1 exists alongside it.
3. **A watch event is never an instruction.** No adapter, and no consumer of `space_watch_events`, may feed a summary back into a model as anything but quoted, labelled data. This is the same boundary the repo already holds between `dispatch_tool` and `execute_step_inner`; it is written down here because a watch is the first thing that carries process output *out* of the run that produced it.

---

## 6. Destination, before S4.4 exists

`delivery_target_ref` is a reference, and in this pass **the only supported value is empty**, meaning: record the event on the watch and let the Work tab show it.

That is not a placeholder — it is the honest boundary. S4.4 owns `pending | claimed | sent_unconfirmed | acknowledged | failed | unknown` and the reconciliation that makes those states mean something. A watch that sent a notification today would be promising at-least-once delivery with no outbox, no claim, and no receipt, and the first transient notification-core outage would silently lose events. `delivery_state` starts at `'recorded'` and S4.4 owns every value past it.

The existing `runwatch` consumer keeps calling notification-core as it does today. It is not moved onto this path until S4.4 gives it somewhere better to stand.

---

## 7. The first adapter: process output

### 7.1 Polling, and the explicit budget the plan demands

The plan says: *"Prefer owner events; permit polling only behind an adapter with explicit lag/rate/budget behavior."* This adapter polls. The argument, stated so it can be disagreed with:

**S4.2 already built what a watch needs, and an event stream would re-solve it worse.** The registry has host-assigned monotonic cursors, `(process_id, seq)` idempotency, `gap_before`, `retained_from_seq`, and a terminal state on every row. `ReadProcessOutput(after_seq)` is idempotent by construction, so "crash before cursor commit" is satisfied by re-reading — there is nothing to reconcile. An event per chunk would be a firehose (the host flushes every 250ms per stream, per process), would need its own dedup and its own retention, and sandbox-manager publishes nothing to NATS today, so it would also need a publisher, a stream, and a provisioner change before the adapter could be written at all.

**The explicit lag/rate/budget behaviour:**

| bound | value | why |
|---|---|---|
| base poll interval | 2s per ACTIVE watch | a person waiting on a build tolerates 2s; a tighter loop buys nothing a human notices |
| backoff when idle | ×2 per empty poll to a 30s ceiling, reset on any output | a watch on a quiet process must not cost the same as a watch on a chatty one |
| read budget | one `ReadProcessOutput` page (≤32 KiB) per poll | the cursor carries over; a busy process is drained across polls rather than in one unbounded read |
| per-Space concurrency | at most 8 ACTIVE process-output watches | the same count-bounded shape S4.2 gave processes themselves |
| failure budget | 5 consecutive failures → `consecutive_failures` recorded and the watch backs off to the 30s ceiling; it does NOT terminate | a transient sandbox-manager outage must not silently cancel a person's watch |

**Stated cost:** up to 2s of lag on the first event, up to 30s on a process that has been quiet. A watch is not a live tail. If the Work tab later needs a live tail, that is a different mechanism (a stream the browser holds open), not a watch with a smaller interval.

### 7.2 How it reads

The sweeper claims due watches `FOR UPDATE SKIP LOCKED`, exactly as the cron sweeper does, then per watch:

1. **Reauthorize.** A watch row is not authority. The adapter obtains a fresh Control decision for the Space the same way the cron sweeper does per fire, and compares the five revisions against the ones stored on the row. A changed `recipient_audience_revision` or a revoked membership terminates the watch as `SOURCE_GONE` rather than emitting.
2. **Read** `ReadProcessOutput(process_id, after_seq = cursor_value, max_bytes = 32 KiB)` on capability-core's own `aud=sandbox-manager` service token — which it does not hold today (§8.1).
3. **Evaluate** the predicate over complete lines only, holding a trailing partial line in memory (not in the row: it is re-derivable by re-reading from the same cursor, and storing it would make the cursor lie).
4. **Emit** at most one event per poll, inserted `ON CONFLICT DO NOTHING` on `(watch_id, cursor_value, kind)`.
5. **Commit the cursor** in the same transaction as the event. This ordering is the whole correctness argument: an event without a committed cursor is re-derived and deduped by the unique index; a committed cursor without an event would silently skip a match.
6. **Honour `gap_before`.** If the registry reports the cursor fell into a retention hole, the watch emits a `kind='match'`-adjacent gap note and advances — a watch that silently skipped output would be worse than one that says it did.

### 7.3 When the process ends

A terminal process still answers reads (S4.2 guarantees a drained terminal stream returns an empty page carrying its outcome). So the adapter drains to the end, emits any final match, emits a `state_change` carrying the exit code as `trust='owner_metadata'`, and moves the watch to `SOURCE_GONE`. The order matters: terminating on the state transition before draining would lose the last lines, which is exactly where a failing build says why it failed.

---

## 8. Authority

This is the part that needs the most care, and the S4.2 experience is directly relevant: **capability-core reading a Space's process output is a disclosure**, and its service token, like execution-core's, is org-wide.

- **Creating a watch** requires a Control decision for the Space, the same shape `schedule-create-decision` has. The five revisions land on the row.
- **Emitting** requires fresh reauthorization per emission (§7.2 step 1) — the cron `FireAuthorizer` rule, unchanged.
- **Reading the source** goes through sandbox-manager's `ReadProcessOutput`, which since S4.2 step 6 admits either a service principal (unbounded) or a human bearer under an audience ceiling. capability-core is a service principal, so it reads unbounded — which means **capability-core must apply the Space check itself**, exactly as execution-core's `process_tools` does: resolve the process, refuse unless its `space_id` equals the watch's `space_ref`. The watch row's Space is trustworthy (Control signed it at create time); the `source_ref` is not, because a caller supplied it.
- **The audience ceiling applies to what a watch may SHOW, not only to what it may read.** A watch created under audience revision N must not emit content recorded under a later audience. The process row already carries `recipient_audience_revision` (S4.2 migration 0004), so the comparison is available; the watch stores its own and refuses to emit above it.

### 8.1 A deployment dependency, named because the last one of these cost a day

**capability-core cannot address sandbox-manager today.** `apps/Control Plane/config/plane-service-principals.json` — the policy half of the service-principal registry, and the reviewable half that lives in git — grants capability-core exactly three audiences: `session-core`, `inference-core`, `orchestrator-core`. Auth Core issues exactly the scopes requested, narrowed by that allowlist, so a token request for `aud=sandbox-manager` returns nothing usable and every poll refuses at sandbox-manager's interceptor.

This is the same failure S3.3 shipped: its token provider requested `sandbox:write` only, `GetWorkspaceManifest` needed `sandbox:read`, and the hydrate path was refused at the real interceptor while every Rust test passed — because no unit test crosses that boundary. Found 2026-09-13, a slice later than it was introduced.

The exact edit, modelled on execution-core's own entry (`audiences: [..., "sandbox-manager"]`, `scopesByAudience["sandbox-manager"]: ["sandbox:read", "sandbox:write"]`, `retentionByAudience["sandbox-manager"]: "persistent"`):

```json
"capability-core": {
  "audiences": ["session-core", "inference-core", "orchestrator-core", "sandbox-manager"],
  "scopes": [..., "sandbox:read"],
  "scopesByAudience": { "sandbox-manager": ["sandbox:read"] },
  "retentionByAudience": { "sandbox-manager": "persistent" }
}
```

`sandbox:read` only — the watch sweeper never writes to the registry, and granting `sandbox:write` would let a bug in a poller move a process's state. `persistent` retention matches execution-core's, because the output a watch reads is the same content under the same posture.

**DONE in step 2 (2026-09-14):** the grant is in the file, `sandbox:read` only, `persistent` retention. The token provider's failure message names this file explicitly, because a 403 from Auth Core reads as a sandbox-manager problem and is not one.

**Still owed:** a real token minted and presented end to end. A Go unit test cannot see that file, and the check that could is an integration test that mints through Auth Core — the shape S3.3's postmortem asked for and nobody has built. Until then the grant is reviewed but not exercised, which is worth saying plainly rather than treating the edit as proof.

**Open question, flagged not resolved:** whether capability-core should instead read through a user-bound bearer it does not currently hold. That would make the ceiling structural rather than enforced, but capability-core has no user bearer at sweep time — the member who created the watch is not present — which is the same problem the cron sweeper has and solves with a per-fire Control decision. The recommendation is to keep the service read plus the explicit Space check, and to revisit if S4.4 gives sweepers a delegated credential.

---

## 9. Sequencing

Each step is independently testable and ships with no caller ahead of the step that needs it — the S3.2/S3.3/S4.2 precedent.

1. **Watch core.** session-core migration (`space_watches`, `space_watch_events`), `capability-core/internal/watch` (store, state machine, bounded predicate + its tests), and a sweeper that claims rows and does nothing with them because no adapter is registered. No proto, no HTTP, nothing fires. **DONE 2026-09-14.** What differed from this list, and why:

   - **The claim commits BEFORE the work, which the cron sweeper does not do.**
     That sweeper runs everything — including a network call to Control — inside
     one transaction holding row locks on all 100 claimed rows. Survivable at
     one fire per minute; not at a two-second poll whose work is a remote read,
     where the same shape would hold locks across an RPC for every watch in the
     fleet at once. So `ClaimDue` is its own short transaction: select `FOR
     UPDATE SKIP LOCKED`, push `next_poll_at` forward, commit. The cost is that
     a sweeper which dies after claiming delays that watch by one interval,
     which is invisible next to a 2–30s poll and far cheaper than the
     alternative.
   - **`next_poll_at` and `idle_polls` are new columns.** §3's draft had
     `last_polled_at` and `consecutive_failures` but nothing the claim query
     could order or filter by, so the backoff §7.1 specifies was not
     expressible. `next_poll_at` is both the due-time and the claim — exactly
     what `cron_schedules.next_fire_at` already is. `idle_polls` is the backoff
     LEVEL rather than a stored duration, so there is one source of truth: a
     counter and a duration kept side by side are two things that can disagree,
     and the one that disagrees silently is the duration.
   - **A partial unique index on ACTIVE watches**, keyed on
     `(org, space, source, predicate, creator)`. Not in §3, and it earns its
     place: watching the same thing for the same reason twice is a duplicate
     rather than two answers, and without the index a retried create silently
     doubles the events a person receives. Terminal watches fall outside it, so
     re-watching after one ends is still allowed.
   - **`gap` became its own event kind** rather than a flag on a match. Output
     produced and then trimmed is not output that never existed, and a reader
     summarising a log has to be told which they are looking at. It is emitted
     at the WATCH's own cursor, not the next line's, so it is attached to where
     the hole actually was.
   - **The kind/trust pairing is enforced, not just documented.** A `match`
     event must carry `unscreened_source_payload`; `validateEmission` refuses
     anything else. Without that rule an adapter could launder a payload into
     the label a consumer renders as the owning plane's own assertion — the
     exact failure §5's trust column exists to prevent.
   - **`delivery_state` carries a CHECK pinning it to `'recorded'`.** §6 said
     S4.4 owns everything past it; the constraint makes that enforceable rather
     than a convention, and forces S4.4 to widen the vocabulary deliberately.
   - **The sweeper is NOT wired into `cmd/main.go`.** There is no create path
     until step 3, so no watch row can exist, so a wired sweeper would tick
     every second over an empty table forever. It ships built and tested; step 2
     wires it when it has an adapter and step 3 gives it rows. This is the
     no-caller-ahead-of-its-step rule the whole sequence follows.
   - **An integration test against a real Postgres, not only stubs.** The
     store's SQL is where this repo has actually been bitten: S4.2's process
     store passed every stub test — right text, right bound arguments — and
     still failed 14 of 17 integration tests on a type deduction only a real
     planner performs. A 25-parameter INSERT, a `FOR UPDATE SKIP LOCKED` claim,
     and an UPDATE fenced on state are all in that category, so they are
     exercised against a throwaway container under `-tags integration`.
2. **Process-output adapter.** The `SourceAdapter` seam and its first implementation: poll, evaluate, emit, commit cursor, drain-then-terminate. The cursor/partial-line/gap proofs live here. Still no caller — the sweeper runs, but nothing can create a watch. **DONE 2026-09-14.** What differed from this list, and why:

   - **§8.1's deployment dependency is part of this commit, not a note.**
     `apps/Control Plane/config/plane-service-principals.json` now grants
     capability-core the `sandbox-manager` audience with `sandbox:read` and
     `persistent` retention. `sandbox:read` only — the sweeper never writes, and
     `sandbox:write` would let a bug in a poller move a process's state. The
     token provider's own error message names this file, because a 403 there
     looks like a sandbox-manager problem and is not one.
   - **`Process` gained `recipient_audience_revision` on the wire, and this was
     a real gap in S4.2.** Step 6 put the ceiling on the row and enforced it
     *inside* sandbox-manager's own read path, but never exposed it — so a
     SECOND reader could not apply it. The watch sweeper resolves a process
     through `GetProcess` on a service credential and has to refuse content
     recorded under an audience its watch's decision predates; without the field
     that refusal was not expressible. A ceiling only one reader can see is a
     ceiling the next reader silently does not have.
   - **At most ONE match event per cursor position, which is a correctness
     requirement rather than a throttle.** A chunk is a flush of many lines, so
     several matching lines commonly share one seq — and the event key is
     `(watch_id, cursor_value, kind)`. Emitting one event per line makes the
     second collide with the first and be silently dropped by `ON CONFLICT DO
     NOTHING`, which LOSES a match rather than deduplicating a replay. Step 1
     could not see this; attributing lines to chunk seqs is what made it
     reachable.
   - **The committed cursor waits for EVERY stream to be clean.** stdout and
     stderr interleave in one seq sequence, so a stderr fragment opened at seq 1
     holds the cursor at 0 even while stdout advances — because chunks at or
     below the cursor are never re-read and the fragment's continuation arrives
     later. Fragments are also tracked per stream: joining a stdout fragment to
     a stderr chunk would fabricate a line neither stream emitted, and with a
     `contains` predicate could manufacture a match out of two innocent halves.
   - **A `system` chunk is consumed but never surfaced.** S4.2's registry writes
     its own "host lost" marker on that stream. An `any` watch firing on it
     would report the REGISTRY as the thing that spoke, and a `contains` watch
     could be matched by text the process never wrote. The cursor still advances
     past it so a marker cannot stall a watch, and nothing is lost — what the
     marker means is already carried by the process's state.
   - **A terminal process is only DRAINED once a read comes back empty.**
     Reporting terminal while output remains ends the watch before its last
     lines, which is exactly where a failing build says why it failed.
   - **`ErrSourceGone` moved into the core**, because the sweeper is what acts
     on it and the core cannot import an adapter. It is the ONLY adapter error
     that ends a watch; everything else — an unreachable service, a refused
     credential — backs off, because terminating on transient failure would
     silently cancel a person's watch over an outage.
   - **A missing reauthorizer now REFUSES rather than skips.** Step 1 left
     `authz == nil` falling through to "observe anyway", which is the kind of
     default that ships and is then forgotten — and what it defaults past is the
     check that stops a watch created weeks ago under a membership since
     revoked. The sweeper is wired in this step and cannot observe anything
     until step 3 supplies the reauthorizer, which is the honest state.
   - **An unterminated fragment is cut at 64 KiB.** Without a bound, a program
     writing megabytes with no newline stalls the cursor forever — the watch
     never consumes anything and never progresses. S4.2's host already made the
     same compromise on the writing side and recorded the residual risk; this is
     the reader's half of it.
3. **Create/cancel + authority.** Control `watch-create-decision`, capability-core HTTP surface, the per-emission reauthorization, the Space check on `source_ref`. First point at which a watch can exist. **DONE 2026-09-14.** What differed from this list, and why:

   - **TWO Control actions, not one.** `model.watch.create` runs with a human
     present; `model.watch.observe` runs later with nobody present. Separating
     them is the schedule pair's own rule: a creator's token must not serve as a
     standing worker grant after a role, audience or policy change. The action
     id and schema hash are digest inputs, so a create token cannot satisfy an
     observe check even when every other bound fact is identical.
   - **The predicate is bound as a DIGEST.** Control does not need the matching
     rule to enforce Space policy, and binding the digest is what stops a watch
     approved for "tell me when it says ERROR" from becoming "tell me
     everything" — which, for a watch, is the difference between a notification
     and a transcript. The same reasoning the task template already uses.
   - **A viewer may watch**, where schedule creation needs editor. A watch
     OBSERVES and creates no effect in the Space, and reading the shared record
     is what a viewer role is for — §7's own thread-read decision says so in as
     many words. A test asserts the contrast directly, because a rule this
     surface-level is exactly the kind that gets "tidied" into consistency with
     the wrong neighbour.
   - **No new entitlement, and a shared-Space watch reuses `ThreadReadEntitled`.**
     §11 left this open with a recommendation; this is the answer. A standing
     read of a room is still a read of that room, so a Space not entitled to
     shared reads should not get standing ones — and the thing that genuinely
     needed its own switch (background processes) already has
     `process_registry_entitled`, so a watch on a process a Space may not run has
     nothing to watch. A second switch for the same disclosure would be
     unexplainable against the first.
   - **The reauthorization moved from the READ to the DISCLOSURE**, correcting
     step 1. Checking before every poll is one Control call per watch per two
     seconds, where the cron sweeper this copies makes one per FIRE. A poll that
     matches nothing reads into the sweeper's memory on capability-core's own
     service credential, bound to the watch's Space by the adapter, and discards
     it: nothing durable, nothing anyone can see. What needs fresh human
     authority is turning a read into a recorded event, so the check sits
     immediately before the commit and a quiet watch costs Control nothing.

     The cost, stated rather than hidden: a member whose membership was revoked
     keeps polling until their watch's first would-be event rather than until
     its next poll. Nothing is disclosed meanwhile and `expires_at` bounds it. If
     that window ever needs closing the fix is a periodic revalidation sweep,
     not moving the check back in front of every read.
   - **`ErrAuthorityUnavailable` separates "Control said no" from "Control could
     not answer".** Terminating on the second would cancel every watch in the
     fleet during one deployment of the identity plane. An unavailable authority
     backs off WITHOUT committing the cursor — the adapter is idempotent in it,
     so the pending events are recovered rather than skipped. Committing there
     would lose a match permanently.
   - **Create checks authority BEFORE the source.** A caller who may not watch a
     Space at all must not be able to use the source check as a probe for which
     resources exist in it. The source resolver's refusals are also one
     indistinguishable error — missing, foreign, and above the audience ceiling
     all read the same — for the same no-oracle reason the adapter uses.
   - **Creating a watch resolves a STARTING cursor** rather than beginning at
     zero. A watch created on a process that has been running for an hour must
     not replay that hour: the person asked what happens next, and dumping the
     backlog into the room would bury the thing they were waiting for. The cost
     is that output produced before the watch existed is never matched — intended
     behaviour, because a watch is not a search.
   - **The create path refuses a source kind this deployment cannot poll.**
     Adapters and resolvers are constructed together for that reason: a
     deployment able to create a watch it cannot serve would hand a person a
     watch that never reports.
   - **Only the creator may cancel, reported as not-found.** A watch is one
     person's standing intent addressed to that person; cancelling someone
     else's is a write against their intent, which is a different authority
     question this slice does not answer. Not-found rather than forbidden
     because the id belongs to someone, and saying which is a disclosure of its
     own. Cancelling is idempotent — the caller's intent is already satisfied,
     and a failure would invite a retry that can never succeed.
   - **`authctx` gained `ContextWithPrincipal`.** The package exported a reader
     and no writer, so every downstream handler was untestable without standing
     up the verifier and a signed token. The exported counterpart performs no
     verification and says so: production code uses the middleware, tests use
     this.
   - **The per-Space limit is counted, not fenced.** Two concurrent creates can
     each see 7 and both land. Acceptable here where it was not for S4.2's
     process limits, because a watch consumes a poll slot rather than an OS
     process — one over the bound costs a little sweeper time, not a resource
     the host has to find.
4. **Read path.** model-gateway `/v1/watches`, the V3 gateway's `/work` fourth section, `activityFromWatch`. A watch becomes visible in the room.
5. **Model-runs adapter, and the `run_watch_subscriptions` retirement path** (§2.1).

Later slices, out of this pass: Ingestion jobs, Data document/source changes, deployments, connector events.

---

## 10. Test plan — one row per verification the plan asks for

| verification | where |
|---|---|
| crash before cursor commit | step 2: emit + cursor in one transaction; a killed sweeper re-reads and the unique index dedups |
| duplicate event | step 2: `ON CONFLICT DO NOTHING` on `(watch_id, cursor_value, kind)` |
| partial line | step 2: a chunk with `ends_with_newline = false` must not match until completed |
| unwatch race | step 3: cancel during a poll — the claim holds the row, so cancel lands after and the event is still correct |
| expiry | step 1: state machine; step 2: a watch past `expires_at` never polls |
| noisy-source debounce | step 2: the idle backoff table, and at most one event per poll |
| resource revocation | step 3: a changed revision terminates as `SOURCE_GONE` without emitting |
| unscreened payload | step 2: every derived event carries `trust='unscreened_source_payload'`; a test asserts no adapter can emit content as `owner_metadata` |
| cross-Space `source_ref` | step 3: a watch naming a process in another Space is refused at create and, if it somehow persists, refuses to emit |

---

## 11. Open questions

- **Should a model be able to create a watch?** A `watch_start`/`watch_list`/`watch_cancel` tool family is the obvious parallel to S4.2's tools, and the strongest use case is model-created ("I started the build; watch it"). But a watch outlives the run and is *addressed to a human*, which is a different authority class from anything a tool has created so far. **Recommendation: not in this pass.** Ship human-created watches through the Work tab first, see what people actually watch, and give the model the tool when the destination question (S4.4) is settled — a model creating a standing intent that delivers to a person, before delivery has semantics, is the wrong order.
- **`trigger_mode='continuous'`** is in the schema and not in slice 2's adapter. A continuous watch on a chatty process is a delivery-rate problem, which is S4.4's. Recommend shipping `once` only and leaving the column.
- **Watch count limits per Space** are stated (8) but not tied to an entitlement. **RESOLVED in step 3 (2026-09-14): no new entitlement.** A shared-Space watch reuses `ThreadReadEntitled` — a standing read of a room is still a read of that room — and the thing that needed its own switch already has `process_registry_entitled`. The count stays a quota rather than an authority, enforced by a plain count at create.
- **The 2s base interval is a guess.** It is the one number here with no evidence behind it; the first real watch on a real build should be used to correct it.

---

## 12. Non-goals

- **Delivery.** S4.4. `delivery_state` stops at `'recorded'`.
- **A live tail.** §7.1. A watch has lag by construction.
- **Watching across Spaces or across orgs.** A watch is bound to one resource in one Space.
- **Expression-language predicates.** §4.
- **Retiring `run_watch_subscriptions` now.** §2.1 — it goes when the Model-runs adapter replaces it, not before.
