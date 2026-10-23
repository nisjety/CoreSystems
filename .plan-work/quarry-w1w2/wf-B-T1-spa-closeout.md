# B-T1-spa-closeout (SPA closeout: Idempotency-Key, idempotency copy, BrowserBlockedNotice)

Design note (decisions, written before the code): wf-B-T1-spa-closeout-design.md. Progress log: wf-B-T1-spa-closeout-progress.md.

## Commits (local, quarryv2-next)
- 1f0885e9 feat(spa): Idempotency-Key on an AI-run start, keyring that keeps the key across a retry of the same start, copy for the 4 idempotency codes
- 5d0961ef feat(actions): action contract browser.blocked.try_other_source + gateway dispatcher; catalog-manifest sorts by code unit
- a4d1a246 feat(spa): BrowserBlockedNotice (Take over / Try another source), strong signal only; composer wiring; tests

## What it does
- startBrowserAiRun(.., { idempotencyKey }) sends `Idempotency-Key` (gateway shape checked locally). createAiRunStartKeyring: same session + same
  body = same key after an AMBIGUOUS failure (no reply, 408, 5xx incl. 503 browser_idempotency_unverified); success or a definite 4xx drops it.
- Copy (no/en): invalid_idempotency_key, browser_idempotency_key_reused, browser_idempotency_key_run_ended, browser_idempotency_unverified (retryable).
- Notice: only `challenge.strength === 'strong'` (missing or unknown strength = nothing; login/consent/paywall/rate limit/weak = nothing).
  Sentence from the closed block_reason (BLOCK_REASON_COPY), never evidence/page text. Take over = the chrome's toggleBrowserControlMode.
  Try another source = composer runs the action, then clearPreview(); on failure it keeps the preview and shows the copy.
- Action contract browser.blocked.try_other_source { sessionId, runId? } (human-only). Gateway dispatcher: stop the run (404/409 = already ended = fine,
  any other stop failure aborts BEFORE the close), then close the session; result { closed, runStopped }.

## DECISIONS TAKEN THAT THE OWNER SHOULD REVIEW
1. The server meaning of the action (stop run + close session, nothing new in any owner plane). The parity test forced a dispatcher; this is the
   only meaning "leave a blocked site" has today. Change it in dispatch_browser_blocked_try_other_source if an audit row or a Quarry marker is wanted.
2. catalog-manifest.ts: localeCompare -> code-unit compare (the registered id browser.blocked.* sorts differently from browser_session.* under ICU
   and broke the file's own "sorted" test; localeCompare also made the catalog hash host-dependent).
3. KnowledgeScrapePreview.tsx (outside the owned list): 4 one-line pass-through edits for onBrowserTryOtherSource.

## Traps found
- A test of mine used `model_gateway_unreachable`, whose copy is U4's UNCOMMITTED hunk: it passed in the working tree and failed in an isolated
  HEAD+patch copy. Always verify a hunk-only commit in an isolated copy (scratch/B-T1-spa-closeout/iso.sh, isogw.sh).
- Mutation restore: a restore by "replace mutated text with original" fails when the mutated text is not unique (restored by hand, then diffed
  against the transform output to prove equality). Check the file after every mutation script.
- vitest workers sometimes fail to start ("Timeout waiting for worker to respond") while cargo/eslint run in parallel; re-run alone.
- Isolated trees need a SHORT path (C:/b1iso, C:/b1gw): the scratchpad path gives "Filename too long" in git apply.
- qcommit-mix.sh (scratch/B-T1-spa-closeout) = private-index commit of a HEAD-based patch plus whole files; moves the real index only for entries
  that were absent or at the old HEAD blob; the Docker gitleaks hook ran and passed (no --no-verify).

## For U4 (uncommitted round-4 hunks still in the tree)
The working-tree diff vs HEAD of errors.ts, BrowserChrome.tsx, browser-errors.test.ts, knowledge-client.ts, browser-client.errors.test.ts is now exactly
U4's (and WS5.G's) hunks. U4's saved patches (p-copy5 etc.) were built against e7e9b9e1; regenerate them from `git diff` against the new HEAD
(my commits touched errors.ts (4 rows before the "Scrape preview refusals" comment) and browser-errors.test.ts (removed UNREACHABLE_FROM_THE_SPA,
added 4 codes to BROWSER_ERROR_CODES)). Plan ledger row WS3.J can now say: SPA notice built (a4d1a246) except the Model Plane field and the e2e.

## Not done
- Playwright e2e tests/e2e/browser-workspace-blocked.spec.ts (needs a stack).
- Model Plane half: BrowserObservation.challenge on run observations, planner rule text, 403 -> blocked observation (sibling b). A model-driven run's own
  Quarry session shows no notice; the visible tab does.
- The stop path of the dispatcher (control_ai_run with owner lookup and model-gateway) has no handler test with upstreams mocked; only its status
  reading is unit-tested. For a ZDR org the owner refuses run controls (412), so the action fails there when a runId is sent (Discard still works).
- A start made through the browser_run.start ACTION (action surface) sends no Idempotency-Key.
- Plan doc ledger row not edited (the plan file carries U4/U5's uncommitted rows).
- No live-stack run; QUARRY_CHALLENGE_ENFORCE default (log) unchanged, so a real bot wall only reaches the SPA as a strong signal where Quarry classifies it.

## Leftover scratch outside the worktree (safe to delete, but NOT with rm -rf on C:/b1iso: node_modules there is a junction into the worktree)
C:/b1iso/* (SPA isolated copies; remove the node_modules junction first with `cmd /c rmdir`), C:/b1gw/* and C:/b1t (gateway isolated copy and cargo target).

## Round 4 (reviewer findings) - commits 2aa92fbc, f57bf091
- Count corrections: 1f0885e9 "fails 5" is 4 on its own tree (5 on a4d1a246); a4d1a246 "(17)" is 18 across its three test files; "leaving does not clear the preview (2)" is 1. Stated in 2aa92fbc's message.
- Strong signal of every kind now tested (browser-blocked.test.ts, 9 rows); mutation caught.
- Dispatcher stop path: 3 handler tests with auth-core/model-gateway/Quarry mocked (stop 200/404/409/412 close; 401/403/5xx abort before close; mismatched run/session ids). Mutation caught.
- Registry/doc text: run is NOT checked against the session (run read model has no session id here); reworded, not "fixed".
- lastAiRun now { sessionId, runId }, cleared in addToKnowledge too; the add button is not wired in KnowledgeScrapePreview (onAdd unused), so only Discard has a UI test. A "replaced without Discard" test is impossible: the URL input is hidden while a preview shows.
- ZDR: 412 on stop now still closes the session; result has runControlsDisabled:true. Decision for the owner: that closing the session is enough to end a run that cannot be stopped is not live-verified.
- Trap: a `cp x ../../../..` from the gateway dir wrote C:/dev/CoresSystem/scratch-dispatchers.bak outside the worktree; removed. Use absolute paths.

## Round 5 - 137ba2ac
- Only a 412 whose body is model-gateway's ZDR text ("ZDR browser-run controls are disabled...") counts as run controls off; any other 412 refuses the stop and nothing is closed. Coupled to that sentence (fails closed). A structured code on the owner side would remove the coupling (model-gateway change, not made).
- SPA shows a status line after leaving when result.runControlsDisabled is true (composer leaveNotice).
- Mutation claims of earlier messages re-measured on the current tree: see the commit message.
