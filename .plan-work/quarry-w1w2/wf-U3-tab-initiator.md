# U3-tab-initiator hand-over (2026-10-19)

User request 3: make it possible to tell whether an agent opened a tab, and which agent.

## What was built
- Both spellings (`initiated_by` / `initiatedBy`) on: tab open body (was the bug: snake_case ignored), step body, run start body, socket action (next to `actor`), tab select/close query. Both in one body = 422.
- `AgentIdentity {basis: service_principal|user_session, principalId (verified: the token's signed service id), aiRunId + label (advisory, identifier-shaped, dropped otherwise)}` in quarry-runtime step_receipts.rs.
- `derive_initiator_ref(claims, hint, advisory)` in agent_routes/run_live.rs; class rule unchanged (service = always agent; person = person unless hint agent; hint only tightens). Table test of the 6 principal x hint combinations.
- Recorded on: tab timeline event (JSONB), step receipt (new column `initiated_agent`, migration 0011), action timeline item, owner projection tab `openedBy` (in-memory map on RunEntry, bounded 256, pruned, dropped on close).
- openapi: tab routes documented (GET/POST /tabs, POST .../select, DELETE .../{tab_id}) + AgentIdentity, TabOpener, AdvisoryIdentifier...; 3 allowlist entries removed; CONTRACTS.md paragraph.
- Gateway: headers `X-Agent-Run-Id`, `X-Agent-Label` -> advisory `aiRunId`/`agentLabel` on an agent's tab open/select/close and step. Commit 4e333f75.
- Contract for the Model Plane owner: wf-U3-tab-initiator-contract.md. Finding: execution-core / model-gateway never call the tab routes.

## State when I stopped
See the final report in the workflow result; the progress file lists exact commands. Docker Desktop stopped answering at about 17:40 (docker version hangs; the pre-commit hook runs gitleaks through docker and refuses the commit when docker returns 500).

## Traps found
- The pre-commit hook fails CLOSED when docker is down (500 from the engine): "gitleaks found a secret" is the wrong message for it. Do not use --no-verify; wait for docker.
- Another unit (WS6.E grant budget claim) has uncommitted hunks in lifecycle.rs, tests.rs and step_receipts.rs, which are also my files. scratchpad/U3-tab-initiator/filter-foreign.js builds copies without them and commit-partial.sh commits HEAD + my hunks through a private GIT_INDEX_FILE under the git lock (then fixes the shared index entries). Re-run both once docker is back.
- qtest queue: another agent's chromium loop held the docker lock 16:28-17:10; my edge test then compiled for 40+ minutes.
- Host (Windows) cannot build Quarry: no libclang (boring-sys2), and link.exe 1104 on a long scratch path.
- The raw receipt route is snake_case (`initiated_by`) while the nested `agent` object is camelCase; the timeline items are camelCase throughout.
- Axum Json rejection (duplicate field, bad enum) is a plain-text 422, not an envelope; Query rejection is a 400.

## Round 4 hand-over (2026-10-19)
Nothing new is committed: the Docker engine did not answer for ~30 min (watcher log scratchpad/U3-tab-initiator/dw.log), so no qtest and no commit (the pre-commit hook needs docker; never --no-verify). HEAD still holds only 4e333f75 for U3.
To finish, when `docker version` answers:
 1. node scratchpad/U3-tab-initiator/filter-foreign.js (regenerates the filtered lifecycle.rs / step_receipts.rs / tests.rs without WS6.E's hunks) and filter-openapi.js (same for openapi.yaml).
 2. Compile + run the exact blobs if possible (temp `git worktree add` of HEAD with the 3 filtered files + the FULL list from commit-partial.sh copied in), via qtest: cargo check --workspace --all-targets; cargo test -p quarry-edge --test agent_run_control; cargo test -p quarry-edge; cargo test -p quarry-core.
 3. Postgres: cargo check -p quarry-runtime --features postgres-queue --all-targets, then DATABASE_URL=... cargo test -p quarry-runtime --features postgres-queue --lib postgres_step_receipts (tools/pg-test.sh builds a throwaway PG but uses plain rust:1 and no lock: adapt to quarry-rust-env:1 + with-lock.sh).
 4. bash tools/with-lock.sh git 900 -- bash scratchpad/U3-tab-initiator/commit-partial.sh scratchpad/U3-tab-initiator/msg-code.txt  (code) then ... commit-docs.sh scratchpad/U3-tab-initiator/msg-docs.txt (docs). Update both messages with the real counts first (they currently say what was NOT run).
Trap: rustfmt rewrites CRLF files to LF; re-add CRs (sed -i 's/$/\r/') and check `git diff --stat` shows only insertions.
Trap: no CI job runs quarry-runtime's postgres-queue feature with a database; the new Postgres round trip never executes in CI (needs a job with a postgres service + DATABASE_URL; .github/workflows is outside this unit's paths).
