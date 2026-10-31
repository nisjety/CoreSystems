# wf-W2-sweep progress

Started 2026-10-24 04:05. Findings so far:
- Uncommitted tracked work = two units: WS7.F rust half (U14: quarry-core policy.rs, tests/contracts.rs) and WS17.P0 (U30: profile_guard/profile_routes/auth/main hunk/routes hunks/tests/openapi/gateway profiles.rs; its RUNBOOKS section was never in the tree, taken from scratchpad U30/runbook_section.md).
- Stale working copies (behind HEAD, lost committed content): routes.rs (WS13.4b 501 block), CONTRACTS.md (WS13.4b section), RUNBOOKS.md (WS8.D/F/H sections), openapi.yaml (WS8.F attach routes). Working copies synced to HEAD + the U30 hunks (nothing unique lost: checked by line-set diff).
- Strays, not committed: step_receipts.rs (field order in a test), lib.rs and main.rs (mod crawl_request/batch_check order), tests/support/agent_harness.rs and fixture_site.rs (status M, no content diff), rb.patch (0 bytes), W2-U27-progress.tmp, .plan-work/next-slices.md.
Next: cargo check --workspace --all-targets (qtest), then commits per unit.

## Done (2026-10-24 04:20)
Commits: 9a0f11ffb (WS7.F rust, U14), 911d69b95 (WS17.P0 code and tests, U30), 88941d04e (gateway profile list), b068c33bc (OpenAPI profile paths), e71fc4ca5 (RUNBOOKS WS17.P0 section), then the notes commit.
Checks: cargo check --workspace --all-targets --keep-going (qtest): one error, lab/evals browser_benchmark.rs missing tab_pin (WS8.E, not ours); cargo check -p quarry-edge --tests --features postgres-queue: exit 0; gateway cargo check --tests: exit 0.
Left: strays listed in depth/W2-sweep.md.
