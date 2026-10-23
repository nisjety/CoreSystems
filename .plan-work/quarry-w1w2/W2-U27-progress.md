# W2-U27 progress: WS9.B reload and navigation error card

Date: 2026-10-23. Unit: W2-U27 (batch 5). Slice: WS9.B (plan D9 S2). Built to scope, NOT run (wave build phase).

## Committed (branch quarryv2-next, local only)

| Commit | What |
|---|---|
| 54dae42bc | feat(quarry-v2): reload action on the browser driver. contracts (AgentAction::Reload, appended), browser Action and BrowserDriver::reload (default unsupported), chromiumoxide Page.reload with ignoreCache, runtime mapping and dry run, outcome class, challenge gate, cost table, budget label, web policy class, openapi ReloadAction. Tests written. |
| f330a5f18 | docs: RUNBOOKS section for WS9.B. **Mistake:** the blob was built from a RUNBOOKS snapshot taken before other units committed their sections, so this commit dropped the WS9.C, WS10.S3 and WS15.L2 sections (40 lines). |
| e8103cef8 | docs: restore the dropped RUNBOOKS sections. Blob = parent 54dae42bc plus the WS9.B section. Net effect against 54dae42bc is +12 lines. History keeps the bad intermediate (no amend or reset allowed). |
| fff5b7c79 | feat(gateway): reload through the browser action route. sanitize_action (exact {type, ignore_cache}), agent refusal (403 browser_action_denied), capability strings reload and hard_reload in both projections, tests written. |
| b57e3edb6 | feat(verevon-v3): navigation error model and card (new files: browser-navigation-error.ts, BrowserNavigationError.tsx and .css, the model test). |
| f8979d0da | feat(verevon-v3): wiring into KnowledgeComposer, KnowledgeScrapePreview, browser-client and BrowserChrome (wiring hunks only). |
| ce573805c | test(verevon-v3): BrowserChrome.reload.test.tsx (written, not run). |

Plus this progress note and the depth note (commit below).

## Compile checks run

- `cargo check --tests -p quarry-core -p quarry-browser --features chromiumoxide -p quarry-runtime -p quarry-edge` through qtest, on the working tree (which also held other units' uncommitted work): finished, 0 errors. Warnings are pre-existing dead code in quarry-edge.
- Native `cargo check --tests` in apps/Frontend Plane/verevonv3/apps/gateway: finished, 0 errors (one pre-existing unused-import warning in another test).
- `tsc --noEmit` over apps/Frontend Plane/verevonv3 (tsconfig.json): exit 0, no output.
- Not run (test phase): every written test. Specifically: `cargo test -p quarry-core agent_action_reload_roundtrips`, `cargo test -p quarry-runtime reload`, `cargo test -p quarry-edge action_label_reload`, `cargo test -p quarry-browser --features chromiumoxide -- --ignored reload_ignores_cache` (CHROMIUMOXIDE_TEST=1), gateway `sanitize_action_allows_reload_without_extra_keys`, `sanitize_action_rejects_reload_with_url`, `reload_is_refused_for_agent_actor_only`, vitest `browser-navigation-error.test.ts` and `BrowserChrome.reload.test.tsx`.

## Not done (in scope, deferred, see the depth note)

- Playwright extension of browser-workspace-omnibox.spec.ts (test phase).
- Deployment evidence (rebuild quarry-edge, gateway, frontend; request body and 403 checks).
- Hard reload is a toolbar button, not the menu item of the plan.
- Capability strings are static in the gateway; the plan wants them listed only after the Quarry edge is deployed (sequencing, or a config gate).
- The agent refusal reads the actor from the request body (trust issue, pre-existing model).

## Files that carry other units' uncommitted work (not committed by U27)

- Quarry RUNBOOKS.md working copy: other units' sections and CRLF changes, not committed by U27 (U27 only committed its own section through HEAD).
- openapi.yaml, gateway browser.rs, browser-client.ts, BrowserChrome.tsx: other hunks were left out by hunk selection. Re-check their status before the next batch.

## For the next batch

- Do not use qcommit-blobs.sh on a file built from a snapshot taken earlier in the same session: rebuild the blob from `git show HEAD:<path>` in the same command as the commit, and check that HEAD did not move.
- Reload is a new enum variant. Any new exhaustive `match` on AgentAction or the browser Action needs the arm (the sites are in the hand-over note).
