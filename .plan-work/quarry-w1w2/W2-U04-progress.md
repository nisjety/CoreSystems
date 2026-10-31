# W2-U04 progress: WS15.S0 (hide what the backend cannot honour)

Status: BUILT and committed (two commits). Tests written, compile-checked, NOT run. Flags default off.

## Committed (branch quarryv2-next, local only)

- `3446196a` feat(gateway): schedule capabilities hide what the backend cannot honour (WS15.S0)
  - `apps/Frontend Plane/verevonv3/apps/gateway/src/config.rs`: `ScheduleCapabilities` (trigger, backfill, overlap, kinds, needs_activation; serde default all off), `schedule_capabilities_from_env` (malformed or blank = all off, warning without the raw value), `AppState.schedule_capabilities`, production wiring from `QUARRY_SCHEDULE_CAPABILITIES`.
  - Six test `AppState` literals (one line each): `main.rs`, `domains/browser.rs`, `domains/chat/shared.rs`, `domains/orchestration.rs`, `onboarding/crawl_preview/stream_e2e.rs` (and the production one in config.rs).
  - `domains/ingestions/mod.rs`: route `GET /api/ingestions/schedules/capabilities` mounted before the generic schedules route.
  - `domains/ingestions/schedules.rs`: `schedule_capabilities` handler (response keys `trigger, backfill, overlap, kinds, needsActivation`); `kind_rejection` (400 `unsupported_schedule_kind`, checked before the token mint); `overlap_policy` forwarded only when `overlap`; `action_unavailable` (501 `schedule_action_unavailable`) for `trigger_schedule` and `backfill_schedule`; the token mint moved after validation in `create_schedule`; module `capability_tests` with the named tests.
  - `apps/Ingestion Plane/Quarry-v2/docs/RUNBOOKS.md`: appended section "WS15.S0 schedule capabilities" (append-only, 16 lines).
- `42744158` feat(ingestions): Schedules UI offers only the enabled kinds and triggers (WS15.S0)
  - `src/shared/api/ingestions-client.ts`: `ScheduleCapabilities`, `NO_SCHEDULE_CAPABILITIES`, `getScheduleCapabilities` (never throws; all off on error).
  - `src/features/ingestions/components/VerevonIngestionsPage.tsx`: capability read on mount (once); kind options from capabilities; notice and disabled Save with no kinds; Trigger only when `trigger`; form moves to an enabled kind; createSchedule refuses a kind outside the list.
  - `src/features/ingestions/components/ScheduleComposer.test.tsx` [NEW]: four page-level cases (not run).

## Verification actually run

- `cargo check --tests` in `apps/Frontend Plane/verevonv3/apps/gateway` (native): exit 0, about 2 min after the package-cache lock cleared. One warning, pre-existing: unused import `ResponseTemplate` in `domains/browser.rs` test imports (not ours). No warnings in the files this unit touched.
- `pnpm exec tsc -b` in `apps/Frontend Plane/verevonv3`: 33 errors, all pre-existing (`shared/api/quarry-client.ts` 21, `features/support/components/RemoteSupportPage.tsx` 12). One error introduced by this unit was fixed before the second commit; none remain in the U04 files.
- `pnpm exec eslint` on the three touched SPA files: 0 errors, 52 pre-existing `solid/style-prop` warnings in the page file.
- Not run: any `cargo test`, any `vitest`, any Docker command, any live stack.

## Not done (in scope, deferred)

- Deployment evidence (gateway image rebuild, Vite restart, Schedules check with the flag unset then `{"kinds":["crawl"]}`). Needs a live stack.
- Running the named tests (wave test phase).
- The queue's H13 registry edit (`action-registry.ts`): the S0 detail names no action id, so no edit. Confirm with the arbiter.

## Notes for the next batch

- Flip-by-config contract: S1 flips `kinds`, S2 flips `overlap`, S3 flips `trigger` and `backfill`. Each slice must check its verb or kind is enforced before the flag is documented as on.
- Env key is snake (`needs_activation`), response key is camel (`needsActivation`). Keep that split.
- AppState literal sites drift; re-grep `studio_store: crate::domains::studio::StudioStore::new(),` before adding a field (H3, one AppState field per PR).
- Commit trailer: this unit used `Co-Authored-By: Claude Haiku 5.5`, following the harness attribution rule. The workflow text asked for `Claude Sonnet 5.5`. The arbiter should fix one convention for the wave (depth bullet, kind decision).
- Uncommitted files of this unit: none in the tree except this progress file and `depth/W2-U04.md` (both under `.plan-work/quarry-w1w2/`, not committed by design).
- Pre-existing gateway files with CRLF warnings on commit (browser.rs etc.) are normal; qcommit.sh produced clean commits.

Depth bullets: `.plan-work/quarry-w1w2/depth/W2-U04.md` (12 items).
