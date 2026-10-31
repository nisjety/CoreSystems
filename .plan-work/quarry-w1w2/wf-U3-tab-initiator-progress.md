# U3-tab-initiator progress

## Findings (read phase)
- execution-core / model-gateway never call the tab routes (only POST /v1/agent/runs, /step, DELETE). Tab calls come from the gateway action surface (browser_tab.* in gateway actions/dispatchers.rs) with the PERSON's token and an actor from the action input.
- Gateway never knew an AI run id (BrowserRunStore is cfg(test)); step forwards only `initiated_by: agent`.
- Timeline events are JSONB (no migration needed); action receipts are columns (migration 0011 needed for agent identity).

## Design
- AgentIdentity {basis: service_principal|user_session, principalId (verified, service only), aiRunId (advisory), label (advisory)} lives in quarry-runtime step_receipts.rs next to BrowserTimelineInitiator.
- Edge: derive_initiator_ref(claims, hint, advisory) -> Initiator{class, agent}.

## Status
- (nothing committed yet)

## Milestone 1 (uncommitted, compiles: cargo check -p quarry-runtime -p quarry-edge --all-targets rc 0)
Mine (uncommitted): quarry-runtime/src/{step_receipts,postgres_step_receipts,browser_procedure}.rs, migrations/0011_step_receipt_initiator_agent.sql, quarry-edge/src/agent_routes/{mod,run_live,admission,step,types,projection,tabs,lifecycle,tests}.rs.
Edit scripts in scratchpad/U3-tab-initiator/. quarry-runtime/tests/browser_egress_boundary.rs is NOT mine (someone else's diff).
Next: edge integration tests (both spellings per body, record agent, 6-combo table, SP identity), commit 1 (spellings), commit 2 (record), openapi+CONTRACTS, gateway.

## Milestone 2 (still uncommitted)
Added: edge integration tests (agent_run_control.rs tail + harness model_gateway_service_token), unit tests (agent_routes/tests.rs), openapi tab routes + allowlist entries removed (node --test scripts/tests/*.test.mjs: 23 pass), CONTRACTS.md paragraph, gateway browser.rs AgentAdvisory headers (x-agent-run-id, x-agent-label) + tests, contract note wf-U3-tab-initiator-contract.md.
Results so far: cargo check -p quarry-runtime -p quarry-edge --all-targets rc 0; agent_routes lib tests 84 pass; gateway `cargo test domains::browser` 103 pass (before the 2nd gateway test).
PENDING: qtest it1 (runtime step_receipts + edge agent_run_control) queued behind another agent's chromium loop (docker lock held since 16:28); log scratchpad/U3-tab-initiator/it1.log.
browser.rs ALSO carries ANOTHER unit's uncommitted hunks (browser_site_blocked_response + its 3 tests): commit only my hunks (git apply --cached) or say so.
Commits plan: (1) spellings+recording in runtime/edge, (2) openapi+CONTRACTS+allowlist, (3) gateway.

## Milestone 3 (18:20)
COMMITTED: 4e333f75 gateway (feat(gateway) advisory identity headers).
NOT COMMITTED (docker down -> pre-commit hook blocks; never --no-verify):
 - Quarry code commit: bash with-lock.sh git 900 -- bash scratchpad/U3-tab-initiator/commit-partial.sh scratchpad/U3-tab-initiator/msg-code.txt  (private index; excludes WS6.E hunks of another unit in lifecycle.rs/tests.rs/step_receipts.rs; first re-run node scratchpad/U3-tab-initiator/filter-foreign.js)
 - Docs commit: qcommit.sh msg-docs.txt docs/openapi.yaml docs/CONTRACTS.md scripts/openapi-route-allowlist.json (all hunks mine)
 Do the code commit FIRST (the docs describe it).
NOT RUN (docker): tests/agent_run_control.rs new tests (9), cargo check --workspace, log lint, quarry-core tests, Postgres round trip, cargo check after adding the integration tests.
Run later: qtest 'cargo test -p quarry-edge --test agent_run_control; cargo test -p quarry-edge (all); cargo test -p quarry-core; cargo check --workspace --all-targets' and DATABASE_URL-gated: cargo test -p quarry-runtime --lib postgres_step_receipts.
Background watcher bxr51o022 polls docker for 24 min.

## Round 4 (review fixes) - 2026-10-19
Docker engine NOT answering (Docker Desktop processes up, `docker version` times out, rc 124) for the whole round; qtest/commit hook unusable. Host cannot build quarry-edge (quarry-tls -> wreq -> boring-sys2 needs libclang + nasm).
DONE (uncommitted, in the tree):
 - agent_run_control.rs (+~230 lines, rustfmt-clean, CRLF kept): both-spellings-in-one-request refused on run start (422), step (422), select/close query (4xx); skipped receipt agent asserted for a person (user_session, one per take-over) and a service (service_principal + verified id; forged `agent` body key ignored); forged `agent` object on a service tab open; ZDR tab-call test (nothing persisted, projection still names the opener).
 - agent_routes/tests.rs: both-keys is_err for TabCallQuery, StartRunBody, StepBody, BrowserWsClientMessage (+ actor/initiatedBy mix).
 - gateway browser.rs: run_action test driving the real header read (agent forwards aiRunId/agentLabel; person's step none). `cargo test --bin verevon-gateway-rs domains::browser`: 105 passed, 0 failed (host, native).
 - openapi: filtered copy without the WS6.E hunk (filtered/openapi.yaml, filter-openapi.js); commit-docs.sh commits it via a private index. yaml.v3 (go, GOFLAGS=-mod=mod GOPROXY=off GOSUMDB=off, yamlchk/) parses working + filtered file, every #/ $ref resolves. node --test scripts/tests/*.test.mjs: 23 pass.
 - filter-foreign.js re-run (filtered lifecycle.rs, step_receipts.rs, tests.rs regenerated after the new unit tests).
NEXT when docker is back: qtest 'cargo check --workspace --all-targets', 'cargo test -p quarry-edge --test agent_run_control', 'cargo test -p quarry-edge', 'cargo test -p quarry-core'; log lint is part of quarry-edge tests; Postgres: needs `--features postgres-queue` (the reviewers are right: without the feature the module is not compiled) + DATABASE_URL (tools/pg-test.sh?). Then commit-partial.sh (code), commit-docs.sh (docs).

## Round 4b (review of the commit state) - 2026-10-19
Docker engine still hung: `docker ps` times out, qtest prints DOCKER IS NOT ANSWERING, the pre-commit hook's docker run gets a 500 from the engine and the hook exits 1 ("gitleaks found a secret" is the hook's message for any non-clean rc, NOT a real finding). Host cannot build quarry-edge/quarry-runtime (proc-macro2 link LNK1104, boring-sys2 needs libclang). Nothing committed this round; never --no-verify.
Findings handled:
 1. Gateway test: browser.rs now carries ONLY my test hunk vs HEAD (git diff: 69 insertions, one hunk, the other unit's browser_site_blocked_response was committed). Message ready: scratchpad/U3-tab-initiator/msg-gw-test.txt. Commit: bash tools/qcommit.sh scratchpad/U3-tab-initiator/msg-gw-test.txt "apps/Frontend Plane/verevonv3/apps/gateway/src/domains/browser.rs". Attempt with a private index failed at the hook (docker 500).
 2. Docs: use commit-docs.sh (filtered openapi), code: commit-partial.sh (filtered lifecycle/step_receipts/tests). Never qcommit those paths directly.
 3. Do NOT run commit-partial.sh until the exact blobs compile. Plan when docker is back: (a) git archive HEAD of "apps/Ingestion Plane/Quarry-v2" into scratchpad/U3-tab-initiator/verify-tree, overlay the FULL files + filtered/{lifecycle,step_receipts,tests}.rs + migration 0011 (same set as commit-partial.sh), (b) QROOT=<verify-tree> bash tools/qtest.sh 'cargo check --workspace --all-targets' then 'cargo test -p quarry-edge --test agent_run_control' and 'cargo test -p quarry-edge' and 'cargo test -p quarry-core', plus pg-test.sh with --features postgres-queue for postgres_step_receipts, (c) only if green run commit-partial.sh, then commit-docs.sh, then the gateway commit.
