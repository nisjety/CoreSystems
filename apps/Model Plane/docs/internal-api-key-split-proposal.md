# Proposal: split the shared `INTERNAL_API_KEY` into per-core secrets

**Status: PROPOSAL ONLY — do NOT execute as part of this change.** Rotating a
secret that every running core authenticates with is high-blast-radius and must
be done by a human operator with the stacks up and a rollback path ready. This
document is the plan, the env-var inventory, the safe rollout order, and the
verification steps.

Authored during the Model-Plane approval-enforcement audit (Phase 7). It is a
cross-plane concern; it is filed here because Phase 7 produced it, not because it
is Model-Plane-specific.

## 1. Current state (the risk)

Inter-service calls authenticate with a **single shared symmetric secret**,
supplied by the caller as the `x-internal-api-key` header and validated by the
callee. It is wired through ~30 services under one env var, `INTERNAL_API_KEY`
(memory: "internal keys aligned to shared `1160414…`"), with a handful of
per-core override names already present (e.g. `INTEGRATION_COREV2_INTERNAL_KEY`,
read with a fallback to `INTERNAL_API_KEY` in exec-core's
`integration_tools::IntegrationActionsClient::from_env`).

Blast radius: any one service (or leaked env/log/image) that holds the shared
key can impersonate **every** caller to **every** core. There is no way to rotate
one trust edge, attribute a call to a specific caller, or revoke a single
compromised service.

Reference counts of `INTERNAL_API_KEY` / `x-internal-api-key` by service
(code + compose + env; indicative, not exhaustive):

| Service (plane) | refs |
|---|---|
| Frontend/verevon (+ v2/v3) | 275 / 43 / 23 |
| Control/auth-core | 29 |
| Ingestion/finspo-core | 11 |
| Data Plane v2/services | 11 |
| Ingestion/integration-corev2 | 10 |
| Control/user-core | 10 |
| Application/conversation-core | 9 |
| Application/convex-core, insight-core | 8 / 7 |
| Ingestion/services (shipping/imports), Quarry-v2 | 7 / 6 |
| Control/org-core, session-core, billing-core | 6 / 5 / 5 |
| Application/notification-core, information-core | 5 / 5 |
| Application/leads-core, social-core | 3 / 2 |
| Model Plane (exec-core, deploy, .env) | 2 / 1 / 1 |

## 2. Target state

**Per-callee (per-core-being-called) inbound secrets.** Each core that VALIDATES
inbound internal calls owns its own secret, `INTERNAL_API_KEY_<CORE>` (e.g.
`INTERNAL_API_KEY_ORG_CORE`). Each CALLER is configured with the specific key of
each core it calls, in a caller-side env named for the target
(`<TARGET>_INTERNAL_KEY`, extending the pattern that already exists for
integration-corev2). A caller that talks to N cores holds N distinct keys.

Result: leaking one core's inbound key grants access to that one core only; each
trust edge is independently rotatable; callee logs can attribute which caller key
was used (if keys are per-caller-per-callee — see the optional finer grain below).

Two granularities (pick one; recommend starting with per-callee):
- **Per-callee (recommended first step):** one inbound secret per validating
  core. Simple; N secrets total (~15 validating cores). Limits blast radius to a
  single core per leak.
- **Per-edge (optional, later):** one secret per (caller, callee) pair. Strongest
  attribution/revocation, but O(edges) secrets and heavier ops. Only pursue if a
  compliance driver requires per-caller revocation.

## 3. Services that need new env vars

**Validating cores (need a new INBOUND secret each):** auth-core, user-core,
org-core, session-core, billing-core (Control); documents-api / retrieval /
embedding / graph (Data Plane v2 `services`); integration-corev2 (already has
`INTEGRATION_COREV2_INTERNAL_KEY`), finspo-core, imports-core, shipping-core,
Quarry-v2 edge (Ingestion); conversation-core, insight-core, notification-core,
information-core, social-core, leads-core, convex-core (Application);
model-gateway / execution-core internal endpoints (Model Plane).

Each gets `INTERNAL_API_KEY_<CORE>` and, during rollout, ALSO keeps accepting the
legacy shared `INTERNAL_API_KEY` (dual-accept — see §4).

**Callers (need the target core's key under a per-target name):** the Frontend
gateway/BFF (verevon v3 gateway is the biggest caller surface), and every core
that calls another core (e.g. exec-core → integration-corev2, conversation-core →
integration-corev2, gateway → all cores). For each caller→callee edge, set
`<CALLEE>_INTERNAL_KEY=<the callee's INTERNAL_API_KEY_<CALLEE>>`.

Config source of truth: the per-plane `docker-compose.yml` `environment:` blocks
and the plane `.env` files. Compose `environment:` overrides `env_file`, so set
the new vars in compose for containers and in `.env` for host-run tools.

## 4. Safe rollout order (no mid-rollout auth breakage)

The invariant: **at every step, every in-flight caller still presents a key the
callee accepts.** Achieved with a dual-accept window.

1. **Generate** N new secrets (one per validating core), 32+ bytes CSPRNG, stored
   in the secret manager / plane `.env` (never committed).
2. **Callee dual-accept (deploy validators first):** update each validating core
   to accept EITHER its new `INTERNAL_API_KEY_<CORE>` OR the legacy shared
   `INTERNAL_API_KEY`. Deploy. No caller has changed yet → nothing breaks.
3. **Rotate callers, one edge at a time:** point each caller at the callee's new
   key via `<CALLEE>_INTERNAL_KEY`. Because the callee dual-accepts, a caller on
   the old key and a caller on the new key both succeed during the cutover.
   Roll callers callee-by-callee; verify each edge (see §5) before the next.
4. **Drop legacy acceptance:** once metrics/logs confirm ZERO calls arriving with
   the legacy shared key for a given callee (add a temporary counter/log line in
   step 2 that tags which key matched), remove the legacy `INTERNAL_API_KEY`
   acceptance from that callee. Deploy. That edge is now single-keyed.
5. **Decommission** the shared `INTERNAL_API_KEY` value everywhere once all
   callees have dropped legacy acceptance; scrub it from `.env`/compose/secret
   store and rotate it out of history handling per the existing secret-leak
   remediation process.

Rollback at any step: callers/callees still holding the legacy shared key
continue to work until step 4 for that callee, so reverting a single deploy is
safe.

## 5. Verification (per edge, before advancing)

- **Positive:** with the caller on the NEW key, a representative internal call
  returns 2xx (e.g. gateway → org-core list, exec-core → integration-corev2
  `/connections`). Existing smoke suites (`smoke-*`, `make test-endpoints`) with
  the new env exercise this.
- **Negative (the point of the split):** a caller presenting core A's key to core
  B is REJECTED (401). Add a focused test: call core B's internal endpoint with
  core A's `INTERNAL_API_KEY_<A>` and assert 401. This proves the keys are truly
  distinct and not cross-accepted.
- **Legacy drain:** the step-2 "which key matched" counter shows 0 legacy hits
  for the callee before step 4.
- **No silent fallback:** confirm no caller falls back to a shared/empty key
  (grep callers for `unwrap_or(INTERNAL_API_KEY)` style fallbacks — e.g.
  exec-core's `IntegrationActionsClient::from_env` falls back
  `INTEGRATION_COREV2_INTERNAL_KEY` → `INTERNAL_API_KEY`; after rollout the
  fallback should be removed so a missing per-target key fails loudly, not
  silently reuses the shared key).

## 6. Why this is not executed here

Every running core authenticates with the shared key today. A wrong ordering (or
a single missed caller) locks services out of each other fleet-wide. This needs
the stacks up, per-edge verification, the dual-accept counter for the drain
decision, and an operator watching. It is out of scope for an unsupervised
code-change stream.
