# `details.reason` values

One closed list of the machine-readable reasons the edge puts in `error.details.reason` for a 403 (bot wall), a 404, a 415, a 429, a 502 or a 504. A client keys on the `reason`, never on the message. A design that adds a reason appends it here in the same change.

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
| `op` | `launch`, `tab_attach`, `navigate`, `content`, `ax_snapshot`, `screenshot`, `pdf`, `evaluate`, `input`, `native_action`, `wait_for_poll`, `wait_for`, `page_close`, `context_close`, `profile_capture`, `download_begin`, `download_complete` | what timed out. `health_probe` is also an `Op` (`deadlines.rs`) but only the background prober runs under it, so it is not a value a client sees (see the paragraph above the table) |
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
| `too_large` | | the document is over a limit of the parser itself: the DOCX entry count (4096) or the unpacked WordML (32 MiB), or, for PDF, 256 MiB of total inflated `/FlateDecode` output (the inflate budget, checked before lopdf inflates anything). The 25 MiB input cap sits behind the 16 MiB body cap, so HTTP does not reach it |

`limit` is `parser_stopped` only in the child-process mode (`QUARRY_EDGE__DOC_PARSE_ISOLATION=process`). A PDF with no text layer is not an error: it returns the warning `document_has_no_text_layer`.

## 502 `DRIVER_FAILED` from the browser driver

| reason | other fields | meaning |
|---|---|---|
| `viewport_emulation_failed` | `error` (Chromium's message) | a new page could not be given its viewport, so the tab does not open. `QUARRY_BROWSER_PAGE_HYGIENE=off` does not skip this call (`docs/RUNBOOKS.md`, "Browser viewport and page hygiene") |

## 429 `RATE_LIMITED` from the browser driver (driver caps, WS3.F)

Checked after the run caps above (R-26), by the local Chromium driver's slot book (`crates/quarry-browser/src/slots.rs`). The body is `details {reason, scope, limit, active, retry_after_seconds}` with `retry_after_seconds` 15. Retryable.

| reason | scope | meaning | knob |
|---|---|---|---|
| `browser_session_cap` | `driver` | the driver already holds `limit` browser sessions; an `acquire` for a new session is refused | `QUARRY_BROWSER_MAX_SESSIONS` (40) |
| `browser_page_cap` | `driver` | the browser process already has `limit` pages open; a new tab is refused | `QUARRY_BROWSER_MAX_PAGES_PER_SLOT` (24) |

`0` disables a cap.

## 502 `DRIVER_FAILED`: a browser session that is gone (WS3.F)

All of these are `retryable: false`: repeating the call cannot help, the caller starts a new session (or resumes a run). A lost session never quietly opens a fresh tab.

| reason | meaning |
|---|---|
| `session_reaped` | the driver's idle reaper closed the session (idle past `max(2 x lease ttl, QUARRY_BROWSER_SESSION_IDLE_S)` or older than `max(2 x ttl, QUARRY_BROWSER_SESSION_MAX_AGE_S)`); remembered for 10 minutes |
| `browser_process_lost` | the Chromium process the session lived in exited or crashed; the next `acquire` launches a new one. Remembered for 10 minutes |
| `no_current_page` | the session is open but has no page yet (call `goto` or open a tab first) |
| `session_not_open` | the driver does not hold this session (released, or never acquired) |

## 503 on `GET /ready`: `browser_unhealthy` (WS3.F)

Not an error envelope: `/ready` answers `{"status": "not_ready", "durable": true, "reason": "browser_unhealthy", "browser": {...}, "scrape_browser": {...}}` when, for the agent driver's browser (`browser`) or the scrape driver's (`scrape_browser`), every launched browser is dead or failed two health probes in a row. The edge runs two local Chromium drivers, and each object is present only when that driver runs a local browser. A browser that was never launched is ready. The body is documented in `docs/openapi.yaml` (`GET /ready`, 200 and 503). `/health` does not change. The prober's own `health_probe` timeouts stay inside the driver and never reach a client.

## 403 `CHALLENGE_DETECTED`: a bot wall (WS3.H, WS3.J)

Not retryable by the same caller, and only when `QUARRY_CHALLENGE_ENFORCE=enforce` (`docs/RUNBOOKS.md`, "Challenge classifier and the one-recovery rule"). Neither the message nor the details carry a URL or page text; `host` appears only on an agent step that is not Zero Data Retention. The gateway reads `reason` and `block_reason` and shows them as a typed `blockReason` on `site_blocked` (scrape) or `browser_site_blocked` (browser calls); a value outside the closed lists is dropped there, never echoed.

| where | `details` | meaning |
|---|---|---|
| `POST /v1/scrape` | `reason` and `block_reason` (the same value), `recovery: "not_applicable"`, `next: "other_source"` | the fetched page is itself a bot wall; a scrape has no recovery to spend and no browser to hand to a person. `block_reason` is never `challenge_loop` here |
| an agent step | `reason`, `recovery: "exhausted"`, `next: "user_takeover_or_other_source"`, `host` | the host kept challenging after its one allowed recovery; `reason` is the block reason, `challenge_loop` for a host that keeps challenging after the recovery |

| `block_reason` | what the classifier saw |
|---|---|
| `captcha_widget` | a CAPTCHA widget on a page that is itself the challenge |
| `waf_interstitial` | a WAF interstitial page |
| `unexpected_bot_error` | two independent block markers |
| `challenge_loop` | agent path only: the host challenged again after the one recovery |

Observations carry the same closed values in `challenge.block_reason` and a `recovery` of `available`, `exhausted` or `not_applicable`.
