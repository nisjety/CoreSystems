# W2-U13 hand-over (WS7.C, scrape-family wiring)

Built: policy_baseline.rs, web_policy_edge.rs, RunPolicy.web_policy stamp in quarry-runtime, handler wiring in routes.rs (scrape, stream, internal run_page), boot env reads in main.rs, openapi RequestAsk, RUNBOOKS section. Details: W2-U13-progress.md and depth/W2-U13.md.

Left: tests/web_policy_scrape.rs (plan, real-token harness), SDK regeneration, org/grant layers (WS7.H to J), agent path (WS7.D), remaining routes (WS7.E).

Traps found:
- quarry-browser can break the quarry-edge build while another unit edits chromiumoxide.rs; wait and re-run qtest, do not edit it.
- main.rs and RUNBOOKS.md carry other units' hunks; commit only this unit's blobs (qcommit-blobs.sh).
- The DomainDenied details lack capability in WS7.B; the edge adds it. Fix in core is a depth item.
- Background qtest with a bare '&' writes an empty log; use run_in_background with the qtest command and a log redirect.
