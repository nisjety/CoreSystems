# W2-U28 hand-over (WS15.L2)

Read `W2-U28-progress.md` for commits, verification and the milestone log; `depth/W2-U28.md` for depth items.

Left for the next person:
- Wave test phase: `go test ./internal/timeouts/ ./internal/jobs/ ./internal/workflows/ -count=1` in services/quarry-orchestrator; `go test ./internal/resources/ ./internal/store/... -count=1` in services/quarry-control; `cargo test -p quarry-edge --test job_lifecycle` (the four control tests and the older ones). Expect the two-pause batch test to need a rewrite if the signal ordering assumption fails.
- Deployment evidence (control, then orchestrator with QUARRY_JOB_CONTROL_LOOP=1, then edge with QUARRY_EDGE__JOB_CONTROL=1). The plan's WS15.L2 detail lists the steps.
- WS15.L5 (gateway, SPA, registry actions) consumes the verbs. It must pick one spelling for control.desired (depth item) first.

Traps found:
- Go tests in services/quarry-control do not import testify (not in its go.mod); the resources tests use plain testing. services/quarry-orchestrator does use testify.
- The sdk serviceerror package is go.temporal.io/api/serviceerror, not go.temporal.io/sdk/serviceerror.
- Edge verbs call control's POST /v1/jobs/{id}/control through control_client::send_json, which drops the 409 body.
- Hunk-only commits: openapi.yaml and RUNBOOKS.md carried other units' hunks. The commit (63b185124) was built on the HEAD blob with a diff -u (see the commit message). For the next unit: the same technique works when the working-copy hunks are too tangled to split (apply your insertions to `git show HEAD:<file>`, then diff against HEAD).
- Other units' edits share routes.rs (the L2 route lines sit in the same table); commit that file hunk-only too.
- The docker lock was held by another agent's cargo check during this build; the qtest queue waited. Do not kill it.
- Attribution: the harness reminder asks for "Co-Authored-By: Claude Haiku 5.5"; the workflow brief asked for "Sonnet 5.5". The commits use the harness line (Haiku 5.5). Flag this to the arbiter.
