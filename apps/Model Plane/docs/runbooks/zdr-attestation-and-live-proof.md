# ZDR: ordered wave deploy and the fail-closed live proof (Z-60 pass 1)

Context: Zero Data Retention (ZDR) is enforced by construction in session-core, execution-core, model-gateway and inference-core (`docs/ZDR.md`). Until a model deployment is attested (Z-40), a ZDR request must be refused, with a typed code, before any model call, and must write nothing. This runbook deploys the ZDR wave in order and proves that. Pass 2 (a real answer from an attested deployment) is a separate step at the end.

Development stack only. Production images arrive through a signed artifact (`scripts/release-artifact.sh`), never through this path.

Rules for every step:

- Reads are `SELECT`, `docker inspect`, one `redis-cli --scan` count and the Playwright spec. Nothing prints a credential, a payload or a tenant identifier; do not add one to the evidence file.
- Waiver minting stays off: `ZDR_WAIVER_ENABLED` is unset on every service. The preflight fails when it is `true` anywhere.
- The only organisation the turn is sent from is a dedicated `ZDR-TEST` org. The spec refuses any other.
- A refused turn must leave no row. The proof compares counters before and after, so run it on a quiet stack: other traffic makes the comparison fail honestly.

## 0. Preconditions

- WS01-S0 (the live ZDR wave) and WS01-S6 are committed. Build from a clean detached worktree at that commit: `rebuild-service.sh --require-clean` refuses a dirty tree. Note the commit as `ZDR_WAVE_COMMIT`; the preflight checks every wave service was built from a revision that contains it.
- The ZDR-TEST org exists on a plan that includes ZDR (a plan without it is refused `402 plan_upgrade_required`), named `ZDR-TEST ...`, with a seeded owner. Seeding and switching it back: `apps/Frontend Plane/verevonv3/docs/ZDR_OPERATOR_SEED_RUNBOOK.md`. The seed credential comes from the secret store into `ORG_CORE_SERVICE_TOKEN`; never on a command line, never in a file.
- Auth Core posture claims are deployed (2026-10-04); confirm once with the preflight of section 2 and the spec of section 4.
- `MODEL_DRAGONFLY_PASSWORD` is loaded into the shell that runs the proof, from the deployment secret store, without echoing it. The proof hands it to `redis-cli` through the environment of `docker exec`.

## 1. Deploy the wave in order

Order matters: session-core owns the retention receipt the others stamp and read, and inference-core is the service that refuses, so it goes last. Each step is one service, one rollback tag.

```bash
cd "<clean detached worktree at the wave commit>"
bash "apps/Model Plane/scripts/rebuild-service.sh" --require-clean session-core      # applies 0044 and 0045 at start
bash "apps/Model Plane/scripts/rebuild-service.sh" --require-clean execution-core
bash "apps/Model Plane/scripts/rebuild-service.sh" --require-clean model-gateway
# model-gateway carries the catalogue; then the SPA pinning hash and the frontend gateway
# (Frontend Plane build), then restart the SPA container so it serves them:
docker restart frontend-plane-verevonv3-frontend-1
bash "apps/Model Plane/scripts/rebuild-service.sh" --require-clean inference-core
```

A worktree build needs the live stack's environment chain passed as `--env-file <path>` (paths only; never read or paste values), as in `audit-delivery-replay.md`. After each step wait for the script's health line before the next.

Rollback: each `rebuild-service.sh` printed `model-plane-<service>:pre-<yyyymmdd>-<revision>`; retag and recreate in the reverse order (inference-core first). session-core's migrations are additive; reverting them (`revert/0044_*.down.sql`, `revert/0045_*.down.sql`, then `DELETE FROM _sqlx_migrations WHERE version IN (44, 45)`) drops `runs.retention_class` and is allowed only when no waived run exists under review. With waiver minting off none can.

## 2. Preflight

```bash
ZDR_WAVE_COMMIT=<wave commit> bash "apps/Model Plane/scripts/tests/zdr-live-proof.sh" --phase preflight
```

Passes only when: the four services started in the order of section 1 (a later restart of an earlier service fails it: redo the wave in order), each from a revision containing the wave commit, migrations 0044 and 0045 are applied with `runs.retention_class` present, `ZDR_WAIVER_ENABLED` is not `true` on any of them, and there is no waived run and no ZDR row in `session_audit_outbox`. The output is appended to `docs/compliance/evidence/live-reads-zdr-<date>.txt` (override with `--out`).

## 3. Counters before

```bash
bash "apps/Model Plane/scripts/tests/zdr-live-proof.sh" --phase before --state "$TMPDIR/zdr-proof-state.txt"
```

Records threads, messages, runs, feedback_ratings, session_audit_outbox, ZDR rows in the outbox and the number of `mp:gw:cache:*` keys. Counts only; the state file holds no key name. If the cache count prints `FAIL` because `MODEL_DRAGONFLY_PASSWORD` is unset, fix that first: the after phase cannot prove the cache unchanged without it.

## 4. The turn

From `apps/Frontend Plane/verevonv3`, signed in as the ZDR-TEST owner (`E2E_EMAIL` and the usual e2e storage state):

```bash
ZDR_LIVE_TURN=1 pnpm exec playwright test tests/e2e/zdr-live-turn.spec.ts
```

It refuses to send unless the signed-in org is named `ZDR-TEST...` with ZDR on and the live catalogue lists no ZDR chat model. It sends one turn and passes when the stream ends in `zdr_unavailable` or `zdr_connection_not_supported`, no content frame was streamed, and the page tells the reader the request was not sent to a model.

The spec switches web search off before the turn: the composer sends it by default, and model-gateway refuses it under ZDR on its own (`zdr_feature_not_supported`, feature `web_search`) before the request reaches inference-core, so with it on the attestation check is never reached. On a stack that may only use one model, pin it with `ZDR_LIVE_TURN_MODEL=<model id>` (2026-10-28: `claude-sonnet-5-5`); otherwise the composer's default mode is sent. On a Vite dev server, open `/chat` once before the proof: the first visit can re-bundle dependencies and reload the page, and the spec then times out before it sends anything.

Then the gate half, on the same org (the third test sends one PATCH that must be refused `409 zdr_no_attested_provider` and writes nothing):

```bash
pnpm exec playwright test tests/e2e/zdr-org-switch-gate.spec.ts                       # first two tests, read-only
ZDR_GATE_LIVE_PATCH=1 pnpm exec playwright test tests/e2e/zdr-org-switch-gate.spec.ts  # adds the refused PATCH
```

Keep the Playwright report path for the evidence.

## 5. Counters after

```bash
bash "apps/Model Plane/scripts/tests/zdr-live-proof.sh" --phase after --state "$TMPDIR/zdr-proof-state.txt"
```

Every counter must be unchanged and the outbox must hold no ZDR row. A delta is a defect, not a flake: stop, keep the output, find the writer (thread, message, run, cache key, feedback row or audit envelope from a ZDR turn violates `docs/ZDR.md` section 2).

## 6. Put the org back and record

```bash
# from apps/Frontend Plane/verevonv3, ORG_CORE_SERVICE_TOKEN from the secret store
node apps/gateway/scripts/zdr-seed-test-org.mjs --org-id <ORG_ID> --user-id <OWNER_USER_ID> --expect-name "<exact org name>" --zdr false --apply
```

An org left on is an org whose chat is refused. Then:

1. The evidence file `docs/compliance/evidence/live-reads-zdr-<date>.txt` holds the preflight, before, after and the delta; add the Playwright report path and the seed output (status lines only).
2. `docs/ZDR.md` section 4 rows for Z-60 and `docs/decisions/ledger.md` (the own-connection ZDR entry's build state and trigger): record pass 1 as run on that date, with the evidence file. Pass 1 does not flip Z-60 to fully proven: the answer path is pass 2.
3. Waiver minting is still off; say so in the evidence.

## Pass 2 (after a real attestation, task Z-40)

Not part of pass 1. Once a deployment is attested in writing and the six `*_ZDR_*` fields are set in the signed policy, `/api/v1/models` lists `zdr` on the covered model. Repeat sections 3 to 5 with the turn expected to stream an answer (`ZDR_LIVE_TURN=1 ZDR_LIVE_TURN_EXPECT=answer`; the test is "pass 2: a ZDR-TEST org turn streams an answer from an attested chat model" in `tests/e2e/zdr-live-turn.spec.ts`): zero thread and message delta, `mp:gw:cache:*` unchanged, no feedback row, no audit envelope.

## Attestation lapse alerts

Three alerts in `apps/Control Plane/observability/rules/zdr-attestation.rules.yml` read the review-by gauge that inference-core publishes for each provider surface, `mp_inference_zdr_attestation_review_by_timestamp_seconds`. The gauge is the start of the review-by UTC day. The attestation stops granting ZDR after that day, so the alerts count down to it. No pager is wired yet (decision D1): the alerts show in the Prometheus UI only.

- `ZdrAttestationReviewDue` (warning): 3 to 14 days before the review-by day. Start the renewal for that surface.
- `ZdrAttestationReviewImminent` (critical): under 3 days before the review-by day, until that day ends. The renewal must be in force before it ends.
- `ZdrAttestationLapsed` (critical): the review-by day has ended, so ZDR is no longer granted for that surface.

What to do:

1. Renew the attestation for the surface in writing.
2. Re-sign the policy that carries the attestation (the six `*_ZDR_*` fields, see Pass 2 above).
3. Restart inference-core. The gauge is set once at boot from the verified attestation, so the alerts clear only after the restart and the next scrape.
4. Confirm `/api/v1/models` lists `zdr` on the covered model again.

A missing series raises no alert. If inference-core is down, or the `model-inference-core` scrape target is not up in the Prometheus UI, the countdown is blind: fix the target first.
