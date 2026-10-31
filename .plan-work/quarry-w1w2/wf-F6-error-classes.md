# F6-error-classes hand-over (after review)

Commits: df7feedf, 1db86f58, 6b9c636a (first round); 67cff1f3, 700ac45e, 66a0cd16 (review fixes).

## Review findings
- HIGH fallback_driver/fetch kept url/host: real. Removed both fields (67cff1f3). The fallback one has a ZDR capture test (+ non-ZDR control).
  The fetch.rs host one has NO capture test (needs a proxy pool); pinned only by the lint ratchet (fetch.rs::R1 2 -> 1).
- HIGH residual-R2 accounting false: real. Corrected in 67cff1f3's message. tls and preflight converted; agent_loop, robots_cache annotated.
  15 R2 remain, all quarry-runtime (list in the commit message); they were read, not traced to ZDR callers.
- MEDIUM lint keys on names: real. R5 (error_class value must be a closed label), R6 (debug-printed whole event), bare %e in R2, class_reqwest unit tests (66a0cd16, 700ac45e).
- MEDIUM stream spawn without span: real for the spawned runner, partly wrong on the cause. The SSE body IS polled inside the request span
  (tower-http's body wrapper enters it), so a first attempt that wrapped the stream (in_span) changed nothing and was removed. What lost the span
  was tokio::spawn. Fix: capture Span::current() in the handler, .instrument(request_span) on the spawn. Verified the test fails without it
  (the probe URL appears in a WARN from quarry_runtime::fallback_driver).

## Traps
- The same field-name filter means a line with url/host/error is dropped under ZDR only when a ZDR span is current; spawned tasks need the span passed in.
- usage.rs spawned meter task has no span: its error text is not filtered at all (it is serde/NATS text, no caller data).
- Other spawn sites still R4: change_routes.rs, run_sweeper.rs.
