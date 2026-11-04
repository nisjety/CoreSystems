# W2-CP-R13-2 status: static credentials PR 2 and the Quarry scrape job (lane d, branch w23/lane-d)

## Built
All of the scope. Nothing of it existed beforehand (checked: no quarry rows in any Control compose registry, no `quarry-edge` job, no `quarry*` rules file). The lane was fast-forwarded to `w23/integ` first (clean).

Compose (`apps/Control Plane/docker-compose.yml`, LF file):
- org-core `ORG_CORE_SERVICE_CREDENTIALS`: new principal `quarry-edge` (`org:retention:read:any`, `org:keys:wrap:any`, `org:keys:unwrap:any`), token `${QUARRY_ORG_CORE_SERVICE_TOKEN:?}`; the `auth-core` row gains `org:web-policy:read:any`. The comment that said "nobody else holds it" now states the two-holder rule.
- billing-core `BILLING_CORE_SERVICE_CREDENTIALS`: new principal `quarry-edge` (`billing:quota:read:any`, `billing:entitlement:read:any`), token `${QUARRY_BILLING_CORE_SERVICE_TOKEN:?}`.
- audit-core `AUDIT_CORE_SERVICE_CREDENTIALS`: new principal `quarry-control` (`audit:write`, planes `["ingestion"]`), token `${QUARRY_CONTROL_AUDIT_CORE_SERVICE_TOKEN:?}`.
- Default-off env: org-core `ORG_CORE_KEYS_ENABLED=0` (plus `ORG_CORE_KEY_MASTER_KEYS` and `ORG_CORE_KEY_MASTER_ACTIVE` passed through empty); auth-core `WEB_POLICY_CLAIM_ENABLED=0` and `API_KEY_AGENT_EXCHANGE_ENABLED=0`.
- Prometheus: mounts `${QUARRY_METRICS_TOKEN_FILE:-./observability/quarry-metrics-token.placeholder}` at `/etc/prometheus/secrets/quarry_metrics_token`. The placeholder is a tracked EMPTY file. Reason, proven here: `promtool check config` with the `quarry-edge` job and no credentials file fails ("stat ... no such file"), so the file must always exist.
- `docker-compose.production.yml`: a comment-only block on `controlplane-prometheus` (the base mount already owns the path, so a Compose secret cannot be added; the release points `QUARRY_METRICS_TOKEN_FILE` at the secret manager's file). Both renders pass `docker compose config --no-interpolate`.

Minting and the ledger (files outside the unit's list, required; the same reason R13-1 gave for its jq edit):
- `scripts/run-control-plane.sh`: the three new tokens join `required_credentials` (minted once with `persist_if_missing`, never repointed); a new block mints `QUARRY_EDGE_METRICS_TOKEN` once, writes it to the gitignored `observability/.secrets/quarry_metrics_token` (0644, so the `nobody` Prometheus can read the bind mount) and sets `QUARRY_METRICS_TOKEN_FILE` to the relative path. Same variable name as the edge reads, so mirroring into Ingestion is a copy.
- `scripts/validate-release-credentials.sh` and its test: the three names join the preflight; a configured `QUARRY_METRICS_TOKEN_FILE` must be a readable file of at least 32 characters (never printed); unset is fine.

org-core (`internal/http`, CRLF files; `service_auth.go` is outside the list, required):
- `service_auth.go`: `org:keys:wrap:any`, `org:keys:unwrap:any`, `org:web-policy:read:any` joined `orgServiceScopeAllowlist`. Without it org-core refuses the whole registry at boot ("unknown scope"), and `TestComposeOrgCoreRegistryPassesStartupValidation` fails. No route exists for them yet (WS17.E2a and WS7.H register routes later); no holder pin was added in the validator, so those units keep their own freedom.
- `retention_posture_contract_test.go`: `TestOnlyAuthCoreMayReadRetentionPosture` became `TestOnlyAuthCoreAndQuarryEdgeMayReadRetentionPosture` (every other principal 403, both holders 200, both refused the broad org reads); added `TestComposeRetentionPostureScopeHasExactlyTwoHolders`, `TestQuarryEdgePrincipalHoldsOnlyRetentionRead` (the name the queue asked for; the principal holds exactly one read scope, the retention posture read, plus the two key-custody scopes, and its token comes from `QUARRY_ORG_CORE_SERVICE_TOKEN`), `TestComposeKeyAndWebPolicyScopeHolders`; auth-core's reviewed set now has four scopes.
- `service_auth_test.go`: `TestOrgKeysAndWebPolicyScopesAreMachineOnly`, `TestValidateRequiredRegistryAcceptsTheQuarryEdgePrincipalAndDoesNotRequireIt`.

New compose contract tests (new files, disjoint from L1 and S7b which edit existing files): `billing-core/internal/http/quarry_compose_contract_test.go` (3 tests: the block passes billing-core's real startup validator, quarry-edge is read-only and token var pinned, usage writers unchanged) and `audit-core/internal/api/quarry_compose_contract_test.go` (3 tests: validator, quarry-control is an ingestion-only writer with its token var pinned, writers are exactly integration-corev2 and quarry-control).

Observability:
- `observability/prometheus.yml`: job `quarry-edge` (target `quarry-edge:9092`, `/metrics`, `authorization.credentials_file`), commented with the owner flip.
- `observability/rules/quarry.rules.yml` (+ `quarry.rules.test.yml`): 6 alerts. NAMED `quarry.rules.yml`, not `quarry.yml` as the queue wrote: the existing `rule_files` glob and the CI promtool job only take `*.rules.yml`, so the queue name would never load.

## Commits (lane d)
- 611c3e937 feat(control): Quarry static credentials in compose, dark flag defaults, Prometheus token mount (R-13 PR 2)
- abce3c889 test(control): two-holder retention rule, quarry-edge org scopes, billing and audit compose contracts (R-13 PR 2)
- 9cf6b2bbf feat(control): quarry-edge Prometheus scrape job and alert rules (U48)
- docs(plan-work) commit with this file and the depth rows

## Tests run (all on the host unless noted; all passed)
- org-core `REQUIRE_CONTRACT=1 go test -count=1 -p 1 ./...`: every package ok; `internal/http` 99 passed, 3 skipped (the existing DSN-gated ones), 0 failed. Of those, new or changed: 3 new compose contract tests + 2 new service_auth tests + 1 renamed and rewritten + 1 updated.
- billing-core `go test ./internal/http`: ok (3 new compose contract tests pass, run with `REQUIRE_CONTRACT=1`).
- audit-core `go test ./cmd/server ./internal/api`: ok (3 new compose contract tests pass, run with `REQUIRE_CONTRACT=1`).
- promtool (docker `prom/prometheus:v2.54.1`, the CI image): `check rules` quarry 6 rules and audit-core 11 rules SUCCESS; `test rules` quarry.rules.test.yml 16 test cases SUCCESS, and the three existing rule test files still SUCCESS; `check config` with the new job and the placeholder mounted: valid, 4 rule files found; the same config without the credentials file: FAILED (kept as the evidence for the placeholder).
- auth-core jest `deployment-contract` (reads both compose files): 30 passed; `plane-service-principals-policy.contract`: 88 passed.
- Script probes (scratchpad, not committed): release preflight, the R-13 PR 2 lines only: 10 of 10 (names present, valid release clean, token file ok/unset/missing/short, missing/short/placeholder/reused credential, token value never printed). Runner token block with the runner's own helper functions: 6 of 6 (minted once, 64 hex, file equals the persisted value, compose var relative, value not written to the compose env file, second run does not re-mint, an existing value is kept).
- `docker compose config --no-interpolate`: base and base+production both render.

## Flag and default
`ORG_CORE_KEYS_ENABLED=0`, `WEB_POLICY_CLAIM_ENABLED=0`, `API_KEY_AGENT_EXCHANGE_ENABLED=0` (compose passes them; the consuming units read them later). The Quarry edge's `QUARRY_EDGE_METRICS_ENABLED` stays 0 (Ingestion compose, not touched): that is the owner flip for METRICS.

Owner flip to turn Quarry metrics on (nothing here does it): (1) copy `QUARRY_EDGE_METRICS_TOKEN` from Control's `.env.generated-secrets` into Ingestion's `.env.generated-secrets` (same name, same value); (2) set `QUARRY_EDGE_METRICS_ENABLED=1` on the Ingestion stack and recreate `quarry-edge`; (3) `docker restart controlplane-prometheus` (or recreate it, so the runner's `QUARRY_METRICS_TOKEN_FILE` mount applies); (4) in the Prometheus UI the `quarry-edge` target turns up. Rollback: set the flag to 0. `QuarryEdgeMetricsDown` then fires 5 minutes after the target disappears and stays firing until its 7-day lookback ends (silence it or remove the job to stop it sooner). Credential mirror for the other three tokens is the same copy into Ingestion's store when the Quarry units that present them are enabled (W2-Q-S4a org-core, WS6.D billing-core, W2-CP-S7b audit-core).

Rollout order (this matters): rebuild org-core BEFORE recreating it with the new compose, because an org-core image without the three new scopes refuses the whole registry at boot (fail-closed, as the existing comment warns). billing-core and audit-core need no rebuild: both scope sets were already valid.

## Assumed decisions
- Token variable names: `QUARRY_ORG_CORE_SERVICE_TOKEN` and `QUARRY_CONTROL_AUDIT_CORE_SERVICE_TOKEN` as the plan names them; `QUARRY_BILLING_CORE_SERVICE_TOKEN` (the plan gives none for billing) by the same pattern.
- billing-core scopes are the plan's (`billing:quota:read:any`, `billing:entitlement:read:any`), not a new "org-bound" scope: billing-core has no org-bound service scope for a machine caller (`:self` is gateway-only by validation). "Org-bound" is therefore only as strong as the handler L1 writes (it must take the org from the path and scope the answer to it).
- Org key master keys (`ORG_CORE_KEY_MASTER_KEYS`, `ORG_CORE_KEY_MASTER_ACTIVE`) are passed through empty and NOT minted by the runner: losing them destroys every wrapped KEK, so the owner generates them once under a custody runbook before `ORG_CORE_KEYS_ENABLED=1` (E2a decides the final names; the plan's names are used).
- The Prometheus token file is 0644 in the dev runner (Prometheus runs as `nobody`); production uses the secret manager's file via the same variable.
- No holder pin for `org:keys:*` or `org:web-policy:read:any` in org-core's validator (only compose-level tests), so WS7.H and WS17.E2a may adapt without touching a pinned validator. Tests pin the holders in compose.
- `QuarryEdgeMetricsDown` looks back 7 days so a stack that never enabled metrics never alerts.

## Model Plane asks
None. Nothing appended to MP-ASKS.md.

## Not verified
- No compose `up`, no live org-core/billing-core/audit-core restart, no live Prometheus scrape: the stack was not touched (rule). The registries were validated by each core's real startup validator on the compose text only.
- Not run: `validate-release-credentials-test.sh` end to end. On this Windows host it fails before reaching the new checks, for reasons that exist without this unit: the test expects a root `.env.example` that is not tracked (the sibling `control-service-env-contract-test.sh` forbids it), `openssl -subj '/CN=...'` is rewritten by Git Bash path conversion, and the file-permission checks reject NTFS modes. The new assertions in it were added and exercised only through the probe above.
- `scripts/control-service-env-contract-test.sh` not run: needs per-core `.env` files that a worktree lacks.
- The runner was not run as a whole (needs the cores' `.env` files); only its new block, with its own helpers.
- The real `Authorization: Bearer` handshake between Prometheus and the edge listener (U48 deployment evidence in RUNBOOKS stays owed).
- Docs: RUNBOOKS.md ("Not built: ... the Control Plane Prometheus job quarry-edge and its token secret ... alert rules" and the sentence at the "To scrape it from the Control Plane Prometheus" line) is now stale; not edited here (many units write RUNBOOKS.md this wave; W2-DOC-A or the next Ingestion unit should correct the two sentences).

## Depth rows
Appended to `.plan-work/quarry-w1w2/W23-depth-lane-d.md` (tag W2-CP-R13-2).
