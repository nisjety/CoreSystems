# Audit delivery: deploy the publisher fix and replay the stuck rows

Context: governed tool actions are queued by session-core in `session_audit_outbox` and published to the JetStream stream `VEREVON_CONTROL_OBSERVABILITY`, from which audit-core stores them in `audit_events`. Postmortem `docs/postmortem/0002-session-audit-trail-never-delivered.md` records why thousands of rows stayed terminal and unpublished, and what the fix (commit b9ed9197) and migration 0043 (`requeue_session_audit_terminal`) change. This runbook is the order of operations and the gates. The alerts that watch the same series are in `audit-delivery-alerts.md`.

Development stack only. Production images arrive through a signed artifact (`scripts/release-artifact.sh`), never through this path.

Every read below is a `SELECT`, a `docker inspect`, one NATS monitoring GET or one metrics scrape. Nothing prints a credential, a payload or a tenant identifier; do not add one to the evidence file. Read the counts live: the figures in the postmortem are those of 2026-10-06 and drift.

Container names are the development defaults (`model-plane-postgres-1`, `model-plane-nats-1`, `model-plane-session-core-1`, `controlplane-postgres`); the proof script takes overrides from the environment (see its header).

## 0. Preconditions

- The session-core build comes from a clean detached worktree at the commit that landed WS01-S0, not from a dirty tree. `rebuild-service.sh --require-clean` refuses a dirty tree.
- The audit stream exists with both durables (postmortem, "Deploy steps and order", step 1). The proof's block 7 shows it.
- The alert rules of WS01-S1 are loaded, so a regression during the replay is visible (`audit-delivery-alerts.md`). A replay without them can still run; say so in the evidence file.

## 1. Baseline read

```bash
bash "apps/Model Plane/scripts/tests/audit-delivery-proof.sh" --stage baseline
```

Keep the output: it is the "before" of the evidence file `docs/compliance/evidence/live-reads-audit-delivery-<date>.txt`. Note three live numbers: terminal and unpublished rows (N), how many are of each event, and the running session-core revision (block 1). Expect `no stream found for given subject` as the reason on rows that predate the stream, and `publisher-fix-deployed` as not yet true.

Also read the ZDR line: the proof's blocks 5 and 9 must show 0. If either is not 0, stop: a ZDR run must write no audit row, and that is a defect to raise before any replay.

## 2. Deploy session-core with the fix and migration 0043

```bash
bash "apps/Model Plane/scripts/rebuild-service.sh" --require-clean session-core
```

The script tags the running image as `model-plane-session-core:pre-<yyyymmdd>-<revision>` first, rebuilds and recreates only that service (`up -d --no-deps --build`), waits until it is healthy and prints the new revision. It refuses `MODEL_PLANE_PRODUCTION=1`. A build from a worktree needs the live stack's environment files passed as `--env-file <path>` (paths only; never read or paste their values). Session-core applies migration 0043 when it starts.

Older sibling services keep running against the new session-core, and compatibility was not proven in code. Run one smoke turn that uses a tool, one thread read and one approval, and record that they worked. If one fails, roll back (below) before replaying anything.

Do this before any replay: the old build publishes a reservation on the wrong subject and audit-core would dead-letter every replayed reservation.

## 3. Verify the deploy

```bash
bash "apps/Model Plane/scripts/tests/audit-delivery-proof.sh" --stage baseline
```

Expect: migration 43 present with `success = t`, the function present, `publisher-fix-deployed` yes, and the metrics scrape listing `mp_session_audit_outbox_terminal_by_subject{subject_class="audit_tool_action"}` with the number N from step 1. Run one tool-using chat turn and check that `audit_events` gains a `tool_action_reserved` row and a `tool_action` row for it within seconds. A turn that does this proves the new path before the old rows are touched.

## 4. Canary replay: 10 rows

```sql
-- session_core database
SELECT requeue_session_audit_terminal('verevon.audit.v2.model.session-core.tool_action', 10);   -- 10
```

Wait about 15 seconds, then:

```bash
bash "apps/Model Plane/scripts/tests/audit-delivery-proof.sh" --stage canary
```

The canary passes when the 10 requeued rows are published, none is terminal, and no ZDR row exists on either side. If it fails, do not continue; read `last_error` of the requeued rows (block 3b) and the failure counters in the scrape.

## 5. Gate before the large replay (K2-11)

The 10-row canary is not gated. The large replay stores every stuck event in `audit_events`, and `GET /api/v1/audit` in the gateway (`apps/Frontend Plane/verevonv3/apps/gateway/src/domains/audit.rs`) is readable by any signed-in org member until the Space role gate (F-t1-031) is deployed there. Replay N rows only when one of these is recorded in the evidence file:

- the role gate on `GET /api/v1/audit` is deployed (name the gateway revision), or
- the owner accepts that every org member can read the replayed rows (one line: who, when, the number of rows).

Without one of them, stop here and leave the rows terminal. They stay recoverable.

## 6. The rest

```sql
SELECT requeue_session_audit_terminal('verevon.audit.v2.model.session-core.tool_action', 5000);  -- N - 10, at most 5000 per call
```

If N - 10 is more than 5000, repeat after the previous batch drained. The drainer publishes up to 50 rows a second; watch `mp_session_audit_outbox_pending` fall to 0 and `mp_session_audit_outbox_publish_failures_total` stay flat.

The reservation that the old build published on the wrong subject (postmortem, "Replay of the stuck rows") needed a re-arm before its outbox row was pruned seven days after publication. Read whether any such row is left (the `SELECT count(*)` in the postmortem); if none is, record that and do not invent one.

## 7. Final read

```bash
bash "apps/Model Plane/scripts/tests/audit-delivery-proof.sh" --stage final
```

All checks must pass: no terminal or pending `verevon.audit` row in the outbox, both durables at `num_pending 0`, `num_ack_pending 0`, `num_redelivered 0`, the terminal gauge at 0, no `details.zdr = true` row in the outbox or in `audit_events`. Then the two manual reads the script cannot make: one more tool turn adds both rows, and `GET /api/v1/audit?event=tool_action` through the gateway returns rows for a member of the org.

Append the outputs of steps 1, 3, 4 and 7 to the evidence file, each under its command, with the `docker inspect` revision and creation time of the running image. Only after this final read passes, change the status of the postmortem and re-read the trust claim `audit-trail-not-verified-end-to-end` (`docs/compliance/evidence/trust-claims-pending-reverification.md`, section 6).

## Rollback

- Stop a replay midway: put the unsent rows back where they were (postmortem, "Rollback"). Delivered events are history in an append-only table and are not recalled.
- Roll back session-core: sqlx refuses to start an image that does not know an applied migration, so delete the row first, then return to the tagged image the rebuild printed.

  ```sql
  DELETE FROM _sqlx_migrations WHERE version = 43;   -- session_core database, before the image goes back
  ```

  ```bash
  docker tag model-plane-session-core:pre-<yyyymmdd>-<revision> model-plane-session-core:latest
  bash "apps/Model Plane/scripts/compose.sh" up -d --no-deps --no-build session-core
  ```

  The two columns and the function stay; they are additive.
- The stream: leave it. Deleting it recreates the defect and loses its messages.

## ZDR

Audit rows are content free (tool, data category, `zdr`, identifiers). A ZDR run writes no audit row, so a replay must show 0 rows with `details.zdr = true` in the outbox and in `audit_events`; the proof checks both and fails otherwise. Logs and evidence carry names and counts only.
