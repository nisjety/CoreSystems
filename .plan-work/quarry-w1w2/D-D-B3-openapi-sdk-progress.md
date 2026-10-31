# D-B3-openapi-sdk progress (2026-10-27)

Unit: D-B3-openapi-sdk (W2-finish). Branch `quarryv2-next`, local commits only. Scratch: `scratchpad/D-B3-openapi-sdk/`.
Findings not fixed, with the owner's next step: `depth/D-D-B3-openapi-sdk.md`. Hand-over: `wf-D-B3-openapi-sdk.md`.

## Committed (this unit)

| Commit | Item | What | Check |
|---|---|---|---|
| 3dff390e6 | D2-026 | openapi.yaml parses: duplicate `catchup_window_s` / `pause_on_failure` merged; `preset` description on one line | yaml.v3 `ok` (was two duplicate-key errors) |
| a5baa7563 | D2-003 route coverage | allowlist: 3 internal routes added, 6 stale entries removed, pin 86 to 108 with a note | route coverage 10/10 (was 3 failing) |
| 7bf9d40fe | D2-003 / D2-289 scrape parity | test reads the anyOf ScrapeFormat golden; snake_case both sides; capture enum checked | parity 5/5 (was 1 failing) |
| ea345ab94 | D2-027 gate | docs-contract.test.mjs restored from the live tree + whole-file YAML subset check (scripts/yaml-subset.mjs); 12 assertions pending as todo | docs-contract 19: 7 pass, 0 fail, 12 todo |
| 9b22e00ac | WS16.S0 (part) | CROSS_PLANE_AUTH model-plane row = not granted; data-plane row names the 4 granted scopes; SELF_HOST model section corrected | the two CROSS_PLANE_AUTH row tests pass |
| 04cd5b1d9 | D2-037 | GAP rows 177/201 say what is wired | docs only |
| 10e7a406c | D1-024 | challenge.rs: whole-word title cues (`pardon` replaced by `pardon our interruption`); only attempts count toward the third strike (`observe_after`, `live_strong` spends the recovery); status bound to the request that started the navigation; 4 table rows, 4 tests | quarry-runtime lib 1009 pass, 0 fail |
| 85967a31d | D1-024 | observation.rs: one call site (`observe_after`); hunk-only commit, the file's other units' work is not in it | same run |
| aee4c2992 | WS16.S0 (part) | json_schema.rs header names Quarry as its only user | cargo check quarry-edge + quarry-core clean |
| 50c567dd7 | WS13.5 hand-over | openapi: ScrapeResponse.actions[] and ScrapeActionOutcome (from D-B3-scrape-actions' depth file, item 1) | yaml.v3 ok; node suite unchanged |

## Checks at the end

- `node --test scripts/tests/*.test.mjs` (Quarry-v2 root): 42 tests, 30 pass, 0 fail, 12 todo (pending: option B x11 and QB-T17 x1, see depth item 1-2). Exit 0. Baseline was 23 tests, 19 pass, 4 fail.
- `cargo test -p quarry-runtime --lib`: 1009 passed, 0 failed (includes the challenge tests).
- `cargo check -p quarry-edge -p quarry-core`: clean (after the observation.rs and challenge.rs changes).
- `cargo check --workspace --all-targets`: rc=0, 41 warnings, no errors.
- `cargo test -p quarry-core --features jsonschema --test scrape_schema`: 2 passed (the scrape golden matches the Rust type).
- TypeScript 5.9.3 on the current (pre-regeneration) SDK: rc=0.

## Not done (details in the depth file)

- SDK regeneration (WS13.8 / U56): blocked. Python generation aborts and the TypeScript output does not compile, both on the spec's one bare OpenAPI 3.1 `type: 'null'` (ProfileCookieView.value, added by WS17.P0; the earlier "13 places" was wrong). RESOLVED 2026-10-28 by D2-A-openapi-null. Nothing under `sdks/` committed; the tracked docs were restored to HEAD; the gitignored `sdks/typescript/src/` was overwritten and does not compile.
- docs-contract: 12 assertions pending (option B lives only in the live tree; QB-T17 compose defaults).
- WS16.S0: model_plane_gate.rs is not on this branch; ledger entry and gateway.proto comment are the Model Plane session's.
- D1-024: RespondDialog and MouseWheel stay allowed (documented policy); redirect into an exhausted host not checked.

## Traps

- The docker lock is shared; generator and qtest runs queue behind other agents for minutes.
- `sdks/generate.sh` must run with `MSYS_NO_PATHCONV=1`, or the spec path becomes `C:/Program Files/Git/local/...`.
- `sdks/typescript/src/` is gitignored; the TypeScript build check needs TypeScript 5.x (the SDK declares ^4 || ^5); TS 6.0.x rejects its tsconfig.
- observation.rs carried other units' uncommitted hunks while I worked; committed only my hunk with qcommit-patch.sh.
