# Model entitlement: shadow to enforce, and the live proof (WS10-S7)

Context: which chat models an organisation, and each role in it, may use is stored and resolved by org-core and enforced by inference-core (`apps/MODEL_ENTITLEMENTS_ADR_2026-10-15.md`, ADR-0005). `MODEL_ENTITLEMENT_MODE` on inference-core is `off` (the default, no call to org-core), `shadow` (resolves and counts what `enforce` would refuse, never refuses) or `enforce` (applies the answer, fails closed). This runbook moves the stack through the three, with a read-only proof at each step, and ends with the admin switch for production organisations.

Development stack only. Production images arrive through a signed artifact (`scripts/release-artifact.sh`), never through this path. Every flip needs the owner's approval in writing; a script never flips a switch.

Rules for every step:

- The proof reads: `SELECT` in read-only transactions, `docker inspect` (each switch reduced to one word; the org-core credential to `set` or `unset`), one `redis-cli --scan` count and one GET of `/metrics`. It prints no content, no tenant identifier, no model id and no credential; do not add one to the evidence file.
- A mistyped `MODEL_ENTITLEMENT_MODE` enforces (the service treats any value that is not `off` or `shadow` as `enforce`). Type the word exactly; the preflight fails on a value that is none of the three.
- Under `enforce` an unresolved entitlement refuses, ZDR organisations included. Both `ORG_CORE_URL` and `INFERENCE_ORG_CORE_SERVICE_TOKEN` must be set on inference-core before `shadow` (shadow cannot resolve without them) and before `enforce` (every user call and the catalogue are refused without them).
- Counters are compared before and after, so run each flow on a quiet stack: other traffic makes the comparison fail honestly.
- Evidence file: `docs/compliance/evidence/live-reads-model-entitlement-<date>.txt` (override with `--out`).

## 0. Preconditions

- WS10-S0 to S6 are committed and built into images with every switch off. `MODEL_ENTITLEMENT_WAVE_COMMIT` is the commit that landed WS10-S4 (the preflight checks that inference-core, model-gateway and execution-core were built from a revision that contains it).
- org-core runs migration `021_org_model_entitlements`. The proof reads policy counts as a role that bypasses row-level security (the database owner): set `CP_POSTGRES_USER` if it is not `coresystem`; the preflight fails honestly when the role cannot see past RLS.
- The `inference-core` principal is registered in org-core with the single scope `org:model-entitlement:read:any` (`apps/Control Plane/scripts/run-control-plane.sh` mints `INFERENCE_ORG_CORE_SERVICE_TOKEN`; org-core was restarted after).
- `MODEL_DRAGONFLY_PASSWORD` is loaded into the shell that runs the proof, from the deployment secret store, without echoing it.
- Two organisations: a non-ZDR staging organisation with an owner and a member, and the WS01 `ZDR-TEST` organisation (`apps/Frontend Plane/verevonv3/docs/ZDR_OPERATOR_SEED_RUNBOOK.md`). The dry run of every step is on the non-ZDR organisation first.

## 1. Land everything with mode off

Deploy in this order, one service at a time, each with its rollback tag:

```bash
bash "apps/Control Plane/scripts/run-control-plane.sh" up -d --no-deps --build org-core
bash "apps/Model Plane/scripts/rebuild-service.sh" --require-clean inference-core
bash "apps/Model Plane/scripts/rebuild-service.sh" --require-clean model-gateway
bash "apps/Model Plane/scripts/rebuild-service.sh" --require-clean execution-core
# then the frontend gateway image (verevon-gateway-rs) and the SPA container, as in zdr-attestation-and-live-proof.md
```

org-core goes first so the answer and the credential exist before anyone asks; inference-core before the services that ask it. A worktree build needs the live environment chain passed as `--env-file <path>` (paths only; never read or paste values).

```bash
MODEL_ENTITLEMENT_WAVE_COMMIT=<commit> bash "apps/Model Plane/scripts/tests/model-entitlement-live-proof.sh" --phase preflight --expect-mode off
```

Passes only when the four services started in that order, migration 021 is applied and readable, `MODEL_ENTITLEMENT_MODE` is off, the gateway admin switch is not `true`, and inference-core has counted no entitlement decision (mode off made no org-core call). Operators may write policy through org-core while the mode is off; nothing applies it (banner and BFF flag, see WS10-S5).

## 2. Shadow, at least one working day

Set `MODEL_ENTITLEMENT_MODE=shadow` on inference-core (the deploy env chain, never a value pasted into a chat), recreate inference-core only, and give it a policy to count against: on the staging organisation the owner sets role `member` to one allowed chat model through the admin UI or the BFF.

Dry run on the non-ZDR organisation, shadow:

```bash
bash "apps/Model Plane/scripts/tests/model-entitlement-live-proof.sh" --phase before --state "$TMPDIR/me-proof-state.txt"
# the member pins a model outside the policy in the chat: it is served
bash "apps/Model Plane/scripts/tests/model-entitlement-live-proof.sh" --phase after --state "$TMPDIR/me-proof-state.txt" --expect shadow-served
```

`shadow-served` passes when `would_refuse` moved, `refuse` did not, the history rows only grew, and inference-core was not restarted in between. The log line for the would-be refusal names organisation, revision and reason and holds no content.

Then leave the stack alone for the soak. At the end of the working day:

```bash
bash "apps/Model Plane/scripts/tests/model-entitlement-live-proof.sh" --phase preflight --expect-mode shadow
```

It passes only when inference-core has been up for `SHADOW_MIN_SECONDS` (default 28800, eight hours: raise it to the real working day), org-core answered (`resolved` above zero), nothing was `unavailable` or `would_unavailable`, and `refuse` is zero. It prints `would_refuse` and `service_exempt` with no verdict; read them. Every `would_refuse` is a call `enforce` will refuse: decide with the owner whether each is intended. Every `service_exempt` is a platform job (service principal, no user) that policy never touches by design (DEC-13 D6; scheduled runs stay exempt until an acts-for-user contract exists). A restart of inference-core restarts the soak.

Rollback: set the mode to `off` (or remove the line) and recreate inference-core. One restart, nothing else changes.

## 3. Admin on the test organisations

Set `MODEL_ENTITLEMENT_ADMIN_ENABLED=true` on the frontend gateway for the staging stack only, recreate `verevon-gateway-rs`, and run the preflight with the switch expected on:

```bash
bash "apps/Model Plane/scripts/tests/model-entitlement-live-proof.sh" --phase preflight --expect-mode shadow --expect-admin true
```

On the ZDR-TEST organisation: a policy that leaves no attested model must be refused `409 model_policy_leaves_no_zdr_model` and write nothing: take `--phase before`, send the PUT, make one entitled member call so the `served` expectation holds, take `--phase after --expect served` and read the `org_policy_history` and `org_policy_rows` lines of the delta: they must not move for the refused PUT. With org-core unreachable the same PUT is a `503`, never a write. Keep the curl transcripts.

## 4. Enforce

Owner approval, then `MODEL_ENTITLEMENT_MODE=enforce`, recreate inference-core, and:

```bash
bash "apps/Model Plane/scripts/tests/model-entitlement-live-proof.sh" --phase preflight --expect-mode enforce
```

The live flow, first on the non-ZDR organisation, then the ZDR-TEST organisation. Take `--phase before` immediately ahead of each proof step and `--phase after` right after it; the states are separate files.

| Step | What the operator does | Proof |
|---|---|---|
| a | The owner sets role `member` allow-listed to one chat model. | history row counted in the delta of the next step |
| b | The member opens `GET /api/v1/models` (the picker): only that model plus the Verevon modes. | by eye, screenshot |
| c | The member pins another model on the `/api/v1/chat` stream. | `--expect refused`: typed `model_not_entitled`, `refuse` moved, zero new thread, message, run, feedback, audit envelope and cache key |
| d | The member sends with Verevon Auto. | `--expect served` when it resolves inside the allowed set; `--expect refused` for `no_entitled_route` |
| e | The owner sends the model the member was refused. | `--expect served`: the owner is unaffected |
| f | The owner edits the policy; the member retries after the 30 s TTL. | `--expect served` or `--expect refused` by the edit |
| g | Stop org-core (`docker stop org-core-service`). Calls inside the stale bound are served (900 s when the last answer restricted, 120 s when it did not). Past it, a call is refused 503 `model_entitlement_unavailable`. Start org-core again; the next call resolves. | past the bound `--expect unavailable`; inside it `--expect served` |
| h | On ZDR-TEST: a policy with no attested model is refused 409; a ZDR turn that pins a denied model is `model_not_entitled`, an unresolved entitlement under ZDR is refused, never served. | `--expect refused` / `--expect unavailable`, the ZDR outbox row count must stay unchanged |
| i | Screenshots and the Playwright trace (`E2E_BASE_URL` second server on 5174: `pnpm test:e2e -- model-entitlement`). | files |

A delta where the proof says nothing may change is a defect, not a flake: stop, keep the output and find the writer. Known cause to check first: model-gateway admission fails open on a non-typed `GetModelAccess` error (depth register D1-498), so a refused turn can still create thread, message and run rows before inference-core refuses it.

Between steps record `docker compose images` digests for org-core, inference-core, model-gateway, execution-core, the gateway and the frontend, and the `model_entitlement_total` and `inference_requests_total` series (the proof prints the first; copy the second from `/metrics`).

Rollback: set the mode to `off` and recreate inference-core.

## 5. Production admin switch

Owner approval, then `MODEL_ENTITLEMENT_ADMIN_ENABLED=true` on the production gateway and `--phase preflight --expect-mode enforce --expect-admin true`. Until it is on the BFF answers `{enabled:false}` and the SPA section stays hidden.

## 6. Record

1. The evidence file holds every preflight, before, after and delta; add the Playwright report path, the curl and SSE transcripts, the image digests and the history rows (count and revision, no content).
2. `apps/Model Plane/MODEL_PLANE_STATUS.md` (append-only): the dated result of each flip with the evidence file. `apps/Model Plane/docs/decisions/ledger.md`: move the ADR-0005 entry from proposed to implemented only after step 4 passed on both organisations and the owner ratified DEC-13 (a), including the choice that an unknown mode value enforces.
3. Say plainly what stays outside the policy: service principals and scheduled runs (`service_exempt`), non-chat modalities, and the agentic path's effort ceiling until D1-499 is fixed.
