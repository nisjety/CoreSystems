# 0002 — The "Used by AI?" audit trail never reached audit-core

**Date:** 2026-10-06 (read on the developer stack; the first stuck row is from 2026-09-04)
**Status:** Cause proven. The broker side is fixed and deployed (2026-10-05). The
publisher fix, the requeue function and the new metrics are built and tested and **not
deployed**. The 4,992 stuck rows are **not replayed**. Delivery of those rows is not
proven until the replay below has run and a fresh read shows them in `audit_events`.
**Evidence:** `docs/compliance/evidence/live-reads-audit-delivery-2026-10-06.txt`
(read-only reads, each block names its command), and
`docs/compliance/evidence/live-reads-trust-claims-2026-10-03.txt` BLOCK 3 for the first
sighting.
**Engineering evidence, not legal advice.** Public wording is unchanged by this record.

## What happened

Session Core queues one audit event per governed tool action in `session_audit_outbox`
and a background drainer publishes it to JetStream, marking the row published only
after the stream's `PubAck`. audit-core (Control Plane) consumes `verevon.audit.v2.model.>`
from the Model Plane broker and stores it in `audit_events`; the in-app Trust Center
column "Brukt av AI?" reads it from there.

Between 2026-09-04 and 2026-10-01 Session Core queued 4,992 events (2,515 `tool_action`
final outcomes, 2,477 `tool_action_reserved` reservations). Not one was published. Each
ran out of its 20 attempts with `no stream found for given subject`, was marked
terminal, and was never claimed again. audit-core held events from the control and
ingestion planes only. Nothing alerted: the only signal was an aggregate gauge
(`mp_session_audit_outbox_terminal`) that no rule watches.

## Root cause 1: the broker had no stream for the subject

JetStream accepts a publish only when a stream captures its subject. The stream that does
(`VEREVON_CONTROL_OBSERVABILITY`, subjects `verevon.audit.v2.model.>` and friends) had
exactly one intended owner: the Control Plane's `audit-extra-nats-provisioner`, which logs
in to the Model Plane broker as `observability-provisioner-model` with **its own copy** of
that broker user's password. The Model Plane's own provisioner (`go/services/nats-provisioner`)
did not list the stream.

Three things combined (evidence blocks 1 and 4 of the live-reads file):

1. The two copies of `MODEL_NATS_PROVISIONER_PASSWORD` (Model Plane `deploy/.env`, Control
   Plane `.env.generated-secrets`) are **different** (read for equality only; no value or
   digest was printed). A credential repair of 2026-09-14 synchronised the audit-core
   login (the audit copies are equal now and were different in that day's backup) and not
   the provisioner login.
2. The broker log (which starts 2026-09-10) shows 8 refused logins of
   `observability-provisioner-model`, on 2026-09-14 (twice), 09-17, 09-19, 09-28, 09-29
   (twice) and 10-01. The only other client of that user is the Model Plane's own
   provisioner, which logs in with the broker's own variable and succeeds every time, so
   the refusals are the Control Plane's copy (read from the user name and the dates, not
   from the client address).
3. The Control Plane declares the model bus `"optional":true`; its provisioner logs a skip
   for an optional bus it cannot reach and exits 0. A launcher run therefore looked
   green, and Compose never retried.

The stream first existed on 2026-10-04 23:46:31 UTC, three seconds after the Model Plane
provisioner image that carries the fix was built, and its subjects and limits are the
fix's. The first events then flowed on 2026-10-05 01:19 UTC: one `tool_action` is in
`audit_events` (the only model-plane row).

## Root cause 2: a reservation was published on the final-outcome subject

audit-core accepts an audit message only when the subject's last token equals the payload's
`event` (`subscriber.eventAuthorityMatches`) and dead-letters anything else as
`authority_mismatch`. The gateway's reserve-before-execute phase builds an event named
`tool_action_reserved`, but its outbox row was stored under the `tool_action` subject and the
drainer published every tool-action row on that one subject. Every reservation would have been
refused even with the stream in place. 2,477 of the 4,992 stuck rows are of this kind, and so is the one
reservation published on 2026-10-05: its message was acknowledged by the broker, and
audit-core acknowledged it and stored nothing (the stream holds three messages on two
subjects, audit-core stored one).

## Why it stayed silent

* The drainer counted attempts and set `terminal_at` after 20 failures with no log line and
  no per-subject metric. The aggregate gauge existed and nothing watched it.
* A terminal row was a dead end: no code path claimed it again, so recovery meant
  hand-written SQL against a production table.
* No test connected a publisher's subject to a stream. The provisioner's tests only checked
  the streams it already listed.

## What changed

Broker side (deployed 2026-10-05):

* `go/services/nats-provisioner/audit_observability.go` (new) and `main.go`: the Model Plane
  provisioner now owns `VEREVON_CONTROL_OBSERVABILITY` and audit-core's two durables
  (`audit-core-model-v3-audit`, `audit-core-model-v3-usage`). The provisioner runs with the
  broker's own credential variable, and `session-core` does not start until it exits 0
  (`depends_on: service_completed_successfully`). `provisionTopology` is the one entry point
  `main` and the tests share. The values equal audit-core's own provisioner's, so if both ever
  run they converge on the same stream (a test reads audit-core's source and fails on drift).

Publisher side (built, tested, **not deployed**):

* `session-core/src/audit_publisher.rs`: one place names the event of each phase
  (`tool_action_event_name`) and the wire subject is derived from the payload's event
  (`resolve_wire_subject`), so a reservation stored under the old subject is published on the
  right one; a new row is stored under the subject its event names (`grpc.rs`,
  `enqueue_inline_tool_audit`).
* Runtime guard. Per failed attempt: `mp_session_audit_outbox_publish_failures_total{subject_class,reason}`.
  When a row turns terminal: `mp_session_audit_outbox_terminal_total{subject_class,reason}`
  and one ERROR line per (class, reason) per minute that carries the class, the reason, the
  attempt count and how many rows it stands for, and **no** event id, run id, org id or
  payload. A topology failure (`stream_not_found`, `permission_denied`) is also logged on its
  first attempt. Gauge `mp_session_audit_outbox_terminal_by_subject{subject_class}` is set for
  every class every tick (0 when none), so an absent series means a stopped reporter.
  Classes and reasons are bounded sets, never tenant tokens.
* Recoverable terminal state: migration `0043_audit_outbox_requeue.sql` adds
  `requeue_count`, `requeued_at` and `requeue_session_audit_terminal(subject_or_pattern, limit)`,
  which touches only terminal, unpublished rows of that subject, never more than `limit`
  (1 to 100,000), keeps `last_error` until a publish succeeds, counts each requeue, is safe to
  run twice and is not executable by PUBLIC.

Tests (all fail on the defect, all apply the real rules):

* `nats-provisioner/manifest_contract_test.go` starts an in-process broker from the real
  `deploy/nats.conf` (real users and grants), runs the real `provisionTopology` as the real
  provisioner principal, then (1) publishes **every JetStream publisher's real subject** from
  `publishers_registry_test.go` as that publisher's own principal and requires a `PubAck` from
  the stream the registry names; (2) binds audit-core's durable as audit-core does and requires
  it to receive session-core's wire fixtures and the broker to accept every acknowledgement it sends (an audit-core principal without its `$JS.ACK` grant would otherwise redeliver every event and stall its durable); (3) fails when the source gains a JetStream
  publish call, a NATS client module or a subject builder that the registry does not list; (4)
  fails when a registry row's source text is gone; (5) checks the broker grants of core-NATS
  publishers; (6) fails on drift against audit-core's provisioner; (7) fails when compose stops
  gating session-core on the provisioner. execution-core and inference-core are asserted to link
  no NATS client (they reach the bus only through session-core's gRPC).
* With `auditObservabilityStream()` and its consumers removed from the manifest (a Go
  `-overlay`, the tree untouched) four tests fail: the stream is missing after provisioning, both
  session-core subjects get no acknowledgement ("no response from stream"), and audit-core cannot
  bind its durable ("stream not found").
* `session-core/tests/fixtures/audit-core-wire/*.json` are written by session-core's real
  builder (pinned by a test) and decoded by audit-core's real `DecodeAudit` and authority rule
  (`audit-core/internal/subscriber/session_core_wire_fixtures_test.go`).
* `audit-core/internal/store/session_core_replay_test.go` (against Postgres): a replayed event is
  an acknowledged duplicate, a different payload under a used event id is refused, and the three
  unique indexes exist.

## Deploy steps and order

1. **Done 2026-10-05:** the Model Plane provisioner with the stream. Verify (read-only), expect
   the stream with `consumer_count 2` and both durables `push_bound true`:

   ```text
   docker exec model-plane-nats-1 wget -qO- 'http://127.0.0.1:8222/jsz?streams=true&consumers=true&config=true'
   ```

2. **Build and deploy `session-core` only** (through the launcher, by the orchestrator). It
   applies migration 43 on start. Do this before any replay: the old build publishes a
   reservation on the wrong subject and audit-core would dead-letter all 2,477 of them.
3. **Verify the deploy** (read-only):

   ```sql
   -- session_core database
   SELECT version, success FROM _sqlx_migrations WHERE version = 43;                  -- 1 row, t
   SELECT proname FROM pg_proc WHERE proname = 'requeue_session_audit_terminal';      -- 1 row
   ```

   The session-core metrics scrape lists
   `mp_session_audit_outbox_terminal_by_subject{subject_class="audit_tool_action"} 4992` (both
   kinds were stored under the one subject) and every other class at 0. Run one chat turn that
   uses a tool: `audit_events` gets a `tool_action_reserved` row and a `tool_action` row for it
   within seconds.
4. Replay (next section).

## Replay of the stuck rows

Read first (read-only), expect 2,515 and 2,477, all `no stream found for given subject`:

```sql
-- session_core database
SELECT payload->>'event' AS event, count(*)
FROM session_audit_outbox
WHERE terminal_at IS NOT NULL AND published_at IS NULL
  AND subject = 'verevon.audit.v2.model.session-core.tool_action'
GROUP BY 1;
```

Canary, then the rest. The function clears `terminal_at` and `attempts` (and `processing_at`),
sets `next_attempt_at = now()` and leaves the drainer to publish; it returns the number of rows
requeued:

```sql
SELECT requeue_session_audit_terminal('verevon.audit.v2.model.session-core.tool_action', 10);    -- 10
-- wait about 15 s, then check both sides:
SELECT count(*) FILTER (WHERE published_at IS NOT NULL) AS published,
       count(*) FILTER (WHERE terminal_at IS NOT NULL)  AS terminal
FROM session_audit_outbox WHERE requeued_at IS NOT NULL;                                         -- 10 | 0
-- controlplane database:
SELECT event, count(*) FROM audit_events WHERE plane = 'model' GROUP BY 1;                       -- 11 rows in all

SELECT requeue_session_audit_terminal('verevon.audit.v2.model.session-core.tool_action', 5000);  -- 4982
```

The drainer handles up to 50 rows a second, so the rest takes about two minutes (derived from the
loop, not measured on this stack). Watch
`mp_session_audit_outbox_pending` fall to 0 and `mp_session_audit_outbox_publish_failures_total`
stay flat.

One more kind of row needs a deliberate re-arm: a reservation the OLD build published. It went out
on the wrong subject, was acknowledged by the broker and refused by audit-core, so it is published
in the outbox and absent from `audit_events`. The stored subject marks it (the fixed build stores a
reservation under `...tool_action_reserved`), so the predicate needs no date window: any tool-using
turn between 2026-10-05 and the deploy of step 2 adds one more. Read the count first (read on
2026-10-06: 1, published 2026-10-05 01:19). The outbox row is deleted 7 days after publication
(2026-10-12 for that one), so do this before then and only after step 2:

```sql
SELECT count(*), min(published_at), max(published_at) FROM session_audit_outbox
WHERE published_at IS NOT NULL AND payload->>'event' = 'tool_action_reserved'
  AND subject = 'verevon.audit.v2.model.session-core.tool_action' AND requeue_count = 0;
```

```sql
UPDATE session_audit_outbox
SET published_at = NULL, processing_at = NULL, terminal_at = NULL, attempts = 0,
    next_attempt_at = now(), requeue_count = requeue_count + 1, requeued_at = now()
WHERE published_at IS NOT NULL
  AND payload->>'event' = 'tool_action_reserved'
  AND subject = 'verevon.audit.v2.model.session-core.tool_action'
  AND requeue_count = 0;   -- as many rows as the read above counted; a second run matches none
```

**Final read, all must hold** (counts are those of 2026-10-06 and grow by one row per phase for each
tool-using turn run in between): the outbox has no terminal, unpublished `verevon.audit` row;
`audit_events` for plane `model` holds 2,516 `tool_action` (the 2,515 plus the one stored on
2026-10-05) and 2,478 `tool_action_reserved`; on the broker, the stream
`VEREVON_CONTROL_OBSERVABILITY` holds 4,996 messages (3 before the replay, 4,992 replayed, 1 re-armed:
a higher count than that means audit-core dead-lettered something), and both durables show
`num_pending 0`, `num_ack_pending 0` and `num_redelivered 0`.

### Why a replay cannot double-count

The replayed row has the same `event_id`, so audit-core sees the same body as a new stream message.
It stores an event once per `(source_bus, source_producer, event_id)` and once per
`(source_bus, source_subject, event_id)` (live unique indexes
`uq_audit_events_source_producer_event` and `uq_audit_events_source_subject_event`); an identical
repeat is acknowledged and ignored; a different payload under a used id is refused. JetStream's own
duplicate window is two minutes and does not help here, so the downstream key is what protects
the table. Verified three ways: the live indexes were read (evidence block 3); the audit-core test
above ran against a real Postgres (stored twice = one row, altered payload refused); and
session-core's `requeued_terminal_rows_are_delivered_once_the_cause_is_fixed` ran the real drainer
against a real broker provisioned by the real provisioner (three rows, one of them a reservation
stored under the old subject: not claimed while terminal, claimed after the requeue, published on
its first attempt on the subject its event names, `last_error` cleared by the publish).

## Rollback

* **Stop a replay midway:** put the unsent rows back where they were. Delivered events are true
  history in an append-only table and are not recalled.

  ```sql
  UPDATE session_audit_outbox SET terminal_at = now(), attempts = 20
  WHERE published_at IS NULL AND terminal_at IS NULL AND requeued_at IS NOT NULL;
  ```

* **Roll back `session-core`:** migration 43 is additive (two defaulted columns and a function), but
  `sqlx` refuses to start an image that does not know an applied migration. Delete the row first:
  `DELETE FROM _sqlx_migrations WHERE version = 43;`. The columns and the function can stay.
* **The stream:** leave it. Deleting it recreates the defect and loses its messages.

## The same failure class elsewhere on this stack

Read-only counts, evidence block 5 and 6 of the live-reads file. None of these is changed by this
work.

| Where | State | Cause read | Needs |
|---|---|---|---|
| audit-core, Application Plane bus | 34,000 retries of "stream not found" in the retained log, one a second; no application event in `audit_events` although leads-core published 11 | `VEREVON_CONTROL_OBSERVABILITY` does not exist on `app-nats`, and cannot while the Application Plane's `VEREVON_AUDIT` (`verevon.audit.>`) exists: a scratch broker answers "subjects overlap with an existing stream", an error the Control Plane provisioner's optional-bus skip would log and ignore | A design decision: one owner and one subject split for application-plane audit events |
| auth-core `organization_membership_audit_outbox` | 3 rows never published, 3 dead-lettered, 25,500 to 42,000 attempts each, retrying every second | A code error in auth-core: `audit.actor_resolution_dead_lettered_at?.toISOString is not a function` | A code fix and an attempt cap |
| Data Plane `documents_outbox` | 192 of 192 rows have `observability_published = false` | The observability publisher is off (`KNOWLEDGE_OBSERVABILITY_NATS_URL` unset): the rows only accumulate | Decide: enable it or remove the column and the loop |
| Model Plane clients, the name `nats` | Resolves to four brokers inside the Model Plane containers (also on `inter-plane-bus`) | Each client logs in to whichever it reaches first and is refused by the three others (258 refused `session-core-runtime` and 15 `capability-core-runtime` logins on app-nats on 2026-10-05) | Use the `model-nats` alias, which exists, in `NATS_URL`; extend the contract test |
| Publish grants in `deploy/nats.conf` | model-gateway publishes `mp.v1.security.*` and `agents.approval.*`, orchestrator-core publishes `mp.v1.orchestration.*`: the broker drops them and core NATS gives the publisher no error | Registered as `gap` rows in `publishers_registry_test.go` (the test fails when a gap is fixed, forcing the registry to be updated). `MP_ORCHESTRATION_EVENTS` holds 0 messages | Add the grants or retire the publishers |
| `managed_run_terminalization_outbox` | 6 rows `reconciliation_required` (2026-09-08 to 09-17) | Not a NATS publish; a reconciliation queue | Owner review |

Every other outbox table read (session-core `approval_delivery_outbox`, `cost_billing_outbox`;
auth_service identity, membership and projection tables; user-core `user_audit_outbox`; org-core's
three; Data Plane `index_deletion_outbox`; leads-core `leads_audit_outbox`) is empty or fully
published.

## Follow-ups

1. One owner for the Model bus audit stream. The Control Plane's `audit-extra-nats-provisioner` still
   lists the model bus. Both converge on the same stream today, and a test guards the drift, but the
   copy of the provisioner password in the Control Plane should be removed, or the launcher should
   fail when the two copies differ.
2. An alert rule on `mp_session_audit_outbox_terminal_by_subject > 0` and on
   `increase(mp_session_audit_outbox_terminal_total[10m]) > 0`. The repository holds no Model Plane
   alert rules file, so none was added.
3. audit-core already counts `authority_mismatch`; an alert on it would have caught root cause 2.
4. Re-read the trust claim `audit-trail-not-verified-end-to-end` only after the final read above
   passes (`docs/compliance/evidence/trust-claims-pending-reverification.md`, section 6).
