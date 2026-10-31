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
| `org_plan_cap` | `org` | the organization's plan bounds this class of run (agent runs or a person's sessions) and it has this many live; `limit` is the plan value, never above `MAX_CONCURRENT_RUNS` | the plan keys `quarry.cap.agent_runs` and `quarry.cap.interactive_runs`; nothing by default (see below) |

`0` disables a cap.

`org_plan_cap` (WS6.G) appears only when the limits source reports a plan bound for the class of the start. The default source (`QUARRY_EDGE__LIMITS_SOURCE=static`) reports none, so nothing changes until a billing source is built. A class the plan bounds answers to the plan value alone, in place of `org_run_cap` and `org_interactive_run_cap` (so a plan can raise an organization above the floor, never above `global_run_cap`); a class the plan does not bound keeps the static caps. A plan value of `0` refuses every start of the class; a plan that says nothing is absent, not `0`.

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
| `deadline_ms` | number | the bound that expired, after `QUARRY_BROWSER_DEADLINE_SCALE`. chromiumoxide's own 30 s per-command timer can expire first (a scale above 1.2 for `navigate` and `pdf`); then it is 30000 |
| `effect` | `none` | the operation cannot have changed the page: a read or a teardown. `retryable` is true |
| `effect` | `unknown` | the operation may have changed the page (`navigate`, `evaluate`, `input`, `native_action`): look at the page before repeating it. `retryable` is false |

After a `navigate` timeout the load is stopped and the page can be read at once: the tab is on the slow page if its document had arrived, and still on the page it had before if not (SC4); after a timeout on a run's first `navigate` the tab is kept the same way, on `about:blank` if the document had not arrived (SC4 T4b). The next step observes that blank tab (200, `url` `about:blank`): the check of the page a step landed on accepts exactly `about:blank` as a tab with no page yet, never as a navigation target (T4b-1). A step that lands outside the run's (or the attachment's) allowed domains is a 403 `SECURITY_BLOCKED` on every step path (the REST step said `BAD_REQUEST` before 2026-10-29) and leaves a `failed` receipt with no observation. A new tab opened with a URL is still closed when its navigation times out, so as a run's first action it leaves no current page (open follow-up); an observing step after it gets a blank tab, as does the first wait of a run that never navigated or was resumed from a checkpoint with no URL (SC4 D1), and only an acting step still answers `no_current_page`. On an agent step the timeout can also come from the observation read after the action (`evaluate`, `content`, `ax_snapshot`); its `effect` is then the step's: `unknown` (`retryable` false) after an action that can change the page, `none` (`retryable` true) after a wait or a read-only action.

## 403 `SECURITY_BLOCKED`: a navigation the browser's network guard refused (SC4 D3)

The tab's Fetch guard refused the navigation's own document request, or a redirect hop of it. The navigation ends at once with `details {reason, op: "navigate"}` and no URL or host; `retryable` is false. Before 2026-10-29 it waited out the navigate deadline (25 s live) and answered 504 `TIMEOUT` with `effect: "unknown"`. It covers every navigation that goes through the chromium driver's `goto`: an agent `navigate` (REST step, browser socket, delegate step; a run's first one too), a resume's recovery navigation, and a scrape rendered in the browser; and a new tab opened with a URL (whose tab is then closed, as on any failure of that navigation).

| reason | message | meaning |
|---|---|---|
| `domain_grant_blocked` | `browser navigation is outside the run's allowed domains` | the host is outside the run's (or the tab's) egress policy, or on its deny list. The message is the edge's own for an off-list target, so the gateway answers it as `browser_navigation_outside_scope` |
| `url_or_dns_blocked` | `browser navigation target is blocked by security policy` | the URL or its DNS answer is blocked (a private or unresolvable address) |

The tab keeps the page it had (`about:blank` on a run's first step) and answers reads at once; the refusal is an egress receipt (with the same `policy`), which the next observation carries; the next navigate first clears the refused navigation from chromiumoxide's queue (about 0.1 s), as after a navigate `TIMEOUT`. The step's receipt is `failed` with `SECURITY_BLOCKED` and the flat cost of a failed action. A refused image, frame or other subresource never fails a navigation: it shows only as an egress receipt.

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
| `no_current_page` | the session is open but has no page yet (call `goto` or open a tab first). An agent step that only reads the page (`wait`, `wait_for`, `screenshot`, `pdf`, `get_content`) no longer gets it: it observes a blank placeholder tab (200, `url` `about:blank`; SC4 D1). An acting step (click, type, ...) still does |
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

A tab open whose JSON body carries both `initiated_by` and `initiatedBy` is answered `422` with a plain-text body (`duplicate field initiatedBy`), not an error envelope. It was seen on the edge image U9 built from the working tree that became `dec93d1e` and again on an image of the committed `ddbd4b2c` (`docs/RUNBOOKS.md`, "Evidence, round 4" and "Evidence, round 4, repeated on the committed HEAD"). The step body and the run start body take the same JSON extractor, and `agent_run_control.rs` (`both_spellings_of_the_hint_in_one_request_are_refused_on_every_route`) asserts `422` for the run start and the step too, and a `4xx` (the status is not pinned further) for the tab select and close query, which goes through its own extractor. A value that is neither `human` nor `agent` is a `422` in a body and a `400` in the select and close query (`an_unknown_tab_label_is_refused_and_runs_nothing`; the tab-open body case is asserted since `d49dfd48`). Besides that, `agent_routes/tests.rs` (`every_request_body_takes_the_hint_under_both_spellings`) pins that the tab, step, run-start and socket-message types and the select/close query all refuse two spellings, and the socket action is exercised over its real WebSocket route (an HTTP/1.1 Upgrade on `/v1/agent/runs/{run}/frames/ws`) with each spelling alone during a take-over (`agent_run_control.rs`, `a_socket_action_takes_the_hint_under_either_spelling`, each answered `UserTookOver:`), while both spellings in one socket frame are pinned only at the serde level (`BrowserWsClientMessage` in `agent_routes/tests.rs`), not over the wire. Quarry never picks one of two spellings; one spelling of either is accepted everywhere. An agent call during a take-over is `409 USER_TOOK_OVER` whichever spelling it uses (`docs/CONTRACTS.md`, "Which agent opened a tab").


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

## Scrape captures and actions (WS13.4b, WS13.5, WS13.6; WS13.8)

These reasons come from `POST /v1/scrape` (and the stream, where noted). The status is the HTTP status the edge answers. 501 and 400 are not in the header list above, which is why they are here.

| reason | status and code | meaning | what a caller does |
|---|---|---|---|
| `browser_capability_unavailable` | 501 `UNSUPPORTED` | the browser the endpoint uses cannot capture or act (a provider adapter inherits the defaults), or no browser is wired | use the Local driver, or send no capture or actions |
| `heavy_browser_busy` | 429 `RATE_LIMITED` | another action run holds the browser's heavy lane | retry shortly |
| `scrape_interact_scope_required` | 403 `FORBIDDEN` | actions need a user request, or a service principal with `scrape:interact` in the `on` mode; a request labelled `initiated_by: "agent"` is always refused | send actions from the user's own request |
| `unsupported_in_stream` | 400 `BAD_REQUEST` | a capture or actions on `POST /v1/scrape/stream` | use `POST /v1/scrape` |
| `actions_deadline` | 504 `TIMEOUT` | the run with actions hit its 115 s bound (`details.op` scrape, `deadline_ms` 115000) | send fewer or shorter actions |
| `actions_cancelled` | 504 `TIMEOUT` | the run was cancelled before an action finished | retry |
| `wait_too_long` | 504 `TIMEOUT` inside an action; under `abort` the request answers 502 `action_failed` | one wait is over its limit (30 s by default) | shorten the wait |
| `wait_budget_exhausted` | as `wait_too_long` | the run's total wait is over its budget (60 s by default) | fewer or shorter waits |
| `wait_needs_a_target` | 400 inside an action | a `wait` with neither a time nor a selector | give one |
| `scroll_needs_one_target` | 400 inside an action | a `scroll` with none, or with more than one, of `to`, `selector`, `direction` | give exactly one |
| `attached_runtime` | 400 `BAD_REQUEST` | an internal guard: an attached action runtime never takes a session of its own; a caller should not see it | report it |
| `no_browser_session` | 502 `DRIVER_FAILED` | the run holds no browser session for its actions | retry |
| `action_failed` | 502 `DRIVER_FAILED` | an action failed under `abort`; `details.action_index` and `details.action_type` name it and `details.actions` lists every outcome in order | read `details.actions` (each entry's `error`); earlier clicks or writes may already have happened, so do not replay the list blindly |

Field errors (400, in `errors[]`, not in `details.reason`): `feature_disabled`, `action_denied`, `unknown_action_type`, `invalid_action`, `too_many_actions`, `too_many_captures`, `too_many`, `unsupported_format`, `selector_too_long`, `text_too_long`. Each carries a path and never a value.

## Scrape AI formats (INNH-1)

These come from `POST /v1/scrape` (and the stream, where noted) when a request names `json`, `summary`, `question` or `highlights` in `formats`. The edge answers the first three before it fetches the page. Contract: `docs/CONTRACTS.md` section "Scrape AI formats". The request-level reasons are decided by the edge (INNH-1 U3); the field codes by the contract in `quarry-core` (INNH-1 U1).

| reason | status and code | meaning | what a caller does |
|---|---|---|---|
| `extract_ai_scope_required` | 403 `FORBIDDEN` (`details.field` is `formats[i]`) | a service principal without the scope `extract:ai`, or a request labelled `initiated_by: "agent"`, asked for an AI format. A relayed agent request must not spend model money outside its run budget | send the request from the user's own token |
| `model_plane_unavailable` | 501 `UNSUPPORTED` | AI formats are on but `QUARRY_EDGE__MODEL_PLANE_URL` is unset | the operator sets it; send no AI format meanwhile |
| `model_lane_not_granted` | 501 `UNSUPPORTED` | AI formats are on and the URL is set but this deployment has no usable model credential (the Quarry to Model Plane grant, decision X-7, is not built) | send no AI format |
| `unsupported_in_stream` | 400 `BAD_REQUEST` | an AI format on `POST /v1/scrape/stream` | use `POST /v1/scrape` |

Field errors (400, in `details.fields[]`, not in `details.reason`; each carries a path and never a value): `feature_disabled` (at `formats[i]` while `QUARRY_EDGE__AI_FORMATS` is not on), `too_many` (two of one kind), `missing_field` (a bare `json`, `question` or `highlights`, or an absent `schema`, `question` or `query`), `empty`, `text_too_long` (a prompt or question over 10,000 characters, a query over 1,000), `out_of_range` (`max_chars`, `max_highlights`, `max_chars_each`), `invalid_value` (`language`), `invalid_type`, `unknown_field`, `schema_root_not_object`, `schema_too_large`, `schema_too_complex`, `schema_keyword_not_supported` (at the keyword's path). `reserved_format` no longer exists.

Not errors: a model failure, a rate limit, a schema the output could not satisfy, a ZDR request and an empty page are `data.ai.<kind>` entries inside a 200 (`status`, `code`, `reason`): codes `MODEL_UNAVAILABLE`, `MODEL_RATE_LIMITED`, `SCHEMA_INVALID`, `ZDR_UNAVAILABLE`, `BUDGET_EXHAUSTED`, `EMPTY_SOURCE`; reasons `model_lane_denied`, `model_lane_unauthorized`, `model_rejected_request`, `upstream_unavailable`, `timeout`, `transport`, `bad_reply`, `not_json`, `too_large`, `deadline`.

## 403 `FORBIDDEN`: an action an agent may not take (WS9.B, D2-012)

The agent is the one the verified token gives (a service principal, or a person's token that says `agent`), on the step route, its `interact` alias, the browser socket's `action` message (an error frame with the same `code` and `details`) and the attached delegate lane. Nothing is consumed: no budget step, no approval, no receipt. Not retryable by the same caller.

| reason | `details.denied_action` | meaning | what a caller does |
|---|---|---|---|
| `agent_reload_denied` | `reload` | a page reload proposed by an agent; reload is a person's action | navigate to the URL instead, or leave it to the person |

## `POST /v1/crawl`: `sitemap` and `scrape_options` field errors (INNH-6, 2026-10-31)

400 `BAD_REQUEST`, `details.fields[] {path, code, message}`; the path is the spelling the caller sent, prefixed `scrape_options.` for the object's own fields; a message never echoes a value.

| Path | Code | When |
|---|---|---|
| `sitemap` | `unsupported_option` | `include` or `only` (or any non-`skip` value) while `QUARRY_EDGE__CRAWL_SITEMAP` is off, or while the crawl scope switch is off (`QUARRY_EDGE__CRAWL_SCOPE_MODE=off`) |
| `sitemap` | `invalid_option`, `invalid_type` | flag on: a string other than `skip`, `include`, `only`; not a string |
| `scrape_options` | `unsupported_option` | `QUARRY_EDGE__CRAWL_SCRAPE_OPTIONS` is off (any value) |
| `scrape_options` | `invalid_type` | flag on: not an object |
| `scrape_options.<key>` | `unsupported_option` | `actions`, `actions_on_error`, `max_inline_bytes`, `screenshot`, `pdf`, `json`, `summary`, `question`, `highlights`: they cannot apply to every page |
| `scrape_options.formats[i]` | `unsupported_option`, `unsupported_format` | a capture, object or AI format; an unknown name |
| `scrape_options.<key>` | `reserved_field`, `unknown_field` | a key `/v1/scrape` refuses (`headers`, `proxy`, ...); an unknown key in strict mode (a warning in lenient) |
| `scrape_options.include_tags[i]`, `.exclude_tags[i]` | `empty`, `too_long`, `too_many` | the `/v1/scrape` selector limits (32 selectors, 256 characters) |
| `scrape_options.timeout_ms` | `out_of_range` | not between 1000 and 55000 |
| `scrape_options.extractor` | `unsupported_extractor`, `conflicts_with_only_main_content` | the shared validator's codes |

Terminal events of a crawl: `run_failed` with `code` `sitemap_unavailable` (a `sitemap: only` crawl whose discovery failed) or `edge_scrape_options_unsupported` (the edge did not echo `scrape_applied`). `POST /v1/internal/crawl/sitemap` answers 501 `UNSUPPORTED` while `QUARRY_EDGE__CRAWL_SITEMAP` is off; `run_page` answers 400 with `details.fields` under `scrape.` for options that do not pass the same checks.

## `POST /v1/parse` (INNH-11, 2026-10-31)

Standard envelope, `error.details.reason` as below. A message never echoes document text.

| Status | Code | `details.reason` | When | What a caller does |
|---|---|---|---|---|
| 400 | `BAD_REQUEST` | `empty_body` | the body has no bytes | send the document |
| 400 | `BAD_REQUEST` | `body_unreadable` | the body stream failed | retry |
| 413 | `BAD_REQUEST` | `too_large` (`max_bytes`) | the body is over 25 MiB (or the lower `QUARRY_EDGE__PARSE_MAX_BYTES`), by its declared length or its bytes | send a smaller file |
| 415 | `UNSUPPORTED_MEDIA` | `unsupported_media` (`detected`, `supported`) | the bytes are not a PDF or DOCX, or the `Content-Type` is not one the route takes (`supported_content_types`) | send a PDF or DOCX |
| 415 | `UNSUPPORTED_MEDIA` | `content_type_mismatch` (`detected`, `declared`) | the declared PDF or DOCX type names the other document than the bytes | fix the header or send the bytes you meant |
| 415 | `UNSUPPORTED_MEDIA` | `pdf_parse_failed`, `docx_parse_failed` (`kind`, maybe `limit: parser_stopped`) | the file is damaged, or the parser was stopped by its memory or CPU limit | the file cannot be read; do not retry as is |
| 415 | `UNSUPPORTED_MEDIA` | `too_large` (`kind`) | the parser's own size or inflate limit | send a smaller file |
| 429 | `RATE_LIMITED` | `org_parse_busy`, `parse_capacity`, `parser_busy` (`retry_after_seconds`) | the organisation, the process, or the parser slots are busy | wait `Retry-After` seconds |
| 501 | `UNSUPPORTED` | `parse_route_disabled` (`env`) | `QUARRY_EDGE__PARSE_ROUTE` is off | ask the operator |
| 501 | `UNSUPPORTED` | `isolation_required` (`env`, `hint`) | documents would be parsed inside the edge process | ask the operator |
| 504 | `TIMEOUT` | none | the parse took more than 15 seconds | the file is too slow to read; do not retry as is |

A 200 may carry `warnings[]` (`document_has_no_text_layer`, `some_pages_have_no_text_layer`, `document_has_no_text`, `truncated_at_8mib`): they are not errors.
