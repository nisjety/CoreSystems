# W2-U41 hand-over (WS17.E3: backfill, erasure hop, plaintext removal)

Read with: .plan-work/quarry-w1w2/W2-U41-progress.md (state and commits) and depth/W2-U41.md (deferred items).

## What the next agent needs to know

- Commits are local on quarryv2-next (not pushed). The runtime commit is 54958bf96, the control commit is afe212786. The edge commit and the docs commit are listed in the progress file.
- The shared tree carries other units' uncommitted hunks in quarry-edge routes.rs, main.rs, state.rs, lib.rs, change_routes.rs, test_support.rs and tests/support/mod.rs. These units also committed their own work at HEAD. The U41 commits were built from HEAD blobs plus only this unit's edits (scratch: W2-U41/lib-head.js, head-edit*.js, commit-blobs.sh). Re-running those scripts on a moved HEAD needs the anchors checked first; each script throws if an anchor is missing or not unique.
- Another unit (WS15.S2, U38) added a route `/v1/internal/schedules/preflight` and a module `schedule_preflight` in the same internal-router region of routes.rs. While its module was missing, the quarry-edge test target did not compile. Check that the module exists before the edge test compile.
- Another unit's `policy_mode` field gap in agent-run checkpoints (step_receipts.rs, postgres_step_receipts.rs, agent_routes/tests.rs) blocked the test targets of quarry-runtime and quarry-edge for a while. It was resolved before the runtime test check passed (RT_RC=0).
- Migration 0017 is in migrations/ and applies on the next edge boot. Read the header and the depth bullets before any release that includes it.
- The Go side builds and vets. The gdpr package is the only place that changed for the control consumer.
- The erasure route is `DELETE /v1/internal/orgs/:org/profiles` on the internal router. It is not in openapi (nor is the change record route).

## Commands for the test phase

- `cargo test -p quarry-runtime --features postgres-queue profile_backfill org_erasure` (REDIS_URL for the Redis-gated cases; DATABASE_URL for the Postgres lane, which has no new test in this unit).
- `cargo test -p quarry-edge --bin profiles_backfill` is not a test; the binary is checked only. Smoke: `profiles_backfill --max-rows 0` with the env set.
- `go test ./internal/gdpr/...` in services/quarry-control.
- `go vet ./...` in services/quarry-control (vet compiles the tests).

## Traps found

- `git diff` on the shared worktree shows the whole RUNBOOKS.md as changed: the working copy is CRLF for another unit's line-ending change. Edits through lib-edit.js keep CRLF; commits must use the HEAD-based version (lib-head.js).
- gofmt -l lists CRLF files in the gdpr package (erasure.go, subscriber.go, erasure_test.go) and cmd/control/main_test.go. That is line endings, not format. The new files are gofmt-clean.
- Bash heredocs in this tool fail for long bodies: write notes with the Write tool.
- The Write tool does not accept a `\0` escape in JS template literals; String.raw keeps the backslash. Backticks inside String.raw end the literal: use plain text in comments that go through JS scripts.
