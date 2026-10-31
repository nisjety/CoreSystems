# W2-U09 hand-over (WS13.2)

What was done: the contract types for scrape-time screenshots, PDFs and actions, in quarry-core, off by default. Four commits on quarryv2-next (see W2-U09-progress.md).

What is left (next slices): WS13.3 driver capture API (plan_screenshot, PNG and JPEG header reads, PDF caller, spike that sets SINGLE_CAPTURE_MAX_DEVICE_PX); WS13.4a/b (FetchResponse.captures, the /v1/scrape wiring behind QUARRY_EDGE__SCRAPE_CAPTURE); WS13.5 (actions on the same page behind QUARRY_EDGE__SCRAPE_ACTIONS); WS13.8 docs.

Traps found:
- The quarry-core working tree has another unit's uncommitted hunks in src/resources.rs, and docs/RUNBOOKS.md carries other units' sections. Commit only your own paths: qcommit.sh for whole files, qcommit-blobs.sh for a file with foreign hunks.
- Adding an f64 field to a derived-Eq type breaks Eq and Hash. ScrapeFormat, ResolvedScrapeOptions and EffectiveScrapeOptions lost Eq for that reason.
- The scrape schema golden is a regeneration target: run it with UPDATE_SCHEMAS=1 and the jsonschema feature, and check the diff. The Model Plane vendors the same file.
- The "formats" string "pdf" meant a 400 before this unit (unsupported_format). The U09 tests replace it with "xml" where they check that case.
- Attribution: the harness reminder asks for Co-Authored-By Claude Haiku 5.5, the batch text asks for Sonnet 5.5. The commits carry the Haiku line (the harness reminder is the later instruction).
