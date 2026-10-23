# W2-U56 progress (WS13.8: contract docs, OpenAPI, SDK, parity corrections, e2e script; batch 11)

Status: PARTLY BUILT. The docs, the OpenAPI additions, the parity and register corrections and the opt-in e2e script with its make target are committed. Nothing was run (build-first rule, 2026-10-23). The SDK regeneration is NOT done: the spec does not parse yet (a duplicate key pair from WS15, see below).

## Committed (local branch quarryv2-next, not pushed)

| Commit | Scope |
|---|---|
| 16fdb679e | docs/openapi.yaml: ScrapeRequest formats (string and object forms, screenshot and pdf), actions, actions_on_error; ScrapeResponse screenshot, pdf, captures, scrape_id; effective_options note; /v1/scrape 501, 504, and the 429 and 502 detail. Built from the HEAD blob plus this unit's hunks only (qcommit-blobs.sh). |
| 4d501698e | docs/CONTRACTS.md section 10 (closed set, flags, request keys, field errors, response additions, changed refusals), 9.1 pointer; docs/RUNBOOKS.md section (flags, rollback, e2e target, evidence owed). Blob commit, HEAD plus this unit's sections only. |
| cb732988 | docs/OSS_PARITY_BACKLOG.md rows 22 and 23 corrected; docs/DRIVER_MATRIX.md note; docs/SCRAPE_CAPTURE.md status and limits section; docs/ERROR_REASONS.md section (reasons, status codes, what a caller does); apps/STALE_DOC_DELETION_REGISTER.md section for WS13.8. |
| 74dc1e6f | scripts/e2e-scrape-capture.sh (NEW, opt-in, bash -n only); apps/Ingestion Plane/Makefile `test-scrape-capture` target (tab checked) and help line. |
| e30c7119b | docs/openapi.yaml: two plain scalars quoted or reworded (WS10.S3 max_events line 1070; WS14.S2 crawl include and exclude description line 6534). Partial fix: the spec still does not parse (see Not done). |

## Working copies

- Working copies of docs/openapi.yaml, docs/CONTRACTS.md and docs/RUNBOOKS.md carry other units' uncommitted hunks (openapi: /v1/profiles and WS15 content; RUNBOOKS: 106 changed lines; CONTRACTS: the WS13.4b section is missing from the working copy, see depth). The same anchored edits were applied to the working copies, so each working copy is HEAD plus its own uncommitted hunks.
- The four clean docs and the Makefile were committed with qcommit.sh (their diffs held only this unit's hunks).

## Verified (what was actually run)

- `bash -n` on scripts/e2e-scrape-capture.sh: ok.
- Anchor matching: every edit's OLD text matched exactly once (the apply step stops otherwise).
- `git show HEAD:` of the committed spec compared with the built one: the diff is the two YAML lines only (e30c7119b).
- `openapi-generator-cli:v7.22.0 validate` (docker, under the docker lock, on a HEAD copy of the spec): fails at the duplicate key in CreateScheduleRequest (catchup_window_s, pause_on_failure), line numbers near 3436 to 3505 of the spec. Before the two YAML fixes it failed earlier (line 1070, then line 6534).
- `make` is not on this host; the target was checked by reading the recipe (tab present).

## Not run (by strategy)

- The parity test scripts/tests/openapi-scrape-parity.test.mjs (it throws at the formats check, see depth).
- The docs-contract gate (its file is not in this worktree).
- scripts/e2e-scrape-capture.sh against a live stack; `make test-scrape-capture`.
- SDK regeneration (sdks/python, sdks/typescript) and the TypeScript SDK build.
- The schema golden regeneration (not in the owned list).

## Not done (in scope, owed)

1. SDK regeneration: blocked by the spec parse failure (duplicate keys in CreateScheduleRequest, WS15 content, not changed here). Recipe: generate from a HEAD copy of docs/openapi.yaml (scratchpad W2-U56/sdkgen/gen.sh, image openapitools/openapi-generator-cli:v7.22.0), then commit sdks/, then `pnpm run build` for the TypeScript SDK.
2. docs/SELF_HOST.md env rows (plan [M]): the file carries another unit's uncommitted changes; left alone.
3. Per-action outcomes on success (data.actions[]) and action output bytes: not documented, because the wire part of WS13.5 is not built.
4. Gateway: the actions passthrough and the 125 s budget are not built (not in U56's owned list).
5. ERROR_REASONS.md header reconciliation (the new section is there).

## For the next batch

- The spec does not parse until the WS15 duplicate pair in CreateScheduleRequest is merged. Anyone editing openapi.yaml must run the validate recipe before committing.
- Use qcommit-blobs.sh for openapi.yaml, CONTRACTS.md and RUNBOOKS.md while those working copies carry other units' hunks (the path argument needs its quotes).
- The Co-Authored-By line: the attribution reminder in this session names Haiku 5.5, while the workflow brief asked for Sonnet 5.5; the commits carry Haiku 5.5 (the session's attribution rule). Raise it if the owner wants the other line.

Depth bullets: `.plan-work/quarry-w1w2/depth/W2-U56.md` (19 items). Hand-over: `.plan-work/quarry-w1w2/wf-W2-U56.md`.
