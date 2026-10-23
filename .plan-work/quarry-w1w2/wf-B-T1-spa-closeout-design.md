# B-T1 SPA closeout: design (written before the code)

Three deliverables, each with the smallest design that meets the plan text (WS3.J SPA paragraph, WS1.B "Not done").

## 1. Idempotency-Key on an AI-run start
- `startBrowserAiRun(orgId, sessionId, params, signal?, options?: { idempotencyKey })` adds an `Idempotency-Key` header
  only when a key is given (gateway `client_idempotency_key`: 1..128 of `A-Za-z0-9-_.:`; the SPA validates the same shape
  before sending so a bad key is a local error, not a round trip). No key, no header (old behaviour).
- Retry safety lives in a tiny keyring (`createAiRunStartKeyring`, same file): `keyFor(sessionId, params)` returns the held
  key when the same start is repeated (same session and same body fingerprint) and a fresh `crypto.randomUUID`-based key
  otherwise; `settle(error?)` drops the key after success or after any definitive answer (4xx incl. 409 reused / run_ended)
  and keeps it only for an AMBIGUOUS failure (no response, 5xx, 408, 503 `browser_idempotency_unverified`), because only then
  could the first attempt have started a run. A new submit after success always gets a fresh key.
- `KnowledgeComposer.beginBrowserAiRun` owns one keyring and passes the key; nothing else calls the client.

## 2. Copy
errors.ts rows (no/en, end-user text, no raw ids): `invalid_idempotency_key`, `browser_idempotency_key_reused`,
`browser_idempotency_key_run_ended`, `browser_idempotency_unverified` (retryable, so the retry hint is appended).
`browser-errors.test.ts` loses its UNREACHABLE_FROM_THE_SPA allow-list (the codes are reachable now and have copy).

## 3. BrowserBlockedNotice (WS3.J SPA half)
- `browser-client.ts`: `BrowserChallenge` (Quarry's `ChallengeSignal` wire shape: kind, strength, block_reason, recovery,
  confidence, requires_escalation) and `BrowserObservation.challenge?`. Quarry sends it today on every observation the
  gateway passes through (the observation is a raw pass-through), so the manual/visible-tab path gets it without the
  Model Plane half. The Model Plane `BrowserObservation.challenge` (sibling b) is NOT needed for this notice.
- Pure rule in `browser-blocked.ts`: a notice exists only for `strength === 'strong'` (a missing strength is NOT read as
  strong here; login, consent, paywall, rate limit, bare 403 are `non_signal`/`weak` and show nothing). Reason copy is the
  existing `BLOCK_REASON_COPY`, never page text (Quarry's `evidence` strings are never shown).
- `BrowserBlockedNotice.tsx` (new file; BrowserChrome gets a small wiring hunk beside the take-over chip) with two buttons:
  "Ta over / Take over" calls the chrome's existing `toggleBrowserControlMode` (hidden once the person already has control)
  and "Prøv en annen kilde / Try another source".
- Action contract first: `browser.blocked.try_other_source` in `action-registry.ts`, input `{ sessionId, runId? }`.
  The registry<->gateway parity test (`apps/gateway/scripts/action-surface-contract.test.mjs`) requires a live dispatcher
  for every registry id, so the gateway gets `dispatch_browser_blocked_try_other_source`: stop the run when a `runId` is
  given (a stop answered 404 or 409 = the run already ended = fine, anything else aborts before the close), then close the
  session; both through the existing owner paths. DECISION FOR REVIEW: this is the only server meaning "leave a blocked
  site" has; it reuses `browser_run.control` stop and `browser_session.close` and adds no new owner behaviour.
- SPA: the button calls the action through `executeAction` from `KnowledgeComposer` (it knows the session and the run),
  and on success runs the existing `clearPreview()` so the person is back at the link field; on failure the error is shown
  (`browserErrorMessage`) and the preview stays so Discard still works.

## Out of scope / stays open
Model Plane `challenge` on run observations, Playwright e2e, crawl/upload catch sites.
