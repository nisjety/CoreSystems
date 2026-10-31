# wf test-gateway-spa-auth progress

Unit: test-gateway-spa-auth, wave test phase (run once, report, no fixes, no retries).
Worktree: C:/dev/CoresSystem/.claude/worktrees/vigorous-perlman-e7ae5b (branch quarryv2-next). Nothing is committed by this unit.
C: free at start: 457.82 GB (stop threshold 8 GB).

Started in background. Logs are in C:/Users/IMAFER~1/AppData/Local/Temp/claude/C--dev-CoresSystem--claude-worktrees-vigorous-perlman-e7ae5b/562f6350-4d29-46dc-a04b-eee466d0c963/scratchpad/WT/ and each ends with a WT_DONE line:
- gateway-cargo.log: cargo test --no-fail-fast in apps/Frontend Plane/verevonv3/apps/gateway (default target dir, already present).
- vitest.log: vitest run on knowledge-preview.test.ts, zdr-refusals.test.ts, src/shared/i18n and src/shared/actions (verevonv3).
- jest-authcore.log: npx jest on plane-service-principals-policy.contract.spec.ts, plane-principal-registry-assembly.contract.spec.ts and plane-token-scopes.spec.ts (apps/Control Plane/auth-core).
- tsc.log, then eslint.log: pnpm exec tsc -b, then pnpm lint (eslint .) in verevonv3.

Results (all five jobs finished, no timeouts, nothing retried):
- gateway cargo test (native, default target): rc=0, 778 passed, 0 failed, 1 ignored.
- jest, the three auth-core specs: rc=0, 3 suites, 101 tests passed.
- pnpm lint (eslint .): rc=0, 191 problems (0 errors, 191 warnings), equal to the baseline.
- vitest on the four changed test files, src/shared/i18n and src/shared/actions: rc=1, 7 files and 167 tests passed, 0 test failures. Six unhandled "Timeout waiting for worker to respond" errors, so six files never started: browser-errors, home-tab, preset-agents, error-catch-sites, errors, action-execution-summary.
- pnpm exec tsc -b: rc=1, 33 errors (RemoteSupportPage.tsx 12, quarry-client.ts 21), equal to the baseline. The log shows TS2307 for @verevon/remote-core and @quarry/client.
The depth ledger W1-tests.md was not written, because the rule against writing new findings .md files applies; the bullets are returned in the structured output. No commits were made.

## Continuation run (2026-10-24, started about 04:20 +0200)

The results above are from the earlier run (logs of 2026-10-23 17:12 to 17:15, HEAD then older). Since 17:12 on Oct 23, 33 commits have touched the gateway, verevonv3 and auth-core paths (for example 42a9a9e53 and the WS8.D, WS9.B, WS9.D, WS9.I and WS14.S3/S6 commits), so those numbers do not describe the current tree. This unit re-ran the five jobs once on the current tree; the earlier logs are kept unchanged in scratchpad/WT (not overwritten).

- Scripts: scratchpad/gsa2-gateway.sh, gsa2-vitest.sh, gsa2-jest.sh, gsa2-tsc.sh, gsa2-eslint.sh. Logs: scratchpad/WT/gsa2-*.log, each ending with a GSA2_*_DONE rc line.
- Vitest file list: the changed test files since 473d0a78 (32 files, a superset of the 27 that 3ee1bc9f4..HEAD gives), plus src/shared/i18n and src/shared/actions.
- Gateway: default target dir (already present), no C:/q4t created. No temporary target dir to delete.
- Run on HEAD 226a7f2ff, 04:20 to 04:26 (+0200). All five jobs ran once, in parallel, and none was retried. No worker timeouts this time.
  - gateway cargo test: rc=0, 838 passed, 0 failed, 1 ignored (5 binaries: bin unit tests 820, cache_admission_only 1, log_field_discipline 13, sse_relay_no_allowlist 3, ssrf_forward_not_fetch 1). The ignored test is onboarding::crawl_preview::stream_e2e::dump_crawl_preview_stream.
  - vitest (32 changed test files since 473d0a78, plus src/shared/i18n and src/shared/actions; 40 files in all): rc=1, 8 files failed, 16 tests failed, 423 passed. Failing: BrowserChrome.reload.test.tsx (7), OmniboxField.test.tsx (2), BrowserApprovalDecisionCard.test.tsx (2), omnibox-history.test.ts (1), ConsoleFilterBar.test.tsx (1), browser-errors.test.ts (1), gateway-error-codes.test.ts (1), error-catch-sites.test.ts (1).
  - jest on the three auth-core specs: rc=0, 3 suites, 101 tests passed.
  - pnpm exec tsc -b: rc=1, 38 errors against a baseline of 33. RemoteSupportPage.tsx (12) and quarry-client.ts (21) match the baseline. New: BrowserChrome.tsx (4, lines 661, 662, 663, 669) and crawl-site-scope.test.ts (1, line 33).
  - pnpm lint: rc=0, 201 problems (0 errors, 201 warnings) against a baseline of 191 warnings. The +10 is not attributed per file.
- Depth bullets are in the structured output only. Per the system rule against new findings .md files, they were not appended to depth/W1-tests.md. The hand-over file wf-test-gateway-spa-auth.md was not written for the same reason.
- Nothing is committed. No temporary target dir was created, so none needs deleting.
