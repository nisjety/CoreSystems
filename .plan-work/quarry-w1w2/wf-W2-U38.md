# W2-U38 hand-over (WS15.S2)

What was done, what is left and the traps found. The progress table is in W2-U38-progress.md; the deferred items are in depth/W2-U38.md.

## Traps found while building

- `workflow.WithTimeout` does not exist in SDK v1.29.1. Use `NewDisconnectedContext` with a `NewSelector` and a `NewTimer` for a bounded wait.
- SDK v1.29.1 cannot update a schedule's memo. Change detection therefore compares `Describe` output with the desired spec. A memo hash goes stale after the first change.
- `client.ScheduleSpec.CatchupWindow` defaults to a long server default. Leave it unset when the spec has no value, or an existing schedule gets an Update on every pass.
- The orchestrator's `Describe` fake in existing tests returns an empty description. Treat a nil `State` as "act", or the existing tests fail.
- `git show` of a CRLF worktree file gives LF blobs. Build hunk-only patches from the HEAD blob (`git show HEAD:<path>`), and check with `git apply --check --cached` before `qcommit-patch.sh`.
- The shell tool blocks `sleep` followed by other commands. Use `qwait.sh` with an end marker. A `cat > /dev/null` with no input hangs a Bash call.
- The edge `cargo check` waits behind the docker lock for as long as other agents' containers run. Do not retry in a loop.

## Left to do

- Record the edge `cargo check --tests -p quarry-edge` result in W2-U38-progress.md, then commit the edge hunks and the new edge files (pending commit row in the progress table).
- Write the named tests listed as not written in the depth file.
- Standalone compose (`Quarry-v2/deploy/compose/docker-compose.yml`): add the control env line with default 0.
- Stack evidence (plan WS15.S2 steps 1-7) in the stack phase.

## Files

- Orchestrator: internal/workflows/{inputs,scheduled,inputs_test,scheduled_test}.go; internal/activities/scheduled.go; internal/schedules/{schedules,spec,spec_ops,spec_test,schedules_test}.go; internal/jobs/{dispatcher,input_builders_test}.go; cmd/orchestrator/main.go.
- Control: internal/resources/{schedule_runs,schedule_runs_test,schedule_wire,schedules}.go.
- Edge: crates/quarry-edge/src/schedule_preflight.rs; tests/schedule_preflight.rs; src/lib.rs and src/routes.rs (one line each, hunk-only).
- Docs: docs/RUNBOOKS.md (one section); apps/Ingestion Plane/docker-compose.yml (one env line).
