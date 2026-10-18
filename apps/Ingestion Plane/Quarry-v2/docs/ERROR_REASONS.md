# `details.reason` values

One closed list of the machine-readable reasons the edge puts in `error.details.reason` for a 404, a 415, a 429, a 502 or a 504. A client keys on the `reason`, never on the message. A design that adds a reason appends it here in the same change.

## 429 `RATE_LIMITED` on `POST /v1/agent/runs` (run caps)

Checked in this order, before the browser driver's own capacity (R-26). The body is
`details {reason, scope, limit, active, retry_after_seconds}` and the gateway turns it
into `browser_capacity_reached` with `Retry-After`. Retryable.

| reason | scope | meaning | knob (`QUARRY_EDGE__`) |
|---|---|---|---|
| `global_run_cap` | `global` | the edge has this many live runs | `MAX_CONCURRENT_RUNS` (32) |
| `org_run_cap` | `org` | this organization has this many live runs | `MAX_CONCURRENT_RUNS_PER_ORG` (8) |
| `org_interactive_run_cap` | `org` | this organization has this many of a person's own sessions open; the sub-cap leaves room for its agent runs | `MAX_INTERACTIVE_RUNS_PER_ORG` (6) |

`0` disables a cap.

## 429 `RATE_LIMITED` on `POST /v1/crawl` and `POST /v1/batch`

| reason | scope | meaning |
|---|---|---|
| `org_backlog_full` | `org` | the organization already has `limit` jobs waiting (job admission) |

## 429 `RATE_LIMITED` when a page is rendered

| reason | scope | meaning |
|---|---|---|
| `org_render_cap` | `org` | the organization holds its share of the browser pool |

## 404 `NOT_FOUND`

| reason | meaning |
|---|---|
| `run_not_live` | the run was closed or released (by its owner or by the sweeper) while the request was waiting for it, or it is no longer in the registry (a sweeper release or a restart). A step or tab call on a run that is no longer in the registry adds `resumable: true` and `resume_run_id` for the owner of a non-ZDR run whose checkpoint is still active; every other caller, and every ZDR run, gets the plain 404 |

## 504 `TIMEOUT` from the browser driver

A browser operation that did not finish inside its deadline (`crates/quarry-browser/src/deadlines.rs`). The body is `details {op, deadline_ms, effect}`, never a URL, selector or page text. `health_probe` is the op of the browser's background prober: a probe that overruns its deadline is logged and recorded in the slot's health, and no response body carries it. Two other 504 `TIMEOUT`s do not come from the driver: `POST /v1/scrape` answers its own when `timeout_ms` runs out, with `details {op: "scrape", deadline_ms}` and no `effect`; a document parse past its 15 s deadline has no `details`.

| field | values | meaning |
|---|---|---|
| `op` | `launch`, `tab_attach`, `navigate`, `content`, `ax_snapshot`, `screenshot`, `pdf`, `evaluate`, `input`, `native_action`, `wait_for_poll`, `wait_for`, `page_close`, `context_close`, `health_probe`, `profile_capture`, `download_begin`, `download_complete` | what timed out |
| `deadline_ms` | number | the bound that expired, after `QUARRY_BROWSER_DEADLINE_SCALE` |
| `effect` | `none` | the operation cannot have changed the page: a read or a teardown. `retryable` is true |
| `effect` | `unknown` | the operation may have changed the page (`navigate`, `evaluate`, `input`, `native_action`): look at the page before repeating it. `retryable` is false |

## 415 `UNSUPPORTED_MEDIA` on `POST /v1/scrape`

A body refused by its content kind (`QUARRY_EDGE__DOC_ROUTING`, `docs/RUNBOOKS.md`, "Scrape returns 415"). `details.kind` is set in every case.

| reason | other fields | meaning |
|---|---|---|
| `unsupported_media` | `subtype`, `content_type`, `bytes`, `source`, `supported` | `skip` or `on` refused a body that has no extractor (an image, an archive, a spreadsheet) |
| `pdf_parse_failed` | `limit`, only when the parser was stopped | the PDF could not be read. With `limit` `parser_stopped`, the parser child was killed by a resource limit, crashed or answered nothing usable |
| `docx_parse_failed` | as `pdf_parse_failed` | the same for DOCX |
| `too_large` | | the document is over a limit of the parser itself: the DOCX entry count (4096) or the unpacked WordML (32 MiB). The 25 MiB input cap sits behind the 16 MiB body cap, so HTTP does not reach it |

`limit` is `parser_stopped` only in the child-process mode (`QUARRY_EDGE__DOC_PARSE_ISOLATION=process`). A PDF with no text layer is not an error: it returns the warning `document_has_no_text_layer`.

## 502 `DRIVER_FAILED` from the browser driver

| reason | other fields | meaning |
|---|---|---|
| `viewport_emulation_failed` | `error` (Chromium's message) | a new page could not be given its viewport, so the tab does not open. `QUARRY_BROWSER_PAGE_HYGIENE=off` does not skip this call (`docs/RUNBOOKS.md`, "Browser viewport and page hygiene") |
