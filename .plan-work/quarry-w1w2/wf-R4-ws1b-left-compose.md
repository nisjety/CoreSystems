# R4-ws1b-left: compose env lines for WS1.B (not applied; compose files were not edited)

Both settings work today without any compose line, because of the code defaults. The lines below make them
visible and overridable. Apply them when the owner of each compose file next touches it.

## 1. BROWSER_BROKER_MAX_RUN_DOMAINS (Model Plane, the `browser-broker` service)

File: `apps/Model Plane/deploy/docker-compose.yml`, service `browser-broker` (around line 1615), in its
`environment:` block, directly under the existing `BROWSER_BROKER_MAX_ACTIVE_GRANTS_PER_OWNER` line (around line 1629):

```yaml
      # Hosts one derived run grant may hold, and how many a run may inherit from its
      # session grant without naming them (internal/server/config.go, WS1.B). Same rule as
      # the other limits: not a positive integer stops the broker at boot; clamped to 32.
      BROWSER_BROKER_MAX_RUN_DOMAINS: ${BROWSER_BROKER_MAX_RUN_DOMAINS:-8}
```

- Env name and default come from `internal/server/config.go` (`EnvMaxRunDomains`, `DefaultMaxRunDomains = grant.DefaultMaxRunDomains = 8`).
- Only the broker reads it. model-gateway, the Frontend gateway and Quarry do not.
- The L1 test `the broker defaults in compose match the Go defaults` (scripts/tests/browser-grant-contract.test.mjs)
  pins the TTL, per-owner cap and session domains but not this one. If the line is added, a matching
  `assert.match(config, /DefaultMaxRunDomains = grant.DefaultMaxRunDomains/)` and a compose assertion of
  `BROWSER_BROKER_MAX_RUN_DOMAINS: \$\{BROWSER_BROKER_MAX_RUN_DOMAINS:-8\}` belong next to the three existing ones.
- Not an env knob (hard-coded in the broker, `DefaultMaxActiveRunsPerParent`): the cap of 8 live run grants per session grant.

## 2. BROWSER_GRANT_ENFORCEMENT (Frontend Plane, the `gateway` service)

File: `apps/Frontend Plane/verevonv3/docker-compose.yml`, service `gateway` (line 61), `environment:` block,
for example right after `QUARRY_EDGE_URL` (line 134):

```yaml
      # In-app browser session grants (WS1.B, domains/browser.rs): off | permissive | strict.
      # Unset = permissive outside production and strict when APP_ENV=production. An explicit
      # `off` is refused in production (falls back to strict). An unknown value is the
      # environment's default, never a looser mode.
      BROWSER_GRANT_ENFORCEMENT: ${BROWSER_GRANT_ENFORCEMENT:-}
```

The empty default keeps the code's per-environment default (config.rs `browser_grant_mode`). Do not write
`:-permissive` in the base file: the production overlay would then inherit it, and only the code knows that the
production default is `strict`.

Production overlay: `apps/Frontend Plane/verevonv3/docker-compose.production.yml`, service `gateway`, `environment:`
(after `CONTROL_PLANE_ENFORCEMENT: strict`, line 13). Add this line ONLY after SC1 (the L2 run on the production overlay
stack) is green, as the plan's WS1.B rollout says. It is the default already, so the line is documentation plus a guard
against a stray override:

```yaml
            BROWSER_GRANT_ENFORCEMENT: strict
```

Rollout order from the plan: broker plus model-gateway, Quarry image, gateway at the environment default, L2, then
production `strict`. Rollback is `BROWSER_GRANT_ENFORCEMENT=off` on a non-production stack (instant), or swapping the
gateway image back.

Caveat for the dev stack (from the WS1.B hand-over): in `permissive` mode the gateway issues a grant and Quarry validates
it. If Quarry's grant validator is configured but cannot validate user grants (no service token yet: X-1 / WS1.A2),
creating a UI session fails at Quarry. That is a deployment ordering matter, not something these env lines can fix.

## 3. Nothing to add for model-gateway

model-gateway reads neither variable. The new `Idempotency-Key` handling on `POST /v1/browser/runs` needs no env.
