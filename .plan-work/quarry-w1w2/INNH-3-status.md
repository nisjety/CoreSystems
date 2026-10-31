# INNH-3: Structured extract returns 501 on live: document the enablement path and make the 501 hint exact

Status: built in tests on branch innh/lane-a (commit below); not run against a real Model Plane gateway or a stack
Lane: A (scrape/extract). Plan home: config + docs. Estimate: 1 d.
Re-checked against the tree on 2026-10-31 (after INNH-1, INNH-2, INNH-7 and the item 1 fixes).

## What the tree had before this item
- INNH-1 added the real flag `QUARRY_EDGE__AI_FORMATS`, but it gates the AI formats of `POST /v1/scrape` only (`/v1/ai/chat` lane, 501 `model_plane_unavailable` / `model_lane_not_granted` with `details.reason`). It has never gated `/v1/extract`. The audit (commit 14299e659) and the 12-item brief say it does; that is wrong.
- INNH-2 made `/v1/extract` use `prompt` and changed the 501 hint to "set MODEL_PLANE_URL, or omit `schema` and `prompt`". That hint named the compose variable, not the edge setting, and said nothing about the credential: setting the URL on a deployed stack only moved the failure to a 200 with N per-source `error` items after N paid fetches (a `Forbidden` from `/v1/invoke`, because the `model-invoke` row is `granted: false`).
- The item 1 fix added a PRIVACY_POLICY skip to extract (per source, not a 501); unchanged here.
- RUNBOOKS had a "Scrape AI formats" section and a short "/v1/extract with a prompt" section, but no section on enabling structured extract, and nothing said plainly that there is no production path.

## What I built
Code (`crates/quarry-edge/src/extract_routes.rs`, `api_error.rs`):
1. Three exact 501 `UNSUPPORTED` bodies, all `{error, code, hint, next_actions}` (new `ApiError::with_next_actions`), no `details`, no URL/host/token echoed:
   - no URL (unset, empty or blank): `structured extraction needs the Model Plane, and QUARRY_EDGE__MODEL_PLANE_URL is not set on this deployment`; before any fetch.
   - URL but no credential (no service token provider, no development token): `structured extraction needs a granted Model Plane lane, and this deployment holds no model credential`; before any fetch (new).
   - gateway refused the first model call (`ErrorCode::Forbidden`: 401/403, or Auth Core would not mint): `structured extraction needs a granted Model Plane lane, and the Model Plane refused this service`; after exactly one fetch and one model call, only while no source has been extracted (new; was a 200 with per-source errors after paying for every fetch). A refusal after an `ok` source, 5xx, timeouts, non-JSON, ZDR and privacy refusals stay per source; 429 keeps its envelope.
   - Whitespace-only URL now counts as unset (was: built a client against "   ").
2. `hint` starts "omit `schema` and `prompt` to receive cleaned markdown" and names `QUARRY_EDGE__MODEL_PLANE_URL` (and for the credential case `QUARRY_EDGE__MODEL_PLANE_TOKEN`), both real EdgeConfig fields; never `QUARRY_EDGE__AI_FORMATS`. `next_actions` = [retry without `schema` and `prompt`, operators: see RUNBOOKS section].
Docs: RUNBOOKS new section "Enable structured extract and AI formats" (door table extract vs scrape formats; "There is no production path today" until WS16.S1 to S3 and decision X-7 / OD-1; the three 501s with exact texts; what works in dev; UI guidance; checks); the INNH-2 section corrected; CONTRACTS `/v1/extract` Errors paragraph; ERROR_REASONS section for the three texts; SELF_HOST paragraph; openapi 501 description plus two examples and `next_actions` on `ErrorBody`; Ingestion compose comment; deploy compose README row.
Guard: `scripts/tests/docs-contract.test.mjs`: every `QUARRY_EDGE__`/`QUARRY_EDGE_` name and bare `MODEL_PLANE_*` named in the extract hints, openapi, CONTRACTS, ERROR_REASONS, deploy README, the three Model Plane RUNBOOKS sections, the SELF_HOST Model Plane section and the composes' Model Plane lines is read by the code (EdgeConfig field or a string literal) or is a compose variable; a self-test of the checker; the hints name the real URL setting and never the AI formats switch; the RUNBOOKS section states the no-production-path facts. QB-T17 stays pending, description fixed.
ZDR: no change to ZDR behaviour (the 501s are skipped for a ZDR request; the `extract` matrix row and golden files are untouched, so no regeneration).

## Commits
(see the line appended below after the commit)

## Tests (counts)
- `cargo test -p quarry-edge --lib extract_routes`: 28 passed (7 new: the real-setting checker, schema/prompt with no URL (unset, empty, blank: exact texts, exact key set, zero fetches), URL with no credential (zero fetches, URL not echoed, markdown route unaffected), ZDR never gets the 501, gateway 403 and 401 -> one 501 after exactly one fetch and one model call and nothing metered, 500 on source 1 and 200 on source 2 stays per source, refusal after an extracted source stays per source; the INNH-2 501 test now pins {code, error, hint, next_actions}).
- `cargo test -p quarry-edge --lib api_error`: 6 passed.
- `cargo test -p quarry-edge --test zdr_claim_floor extract`: 2 passed (the ZDR extract claim-floor tests still hold).
- Final rerun after formatting edits: `--lib extract_routes` 28 passed. `rustfmt --check` still reports hunks in the new test code (the file already had hunks in untouched code); not reformatted.
- `node --test scripts/tests/*.test.mjs`: 65 tests, 53 pass, 0 fail, 12 todo (the pending ones; docs-contract.test.mjs: 26 tests, 14 pass, 12 todo; 4 new passing).

## Deviations
- The brief asked for a "Forbidden short-circuit if still missing": built, but only while no source has been extracted, so a late refusal never discards extracted data or bills inconsistently.
- Added the "URL, no credential" pre-fetch 501 (not in the brief); it is the same lane-not-granted family and spares the fetches.
- No `details.reason` on the extract 501 (the status file said "no new fields"); the three states are told apart by `error` text. Owner may want `details.reason` (`model_plane_unavailable`, `model_lane_not_granted`) for symmetry with scrape; small change to ApiError.
- SDK docs (`sdks/*/docs/ExtractApi*.md`) still carry the old 501 line; they are generated and regeneration is blocked.

## Not verified
- No real Model Plane gateway or Auth Core was called; "the gateway answers 401/403" is a wiremock double, and Auth Core refusing an ungranted row is read from `service_tokens.rs`/`mp_client.rs`, not exercised. In the deployed (provider) case the first model call probably fails before any HTTP to the gateway (the row is ungranted), so the real sequence is one fetch, then the 501.
- The dev path in the RUNBOOKS section (bypass + `QUARRY_EDGE__MODEL_PLANE_TOKEN`, no service key) is read from `main.rs`, not run; whether a given dev gateway accepts that token on `/v1/invoke` is not checked. The Ingestion compose does not forward `MODEL_PLANE_TOKEN` (the deploy compose does).

## Owner decisions
- X-7 / OD-1 (the Quarry to Model Plane grant) and WS16.S1 to S3 (metered service lane) stay the only route to a production path; nothing here changes them.
- Optional: add `details.reason` to the extract 501; forward `MODEL_PLANE_TOKEN` in the Ingestion compose for dev stacks.

## Contract shape for the Innhenting UI
501 body, always `{error, code: "UNSUPPORTED", hint, next_actions: [string, string]}`:
- `error` is one of the three texts above; show it as the reason. Tell the states apart by `error` text only.
- `next_actions[0]` is the retry: the same request without `schema` and `prompt` returns `status: fetched` cleaned markdown per source.
- Do not show `hint` as user advice and never tell a person to set a flag: it is for operators, and no flag unlocks structured extract on the live stack (WS16.S1 to S3 and X-7 first). `QUARRY_EDGE__AI_FORMATS` has nothing to do with `/v1/extract`.
- 200/400/429 shapes unchanged; a Forbidden-only batch that used to return 200 with every item `error` now returns this 501.
