# A2a-credentials hand-over

Done: WS1.A2 part 1 as designed in A2a-credentials-design.md, in the design's corrected order (trait and call sites, then the Control Plane registry, then Quarry's audience + manifest row + pins in one commit, so the manifest never names an audience the loader does not know). See A2a-credentials-progress.md for commits, hunks, digests and test results.

Deviations from the design: none in substance. Additions: README section for the P2 lane (apps/Control Plane/config/README.md), a docs row in CROSS_PLANE_INTEGRATION.md, a fail-closed mint test (403 twice = two mints and two errors; 503 is an error), a wire test for the new row, and unit tests for the default body and the org reaching the validator on start, step, new tab and the sensitive approval.

Traps found:
- A concurrent unit's whole-file commit (8b3c108b, WS4.H) swept my digest edit in scripts/tests/zdr-contract-sync.test.sh into HEAD before my pins were committed. Shared files with several units' hunks: commit hunk-only (qcommit-patch.sh), or expect that.
- apps/Control Plane/scripts/validate-release-credentials-test.sh fails in this worktree on a missing .env.example (unrelated, untracked file).
- principal-registry-parity.test.sh takes ~15 min here; run it in the background.
- The recording driver in the agent harness reports a page on no allowed host, so a step ends 403 after the grant check; tests that assert the grant check must not assert the step's status.

Left for A2b: everything in the design's "Not doing" list (G1 token source and per-call mint, G6, main.rs wiring and boot gates, browser_grants_prod.rs, compose, overlay, workflow, L2), the lifecycle.rs:3-13 comment, and the optional L1 registry/manifest asserts in scripts/tests/browser-grant-contract.test.mjs.
