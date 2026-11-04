# mp-browser-cap deploy to live dev (2026-10-29)

Owner's instructions: "dette er kun dev" and "Fiks her". This deploys the F1-F4 fix for SC1 leg C (see `LEGC-DIAGNOSIS.md`) to the local live dev stack. Nothing was pushed. No secret value was printed or written, except the one new credential line in Control's secrets store (section 3).

## 1. What was built, and from which commit

- **Source:** branch `mp-browser-cap` at **`8e15216bf9b402a7f64d1a618110c51417674138`**, in worktree `C:/dev/CoresSystem/.claude/worktrees/mp-browser-cap`. The commits on top of base `0b5d2b7b6` are:
  - `9c9cede2c` feat(execution-core): a validated browser run grant satisfies the high-risk ask; capability denies are logged and classified (F2, F3)
  - `fac950175` feat(capability-core,browser-broker): owner attestation lane for cap.browser.open (F1)
  - `a785d8b3c` fix(model-plane): keep the classified browser failure code and replay the run's terminal event (F3, F4)
  - `8e15216bf` fix(browser-broker,auth-core): attest only while gRPC serves; bind the broker principal to global only
- **Not deployed:** the worktree has four uncommitted, modified files: `capability-core/internal/policy/engine.go` and `engine_policy_test.go`, and `execution-core/src/capability_policy.rs` and `runtime_loop/browser_grant_approval.rs`. These are review fixes B1 and A1/B2 from `side-session-notes/.../mp-browser-cap/review-fixes.md`: a stored `ask` is reported as `tool_policy_ask`, and `verify_ask` checks the signed claims under every gate mode. They were not committed when the deploy ran (HEAD was at 18:47 local; the files are dated 18:35). The images were built from a clean `git archive 8e15216bf` export (`%TEMP%/mpcap-src-8e15216`, since deleted), so the uncommitted edits are **not** in the images. To ship them, commit them and redeploy capability-core and execution-core with this recipe.
- **Session-core migration line endings:** the live `session_core._sqlx_migrations` checksums for 0001-0034 are of CRLF files, and 0035-0068 are of LF files. The export is all LF, as `.gitattributes eol=lf` makes it. Before the build, the export's 0001-0034 were rewritten to CRLF, after checking every version against the live SHA-384. All 60 applied versions matched one form, and 0069 is new. Without this step session-core crash-loops ("previously applied but has been modified"; see the `zdr-live-proof-recipe` memory). The repository was not changed.
- **Build:** `docker build --build-arg SOURCE_REVISION=8e15216bf… --build-arg BUILD_DATE=…`, run from the export's `apps/Model Plane`. The context is `.` for Rust and `go` for Go. Each image was tagged `model-plane-<svc>:mpcap-8e15216bf` and then retagged `:latest` just before its recreate.

### Services rebuilt and why

| Service | Why | New image id (`org.opencontainers.image.revision` = 8e15216bf) | Live image before (its revision) |
|---|---|---|---|
| session-core | F3: `permission_denied` maps to `failed`; three failure codes added to the allowlist; migration 0069 | `sha256:f1986377f7bb32b949f227c78fb8da94a371dae7381181b50cf8fe8461c0d624` | `sha256:a5e060cdc936…` (d67c5a43f) |
| capability-core | F1: browser owner lane `/api/v1/capabilities/browser/health` and authz | `sha256:3b7f142072bbb0e133a37c25216d08c0eafb0b644ceb126715cb923fd69fa791` | `sha256:ab7dd6951526…` (d67c5a43f) |
| browser-broker | F1: health attester (`internal/healthattest`) and its env | `sha256:a1908ea64bc35f3230e1581cdad3108c96fd846d0d01ac0607127b5e63f59949` | `sha256:ea91868594dd…` (e603fde78) |
| execution-core | F2: grant-as-approval lift for `browser_agent`; F3: the WARN deny log and its classification | `sha256:9160a6e4c06b62046a7dbb742cb52799bb364a43fa2f253828b88f11bd58e27f` | `sha256:1138c95172b3…` (0b5d2b7b6) |
| model-gateway | F3: keeps the classified failure code (with a fallback retry); F4: replays terminal events | `sha256:6b63a6cc841bcf229d0d44703e83c95d08fc892c2dbe9a0491b2e5f993e45091` | `sha256:189442c16320…` (0b5d2b7b6) |
| auth-core (`auth-service`) | **Not rebuilt.** Only its registry env changed (section 3), and its image is the same (`sha256:60c620875b78…`) | n/a | n/a |

The live session-core and capability-core images were built from d67c5a43f, and the live broker from e603fde78, which are older than base 0b5d2b7b6. The rebuild therefore also carries the base's own delta for those services. That delta is small and additive: one `execution.proto` field and its Go bindings, `inference.proto`, `org_agent_policy` (one line plus tests), `llmreviewer` (one line). It has no session-core source change and no migration other than 0069. No proto changed on the branch, so buf was not needed. The SPA and the verevonv3 gateway were not touched.

## 2. Rollback tags (made before anything changed)

```
rollback/model-plane-session-core-1:pre-mpcap-20261029     sha256:a5e060cdc936399a535d69b4e4197cc00d09d1ac28427bc5d466e2b099f5b4d4
rollback/model-plane-capability-core-1:pre-mpcap-20261029  sha256:ab7dd6951526c880fc5774da699f5cd346ef34caa7b50e8c1cb067c5a37a41de
rollback/model-plane-browser-broker-1:pre-mpcap-20261029   sha256:ea91868594ddeca2448865ae45a514ed6521968dbca0f66291aa95f1ba484c08
rollback/model-plane-execution-core-1:pre-mpcap-20261029   sha256:1138c95172b3ac333c926cbb23040ae19fb261015b0a54e780bd6295b8da37df
rollback/model-plane-model-gateway-1:pre-mpcap-20261029    sha256:189442c16320fa43d853405cd88917fe9182cfa498f820231e8f22967fb89f40
rollback/auth-service:pre-mpcap-20261029                   sha256:60c620875b78a858d97327dffb31356ea3eb499d7b0dd05bbb77ffc792e5ecb5
```

## 3. Control Plane registry row (needed, applied)

The broker attests as `service:browser-broker`, so auth-core needs that principal.

- **Policy:** the row is already committed on `mp-browser-cap`, in `apps/Control Plane/config/plane-service-principals.json`, by `fac950175` and narrowed by `8e15216bf`. It has audiences `capability-core`, `orgIds ["global"]` with no `tenancy`, scopes `capability:browser:health:write` and `capability:read`, retention `persistent`, and consumerEnv `BROWSER_BROKER_SERVICE_API_KEY`. **No separate policy commit was needed**, and none was made. Main's policy does not have the row yet.
- **Live apply:** done by `scratchpad/mpcap/auth-registry-mpcap.sh`, which follows `auth-registry-recreate.sh` semantics. It takes the registry from the running `auth-service`. It keeps every row and credential. It re-applies the quarry-edge `browser-broker` row from **main's** policy and capability-core's `session:memory-controls:read`, both idempotently. It adds the `browser-broker` row, assembled with mp-browser-cap's `scripts/plane-principal-registry.jq` in the development profile, and the result has no `allowAnyOrg`. Then it runs `run-control-plane.sh up -d --no-deps --no-build auth-core`, with the registry in a shell variable only.
- **Credential:** minted with `openssl rand -hex 32`, the same as `random_value`. It was appended once to `C:/dev/CoresSystem/apps/Control Plane/.env.generated-secrets` as `BROWSER_BROKER_SERVICE_API_KEY`, the same as `persist_if_missing`, and the file went from 123 to 124 lines. That file is the first `--env-file` of the Model Plane chain, so auth-core's registry and the broker resolve one value. A later `run-control-plane.sh` run adopts the stored value and does not mint a new one.
- **Registry signature after** (`regsig.sh`): the same 19 principals plus `browser-broker` (cred sha12 `f49dc5c69996`, policy `b697716d3181`). Every other credential and policy hash is unchanged.
- **Incident, fixed:** the first apply (18:05:30Z) took the quarry-edge patch from mp-browser-cap's policy, which does not have quarry-edge's `browser-broker` audience. For about 6 minutes quarry-edge's row lacked `scopesByAudience.browser-broker`, so the SC1 grant path could not mint `browser:read`. The second apply (18:11:16Z) takes it from main's policy, as `auth-registry-recreate.sh` does, and the quarry-edge policy hash is back to `dfa9391fd8fd`. No SC1 leg was running in that window.
- **Caveat (drift):** `run-control-plane.sh` assembles the registry from **main's** policy. Until the `browser-broker` row reaches main, a normal Control Plane bring-up drops the row, so the attester gets 401 or 403 and `cap.browser.open` lapses to `health_attestation_stale` within 5 minutes. To restore it, re-run `auth-registry-mpcap.sh`, which reuses the stored credential.

## 4. Recreate (only the five containers, live env)

- **Wrapper:** `scratchpad/mpcap/mpcap-compose.sh` runs `apps/Model Plane/scripts/compose.sh` of **mp-browser-cap**. It passes the live chain as extra `--env-file` inputs, in this order: Control `.env.generated-secrets`, Model `.env.generated-secrets`, Model `.env`, and then `EXTRA_ENV_FILE=C:/dev/backups/live-proof-2026-10-27/anthropic-test.env`, last, as `recreate-with-live-env.sh` does. It also carries all five Model Plane NATS passwords over from `model-plane-nats-1`. compose.sh refuses to run without `deploy/.env`, so an **empty, gitignored placeholder** `apps/Model Plane/deploy/.env` was created in the mp-browser-cap worktree, and it was left there for rollback and re-runs.
- **Recreate:** `scratchpad/mpcap/recreate.sh <svc>` retags `:latest` and runs `up -d --no-deps --no-build <svc>` with `BROWSER_BROKER_GRANT_TTL_SECONDS=45` in the shell. The order was session-core 18:12:09Z, capability-core 18:13:17Z, browser-broker 18:13:43Z, execution-core 18:14:43Z, model-gateway 18:15:10Z.
- **Dry run first:** `compose config` was resolved through the wrapper and compared key by key (hashes only) with each running container's env.
  - The first pass, without `EXTRA_ENV_FILE`, showed `EXECUTION_AGENT_MODEL` and `MODEL_GATEWAY_AUX_MODEL` drifting. The EXTRA file was added, and after that the only difference was the broker's four new keys.
  - Networks, host ports and `read_only` were identical for all five.

### Env diff (key names only), before vs after, from `envsig.sh`

| Container | Added | Removed | Changed |
|---|---|---|---|
| session-core | none | none | `BUILD_DATE`, `SOURCE_REVISION` (image ENV, i.e. provenance) |
| capability-core | none | none | none |
| browser-broker | `AUTH_CORE_URL` (`http://host.docker.internal:3011`, from the live chain), `BROWSER_BROKER_QUARRY_READY_URL` (`http://quarry-edge:8082/ready`), `BROWSER_BROKER_SERVICE_API_KEY`, `CAPABILITY_CORE_HTTP_URL` (`http://capability-core:8085`) | none | none (`BROWSER_BROKER_GRANT_TTL_SECONDS` is still 45) |
| execution-core | none | none | none |
| model-gateway | none | none | `BUILD_DATE`, `SOURCE_REVISION` (image ENV) |
| auth-service | none | none | `PLANE_SERVICE_PRINCIPALS_JSON` (plus the `browser-broker` row) |

Before the recreate, all three broker endpoints were checked from the broker's network: auth-core :3011 answered, quarry-edge `/ready` returned 200, and capability-core :8085 answered.

## 5. Verification (18:12Z to 18:19Z)

- **Health.** All six recreated containers are `running/healthy` with 0 restarts. capability-core logs "capability decision evidence signer enabled", and no recreated service logs a NATS `Authorization Violation` from its own NATS user.
- **Migration.** session-core logged "migrations applied". 0069 was inserted, and the 60 earlier checksums all matched.
- **Broker.** The broker logs `cap.browser.open health attestation on interval_seconds=60 quarry_probe=true` and then `cap.browser.open health attested state=available reason_code=browser_broker_ready` every 60 s.
- **`session_core.capabilities` (SELECT):**

  ```
  approval_required|browser_broker_ready|2026-10-29 18:15:19.137218+00|00:00:24
  approval_required|browser_broker_ready|2026-10-29 18:16:19.128663+00|00:00:58
  approval_required|browser_broker_ready|2026-10-29 18:17:19.120381+00|00:00:45
  ```

  Before the deploy it read `unavailable|health_not_attested|<null>`. Note that the stored `availability_state` is **`approval_required`**, not `available`. This is capability-core's contract: a high-risk capability's `available` attestation is stored and derived as `approval_required`, with mode `agentic`. The broker attests `available`. `health_checked_at` advanced every 60 s across two refresh intervals and stayed under the 5-minute TTL.
- **`sc1-mode.sh status`.** The SC1 posture is unchanged:
  - auth-core: registry ok, capability-core memory-controls kept;
  - broker: TTL 45 (sc1);
  - gateway: strict (sc1);
  - quarry-edge: production, read-only, cap_drop ALL, no host ports;
  - SPA: 200; unauthenticated `/api/v1/browser/sessions`: 401.
- **Preflight.** `GATEWAY_CONTAINER=verevon-gateway-rs bash scripts/tests/browser-grant-prod-contract.sh --preflight-only` printed **preflight passed**, with all 19 checks ok.
- **ERROR lines since each recreate:**
  - capability-core, browser-broker, execution-core, model-gateway and auth-service: 0.
  - **session-core: 5**, all at 18:12:17.15-.16, during start-up only, and none since. They are `async_nats::connector` "authorization violation" on connections to the `control-shared-nats` cluster. The client reaches peer server ids it has discovered in the cluster gossip, gets the violation there, and connects on another address. Every consumer reports ready: GDPR erasure consumer, Space membership consumer, orchestration bridge, and `tools.completions.*`. session-core's env is byte-identical to before, apart from `BUILD_DATE` and `SOURCE_REVISION`, and the branch does not touch NATS code. This is environmental and not new behaviour, but it could not be compared with the old container's logs, because those were removed when the container was recreated.
- **WARN lines worth knowing, all pre-existing configuration with env unchanged:**
  - capability-core: "cron sweeper not started" and "watches unavailable".
  - model-gateway: "finetune poller credential mint failed … 403". The model-gateway registry row and env are unchanged, and the poller code is not in the diff.
- **Not done here:** an end-to-end leg C run (it needs a signed-in user; it is in the DESIGN post-deploy preflight). A forced-deny check is for a scratch stack only.

## 6. How to roll back

Roll back one service, or all of them, in the reverse order: model-gateway, execution-core, browser-broker, capability-core, session-core. The rollback uses the original compose (epic-shaw) and the live env, the way the containers were created before.

```bash
EP=C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/.plan-work/execution/tools/recreate-with-live-env.sh
ST=C:/Users/IMAFER~1/AppData/Local/Temp/claude/C--dev-CoresSystem--claude-worktrees-vigorous-perlman-e7ae5b/562f6350-4d29-46dc-a04b-eee466d0c963/scratchpad/stack
export EXTRA_ENV_FILE=C:/dev/backups/live-proof-2026-10-27/anthropic-test.env
for s in model-gateway execution-core capability-core; do
  docker tag rollback/model-plane-$s-1:pre-mpcap-20261029 model-plane-$s:latest && bash "$EP" $s
done
# broker (keeps the SC1 TTL; the old image ignores the 4 new keys, which compose from epic-shaw does not set anyway)
docker tag rollback/model-plane-browser-broker-1:pre-mpcap-20261029 model-plane-browser-broker:latest
BROWSER_BROKER_GRANT_TTL_SECONDS=45 bash "$ST/broker-compose.sh" up -d --no-deps --no-build browser-broker
```

**session-core rollback**, in this order:

1. Migration 0069 is applied, so the old binary (d67c5a43f) refuses to start unless `SESSION_CORE_MIGRATION_IGNORE_MISSING=true` is set (it supports this). Either export that variable for the recreate, or do a full revert:
   1. Check that `SELECT failure_code, count(*) FROM managed_run_terminalization_outbox WHERE failure_code IN ('browser_permission_denied','browser_resource_exhausted','browser_timed_out') GROUP BY 1` returns no rows.
   2. Run `migrations/revert/0069_browser_failure_codes.down.sql`.
   3. `DELETE FROM _sqlx_migrations WHERE version = 69`. This is a write, and it needs the owner's approval.
2. Then run `docker tag rollback/model-plane-session-core-1:pre-mpcap-20261029 model-plane-session-core:latest && bash "$EP" session-core`.

**auth-core:** the image is unchanged. To remove the row, run `bash "$ST/auth-registry-recreate.sh"`. That script does not add `browser-broker`, but it starts from the running registry, so you must delete the row first. The simplest way is a one-off copy of `auth-registry-mpcap.sh` with `del(.["browser-broker"])` in place of the assignment. The `BROWSER_BROKER_SERVICE_API_KEY` line in Control's store can stay, because nothing reads it without the row. With the row gone, the broker's attestations fail, and `cap.browser.open` lapses to stale, then denied, within 5 minutes. That is the pre-deploy behaviour.

## Files

- **Scripts** are in `C:/Users/IMAFER~1/AppData/Local/Temp/claude/C--dev-CoresSystem--claude-worktrees-vigorous-perlman-e7ae5b/562f6350-4d29-46dc-a04b-eee466d0c963/scratchpad/mpcap/`:
  - `mpcap-compose.sh` and `recreate.sh`;
  - `auth-registry-mpcap.sh` (`DRY_RUN=1` prints the principal names and the row without its credential);
  - `build.sh`, `migcheck.py` and `envdiff.js`.
- **Evidence** is in `before/` and `after/`: envsig, the registry signature, sc1 status and preflight output.
- **Deleted after the deploy:** the `%TEMP%/mpcap-src-8e15216` export and the resolved `cfg.json`, which held secrets.

## Redeploy 2: review fixes B1 and A1/B2 (2026-10-29, 18:28Z to 18:46Z)

Same owner instruction ("dette er kun dev", "Fiks her"). Only capability-core and execution-core were rebuilt and recreated. Nothing was pushed, no secret value was printed or written, and live databases were only read (`SELECT` inside `BEGIN READ ONLY`).

### Source and commit

- When this step started, `mp-browser-cap` HEAD was still `8e15216bf` (the build of the first deploy). The four review-fix files from section 1 ("Not deployed") were still uncommitted and unchanged since 17:35Z, and no test process was running. The code step's report only said it was waiting for its test runs.
- The tests were therefore re-run here on the worktree as it stood:
  - `go vet` and `go test ./services/capability-core/...`, through the `go/go.work` workspace, passed. `GOWORK=off` does not work for capability-core, because it imports the workspace module `pkg/*`, so the workspace was used. `TestDecideWithPolicies*` was also run with `-count=1`.
  - `cargo test -p execution-core --lib`, with `CARGO_TARGET_DIR=%TEMP%/mpcap-review-fixes` reused, gave **1470 passed, 0 failed**. This includes the two new `an_ask_*` tests and the `browser_grant_approval` suite.
  - gofmt is clean on LF copies. A standalone rustfmt reported differences only outside the changed hunks.
- The four files were then committed as **`246f79bda7abf9feb7bf47d0735b54a60a6a58dc`**: `fix(capability-core,execution-core): a stored ask is reported as tool_policy_ask; the grant lift verifies the signed ask claims in every gate mode`. The gitleaks hook found no leaks. The commit was not pushed, and the worktree is now clean.
  - **B1 (capability-core):** a stored `ask` (or an out-of-vocabulary decision) over an ask reports `tool_policy_ask`.
  - **A1/B2 (execution-core):** `verify_ask` requires v2 claims whose signed `policy_source`, tool and user match, under every gate mode.

### Build

- The source was a clean `git archive 246f79bda "apps/Model Plane"` export at `%TEMP%/mpcap-src-246f79b`, which was deleted after the build. Session-core is not involved, so no migration EOL conversion was needed.
- The build ran `scratchpad/mpcap/build2.sh`. The build contexts are `go` (Dockerfile `go/services/capability-core/Dockerfile`) and `.` (`rust/services/execution-core/Dockerfile`). `SOURCE_REVISION` is 246f79bda. The images were tagged `model-plane-<svc>:mpcap2-246f79bda`, then retagged `:latest` at recreate.

| Service | New image (`org.opencontainers.image.revision` = 246f79bda) | Before (8e15216bf) |
|---|---|---|
| capability-core | `sha256:b92b650ba3f6852154884634cece7b94a42f3a7e423d29ecd77b305934f6e77e` | `sha256:3b7f142072bb…` |
| execution-core | `sha256:eacfc1f7617004c221f48f4152f0c8ba2103fb01364b78326ac4f930e4e63a06` | `sha256:9160a6e4c06b…` |

### Rollback tags (made before anything changed)

```
rollback/model-plane-capability-core-1:pre-mpcap2-20261029  sha256:3b7f142072bbb0e133a37c25216d08c0eafb0b644ceb126715cb923fd69fa791
rollback/model-plane-execution-core-1:pre-mpcap2-20261029   sha256:9160a6e4c06b62046a7dbb742cb52799bb364a43fa2f253828b88f11bd58e27f
```

### Dry run and recreate

- The dry run resolved `compose config` through `mpcap-compose.sh`, with the live env chain, NATS carry-over and `EXTRA_ENV_FILE`, with `SOURCE_REVISION=246f79bda`. It was then compared with `envdiff.js`.
  - Both services: added, removed and changed were all empty.
  - Networks (`default` = `model-plane-network`, `inter-plane-bus`), host ports (`127.0.0.1:8085/9097` and `127.0.0.1:18083/9093`) and `read_only=false` were identical.
  - The resolved `cfg2.json` was deleted straight after.
- The recreate ran `scratchpad/mpcap/recreate2.sh <svc>`, which is `recreate.sh` with the mpcap2 tag and the pinned `SOURCE_REVISION`. capability-core was recreated at 18:42:38Z and execution-core at 18:43:03Z.
- **envsig before and after** (`before2/`, `after2/`): byte-identical for both containers. No key was added, removed or changed.

### Verification (18:43Z to 18:46Z)

- **Health:** both containers are `running/healthy` with 0 restarts. capability-core logs "capability decision evidence signer enabled" and "gRPC listening :9097". There is no NATS `Authorization Violation`.
- **`cap.browser.open`** (`capq.sh`, read-only, from `session_core.public.capabilities`):

  ```
  before  approval_required|browser_broker_ready|2026-10-29 18:32:19.023858+00
  after   approval_required|browser_broker_ready|2026-10-29 18:43:18.975786+00
          approval_required|browser_broker_ready|2026-10-29 18:44:18.954677+00
          approval_required|browser_broker_ready|2026-10-29 18:45:18.948966+00
  ```

  The broker's attestations land on the new capability-core every 60 s ("health attested state=available reason_code=browser_broker_ready"). The stored value is `approval_required`, by the high-risk contract described in section 5.
- **`sc1-mode.sh status`:** unchanged; the before and after outputs differ only in the timestamp line.
- **Preflight:** `GATEWAY_CONTAINER=verevon-gateway-rs bash scripts/tests/browser-grant-prod-contract.sh --preflight-only` printed **preflight passed**, with 19 checks ok.
- **ERROR lines since 18:42:30Z:** 0 in capability-core, execution-core, browser-broker, session-core, model-gateway and auth-service, and all six have 0 restarts. capability-core's only WARNs are the known "cron sweeper not started" and "watches unavailable", and execution-core logged no WARN.
- **Not done here:** an end-to-end leg C run (it needs a signed-in user).

### Behaviour note

After B1, an org or user `ask` policy on `cap.browser.open` is reported as `tool_policy_ask`, so a browser run grant no longer lifts it. Only the bare risk-level ask is liftable. No such policy row was checked for or written here.

### Rollback (Redeploy 2 only)

This rollback returns to the first deploy (8e15216bf):

```bash
P=C:/Users/IMAFER~1/AppData/Local/Temp/claude/C--dev-CoresSystem--claude-worktrees-vigorous-perlman-e7ae5b/562f6350-4d29-46dc-a04b-eee466d0c963/scratchpad/mpcap
for s in execution-core capability-core; do
  docker tag rollback/model-plane-$s-1:pre-mpcap2-20261029 model-plane-$s:mpcap-8e15216bf && bash "$P/recreate.sh" $s
done
```

To go further back, to the state before the first deploy, use section 6.

### Files

All new files are in `scratchpad/mpcap/`:

- **Scripts:** `build2.sh`, `recreate2.sh` and `capq.sh`.
- **Logs:** `build2.out`, `build2-*.log` and `r2-exec-test.log`.
- **Evidence:** `before2/` and `after2/` (envsig, sc1 status, capabilities, preflight).
- **Deleted:** the source export and the resolved config.
- **Kept:** the reused cargo target `%TEMP%/mpcap-review-fixes`. It existed before this step and was not created here.
