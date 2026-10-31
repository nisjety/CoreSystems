# R1-browser: sentences for R8b to fix in docs/RUNBOOKS.md and docs/ERROR_REASONS.md

(R1 did not edit either file.)

## docs/RUNBOOKS.md, section "Browser capacity, crashes, the idle reaper and /ready (WS3.F)"

1. Paragraph "Probe and /ready" (line ~317): after "GET /ready carries a `browser` object (...)" add:
   "The edge runs two local Chromium drivers, so `/ready` carries a second object, `scrape_browser`, with the same shape for the scrape path's driver; `browser` is the agent-run driver's. Each is present only when that driver runs a local browser. The answer is 503 `browser_unhealthy` when either object's `state` is `unhealthy`."
   Replace "answers 503 `browser_unhealthy` only when every launched browser is unhealthy or dead" with "answers 503 `browser_unhealthy` when, for either driver, every launched browser is unhealthy or dead". Check line: `curl -s localhost:8082/ready | jq '{browser, scrape_browser}'`.

2. "Known gaps" paragraph (line ~319): delete the first two gaps. Replace with:
   "The R-26 boot check is wired and WARN-only: at boot the edge logs `browser capacity: QUARRY_BROWSER_MAX_SESSIONS (n) must exceed the concurrent-run cap (m) plus the heavy lane (1)` when the session cap does not exceed `QUARRY_EDGE__MAX_CONCURRENT_RUNS` plus the heavy lane budget (`slots::HEAVY_LANE_BUDGET`, 1 until the heavy lane exists). It becomes a boot failure once the Chromium RSS at 8, 16 and 32 contexts is recorded. `docs/openapi.yaml` documents the JSON `/ready` body."
   Keep the gateway-mapping sentence only if still true (R-?? gateway commit 62f1b66a mapped the four codes; likely stale: check).
   Replace "A tab attach now holds the driver's browser lock only while it creates its context and page" with: "A tab attach no longer blocks other sessions: the driver's browser handle is a read/write lock, every CDP call (context, page, dispose, download listeners) takes the read side so sessions attach in parallel, and only a launch, a relaunch and a kill take the write side. The prober skips one tick when sessions are attaching."

3. New short paragraph (page ownership): "A page and a live frame belong to the session admission that created them. A key that is released and admitted again is a new session: a late close of the old session's page does not give back a page of the new one (the slot book's `page_closed` takes the admission id), and a late capture or navigation of the old session cannot read, fill or reset the new session's live-frame cache."

4. Scrape adapter: "A scrape fetch that is cancelled (the HTTP request was dropped, a caller's timeout) releases its browser session and returns its pool slot only after the release has run; the lease is poisoned so it is not reused."

5. Line ~361 ("WS3.F: /ready carries a `browser` object ..."): add "and `scrape_browser`".

## docs/ERROR_REASONS.md, section "503 on GET /ready: browser_unhealthy (WS3.F)" (line ~91)

Replace "when every launched browser is dead or failed two health probes in a row" with "when, for the agent driver's browser (`browser`) or the scrape driver's (`scrape_browser`), every launched browser is dead or failed two health probes in a row". Add: "The body is documented in `docs/openapi.yaml` (`GET /ready`, 200 and 503)."

## CI note (R1 review, pass 2)
Make the quarry-browser-live CI lane required (or watched): the live-gated driver wiring tests of 46157832 (a_late_goto_of_a_released_session..., close_tab_gives_the_page_back_to_the_entry_that_opened_it) and the flaky-lock fix tests only run there.
