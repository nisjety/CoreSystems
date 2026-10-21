# `details.reason` values

One closed list of the machine-readable reasons the edge puts in `error.details.reason` for a 403 (bot wall), a 404, a 409 (trusted input), a 415, a 429, a 502 or a 504. A client keys on the `reason`, never on the message. A design that adds a reason appends it here in the same change. Three answers that are not a `reason` have their own sections at the end: a ZDR durable-resource refusal (`details.zdr_refusal`), a budget refusal (`details.limit_source`) and a body that names the initiator twice.

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

## 403 `FORBIDDEN`: a durable resource under Zero Data Retention (WS4.H stage A)

Not a `details.reason`: the body is `error.details {zdr_refusal: {code: "zdr_durable_mutation_forbidden", action}}`, with no `feature` key. The message says that ZDR is active and carries no caller data (no URL, host, title or organization). The refusal happens only when `QUARRY_EDGE__ZDR_REFUSE_DURABLE_RESOURCES=true`, before Control Plane or a store is reached, and leaves a `zdr.refused` event. With the flag off, the default, nothing is refused: a ZDR caller is served as before and each request below leaves one `zdr.would_refuse` event (`docs/RUNBOOKS.md`, "A ZDR organization and durable resources"). Not retryable by the same caller.

| matrix row | `details.zdr_refusal.action` | request |
|---|---|---|
| `schedules` | `create`, `unpause`, `trigger`, `backfill` | the schedule write routes |
| `change_history` | `check` | a change check |
| `change_snapshot` | `snapshot`, `refresh` | a change snapshot or refresh |
| `profiles` | `save`, `create`, `update` | `POST /v1/profiles`, `POST /v1/profiles/create`, `PATCH /v1/profiles/{id}` |
| `change_record_internal` | `record` | `POST /v1/internal/change/record`, only when its body says `zdr: true` |

Reads and deletes (schedule list, pause and delete; profile list, load, delete and `restore_probe`; `/v1/change/latest` and `/v1/change/history`) are never refused, so an organization can wind down. The refusals that existed before WS4.H are enforced whatever the flag says (they are not behind it), keep their status, error code and message word for word, and carry the same `details.zdr_refusal {code, action}` since V4:

| matrix row | status and `error.code` | `details.zdr_refusal.action` | request |
|---|---|---|---|
| `durable_jobs` | 403 `FORBIDDEN` | `crawl`, `batch` | `POST /v1/crawl`, `POST /v1/batch` (a ZDR organization's user, or a body with `zdr: true`) |
| `agent_persistent_profile` | 400 `BAD_REQUEST`, message `zdr_persistent_profile_forbidden: ...`; `details.legacy_code` is `zdr_persistent_profile_forbidden` (the token the message has always started with, now also a field) | `profile` | `POST /v1/agent/runs` under ZDR with `persist_profile` or a `profile_id` |
| `agent_resume` | 403 `BAD_REQUEST`, message `ZDR runs cannot be resumed from durable state`; no `legacy_code` (the message never carried a token, so none is invented) | `resume` | `POST /v1/agent/runs` with a `resume_run_id` whose checkpoint is ZDR |

The orchestrator treats a 403 with this code from `/v1/internal/change/record` as a clean terminal outcome of the change-monitor run (`run_failed`, `reason: zdr_refused`, no retry), not as an auth failure; any other 403 stays an auth failure there (`docs/RUNBOOKS.md`, "A ZDR organization and durable resources").

## 402 `BUDGET_EXHAUSTED`: `details.limit_source` (WS6.E)

A step of an agent run is refused when the run reached a bound. `details` is `{dimension, limit, spent, unit, estimated, limit_source, resumable: false}`. `limit_source` is `grant` when a broker grant's budget claim set the bound that was reached (only with `QUARRY_EDGE__GRANT_BUDGET_CLAIM=enforce`, and only when the claim was tighter than what the request, the default and the ceiling gave), and `run_budget` otherwise. `data.clamped[].by` on the run start takes the same value `grant`. The claim caps one run, not the grant (`docs/RUNBOOKS.md`, "A grant carries a budget claim").

## 422 on a body that names the initiator twice

A tab open whose JSON body carries both `initiated_by` and `initiatedBy` is answered `422` with a plain-text body (`duplicate field initiatedBy`), not an error envelope. It was seen on the edge image U9 built from the working tree that became `dec93d1e` and again on an image of the committed `ddbd4b2c` (`docs/RUNBOOKS.md`, "Evidence, round 4" and "Evidence, round 4, repeated on the committed HEAD"). The step body and the run start body take the same JSON extractor, and `agent_run_control.rs` (`both_spellings_of_the_hint_in_one_request_are_refused_on_every_route`) asserts `422` for the run start and the step too, and a `4xx` (the status is not pinned further) for the tab select and close query, which goes through its own extractor. A value that is neither `human` nor `agent` is a `422` in a body and a `400` in the select and close query (`an_unknown_tab_label_is_refused_and_runs_nothing`; the tab-open body case is asserted since `d49dfd48`). Besides that, `agent_routes/tests.rs` (`every_request_body_takes_the_hint_under_both_spellings`) pins that the tab, step, run-start and socket-message types and the select/close query all refuse two spellings, but no test drives the socket message over HTTP: it goes through its own extractor. Quarry never picks one of two spellings; one spelling of either is accepted everywhere. An agent call during a take-over is `409 USER_TOOK_OVER` whichever spelling it uses (`docs/CONTRACTS.md`, "Which agent opened a tab").


## 409 `TARGET_REPAIR_REQUIRED` from trusted input: `details.reason` (WS3.K)

Only with `QUARRY_BROWSER_TRUSTED_INPUT=on` (`driver_capabilities.trusted_input` is `true`, `docs/RUNBOOKS.md`, "Trusted input for ref actions"). A click, type or select on a top-level ref that cannot be performed as a person would is refused before anything happens, with the same status and the same error code as every other stale-ref answer (`409 TARGET_REPAIR_REQUIRED`, `retryable: false`). `details` is `{reason}` and, for `target_covered` only, `{reason, covered_by_tag}`. `covered_by_tag` is the tag name of the element that received the pointer (`div`, `dialog`, a custom element name), lower case, letters, digits and hyphens, 32 characters at most: never its text, id, class or URL. The message is a fixed sentence per reason. Nothing else about the page is in the error or in the log. Closed list (`quarry_core::error::target_repair_reason`); a new reason is appended here in the same change.

| reason | meaning | what the caller does |
|---|---|---|
| `target_not_attached` | the element left the document between the observation and the action (a stale ref, including a node that is already gone when the action starts: it still resolves, and the probe finds it detached) | observe again |
| `target_not_visible` | `display: none`, `visibility: hidden` or no layout box, still so when the 2 s budget ended | make it visible (open the menu, scroll its container) or pick another ref, then observe again |
| `target_disabled` | disabled or `aria-disabled="true"`; for a type also read-only, or not a field that takes text or a value (a checkbox, a radio, a submit button and a file input are not) | pick another ref or enable the control, then observe again |
| `target_covered` | another element receives the pointer at the click point (a modal, a cookie banner, a sticky header). `covered_by_tag` names it. A `type` and a `select` are checked at the middle of the field's visible box too, so a transparent overlay or a decorative icon over a styled field gives this reason as well; the tag (`span`, `svg`, `i`) tells it from a modal. A cover inside the same closed shadow root as the target cannot be seen (the root hides it) and is not reported | dismiss or scroll past the covering element, then observe again |
| `target_unstable` | the box kept moving (more than 0.5 px between two samples 25 ms apart) for the whole 500 ms stability window of each attempt, until the 2 s budget ended | wait for the animation, then observe again |
| `target_has_no_box` | laid out, but with no area to point at (zero width or height) | pick another ref |
| `target_unreachable_after_scroll` | there is a box, but none of it is inside the viewport after scrolling it into view (a fixed element placed off screen, a clipped container) | pick another ref |

The check repeats every 100 ms for up to 2 s (`Op::Actionability`), so a button that becomes enabled or an overlay that closes during that time is acted on; the reason returned is the last one seen. A node that has already left the document when the action starts gets `target_not_attached` with the flag on (a removed node still resolves from its backend node id and the first probe finds it detached; measured, `display_none_fails_target_not_visible`). With the flag off the old script path answers as it always did, with `native_target_not_connected`. The older reason-less "native accessibility target is no longer live" is what a node answers when `DOM.resolveNode` itself fails, which can happen with either setting (measured: a ref from a document that a navigation replaced, error `Node with given id does not belong to the document`; `a_handle_whose_realm_was_destroyed_by_a_navigation_is_not_attached`). A node that leaves between the resolve and the probe, or whose realm a navigation destroys in that window, is `target_not_attached`.

Reasons the in-page script path has always put in `details.reason` stay as they were, for one release after trusted input is on by default: `native_target_not_connected` (an alias of `target_not_attached`: `canonical()` in `target_repair_reason` maps it), `target_not_unique_or_changed`, `target_not_typeable`, `value_setter_unavailable`, `target_not_select`, `select_value_unavailable`, `unsupported_native_operation`, `unsupported_verified_operation`. A client written against the old strings keeps working; a client may map a reason it does not know to "observe again".

The two new driver deadlines (`op` in the 504 table above): `actionability` (2 s; each protocol call inside the check is bounded by it too, and it is the `op` of a 504 `TIMEOUT` only when one of those calls does not answer) and `stable_poll` (500 ms, a polling window that ends in `target_unstable`, never in a 504). Both are `effect: none` and retryable: nothing has been clicked while a target is still being checked.
