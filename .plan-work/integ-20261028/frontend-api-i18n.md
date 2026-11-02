# Group frontend-api-i18n (verevonv3 omnibox, http, sse, errors, vite-env)

All five files resolved, no markers, `git add` done. Line endings: all LF, kept.
Base for the paths below: apps/Frontend Plane/verevonv3.

## src/features/dashboard/home/browser-omnibox.ts
- Branch: split into `classifyOmniboxInput` (empty/url/query) and `browserOmniboxTarget`; free text is never turned into a Google URL (returns null, the resolver decides).
- Live: free text became a Google URL, except in the demo build (`VITE_DEMO=1`) where it returned null.
- Merge: kept the branch version as it is. The live guard has nothing left to guard, because the branch never builds a third-party search URL in any build. The live behaviour (no search engine in the demo) therefore holds. The file is now identical to HEAD.

## src/shared/api/sse.ts
- Branch: error path calls `apiErrorFromResponse(errorBody, response)`.
- Live: error path read `feature` from `extractError` and passed it to `new ApiError(..., feature)`.
- Merge: kept the branch call. `feature` now travels through the hints inside `apiErrorFromResponse`, so the live behaviour is kept. File identical to HEAD.

## src/shared/api/http.ts
- Branch: `ApiError(message, status, code, hints)` with details, retryAfterSeconds, resumable, blockReason; `extractError` returns `{ code, message, hints }`; `apiErrorFromResponse` and `signalSessionExpiry` shared by requestJson/requestForm/SSE.
- Live: `ApiError.feature` (4th positional arg) and `extractError` returning a top-level `feature` for the zero-retention refusal.
- Merge: kept the branch structure. Added `feature` as a hint: `ApiErrorHints.feature`, `ApiError.feature` (null when absent, set from `hints.feature`), and `extractError` fills `hints.feature` from `error.feature`. requestJson / requestForm / SSE keep the branch code (they all go through `apiErrorFromResponse`, so feature reaches the thrown error on every path). No caller in the tree used the 4-arg positional form.
- Changed shape: `extractError` no longer returns a top-level `feature`; it is `result.hints.feature`.

## src/shared/i18n/errors.ts
- Branch: large `ERROR_COPY` table, `describe()` with status-class kinds, `describeApiError`, `LocalizedError`; its own header paragraph.
- Live: the zero-retention catalogue (`shared/api/zdr-refusals.ts`) renders `zdr_*` catalogue codes in `translateApiError`, with the feature from the error.
- Merge: both header paragraphs kept; the zdr-refusals import kept. The catalogue check moved into `describe()` (so `translateApiError` and `describeApiError` both give the catalogue text, and the kind/retry fields stay filled). A `LocalizedError` still wins over it. `ApiErrorLike` got `feature`. The branch's own `zdr_*` entries in `ERROR_COPY` (`zdr_no_attested_provider`, `zdr_gate_unavailable`, `zdr_persistent_profile_forbidden`, `zdr_ai_proposal_forbidden`, `zdr_draft_persistence_forbidden`, `zdr_recurrence_forbidden`) do not overlap with the 10 catalogue codes, so nothing is lost on either side.

## src/vite-env.d.ts
- Branch added `VITE_FLEET_CONSOLE`, `VITE_BROWSER_OMNIBOX_SEARCH`; live added `VITE_SCENE_MODE`. Kept all three.
- Note: `VITE_DEMO` is used by the code (dashboard-search-utils, vite.demo.config.ts) but is declared in neither side; not added, it was untyped before.

## Test files adjusted (next to these modules, needed by the merge)
- `src/shared/api/http.zdr-refusal.test.ts` (live): `extractError` results now `toMatchObject` with `hints.feature`, since the branch shape has a `hints` key.
- `src/shared/i18n/errors.zdr-refusal.test.ts` (live): the "unknown zero-retention code uses the caller fallback" case used `zdr_recurrence_forbidden`, which the branch now has copy for; switched to `zdr_a_future_code`.
- `src/shared/i18n/error-catch-sites.test.ts` (ratchet, branch): live code adds raw-message reads, so I listed them with reasons: `demo/ScenePage.tsx` 2, `demo/install.ts` 1 (scene runtime, not in a normal build), `features/agents/components/AgentRunConsole.tsx` 1 (non-ZDR failures keep the stream text), and `shared/api/chat-client.ts` 5 -> 6. These are `git add`ed.

## Defaults
No compose or env default involved in this group.

## Checks
- `npx vitest run --maxWorkers=2 src/shared/api src/shared/i18n src/features/dashboard/home/browser-omnibox.test.ts`: 52 files pass, 1 file fails to load, 0 failing tests after the ratchet fix. The load failure is `src/shared/api/quarry-client.test.ts`: `@quarry/client` points to `Ingestion Plane/Quarry-v2/sdks/typescript`, which has only `docs/` in the tree (no package.json, no built SDK). Not caused by this merge; it needs the generated SDK from another group or the build.
- `npx tsc -b --pretty false | grep -c "error TS"`: 33 (same as the quarryv2-next baseline). No error in http.ts, sse.ts, errors.ts, vite-env.d.ts, browser-omnibox.ts or the zdr test files.
- I ran `pnpm install --frozen-lockfile --ignore-scripts` in the worktree's verevonv3 (no node_modules existed).

## Unsure
- Whether the chat-client count of 6 hides a genuinely new person-facing raw read from live code (line ~1785 `message: err.message`); I listed it under the existing reason.
