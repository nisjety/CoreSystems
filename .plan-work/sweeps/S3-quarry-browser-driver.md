# S3 quarry-browser-driver

Sweep S3, Haiku code map. Date 2026-10-14. Read-only. Code read from the live tree `C:/dev/CoresSystem` (HEAD afb83928, 324 uncommitted entries). Paths are repo-relative. Tags: [C] committed and unchanged in the live tree; [M] committed but modified in the live tree (uncommitted hunks checked by line range); [U] untracked; [NEW] a file a plan would create; [WRONG-CITATION] path does not exist; [EXT] outside the repo (cargo registry copy of chromiumoxide 0.9.1). Verdicts: OK, DRIFTED (corrected range), FALSE, PARTLY FALSE, WRONG RANGE, MISSING.

## Cross-item overlaps

1. `quarry-browser/src/chromiumoxide.rs` [C]: 007 (ensure_browser 194-271, acquire 1914, goto 1988), 015 (execute_native_target_action 1232-1330, click_point 2841, press 2989), 020 (capture_snapshot 1771, persist_current_page 675, restore 512-532), 034 (native_page_projection 2681, lifecycle listener 640, back/forward sleeps 3050-3062), 021 (native_projection_from_cdp 1089, redact_egress_url 1053).
2. `quarry-runtime/src/observation.rs` [C]: 015 (atomic gate 364-371, outcome_for_action 217-266), 021 (build_active_snapshot 1573, artifact writes 700-790, put_artifact_if_allowed 1428), 027 (classify_challenge 158, challenge counters 903-913, BrowserObservation 965-996), 034 (settle insertion point 576-650, 50-target cap 1598, text_snippet 2140).
3. `quarry-core/src/contracts.rs` [C]: 015 (AgentAction Click 644, ClickRef 650, Press 699), 021 (Type 675, TypeRef 679-692), 027 (ChallengeKind/Signal 236-255, BrowserTelemetry 100-124), 034 (DomSummary 346-360, BrowserSnapshot 366-384, AccessibilityProjection 389-395).
4. `apps/Model Plane/rust/services/execution-core/src/quarry_agent.rs` [C] (observation_from_wire 407-474) and `browser_agent.rs` [C] (BrowserObservation 170-186): 021, 027, 034 all change this wire mapping.
5. `execution-core/src/llm_planner.rs` [C] (prompt 200-260; raw extracted_text at 229; targets take(40) at 212): 021, 027, 034.
6. `apps/Model Plane/rust/services/model-gateway/src/http_routes.rs` [M] (prompt const 2013-2021; browser_suggest_action 2023-; budgets 1987-1989; visual summary 2085-2160): 027, 034; live hunks at 2032-2036 and 2106-2112 are uncommitted ZDR gates in the same two routes.
7. `quarry-core/src/error.rs` [C] (TargetRepairRequired 30 and 409 mapping at 50; ChallengeDetected 23 and 403 mapping at 49): 015, 027.
8. `quarry-runtime/src/lib.rs` [M] (redact_sensitive export 101; S3ProfileStore export 166; hunks at 44 and 159-164): 020, 021.
9. `quarry-edge/src/main.rs` [M], `routes.rs` [M], `state.rs` [M], `config.rs` [M] (pools 323/337, agent_driver 1284-1288, /health 309, profile routes 135-157, store wiring 545-600, durability 694-708): 007, 020.
10. `quarry-edge/src/agent_routes.rs` [C] and `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/browser.rs` [C] (viewport 463 and 489-492; reject_zdr at 174 and 472; ZDR refusal at agent_routes 527-533 and 1470-1476): 007, 020.

## Method

Every cited range was opened and read in the live tree. Where a range was only anchored by grep (flagged as anchor-only), the stated function or symbol was located by line. The per-item cap of 25 was respected by checking the load-bearing ranges in full; three low-value citations are anchor-only and are named in their section. Provenance came from `tools/provenance.sh`. Chromiumoxide 0.9.1 source was read from the cargo registry [EXT].

---

## F-t2-007 Stop viewport-triggered browser relaunches: per-page emulation, a small Chromium process pool and page hygiene

Card header: Priority, Partial, effort M, Ingestion Plane, wave W1, confidence High.

### 1. Citation check

| # | Citation (source) | Tag | Verdict | Finding |
|---|---|---|---|---|
| 1 | chromiumoxide.rs:114-126 (card); 100-135 (JSON) | C | OK | Struct `ChromiumoxideDriver` at 116; `browser` at 117; `active_viewport` at 120. |
| 2 | chromiumoxide.rs:194-270 (card); 185-270 (JSON) | C | DRIFTED | `ensure_browser` is 194-271. Relaunch guard 197-199; close 200-204; clears pages, egress_receipts, egress_sequence, egress_policies, proxy_receipt_cursors, session_telemetry, dialogs, dialog_pages at 208-216; viewport applied at launch 244-246. |
| 3 | chromiumoxide.rs:244-246 (card) | C | OK | `builder.viewport(...)` only in the launch path. |
| 4 | chromiumoxide.rs:236-243 (card) | C | OK | `--disable-quic` added when a pinned egress proxy is set (237-243). |
| 5 | chromiumoxide.rs:366-385 (JSON); 378 (card) | C | OK | `open_tab_page` at 371; calls `ensure_browser(session.lease.viewport)` at 378. |
| 6 | chromiumoxide.rs:1914-1926 (card); 1914-1995 (JSON) | C | OK | `acquire` 1914; Warm/Cold computed 1916-1924; `ensure_browser` at 1925. |
| 7 | chromiumoxide.rs:1991 (card) | C | OK | `goto` at 1988; `ensure_browser` at 1991, so every navigation re-checks the viewport. |
| 8 | chromiumoxide.rs:1851-1860 (JSON) | C | OK | `chromium_viewport` maps width, height, DSF, is_mobile to launch-time emulation flags. |
| 9 | "no Emulation, focus or lifecycle CDP call in the crate" (card; JSON grep) | C | OK | Grep over quarry crates for Emulation, setDeviceMetricsOverride, setFocusEmulation, WebLifecycle finds only `wreq::Emulation` in quarry-tls (TLS fingerprint, unrelated). |
| 10 | agent_routes.rs:1512-1513 (card); 1511-1515 | C | OK | `state.agent_driver.acquire(&lease)`; no pool, semaphore or reaper in front of it. |
| 11 | agent_routes.rs:1436-1456 (JSON); 1447-1453 | C | OK | Viewport is `Option` and goes through `normalize_viewport` (1897-1915), which keeps `None` as `None`, so None and Some(1280x800) differ. |
| 12 | agent_routes.rs:1478-1530 (JSON) | C | OK | Lease built 1487-1508 (`viewport` at 1506); acquire 1511; egress policy 1521-1527; release on error 1529. |
| 13 | routes.rs:309 (card); 300-320 (JSON) | M | OK | `async fn health() -> &'static str { "ok" }` at 309-311. `ready` (312) checks `readiness.durable` only. Uncommitted hunk in this file is one test-only line (1330); 309 is untouched. |
| 14 | main.rs:1285 (JSON); 1278-1292 | M | OK | `agent_driver` built at 1284-1288 with `with_profile_store`. Outside all uncommitted hunks. |
| 15 | main.rs:323-339 (JSON) | M | OK | Browserbase `RuntimeLeasePool::new(4)` at 323; `drivers` registry branch 331-340 with `ChromiumoxideDriver::new()` at 334 and `RuntimeLeasePool::new(2)` at 337. Outside uncommitted hunks. |
| 16 | lease.rs:19-22 (card); 8-33 (JSON) | C | OK | Doc says launch-only drivers relaunch when viewport differs. `viewport: Option<BrowserViewport>` at 23-24. |
| 17 | pool.rs (card); pool.rs:1-80 (JSON) | C | OK for shape; WRONG for use | Header 1-11 describes affinity TTL plus semaphore. `LeasePool<D>` at 32-38 has no production caller (section 3). |
| 18 | gateway browser.rs:481-489 (card) | C | DRIFTED, OVERSTATED | Forwarding is at 463 (`body.viewport.unwrap_or(DEFAULT_VIEWPORT)`) and 489-492 (start body). `DEFAULT_VIEWPORT` is 1280x800 at 345-348. A client-supplied viewport overrides the default. The card's "always sends 1280x800" is only true for the SPA. |
| 19 | gateway browser.rs:455-492 (JSON) | C | OK | `create_session` 451; viewport 463; constraints `max_steps: 12` at 485. |
| 20 | KnowledgeComposer.tsx:813-818 (JSON); 817 | M | OK | Literal `viewport: { width: 1280, height: 800 }` at 817. Uncommitted hunks are at 542, 1172 and 1755-1757, so 817 is unchanged. |

Anchor-only: none. Every row above was read.

### 2. Affected files

- `apps/Ingestion Plane/Quarry-v2/crates/quarry-browser/src/chromiumoxide.rs` [C], 3551 lines. Part A: remove viewport from `ensure_browser` identity (194-271), apply `Emulation.setDeviceMetricsOverride` and the focus and lifecycle calls on attach in `open_tab_page` (371-) and `install_devtools_collectors` (575-). Part B moves `browser` (117), `pages` (118), `handler` (119) and `live_frame_cache` (122, struct `CachedLiveFrame` at 80) into slot state. Restore path at 512-532 (cookies only after navigation, then reload). Call sites of `ensure_browser`: 378, 1925, 1991. `kernel.rs` has its own unrelated `ensure_browser` (233).
- `apps/Ingestion Plane/Quarry-v2/crates/quarry-browser/src/lib.rs` [C], 676 lines. `BrowserDriverCapabilities` 269-286 has no viewport flag. Trait defaults 548-606.
- `apps/Ingestion Plane/Quarry-v2/crates/quarry-core/src/lease.rs` [C], 60 lines. Doc at 19-22 is the only text to update. No contract change needed.
- `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/agent_routes.rs` [C], 3737 lines. Viewport normalization 1447-1453 and `normalize_viewport` 1897-1915. Lease 1487-1508. Acquire 1511-1515. Egress policy 1521-1527. Release 1529, 1542, 1552, 1560, 1618, 1630. Live frame via `agent_driver` at 2993. An admission cap or idle reaper for the agent path would go here, using the TTL already set at 1446 and 1502.
- `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/main.rs` [M], 1569 lines. Registration 331-340 and `agent_driver` 1284-1288 both lie outside uncommitted hunks (hunks at 244-253, 984-1011, 1061-1200, 1232-1240, 1395-1398, 1463). Pool sizes are literals at 323 and 337.
- `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/state.rs` [M]. `pub agent_driver` at 142, outside hunks (57-93).
- `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/routes.rs` [M], 1564 lines. `health` 309-311. `ready` 312.
- `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/config.rs` [M], 167 insertions and 20 deletions uncommitted. No browser pool field exists. "Size the lease pool from config" needs a new `EdgeConfig` field. The file is a merge hotspot.
- `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/test_support.rs` [M] (141) and `tests/support/mod.rs` [M] (86): fixtures that construct `ChromiumoxideDriver::new()`.
- `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/browser.rs` [C], 6317 lines. `DEFAULT_VIEWPORT` 345-348; forwarding 463 and 489-492. Card says no change needed.
- `apps/Frontend Plane/verevonv3/src/features/dashboard/home/KnowledgeComposer.tsx` [M], 1999 lines. Viewport literal 817. Card says no change needed.
- `apps/Ingestion Plane/Quarry-v2/docs/SOVEREIGN_SCRAPING_RELIABILITY.md` [C], 330 lines. Phase 5 is cited by the JSON as already planned.
- No migration. Part B may add a slot module [NEW], for example `quarry-browser/src/<slots>.rs`; the design choice is not made here.

Call sites of `ChromiumoxideDriver::new()`: main.rs:334 and 1285; routes.rs:1344 (test); test_support.rs:141; tests/support/mod.rs:86; `quarry-browser/tests/ssrf.rs` [C] 43, 78, 106, 134, 162; chromiumoxide.rs tests 3260, 3337, 3365, 3442, 3464, 3521; `quarry-runtime/src/browser_egress_proxy.rs` [C] 713 (test); `quarry-runtime/tests/browser_egress_boundary.rs` [C] 93.

### 3. Reusable but unwired

- `quarry-browser/src/pool.rs:32-38` `LeasePool<D>` [C]: UNWIRED. `grep LeasePool` in the crates returns only pool.rs lines 3, 32, 38, 114 and tests 210-271. The JSON reuse item "LeasePool semaphore plus affinity TTL for the scrape path" points at unwired code. The wired scrape pool is `quarry-runtime/src/lease_pool.rs:34` `RuntimeLeasePool`, constructed at main.rs:323 and 337.
- `quarry-browser/src/persistent_session.rs:81` `PersistentSessionRegistry` [C]: UNWIRED. Re-exported at lib.rs:168. Only its own tests (191-232) use it. Its idle-timer design (header 1-12) is a reference for an idle reaper, but it targets Browserless and direct CDP connections.
- `quarry-runtime/src/lease_pool.rs:34` `RuntimeLeasePool` [C]: WIRED (main.rs:323, 337).
- `chromiumoxide.rs` `CachedLiveFrame` 80 and `live_frame_cache` 122: WIRED through the live frame path (1702-1770, 2405).
- `BrowserLease.ttl_s` set at agent_routes.rs:1446 and 1502: set, but no reaper reading it was found [UNVERIFIED: grep for ttl_s consumers was not run].
- `kernel.rs:651-662` `KernelDriver::click_point` [C] uses `document.elementFromPoint` in page JS (not a viewport pattern; noted for F-t2-015).

### 4. Tests

- `quarry-browser/src/chromiumoxide.rs` `mod tests` [C], roughly 3115-3551. Pure tests: `chromium_viewport_maps_desktop_dimensions` (3143), `session_viewport_preserves_persisted_profile_dimensions` (3160), `live_frame_options_are_normalized` (3175), `blocks_loopback_subresources_before_they_reach_the_server` (3252), `loopback_egress_is_proven_in_quarry_runtime` (3315). Browser-backed tests return early unless `CHROMIUMOXIDE_TEST=1` (3332): `launches_and_fetches_content` (3331), `captures_live_frame_after_navigation` (3359), `reopening_a_session_with_a_saved_profile_restores_cookies_without_erroring` (3405), `distinct_sessions_do_not_share_cookies` (3458), `second_tab_on_same_session_shares_that_sessions_cookies` (3515).
- No existing test covers two leases with different viewports on one driver (the card's acceptance test). Checked by reading the test names in the module above.
- `quarry-browser/tests/ssrf.rs` [C]: 5 tests marked `#[ignore]` (38, 73, 101, 129, 157); need Chromium.
- `quarry-runtime/src/lease_pool.rs` [C]: 4 tests, including `RuntimeLeasePool::new(2)` and `new(1)` at 241 and 250.
- `quarry-edge/tests/zdr_behavior.rs` [C]: four agent-run tests (67, 95, 132, 179) that run through the `agent_driver` fixture.
- `quarry-edge/src/agent_routes.rs` [C]: 16 test attributes.
- Commands: `cd "apps/Ingestion Plane/Quarry-v2" && cargo test -p quarry-browser --features chromiumoxide` (same as CI). For live Chromium: `CHROMIUMOXIDE_TEST=1 cargo test -p quarry-browser --features chromiumoxide -- --test-threads=1`. `cargo test -p quarry-runtime lease_pool`. `cargo test -p quarry-edge --test zdr_behavior`.
- CI: `.github/workflows/quarry-v2-ci.yml` [C], job "Cargo check + test (workspace)" lines 146-211: `cargo test --workspace` (205) and `cargo test -p quarry-browser --features chromiumoxide` (211). `CHROMIUMOXIDE_TEST` is not set anywhere in that workflow, so the browser-backed tests are no-ops in CI. No Postgres or Chromium service container exists in it.

### 5. Feature flags and env

- Cargo: quarry-browser `chromiumoxide` feature (Cargo.toml:27 optional dependency, 36 feature). Default features are `["kernel"]` (35). `pub mod chromiumoxide` is gated at lib.rs:156-157.
- quarry-edge default feature `browser-agent` = `quarry-browser/chromiumoxide` (quarry-edge Cargo.toml:73-74), so the driver is compiled in default edge builds. main.rs gates at 331-340 and 1283-1290.
- quarry-runtime feature `chromiumoxide = ["quarry-browser/chromiumoxide"]` (Cargo.toml:64).
- `QUARRY_BROWSER_NO_SANDBOX` (chromiumoxide.rs:247, `env_truthy`, default off).
- `QUARRY_BROWSER_PROFILE_ROOT` (chromiumoxide.rs:889, launch dirs).
- `CHROMIUMOXIDE_TEST` (test-only gate, 3332).
- Pool sizes: no env or config. `RuntimeLeasePool::new(4)` at main.rs:323 (Browserbase) and `new(2)` at 337 (local chromiumoxide, `drivers` registry).
- Pinned egress proxy: `PinnedBrowserEgressProxy::new()` at main.rs:265, passed through `with_pinned_egress_proxy` at 335 and 1286. When set, `--disable-quic` (chromiumoxide.rs:242) and `isolated_egress` and `security_evidence` are true (1887-1888).
- Run TTL: `constraints.max_runtime_s.unwrap_or(120)` at agent_routes.rs:1446 becomes the lease `ttl_s`.
- Viewport bounds: `normalize_viewport` accepts 320x240 to 3840x2160 (agent_routes.rs:1903-1904). Gateway default 1280x800 (browser.rs:345-348).

### 6. Surprises

- Two `ChromiumoxideDriver` instances run in production. The `drivers` registry instance (main.rs:332-340, pool 2) serves the DriverKind::Browser path. The `agent_driver` instance (main.rs:1284-1288) serves every agent route. The in-app UI session posts to `/v1/agent/runs` (gateway browser.rs:503-506), so UI sessions and Model Plane runs share `agent_driver`. That is where the relaunch collision happens. The pool of two is not on that path, so the card's "size the pool from config" does not address the agent path (the JSON already says this).
- `ensure_browser` clears more driver-global state than the card lists: `egress_sequence` (211), `egress_policies` (212), `proxy_receipt_cursors` (213) and `dialog_pages` (216). Part B re-keying must cover these.
- The gateway viewport claim is overstated (row 18).
- The JSON reuse of `LeasePool` is wrong: the code is unwired (section 3).
- The driver crate is [C]. None of chromiumoxide.rs, lib.rs, pool.rs or stealth.rs has uncommitted edits, so the relaunch fix does not collide with in-flight work. The collisions are in quarry-edge main.rs, config.rs and state.rs, which the pool-size config would edit.
- Relaunch frequency in production is unverified (the card says so; no metrics were read).
- Nothing currently opens a page as a health probe. `/ready` (routes.rs:312) checks only `readiness.durable`.

---

## F-t2-015 Actionability checks and trusted input for every ref-targeted action, then rich input actions

Card header: Priority, Partial, effort M, Ingestion Plane, wave W2, confidence High.

### 1. Citation check

| # | Citation (source) | Tag | Verdict | Finding |
|---|---|---|---|---|
| 1 | chromiumoxide.rs:1232-1330 (card) | C | OK | `execute_native_target_action` 1232. Frame context lookup 1241-1261. Backend resolve 1262-1273. Check is only `element.isConnected` (1292-1294). `element.click()` (1297). Type setter and synthetic input events (1299-1313). Select (1314-1325). Native wait_for (1326-1330). |
| 2 | chromiumoxide.rs:2026-2093 (card) | C | OK | `act_on_verified_target` 2026. Non-native path is a page script (2041-2083). Uniqueness check 2050. `element.click()` at 2053. No visibility, scroll or hit test. |
| 3 | chromiumoxide.rs:2841 and 2841-2896 (card, JSON) | C | OK | `click_point` 2841: moved, pressed, released via `page.execute` (2883-2894). Trusted CDP mouse input. |
| 4 | contracts.rs:699 (card); 695-705 (JSON) | C | OK | `Press { key: String }` at 699-701. No modifiers. |
| 5 | error.rs:30 (card); 8-60 (JSON) | C | OK | `TargetRepairRequired` at 30. Maps to HTTP 409 at line 50. |
| 6 | lib.rs:269-286 (card, JSON) | C | OK | `BrowserDriverCapabilities` 269-286, including `atomic_target_actions` at 285. |
| 7 | chromiumoxide.rs:1232-1290 (card) | C | OK | Backend-node resolution and per-frame execution-context lookup. |
| 8 | observation.rs:225-262 (JSON); 217-266 | C | OK | `outcome_for_action` 217. Click, Type, Press, Select and related actions return `ActionOutcome::unknown("postcondition_required")` (259-263). |
| 9 | chromiumoxide.rs:2284 (JSON) | C | OK | `download_from_verified_target` clicks via `execute_native_target_action` (2284). Only download uses it. |
| 10 | chromiumoxide.rs:2828-2915 (JSON) | C | PARTLY FALSE | The card says only `click_point` dispatches trusted events. The selector `click` (2828-2839) calls chromiumoxide `Element::click` (element.rs:269-273: scroll_into_view, clickable_point, `tab.click`), which is trusted CDP input [EXT]. `type_text` is 2898. |
| 11 | chromiumoxide.rs:2989-3001 (JSON) | C | OK with nuance | `press` finds `body`, then `Element::press_key` (element.rs:320) [EXT]. Trusted key events, page-global. |
| 12 | lib.rs:304-315 and 398-405 (JSON) | C | OK | `BrowserNativeTarget` 304-315. `VerifiedTargetOperation` 398-405 (Click, Type, Select, WaitFor). |
| 13 | contracts.rs:640-760 (JSON) | C | OK | `AgentAction` enum at 640. Click 644, ClickRef 650, Type 675, TypeRef 679, Press 699. |
| 14 | runtime_loop/agent.rs:3479-3512 (card, JSON) | M | OK | `browser_act_parameters_json` 3479-3508: a `oneOf` of 14 ref-only actions with `additionalProperties: false`. No press, click_point or navigate. Uncommitted hunks are at 571, 599, 749, 769, 1937, 4566 and in tests from 7176, so 3479-3508 is unchanged. |
| 15 | "upload and download reuse the same native click helper" (JSON) | C | PARTLY FALSE | Only download (2284) calls `execute_native_target_action`. `upload_staged_file_to_target` (2102-2210) does not. |
| 16 | "no hit-test on the ref path" (card) | C | OK | Grep for elementFromPoint, getNodeForLocation, getContentQuads, getBoxModel, scrollIntoView, dispatchKeyEvent in quarry-browser finds none in chromiumoxide.rs. Only kernel.rs:656 and 729 use `elementFromPoint`. |

Anchor-only: `contracts.rs:640-760` (row 13) was checked by variant anchors, not a full read.

### 2. Affected files

- `apps/Ingestion Plane/Quarry-v2/crates/quarry-browser/src/chromiumoxide.rs` [C], 3551 lines. `execute_native_target_action` 1232-1375 (callers 2033 and 2284). `act_on_verified_target` 2026-2101. `click` (selector) 2828. `click_point` 2841-2896. `type_text` 2898. `press` 2989-3001. `download_from_verified_target` 2211. `upload_staged_file_to_target` 2102. `capabilities()` 1873-1891 (`atomic_target_actions: true` at 1889).
- `apps/Ingestion Plane/Quarry-v2/crates/quarry-browser/src/lib.rs` [C], 676 lines. Add a capability flag at 269-286. Trait defaults `click_point` 600-602 and others return Unsupported.
- `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/observation.rs` [C], 3214 lines. Atomic gate 364-371 (`action_requires_atomic_target` 1493-1513). Dispatch: `click_point` at 418 and `press` at 424. `resolve_snapshot_action` 1019.
- `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/action_runtime.rs` [C], 541 lines. Second executor: `click_point` at 305 and `press` at 374. Confirm which path it serves before changing press or click semantics.
- `apps/Ingestion Plane/Quarry-v2/crates/quarry-core/src/contracts.rs` [C], 973 lines. `AgentAction` 640-710. New actions (press with modifiers, drag, select_text, paste) would go here. Rich input is absent: grep for modifiers, drag, paste, clipboard, select_text returns nothing.
- `apps/Ingestion Plane/Quarry-v2/crates/quarry-core/src/error.rs` [C], 94 lines. `TargetRepairRequired` 30 and 409 mapping at 50. The JSON approach keeps the wire enum and uses `details.reason`.
- `apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs` [M], 13521 lines. `browser_act_parameters_json` 3479-3508 is a closed list. Changing it is a Model Plane contract change.
- `apps/Frontend Plane/verevonv3/src/shared/api/browser-client.ts` [C], 566 lines. `BrowserAction` union 3-17 (press has only `key`).
- `apps/Frontend Plane/verevonv3/src/shared/actions/action-registry.ts` [C]. Browser action definition near line 3060.
- Downstream adapters: `kernel.rs` [C] (`click_point` 651-662, synthetic click via elementFromPoint and `target.click()`), Browserbase and Browserless (inherit Unsupported defaults).
- No migration. No new error code (per the JSON approach). [NEW] files only if the pipeline gets its own module.

### 3. Reusable but unwired

- `BrowserDriverCapabilities.atomic_target_actions` (lib.rs:285): WIRED. Set true at chromiumoxide.rs:1889, enforced at observation.rs:364-365, and checked at agent_routes.rs:1206-1207 and 1243. The JSON proposes a new flag; the existing one already gates ref actions and may be the right gate, or a sibling flag may be cleaner.
- `click_point` and `press`: WIRED (observation.rs:418, 424; action_runtime.rs:305, 374).
- Frame execution-context lookup (chromiumoxide.rs 1241-1261): WIRED through 2033 and 2284.
- `ActionOutcome::unknown` (observation.rs 238-263): WIRED. Keeps Quarry from claiming a business effect.
- `ErrorCode::TargetRepairRequired` and helper `target_repair` (observation.rs:1569): WIRED.
- Hit-test helpers: none in the Rust driver. Kernel's JS `elementFromPoint` (kernel.rs:656, 729) is the only hit-test pattern in the crate family. It is a remote-provider path, not reusable as is.
- Chromiumoxide `Element::click` (element.rs:269-273) and `press_key` (320): trusted, already used by the selector path [EXT]. The card's "only click_point is trusted" is wrong.

### 4. Tests

- `quarry-runtime/src/observation.rs` tests [C] (25 test attributes): `changed_target_fingerprint_blocks_snapshot_action_before_click` (2874), `effectful_actions_are_unknown_without_a_postcondition` (2481), `snapshot_uses_opaque_refs_and_semantic_resolution_is_deterministic` (2749), `ambiguous_semantic_locator_requires_explicit_ref` (2779), `click_map_is_stable_dense_and_aligned_with_snapshot_refs` (2670), `agent_action_converts_to_browser_action` (2996).
- `quarry-browser/src/chromiumoxide.rs` tests [C]: none exercise click or press. Browser-backed tests need `CHROMIUMOXIDE_TEST=1`.
- `quarry-edge/src/agent_routes.rs` tests [C] (16) and `quarry-edge/tests/zdr_behavior.rs` [C].
- Model Plane: `execution-core/src/llm_planner.rs` [C] (31 test attributes; `planner_request_carries_the_strict_schema_and_the_observation_refs` at 1050); `execution-core/tests/browser_agent_e2e.rs` [C] (13; the one live-stack test per the CI comment).
- Commands: `cd "apps/Ingestion Plane/Quarry-v2" && cargo test -p quarry-runtime observation` and `cargo test -p quarry-browser --features chromiumoxide`. Model Plane: `cd "apps/Model Plane/rust" && cargo test -p execution-core llm_planner`.
- CI: `.github/workflows/quarry-v2-ci.yml` [C] lines 205 and 211; `.github/workflows/model-plane-rust-ci.yml` [M] line 119 (`cargo test --workspace --no-fail-fast`).

### 5. Feature flags and env

- Driver capability flags (lib.rs:269-286). Chromiumoxide sets `isolated_egress` and `security_evidence` from the pinned proxy (chromiumoxide.rs:1887-1888).
- No env switch exists for an actionability pipeline. A new capability flag would be the gate (the JSON proposes it).
- `QUARRY_BROWSER_NO_SANDBOX` (chromiumoxide.rs:247) and `CHROMIUMOXIDE_TEST` (test gate, 3332).
- ZDR: none specific. The ZDR guard sits at run creation (agent_routes.rs:527-533, 1470-1476).

### 6. Surprises

- `atomic_target_actions` already exists and gates the ref actions (observation.rs:364-371, `action_requires_atomic_target` 1493-1513, which covers ClickRef, TypeRef, SelectRef, WaitForRef, Upload and Download refs but not Press or ClickPoint). The plan should decide whether the trusted pipeline reuses this flag.
- The card's "only click_point is trusted" is wrong. Selector click and press are trusted through chromiumoxide [EXT]. The ref path is the only synthetic one.
- Upload does not use the click helper (JSON partly wrong).
- `action_runtime.rs` [C] is a second executor calling `click_point` and `press`. Changing press or click semantics in the driver could change behaviour there too.
- Kernel's `click_point` is a synthetic dispatch (kernel.rs:651-662), so the Kernel adapter would not gain trusted input without its own change.
- `AgentAction::Press` (contracts.rs:699) has no modifiers, and the SPA `BrowserAction` (browser-client.ts:8) mirrors the same limit. Rich input therefore needs a contract change in both the Model Plane schema and the SPA.
- The quarry-browser files (chromiumoxide.rs, lib.rs, pool.rs, stealth.rs) and observation.rs are clean in the live tree, so the pipeline does not collide with in-flight work; the Model Plane agent.rs hunks are elsewhere in the file.

---

## F-t2-021 Secret redaction at capture time: AX/DOM snapshots, screenshots, OCR, receipts and logs, before storage or model exposure

Card header: Priority, Partial, effort L, Ingestion Plane plus Model Plane, wave W2, confidence Medium.

### 1. Citation check

| # | Citation (source) | Tag | Verdict | Finding |
|---|---|---|---|---|
| 1 | chromiumoxide.rs:1096-1130 (card); 1099-1131 | C | DRIFTED (minor) | Node loop 1099-1131. Verbatim copies at 1107 (name) and 1108 (value) via `ax_value_text`. |
| 2 | chromiumoxide.rs:1104-1110 (JSON) | C | OK | `AccessibilityNode { node_id, role, name, value, ignored, ... }` built 1104-1116. |
| 3 | chromiumoxide.rs:1053-1066 (card, JSON) | C | OK | `redact_egress_url` 1053-1065 keeps scheme, host, port and path. Drops query and fragment. Egress URLs only. |
| 4 | observation.rs:1573-1640 (card) | C | OK | `build_active_snapshot` 1573-1688. Target text is `name.or(value)` at 1607. Native targets capped at 50 (1598). |
| 5 | observation.rs:1979-2100 (JSON) | C | OK | `build_dom_summary` 1979-2159. Attribute extraction 1993-2041 (id, name, placeholder, aria-label, data-testid, role). No `value` extraction. |
| 6 | observation.rs:700-790 (card, JSON) | C | OK | Artifact writes: screenshot_annotated 697-702, page_image_clean 705-718, thumbnail 719-732, tiles 733-746, ocr_preprocessed 747-760, logo 761-774, rendered_palette 775-788. All through the ZDR-gated writers. |
| 7 | (not cited) observation.rs:1428-1446 | C | Gate confirmed | `put_artifact_if_allowed` gates only on `zdr::guard(zdr_mode, WriteKind::Artifact)` (1436). No redaction. |
| 8 | (not cited) observation.rs:653-670 | C | Gate confirmed | A full-viewport PNG is captured on every observation, even without a Screenshot action. |
| 9 | agent_memory.rs:52 (card); 52-70 | C | OK | `redact_sensitive`: emails, long tokens, card-like digit runs. Not called from production code. |
| 10 | agent_memory.rs:1-70 (JSON) | C | OK | `AgentScratchpad` 14-47; `note()` calls redact at 32. Tests 110-157. |
| 11 | quarry-runtime lib.rs:101 (JSON) | M | OK | `pub use agent_memory::{redact_sensitive, AgentScratchpad};` at 101. Uncommitted hunks are at 44 and 159-164. |
| 12 | step_receipts.rs:190-206 (card); 183-210 | C | OK | `StepReceipt` 183. `action: AgentAction` at 198. `observation: Option<BrowserObservation>` at 202-203. |
| 13 | step_receipts.rs:100-125 (JSON) | C | WRONG RANGE | 96-125 are `BrowserProfileScope` and `BrowserTimelineEvent`, not receipt persistence. The timeline doc says it excludes credentials. |
| 14 | contracts.rs:675-692 | C | OK | `Type` (675) and `TypeRef` (679-692) carry `text`, which receipts store verbatim. |
| 15 | scrub.rs:1-60 (card, JSON) | C | OK | Header 1-19. `sensitive_key_re` 28-36. `string_patterns` 47-60: Bearer, JWT, sk-, AKIA, GitHub tokens and more. |
| 16 | grpc.rs:1015-1042 (card, JSON) | M | OK | `scrub_string` on output and error at 1015-1016; `scrub_json_value` on checkpoint at 1042. Uncommitted hunks at 961-964 and 1099, so this range is unchanged. |
| 17 | llm_planner.rs:170-263 (card); 200-235 (JSON) | C | OK | `planner_infer_request` 200. `observation_text` 206-232 includes raw `o.extracted_text` (229). Targets `take(40)` (212). |
| 18 | browser_agent.rs: no password, secret or redact matches (JSON) | C | OK | Grep confirms none in browser_agent.rs. Matches appear only in browser_risk.rs. |
| 19 | browser_agent/browser_risk.rs (JSON) | C | OK | `input[type=password]` appears at 599 and 1220 in the risk classifier. A detector, not redaction. |
| 20 | model-gateway moderation.rs (JSON) | C | OK | `pub fn redact_pii` at 586. Wired for chat content only (http_routes.rs:8969, 9219; sse.rs:1089, 1547). |

Anchor-only: rows 9 and 10 were confirmed by reading the function bodies, not the test bodies.

### 2. Affected files

- `apps/Ingestion Plane/Quarry-v2/crates/quarry-browser/src/chromiumoxide.rs` [C]. `redact_egress_url` 1053. `native_projection_from_cdp` 1089 (the redaction hook). `native_page_projection` 2681 (caller of the projection, 2729). `capture_snapshot` 1771-1832 (cookies and storage; the snapshot builder for profiles, not for AX).
- `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/observation.rs` [C]. `build_active_snapshot` 1573 (called at 628). `build_dom_summary` 1979 (called at 616, 1185). `execute` 346-1018 (artifact writers 653-788, receipts and observation at 965-996). `put_artifact_if_allowed` 1428 and `put_json_artifact_if_allowed` 1448 (21 call and definition lines in the file).
- `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/step_receipts.rs` [C], 818 lines. `StepReceipt` 183-210 and constructors 498-540.
- `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/postgres_step_receipts.rs` [C] (receipt store; the writer function was not identified [UNVERIFIED]).
- `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/agent_memory.rs` [C], 157 lines. Redaction seed.
- `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/lib.rs` [M], export at 101.
- `apps/Ingestion Plane/Quarry-v2/crates/quarry-core/src/contracts.rs` [C] (`Type` 675, `TypeRef` 679).
- `apps/Model Plane/rust/services/execution-core/src/scrub.rs` [C], 376 lines. Pattern source.
- `apps/Model Plane/rust/services/execution-core/src/llm_planner.rs` [C], 1064 lines. Prompt builder (`observation_text` 206-232).
- `apps/Model Plane/rust/services/execution-core/src/grpc.rs` [M], 2121 lines. Scrub call sites 1015-1042.
- `apps/Model Plane/rust/services/execution-core/src/quarry_agent.rs` [C] (`observation_from_wire` 407-474, the place where extracted_text enters the Model Plane).
- `apps/Model Plane/rust/services/execution-core/src/executor.rs` [C], `workspace_hydrate.rs` [C] (148), `process_host.rs` [C] (873): other scrub callers.
- `apps/Model Plane/rust/services/model-gateway/src/moderation.rs` [C], 1265 lines. `redact_pii` 586.
- Migration: none needed for capture-time redaction. Any persisted redaction marker would need a column change in `crates/quarry-runtime/migrations/` [NEW 0010 if needed].

Call sites of `scrub_string`: executor.rs:94, workspace_hydrate.rs:148 and 320 (test), process_host.rs:873, grpc.rs:1015-1016. `scrub_json_value`: grpc.rs:1042 and tests (`execution-core/tests/scrub_checkpoint_test.rs` [C]).

### 3. Reusable but unwired

- `agent_memory.rs:52` `redact_sensitive` and `AgentScratchpad` [C]: UNWIRED. Grep for `redact_sensitive` and `AgentScratchpad` across Quarry crates and Model Plane returns only agent_memory.rs (definitions and its tests 112-153) and the re-export at lib.rs:101. The card's "used today only by the scratchpad" is right, but the scratchpad itself has no caller, so the whole pair is dead in production.
- `execution-core/src/scrub.rs` [C]: WIRED in the Model Plane (executor.rs, grpc.rs, process_host.rs, workspace_hydrate.rs). Not used by Quarry. The JSON recommends it as the richer pattern source. Sharing requires duplication plus a conformance test (or an ADR), because the planes are separate workspaces.
- `model-gateway/src/moderation.rs:586` `redact_pii` [C]: WIRED for chat requests only; not applicable to browser observations.
- `browser_risk.rs` `input[type=password]` detector [C]: WIRED into the Model Plane risk classifier. Could seed a password-field rule at capture time, but it classifies selectors, not AX nodes.
- `quarry-runtime/src/test_site.rs` [C] (default feature `test-site`): a test site exists for a login-page fixture. Used by JSON as the test target.
- `ActionOutcome` and `put_artifact_if_allowed` [C]: WIRED choke points for masking before artifact writes.

### 4. Tests

- `quarry-runtime/src/agent_memory.rs` tests [C] (6): `redact_sensitive` cases for email, `sk-` token, card-like number, plain text (lines 112-153). These exist but cover dead code.
- `execution-core/src/scrub.rs` tests [C] (12) and `execution-core/tests/scrub_checkpoint_test.rs` [C] (4).
- `execution-core/src/llm_planner.rs` tests [C] (31) and `quarry_agent.rs` tests [C] (7). `test_observation()` at 1005 builds an observation with `extracted_text: "Example Domain"`.
- `model-gateway/src/moderation.rs` tests [C] (25) including `redact_pii_redacts_emails_and_long_numbers` (715).
- `quarry-runtime/src/observation.rs` tests [C] (25): `put_artifact_if_allowed_skips_zdr_writes` (3015), `put_artifact_if_allowed_stores_visual_observation_when_zdr_off` (3042), `execute_stores_visual_artifact_fanout_when_zdr_off` (3128).
- Commands: `cd "apps/Ingestion Plane/Quarry-v2" && cargo test -p quarry-runtime agent_memory observation`. `cd "apps/Model Plane/rust" && cargo test -p execution-core scrub`.
- CI: `quarry-v2-ci.yml` [C] line 205 (`cargo test --workspace`); `model-plane-rust-ci.yml` [M] line 119.

### 5. Feature flags and env

- No redaction flag exists. The planned rules are unconditional and deterministic.
- ZDR: `put_artifact_if_allowed` gates on `zdr_mode` (observation.rs:1436). Live observations (DOM, AX, receipts) are returned even under ZDR, so masking must not rely on the artifact gate.
- `QUARRY_PROFILE_KEY` is referenced only in docs (CONTRACTS.md:192), not read in code. Not relevant here.
- Model Plane: no env switch found for scrub (`scrub.rs` has none).

### 6. Surprises

- `redact_sensitive` is dead code. The card and JSON say "used today only by the scratchpad", but the scratchpad is itself unused.
- `observation.rs` takes a full-viewport screenshot on every observation (653-670), so screenshots of password fields are captured even when no action is taken. Screenshot masking by rectangle is a larger job than the card implies.
- The JSON reuse for receipts cites `step_receipts.rs:100-125`, which is the wrong range (row 13).
- Uncommitted change in the same area: model-gateway http_routes.rs [M] (live 2032-2036, 2106-2112) adds ZDR posture gates to the browser suggest and visual summary routes. The Model Plane should be rebased on those gates before the planner input is changed.
- ZDR browser runs are refused in model-gateway (browser_run.rs [M] 307-313, 628-632, 663-669), but the in-app UI session calls Quarry `/v1/agent/runs` directly (gateway browser.rs:503-506). ZDR for the UI path therefore rests on the Quarry-side guard, and capture-time redaction must run there too.
- Redaction at AX capture cannot see input type or autocomplete, because the AX node carries neither (the JSON notes this). The DOM describe call would be a new CDP round trip per input target [UNVERIFIED cost].
- Whether Chromium already masks password values in its AX tree was not verified. The JSON already flags this.

---

## F-t2-020 Harden persistent browser profiles: complete snapshots, per-org envelope encryption at rest, ordered saves, ZDR enforced inside Quarry

Card header: Priority, Partial, effort L, Ingestion Plane plus Control Plane (keys), wave W2, confidence High.

### 1. Citation check

| # | Citation (source) | Tag | Verdict | Finding |
|---|---|---|---|---|
| 1 | chromiumoxide.rs:1771-1832 (card, JSON) | C | OK | `capture_snapshot` 1771-1832. `page.get_cookies()` at 1772. `expires: None` at 1788. Storage read by page evaluate for the current origin only (1792-1811). `indexed_db: vec![]` at 1817. |
| 2 | chromiumoxide.rs:675-695 (JSON) | C | OK | `persist_current_page` 675-697. 5 s timeout (685). Save at 694-696. |
| 3 | [EXT] chromiumoxide-0.9.1 src/page.rs:900-907 (JSON) | EXT | OK | `get_cookies` doc: "Returns all cookies that match the tab's current URL." |
| 4 | Cargo.lock:1153-1154 (JSON) | C | OK | `chromiumoxide` 0.9.1. |
| 5 | postgres_profile_store.rs:30-92 (card, JSON) | C | OK | Upsert 72-92: `ON CONFLICT (org_id, profile_id) DO UPDATE SET snapshot = EXCLUDED.snapshot, updated_at = NOW()`. No saved_at ordering, no tombstone. |
| 6 | postgres_profile_store.rs:38-60 (JSON) | C | OK | `empty_snapshot_json` 44-47; struct 55-58. |
| 7 | postgres_profile_store.rs:160-176 (JSON) | C | OK | `save_metadata` 163-: ON CONFLICT touches name, scope and updated_at only. Snapshot is not clobbered. |
| 8 | postgres_profile_store.rs:247 (JSON) | C | OK, test-only | Line 247 is inside `mod tests` (starts 215). Fixture, not production. The JSON already says this. |
| 9 | quarry-edge main.rs:597-600 (card) | M | OK | `maybe_cache(inner_profiles, redis_url)` at 598-600. Redis TTL `DEFAULT_TTL_S` = 3600 (cached_profile_store.rs:40). |
| 10 | main.rs:545-600 and 692-708 (JSON) | M | OK | Store selection 560-596 (Postgres only with feature `postgres-queue`, `profile_store_kind = "postgres"` and a DSN). Durability bail 704-708. Not in uncommitted hunks. |
| 11 | quarry-runtime lib.rs:166 (card) | M | OK | `pub use s3_profile_store::S3ProfileStore;`. No production `S3ProfileStore::new` call (grep). |
| 12 | s3_profile_store.rs; cached_profile_store.rs (JSON) | C | OK | Files exist (359 and 379 lines). No encrypt, aes or kms in them (grep). |
| 13 | profile_routes.rs:102-125 (card, JSON) | C | OK | `save_profile` 102-126: `save(&claims.org_id, ...)` with `req.snapshot`, a client-supplied snapshot, stored verbatim. No ZDR check, no owner check. |
| 14 | profile_routes.rs:268-296 (card, JSON) | C | OK | `load_profile` 268-299 returns the full snapshot, including cookie values. |
| 15 | profile_routes.rs:298-312 (JSON) | C | OK, drift | `delete_profile` 301-316, org-scoped only. |
| 16 | routes.rs:135-157 (not cited) | M | Confirmed | Profile routes: POST and GET /v1/profiles (135-136); POST /create (140-143); GET, PATCH, DELETE /:id (144-152); POST restore_probe (153-157). |
| 17 | agent_routes.rs:527-533 (card, JSON) | C | OK | `zdr_forbids_persistent_profile` 527-533. |
| 18 | agent_routes.rs:1466-1476 (card, JSON) | C | OK | ZDR refusal at run creation (1470-1476). |
| 19 | page_image.rs:43-61 (card, JSON) | C | OK | `PageImageDeleted` 43-48; emitters 53-65. Consumer covers page images only. |
| 20 | docs/CONTRACTS.md:185-199 (card); 183-200 (JSON) | C | OK | Encryption block 190-195: scheme (xchacha20poly1305 or aes-256-gcm), key_source (env:QUARRY_PROFILE_KEY or kms), nonce, aad. Not implemented in code. |
| 21 | gateway browser.rs:174 (card, JSON) | C | OK | `reject_zdr_persistent_profile` 174-182; applied at 472 in `create_session`. |
| 22 | gateway browser.rs:470-476 (JSON) | C | OK | Call at 472. |
| 23 | quarry-runtime/src/persistent_session.rs (JSON evidence list) | MISSING | WRONG-CITATION | The path does not exist. The file is `apps/Ingestion Plane/Quarry-v2/crates/quarry-browser/src/persistent_session.rs` [C], 239 lines. Grep there for encrypt, aes and kms returns nothing. |
| 24 | migrations 0002 and 0005 (JSON reuse) | C | OK | `0002_profiles.sql`: `quarry_profiles(org_id, profile_id, snapshot JSONB, created_at, updated_at)`, PK (org_id, profile_id). `0005_profile_metadata.sql`: `name TEXT NULL`, `scope TEXT NOT NULL DEFAULT 'user_private'`, with a CHECK. |

Anchor-only: none. All 24 rows were read.

### 2. Affected files

- `apps/Ingestion Plane/Quarry-v2/crates/quarry-browser/src/chromiumoxide.rs` [C]. `capture_snapshot` 1771-1832 (replace `get_cookies` with context-wide cookies; add per-origin DOMStorage and IndexedDB). Restore path `open_tab_page` 512-532 (cookies hydrated after navigation, then reload; `hydrate_cookies` 925 and `hydrate_storage` 944). `persist_current_page` 675-697 (only caller of `capture_snapshot`, the 5 s timeout at 685).
- `apps/Ingestion Plane/Quarry-v2/crates/quarry-browser/src/session.rs` [C], 676 lines. `SessionSnapshot` at 16 (cookies, local_storage, session_storage, indexed_db, user_agent, viewport, locale, timezone). `ProfileStore` trait at 147. This is the serialization boundary the card reuses.
- `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/postgres_profile_store.rs` [C], 339 lines. Upsert 72-92 (add saved_at ordering and tombstone); `save_metadata` 163-.
- `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/cached_profile_store.rs` [C], 379 lines. Redis read-through, TTL 40 and `maybe_cache` 178. Encryption layer must sit below this so the cache holds ciphertext.
- `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/s3_profile_store.rs` [C], 359 lines. Never constructed. Card says skip it in the first slice.
- `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/page_image.rs` [C], 198 lines. Purge consumer for page images; extend to profiles.
- `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/profile_routes.rs` [C], 453 lines. `save_profile` 102-126, `list_profiles`, `create_profile`, `update_profile`, `load_profile` 268-299, `delete_profile` 301-316, `restore_probe` 361. Needs a ZDR check and an owner check, and a decision on raw cookie values in GET.
- `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/routes.rs` [M]. Profile routes 135-157 (unchanged by the uncommitted hunk at 1330).
- `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/main.rs` [M]. Store wiring 545-600; durability 694-708.
- `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/config.rs` [M]. `profile_store_kind` (243), `database_url` (250), `durable_event_history` (256), `redis_url` (21) are outside all uncommitted hunks (hunk arithmetic checked).
- `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/agent_routes.rs` [C]. ZDR guard 527-533 and 1470-1476. The persistent-profile capability check is at 1477-1484.
- `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/lib.rs` [M], export at 166.
- `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/browser.rs` [C]. `reject_zdr_persistent_profile` 174-182, used at 472.
- `apps/Ingestion Plane/Quarry-v2/docs/CONTRACTS.md` [C], 568 lines. Encryption spec 190-195.
- Migrations: `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/migrations/` [C]. Files 0001 to 0009 present. Next free sequence number is **0010**: `crates/quarry-runtime/migrations/0010_<name>.sql` [NEW] (saved_at column, tombstone table, encryption envelope columns). `migrations.rs` [C] runs `sqlx::migrate!("./migrations")` (line 32), so a new file is picked up at build time.
- Control Plane key API [NEW, outside Quarry]. Not located in this sweep.

### 3. Reusable but unwired

- `S3ProfileStore` (s3_profile_store.rs) and its re-export (lib.rs:166) [C/M]: UNWIRED. Grep for `S3ProfileStore::new` finds none outside its module. Its only references are its own key-helper tests (325-353).
- `PersistentSessionRegistry` (quarry-browser/src/persistent_session.rs:81) [C]: UNWIRED (section of the F-t2-007 report).
- `ProfileStore` trait and `SessionSnapshot` [C]: WIRED (chromiumoxide.rs and profile routes).
- `zdr_forbids_persistent_profile` (agent_routes.rs:527) and `Claims::zdr_floor` (agent_routes.rs:1441): WIRED.
- `PageImageDeleted` purge consumer (page_image.rs): WIRED for page images only. Extending it to profiles is new code.
- `cached_profile_store::maybe_cache` (cached_profile_store.rs:178): WIRED (main.rs:598-600).
- integration-corev2 AES-GCM vault [C] at `apps/Ingestion Plane/integration-corev2/internal/crypto/vault.go`: `ciphertextPrefix = "v1:"` (13), `NewVault(key)` (19-24, `aes.NewCipher` and `cipher.NewGCM`), `Encrypt` (31) and `Decrypt` (41) with additionalData. Go, so it is a pattern reference only and cannot be linked into Rust.

### 4. Tests

- `quarry-edge/tests/profile_roundtrip.rs` [C]: `profile_save_load_roundtrip_preserves_all_fields` (82), `profile_list_includes_saved_profile` (142), `profile_create_with_name_and_scope_then_appears_in_list` (186), `profile_create_rejects_ephemeral_scope` (230), `profile_rename_updates_name_without_touching_scope` (246), `profile_restore_probe_includes_name_and_scope` (357), `profile_delete_removes_entry` (391), `profile_load_unknown_returns_404` (422), `profile_save_with_specified_id_round_trips_that_id` (436). None tests ZDR or owner checks on `/v1/profiles`.
- `quarry-edge/tests/zdr_behavior.rs` [C]: `zdr_run_with_a_profile_id_is_rejected_before_touching_the_browser` (67), `zdr_run_with_persist_profile_flag_is_rejected_even_without_a_profile_id` (95), `zdr_run_against_a_real_existing_profile_is_still_rejected` (132), `non_zdr_run_with_persist_profile_signals_is_not_rejected_by_the_zdr_guard` (179).
- `quarry-edge/tests/zdr_claim_floor.rs` [M] (uncommitted +10 lines in audio tests).
- `quarry-runtime/src/postgres_profile_store.rs` tests [C] (4). Skip unless `DATABASE_URL` is set (219-223, 258). CI sets no `DATABASE_URL` and has no Postgres service, so these never run in CI.
- `quarry-runtime/src/cached_profile_store.rs` tests [C] (5). `quarry-browser/src/chromiumoxide.rs` tests [C]: `reopening_a_session_with_a_saved_profile_restores_cookies_without_erroring` (3405) and `distinct_sessions_do_not_share_cookies` (3458), both gated by `CHROMIUMOXIDE_TEST`.
- Commands: `cd "apps/Ingestion Plane/Quarry-v2" && cargo test -p quarry-edge --test profile_roundtrip --test zdr_behavior`. For Postgres: `DATABASE_URL=... cargo test -p quarry-runtime postgres_profile_store --features postgres-queue`.
- CI: `quarry-v2-ci.yml` [C] lines 205, 211. `quarry-docs-contract.yml` [C] runs `node --test` on the docs contract and watches `docs/**`; the test reads `cross-plane-token-requests.json` and does not assert CONTRACTS.md content (grep).

### 5. Feature flags and env

- `profile_store_kind` (config.rs:243), `database_url` (250), `durable_event_history` (256), `redis_url` (21). Config struct `EdgeConfig`.
- Cargo feature `postgres-queue` (quarry-edge Cargo.toml:86; quarry-runtime Cargo.toml:63). Without it, the in-memory store is used and a warning is logged (main.rs:588-593).
- `QUARRY_PROFILE_KEY`: referenced only in docs (CONTRACTS.md:192). Not read by any code. The encryption key source is therefore unimplemented.
- `DEFAULT_TTL_S` = 3600 (cached_profile_store.rs:40).
- Production boot fails unless profiles are durable when durability is required (main.rs:694-708).

### 6. Surprises

- `POST /v1/profiles` (routes.rs:135, `save_profile`) stores a client-supplied `SessionSnapshot` verbatim under the caller's org (profile_routes.rs:116). This is a write path for arbitrary cookies, separate from capture. The card does not mention it.
- `GET /v1/profiles/:id` returns cookie values (profile_routes.rs:292-297). The card asks whether this should ever happen. The JSON says the Model Plane must never see cookie values; the current route answers to any caller with the org claim.
- The card's reuse says `zdr_forbids_persistent_profile` guards `/v1/profiles`. It does not: it guards run creation only (agent_routes.rs:1470). The profile routes have no ZDR check (confirmed).
- UI sessions pass two ZDR guards: the gateway refusal (browser.rs:472) and the Quarry run-creation guard (agent_routes.rs:1470). The `/v1/profiles` routes have neither, so under ZDR any org-authenticated caller can still read or write profiles.
- Postgres profile tests and CDP profile tests never run in CI (no `DATABASE_URL`, no `CHROMIUMOXIDE_TEST`).
- The JSON's Firecrawl and Apify references are not part of this code map. The encryption spec is Verevon's own (CONTRACTS.md).
- Uncommitted edits to `quarry-edge/src/config.rs` (167 lines) and `main.rs` (188 lines) sit in the same files that wire the stores. None of the profile-specific lines are in those hunks, so the merge risk is textual, not semantic.

---

## F-t2-027 Honest bot-block handling: closed block taxonomy, non-signals, one recovery, a user-visible blocked state, and no stealth code

Card header: Recommended, Partial, effort M, Ingestion Plane plus Model Plane plus Frontend Plane, wave W1, confidence High.

### 1. Citation check

| # | Citation (source) | Tag | Verdict | Finding |
|---|---|---|---|---|
| 1 | observation.rs:154-214 (card) | C | OK | `classify_challenge` 158-215 (docs 154-157). Substring rules 170-207. Fixed confidence 0.98 and `requires_escalation: true` (209-214). |
| 2 | observation.rs:158-180 (JSON) | C | OK | Same function. |
| 3 | observation.rs:898-935 (JSON) | C | OK | Per-run counters 910-913; rate 924-930; telemetry 931-937. |
| 4 | observation.rs:903-913 (new) | C | OK | Only production caller of `classify_challenge`. Input is `dom_summary.text_snippet` (908), not the full page text. |
| 5 | browser_agent.rs:171-186 (card) | C | OK | `BrowserObservation` has no challenge field. |
| 6 | llm_planner.rs:236-244 (card) | C | OK, wording | System prompt 236-242. No captcha, bot or challenge rule (grep). |
| 7 | model-gateway http_routes.rs:2013-2022 (card) | M | PARTLY FALSE | `BROWSER_SUGGEST_SYSTEM_PROMPT` 2013-2021 forbids raw JavaScript evaluation, anti-bot bypass, credential entry and CAPTCHA solving (2016-2017). The card says only CAPTCHA and bypass. Uncommitted hunks are in the route (2032-2036), not in the const. |
| 8 | knowledge/quarry.rs:283-304 (card); 287-294 (JSON) | C | OK | `classify_scrape_failure` 287-304. 403 or 429 maps to `site_blocked` (288-293). Message is hard-coded Norwegian and suggests proxy access. 408 or 504, or 20 s or more, maps to `scrape_timeout`. |
| 9 | stealth.rs:1-12 and lib.rs:164 (card) | C | OK | `stealth.rs` 456 lines. Header says it is not a puppeteer-stealth port. The only reference is lib.rs:164 `pub mod stealth;`. |
| 10 | driver_plan.rs:57-90 (card) | C | OK | `stealth_level` field 57; set to 0 (68), 1 (79), 2 (90). Never read anywhere. |
| 11 | driver_plan.rs:13-21, 36-48, 45-47 (JSON) | C | OK | `DriverSignals` 13-21 (`prior_block_signals` at 18). `plan_from_signals` 36-50: 2 or more goes to browser (40); exactly 1 goes to `tls_fetch` (45-46). |
| 12 | host_scheduler.rs:31-47 (card); 36-47 (JSON) | C | OK | `BadKind` 35-46. `Blocked` (40) documented as "401/403/451/999/CDN-challenge". |
| 13 | pipeline.rs:366-376 (JSON) | C | DRIFTED (370-378) | Mapping 371-377: UpstreamBlocked and Forbidden to Blocked (373-374). |
| 14 | BrowserChrome.tsx:1228-1236 (card); 497, 624 (JSON) | C | OK | Human takeover chip 1228-1238 (aria-label, Bot icon). `humanTakeoverActive` 497. Toggle 622-626. |
| 15 | egress_broker.rs:155-163 (card); 102-163 (JSON) | C | OK | `plan` 102-153. `"block_rotation"` at 149 is a reason label, not a function. `mark_http_status` 155-164 marks blocks for `is_block_status` statuses. |
| 16 | fallback_driver.rs:95-108 (card) | C | OK | 96-108: rotate fingerprint or driver on block status. |
| 17 | fingerprint_rotation.rs:16-18 (card); 14-18 (JSON) | C | OK | `is_block_status` 16-18: 401, 403, 407, 429, 503. |
| 18 | presets.go:84-96 (card) | C | OK, drift | "stealth" RunPolicy preset 84-98; Block On TrigSuspectBot and Action BlkEscalate at 95-98. |
| 19 | error.rs:23 and 49 (card); 10-35 (JSON) | C | OK | `ChallengeDetected` 23; maps to HTTP 403 (49). |
| 20 | contracts.rs:236-257 (JSON); 100-122 (card) | C | OK | `ChallengeKind` 236-244; `ChallengeSignal` 246-255 (`requires_escalation` default true, 253-254). `BrowserTelemetry` 100-124 (challenge counters 115-119). |
| 21 | docs SOVEREIGN_SCRAPING_RELIABILITY.md:139-146 (card, JSON) | C | DRIFTED (142-151) | "### 4. Challenge Classifier and Policy Gate" is at 142; reason list starts 146. The card's range misses the heading. |
| 22 | "ErrorCode::ChallengeDetected returned at runtime" (implicit in card) | C | FALSE | No production code constructs it. Matches only error.rs 23 and 49, fallback_driver.rs:586 (name mapping) and quarry-core tests (46, 57). |

Anchor-only: rows 10 and 12 were checked by reading the variant and constant lines; no change of meaning.

### 2. Affected files

- `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/observation.rs` [C]. `classify_challenge` 158-215; challenge counters 903-913; BrowserObservation build 965-996 (`challenge` at 982); `ObservationRunner::execute` for one-recovery enforcement.
- `apps/Ingestion Plane/Quarry-v2/crates/quarry-core/src/contracts.rs` [C]. `ChallengeKind` 236-244; `ChallengeSignal` 246-255; `BrowserTelemetry` 100-124 (add hostname-only counters only in memory for ZDR orgs).
- `apps/Ingestion Plane/Quarry-v2/crates/quarry-core/src/error.rs` [C]. `ChallengeDetected` 23 and mapping 49.
- `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/host_scheduler.rs` [C] (`BadKind` 35-46; split the Blocked bucket). `pipeline.rs` [C] (mapping 371-377). `fetch.rs` [C] (`mark_http_status` 246, 251; block status 252).
- `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/driver_plan.rs` [C], 95 lines. `stealth_level` 57-90 (delete). `plan_from_signals` 36-50 (client-controlled `prior_block_signals`; see section 6).
- `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/fallback_driver.rs` [C], 1279 lines (96-108 rotation). `fingerprint_rotation.rs` [C], 117 lines (`is_block_status` 16-18). `egress_broker.rs` [C], 284 lines (`plan` 102-153; `mark_http_status` 155-164).
- `apps/Ingestion Plane/Quarry-v2/crates/quarry-browser/src/stealth.rs` [C], 456 lines (delete); `lib.rs` [C] line 164 (remove `pub mod stealth;`).
- `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/extract_routes.rs` [M] (145 `plan_from_signals`), `routes.rs` [M] (440, 980, 1095 `plan_from_signals` with `req.signals`).
- `apps/Ingestion Plane/Quarry-v2/services/quarry-control/internal/resources/presets.go` [C], 124 lines. "stealth" preset 84-98.
- `apps/Model Plane/rust/services/execution-core/src/browser_agent.rs` [C], 1709 lines. `BrowserObservation` 170-186 (add a challenge field, additive with serde defaults).
- `apps/Model Plane/rust/services/execution-core/src/quarry_agent.rs` [C], 963 lines. `observation_from_wire` 407-474 (forward challenge).
- `apps/Model Plane/rust/services/execution-core/src/llm_planner.rs` [C], 1064 lines. System prompt 236-242 (add the one-recovery and strong-versus-weak rule).
- `apps/Model Plane/rust/services/model-gateway/src/http_routes.rs` [M]. Const 2013-2021 (rule text); route `browser_suggest_action` 2023-2036 (uncommitted ZDR gate at 2032-2036); `browser_visual_summary` 2106-2112 (uncommitted gate). Rebase before editing.
- `apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs` [M]: not touched by this item.
- `apps/Model Plane/rust/services/model-gateway/src/browser_run.rs` [M], 18 insertions, 11 deletions: ZDR refusal hunks at 307-313, 628-632, 663-669 and 995-996. Same Model Plane browser-run path; coordinate.
- `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/knowledge/quarry.rs` [C], 1158 lines. `classify_scrape_failure` 287-304 (rewrite; do not add a second classifier).
- `apps/Frontend Plane/verevonv3/src/features/dashboard/home/BrowserChrome.tsx` [C], 2172 lines. Takeover chip 1228-1238; build the blocked state beside it.
- `apps/Frontend Plane/verevonv3/src/shared/i18n/errors.ts` [M] (uncommitted additions at 12 and 190-196 are ZDR refusal copy only; no site_blocked key exists). Strings need i18n keys.
- `apps/Frontend Plane/verevonv3/src/shared/api/browser-client.ts` [C], 566 lines. `BrowserObservation` 32-44 has no challenge field.
- `apps/Ingestion Plane/Quarry-v2/docs/SOVEREIGN_SCRAPING_RELIABILITY.md` [C] section 4 (142-151).
- No migration. [NEW] only if a new error variant is chosen (the card avoids this).

### 3. Reusable but unwired

- `ChallengeSignal` and `ChallengeKind` [C]: produced by observation.rs but not consumed by execution-core, the planners or the SPA (grep for challenge: execution-core has none; model-gateway hits are only PKCE code challenges at http_routes.rs:3289 and mcp_oauth.rs:241-249; the SPA has only an auth 2FA challenge at http.ts:108). Partly WIRED (produced), UNWIRED (consumed).
- `ErrorCode::ChallengeDetected` [C]: UNWIRED. No production constructor (row 22).
- `BrowserTelemetry.challenge_observation_count` and `challenge_rate_per_mille` [C]: produced (observation.rs 910-937), copied into BrowserObservation.telemetry (987). Not read by execution-core (`observation_from_wire` ignores telemetry).
- `stealth.rs` (`STEALTH_SCRIPT` and `CONSENT_DISMISS_SCRIPT`) [C]: UNWIRED. Only reference is lib.rs:164.
- `DriverPlan.stealth_level` [C]: UNWIRED. Set in three constructors, read nowhere.
- `plan_from_signals` [C]: WIRED from three edge routes and extract_routes.rs. Caller-controlled.
- `BadKind::Blocked` [C]: WIRED (pipeline.rs:374). Test only at host_scheduler.rs:423.
- Takeover control (BrowserChrome.tsx:1228-1238): WIRED. Proposed host for the blocked state.
- `classify_scrape_failure` [C]: WIRED (gateway) but its 403 or 429 to "site_blocked" rule contradicts the card's non-signal rule.

### 4. Tests

- `quarry-runtime/src/observation.rs` tests [C]: `challenge_classifier_requires_escalation_and_preserves_evidence` (2514), `ordinary_source_pages_do_not_receive_a_challenge_signal` (2528). The second contradicts the current classifier's login and consent rules, so it does not test the card's non-signal list.
- `quarry-runtime/src/host_scheduler.rs` tests [C] (12): `record_bad("a.com", BadKind::Blocked)` at 423.
- `quarry-runtime/src/fingerprint_rotation.rs` tests [C] (4): `is_block_status` cases at 85-88.
- `quarry-runtime/src/egress_broker.rs` tests [C] (4): `mark_http_status(..., 429)` at 279.
- `quarry-core/tests/contracts.rs` [C]: `ErrorCode::ChallengeDetected.http_status() == 403` (46) and not retryable (57).
- `quarry-runtime/src/driver_plan.rs` [C]: 0 tests. `quarry-browser/src/actions.rs` 0 tests.
- Gateway: `verevonv3/apps/gateway/src/domains/knowledge/quarry.rs` [C] (16 test attributes); `browser.rs` [C] (65).
- SPA: `src/features/dashboard/home/browser-session.test.ts` [C], `browser-loop.test.ts` [C], `browser-omnibox.test.ts` (not checked).
- Commands: `cd "apps/Ingestion Plane/Quarry-v2" && cargo test -p quarry-runtime challenge host_scheduler fingerprint_rotation egress_broker` and `cargo test -p quarry-core --test contracts`. Gateway: `cd "apps/Frontend Plane/verevonv3/apps/gateway" && cargo test`. SPA: `cd "apps/Frontend Plane/verevonv3" && pnpm test -- browser`.
- CI: `quarry-v2-ci.yml` [C] 205; `verevonv3-ci.yml` [M] `gateway-rust` job (108-141, `cargo test` at 141); `model-plane-rust-ci.yml` [M] 119.

### 5. Feature flags and env

- No feature flag gates the classifier. Strong and weak signal thresholds are not configurable.
- Driver escalation is driven by client input: `DriverSignals.prior_block_signals` (driver_plan.rs:18) reaches `plan_from_signals` from the edge request bodies (routes.rs:440, 980, 1095) [INF: request struct not re-read].
- Stealth preset `RunPolicy` "stealth" (presets.go:84-98) is shipped, with `Block On TrigSuspectBot` and `Action BlkEscalate`.
- ZDR: per-run challenge counters carry no URL (observation.rs 910-937). A hostname aggregate must be in-memory only for ZDR orgs (card).

### 6. Surprises

- Driver escalation is caller-controlled. A request can set `prior_block_signals` (driver_plan.rs:18) and force TLS impersonation (`== 1`, 45-46) or a browser run (`>= 2`, 40). The no-evasion stance therefore has to cover request input, not only observed blocks.
- Live evasion-adjacent paths: TLS impersonation (driver_plan.rs:45-46); driver and fingerprint rotation on 401, 403, 407, 429 and 503 (fallback_driver.rs:96-108; fingerprint_rotation.rs:16-18); proxy rotation after block statuses (egress_broker.rs:149-163); the "stealth" preset (presets.go:84-98). The card lists only the dead stealth module.
- `ErrorCode::ChallengeDetected` is defined and mapped to 403 but never constructed (row 22).
- The planners never see the challenge signal (BrowserObservation has no field, quarry_agent.rs does not map it). The one place that could show it, the gateway, uses hard-coded Norwegian text (knowledge/quarry.rs:291-297) with no key.
- The blocked UI state does not exist in the SPA. errors.ts has a takeover key (line 130) but no site_blocked key. The new copy needs i18n keys in both languages.
- Observation text for the classifier is the DOM snippet only (observation.rs:908), capped at 500 characters with a byte-index bug (row in F-t2-034).
- The browser-suggest route is changing in the live tree (model-gateway http_routes.rs [M]); the planner-rule edit must be rebased on it.
- The card's "classifier substring matches 'sign in', 'forbidden', 'cookie consent', 'access denied'" is accurate (observation.rs 170-207), and an ordinary login or consent page is treated as a challenge with requires_escalation true.

---

## F-t2-034 Agent observation quality and budget: post-action settle, frame-aware AX snapshot with warnings, set-of-marks screenshots, capped projection and image budget

Card header: Recommended, Partial, effort M, Ingestion Plane plus Model Plane, wave W3, confidence High.

### 1. Citation check

| # | Citation (source) | Tag | Verdict | Finding |
|---|---|---|---|---|
| 1 | chromiumoxide.rs:1086-1128 (card, JSON) | C | OK | `AX_PROJECTION_NODE_LIMIT` 300 (1086), `FRAME_PROJECTION_LIMIT` 64 (1087). `native_projection_from_cdp` 1089-1131 sets `truncated` (1098). |
| 2 | observation.rs:576-640 (card, JSON) | C | OK | `page_state` 576, devtools events 580-584, egress 589-601, dialogs 605, content 607, native projection 618, snapshot 627-637. All captured immediately after the action. No settle. Grep for settle, quiesc, quiet in observation.rs, chromiumoxide.rs, agent_loop.rs and agent_routes.rs finds nothing. |
| 3 | observation.rs:1573-1612 (JSON) | C | OK | Native targets capped at 50 (`take(50)` at 1598). |
| 4 | observation.rs:2133-2147 (JSON); 2140-2141 | C | OK, bug confirmed | `text_snippet` 2133-2146 (in `build_dom_summary`). `&trimmed[..500]` at 2141 after a byte-length check at 2140. Panics on a multi-byte boundary. |
| 5 | observation.rs:673-700 (JSON) | C | OK | Visual processor runs only when `request.zdr` is `Off` (672-674). Annotated PNG persisted at 686-697 through `put_artifact_if_allowed`. |
| 6 | observation.rs:965-995 and 1823-1880 (JSON) | C | OK | `BrowserObservation` build 965-996 (`console_summary` 975, `network_summary` 976, `telemetry` 987). `console_summary_from_devtools` 1823; `network_summary_from_devtools` 1844. |
| 7 | chromiumoxide.rs:640-651 (JSON) | C | OK | Lifecycle listener spawns `push_devtools_event`. Events are collected but never awaited. |
| 8 | chromiumoxide.rs:2681-2722 (card, JSON) | C | OK | `native_page_projection` 2681-2730: AccessibilityEnable (2686), `GetFullAxTree` depth 8 (2696), `GetFrameTree` (2706). Root frame only; no per-frame deadline. |
| 9 | chromiumoxide.rs:1988-2011 (JSON) | C | OK | `goto` 1988-2011. |
| 10 | chromiumoxide.rs:2800-2826 (JSON) | C | OK | `wait_for` at 2800 (polling). Anchor-only beyond the function start. |
| 11 | chromiumoxide.rs:3050-3062 (JSON); 3044-3064 | C | OK | `back` 3044-3052 and `forward` 3054-3064 each sleep 100 ms (3050, 3062). |
| 12 | browser_driver.rs:28-70 (card); 205-250 (JSON) | C | OK | `DEFAULT_SETTLE_MS` 3500 (40), `SETTLE_POLL` 300 ms (41), `SETTLE_MIN_TEXT_CHARS` 600 (44), env `QUARRY_BROWSER_SETTLE_MS` (46-52). Settle loop 240-250 runs only in the scrape adapter. |
| 13 | contracts.rs:347-361 (JSON); 346-360 | C | OK | `DomSummary` 346-360 with `click_map` (359): dense 1-based rows named `@eN`. |
| 14 | contracts.rs:366-395 (JSON); 366-384 and 389-395 | C | OK | `BrowserSnapshot` 366-384 (frames 382-383). `AccessibilityProjection` 389-395 (source, nodes, truncated). No warnings field. |
| 15 | artifact.rs:13 (JSON) | C | OK | `ScreenshotAnnotated` variant. |
| 16 | artifact_store.rs:23 (not in JSON list) | C | OK | Parses `screenshot_annotated`. |
| 17 | vision sidecar main.py:240-256 (JSON) | C | OK | Red changed-region overlay (247-254). Rectangles 255-258. |
| 18 | JSON: "ScreenshotAnnotated has no producer" (gap text) | — | FALSE | A producer exists: the visual processor's `annotated_png` (vision.rs:116) is persisted at observation.rs:686-697 when ZDR is Off. Set-of-marks (numbered AX-box labels) is still absent, so the card's conclusion holds. |
| 19 | quarry_agent.rs:405-474 (card, JSON) | C | OK | `observation_from_wire` 407-474 keeps url, title, `extracted_text` (from `text_snippet`, 417-421), screenshot id (468), snapshot targets (427-458). Drops console and network summaries and telemetry. |
| 20 | llm_planner.rs:200-235 (card) | C | OK | Text-only `observation_text` 206-232. Targets `take(40)` at 212. |
| 21 | model-gateway http_routes.rs:1987-1989 (JSON) | M | OK | `BROWSER_SCREENSHOT_MAX_BYTES` 6 MiB (1987); `BROWSER_OBSERVATION_PROMPT_CHARS` 16_000 (1988); `BROWSER_VISUAL_PROMPT_CHARS` 8_000 (1989). |
| 22 | http_routes.rs:2085-2160 (JSON) | M | OK | Suggest response 2086-2096; `analyze_image` at 2147; `max_tokens: 700` at 2160. Uncommitted ZDR gate at 2106-2112 in `browser_visual_summary`. |
| 23 | http_routes.rs:2175-2200 (JSON) | M | OK | `browser_suggestion_prompt` 2174-2201 uses `compact_json_for_prompt` with the 16k and 8k budgets and truncates the goal at 1200 chars (2178-2185). |
| 24 | browser-client.ts:3-49 (JSON) | C | OK | `BrowserAction` union 3-17 (selector-based; `click_point` and `mouse_wheel` are coordinate actions). `BrowserObservation` 32-44 has no snapshot or frames field. |
| 25 | agent_routes.rs:2791 (JSON) | C | OK, anchor-only | `live_frame_stream` (SSE preview). Not a settle site. |
| 26 | observation.rs:445 (JSON) | C | OK, anchor-only | `AgentAction::Wait` sleeps a fixed duration (444-446). |
| 27 | contracts.rs:671-710 (JSON) | C | OK, anchor-only | AgentAction variants; no budget fields. |
| 28 | gateway browser.rs:485 (card, "UI sessions capped at 12 steps") | C | OK | `max_steps: 12`. The card's "AI loop capped at 5" was not verified [UNVERIFIED]. |

Anchor-only: rows 25, 26 and 27 (the three least load-bearing of 28 unique ranges). All others were read.

### 2. Affected files

- `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/observation.rs` [C], 3214 lines. Settle insertion point between the action (`execute` 346-) and the snapshot capture at 576. Native targets cap 1598. `build_active_snapshot` 1573-1688. `text_snippet` 2133-2146 (byte-slice bug 2141). Screenshot capture 653-670 (per observation).
- `apps/Ingestion Plane/Quarry-v2/crates/quarry-browser/src/chromiumoxide.rs` [C]. `native_page_projection` 2681-2730 (per-frame fetch, warnings). `native_projection_from_cdp` 1089-1131 (`truncated`, order-based truncation). Lifecycle listener 640-652. Back and forward sleeps 3050, 3062.
- `apps/Ingestion Plane/Quarry-v2/crates/quarry-core/src/contracts.rs` [C]. `DomSummary` 346-360; `BrowserSnapshot` 366-384 (add `warnings`); `AccessibilityProjection` 389-395 (add `warnings` or a per-frame status; serde defaults).
- `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/browser_driver.rs` [C], 581 lines. Scrape-only settle (28-70, 205-250). Reuse the `looks_unhydrated` pattern (57-66) for an agent-action settle, in observation.rs or the driver, not copied from the adapter.
- `apps/Ingestion Plane/Quarry-v2/crates/quarry-core/src/artifact.rs` [C] 13 (`ScreenshotAnnotated`); `artifact_store.rs` [C] 23.
- `apps/Ingestion Plane/Quarry-v2/services/quarry-vision-sidecar/app/main.py` [C], 436 lines. Annotation producer 240-258.
- `apps/Model Plane/rust/services/execution-core/src/quarry_agent.rs` [C] (`observation_from_wire` 407-474). Additive fields need serde defaults; this file's observation contract is parsed separately from Quarry's wire (JSON constraint).
- `apps/Model Plane/rust/services/execution-core/src/browser_agent.rs` [C], 1709 lines. `BrowserObservation` 170-186.
- `apps/Model Plane/rust/services/execution-core/src/llm_planner.rs` [C]. `observation_text` 206-232; `take(40)` at 212 (align with the 50-target Quarry cap).
- `apps/Model Plane/rust/services/model-gateway/src/http_routes.rs` [M]. Budgets 1987-1989; suggestion prompt builder 2174-2201; visual summary 2085-2160 (ZDR gate 2106-2112 uncommitted). Image detail and per-run budget have no consumer until pixels reach the planner (JSON defers them).
- `apps/Frontend Plane/verevonv3/src/shared/api/browser-client.ts` [C], 566 lines. `BrowserObservation` 32-44 (add refs and frames later; the UI acts by selector today).
- `apps/Frontend Plane/verevonv3/src/features/dashboard/home/BrowserChrome.tsx` [C] (co-browsing UI only after an action vocabulary change).
- No migration. [NEW] only if a set-of-marks artifact kind is added (`artifact.rs` would gain a kind; card defers it).

### 3. Reusable but unwired

- `browser_driver.rs` settle loop (`visible_text_len`, `looks_unhydrated`, lines 57-66 and 209-250) [C]: WIRED, scrape only.
- `BrowserTelemetry.usable_observation_latency_ms` and `estimated_snapshot_tokens` [C] (contracts.rs 100-124; observation.rs 932-933): WIRED. The JSON suggests reporting `settle_ms` through this struct.
- `AccessibilityProjection.truncated` [C] (contracts.rs:394): WIRED from native_projection_from_cdp (1098).
- `BrowserFrame` topology and frame-ref actions [C] (contracts.rs 379-383; frame_*_ref actions): WIRED.
- `DomSummary.click_map` [C] (contracts.rs:359; built in observation.rs at 1979-2159 and checked in tests 2670-2714): produced, but no production consumer was found. Grep for click_map and clickMap in apps/Model Plane/rust, verevonv3/src and the gateway returns only the test name `click_maps_to_click_with_selector` (execution-core/src/llm_planner.rs:517), which is unrelated.
- `ScreenshotAnnotated` artifact kind [C] (artifact.rs:13): WIRED to the visual processor (observation.rs:686-697). Not a set-of-marks.
- `compact_json_for_prompt` budgets [C] (http_routes.rs): WIRED on the suggest path.
- `VisualObservationProcessor` (vision.rs) [C]: WIRED, gated by ZDR Off.

### 4. Tests

- `quarry-runtime/src/observation.rs` tests [C] (25): `dom_summary_filters_hidden_elements_and_keeps_rendered_ones` (2619), `dom_summary_counts_nodes` (2584), `click_map_is_stable_dense_and_aligned_with_snapshot_refs` (2670), `empty_page_yields_no_click_map` (2714), `dom_summary_wire_shapes_stay_backward_compatible` (2721), `devtools_events_build_console_and_network_summaries` (2935), `execute_stores_visual_artifact_fanout_when_zdr_off` (3128).
- `quarry-runtime/tests/dom_summary_proptest.rs` [C]: property tests on `build_dom_summary`. The text-snippet bug (2141) would surface here with multi-byte input.
- `quarry-runtime/src/browser_driver.rs` tests [C] (scrape settle; uses `RuntimeLeasePool::new(4)` at 395-565).
- `quarry-browser/src/chromiumoxide.rs` tests [C]: the native projection tests are absent; browser-backed tests need `CHROMIUMOXIDE_TEST=1`.
- Model Plane: `execution-core/src/quarry_agent.rs` tests [C] (7): `observation_from_wire` at 809, 817, 901. `llm_planner.rs` tests [C] (31).
- Model Plane: `model-gateway/src/http_routes.rs` [M] has suggest-route tests (not enumerated).
- Commands: `cd "apps/Ingestion Plane/Quarry-v2" && cargo test -p quarry-runtime observation dom_summary` and `cargo test -p quarry-runtime --test dom_summary_proptest`. Model Plane: `cd "apps/Model Plane/rust" && cargo test -p execution-core quarry_agent llm_planner`.
- CI: `quarry-v2-ci.yml` [C] (`cargo test --workspace`, 205); `model-plane-rust-ci.yml` [M] (119).

### 5. Feature flags and env

- `QUARRY_BROWSER_SETTLE_MS` (browser_driver.rs:46-52, default 3500): scrape settle only. An agent settle would need its own budget.
- No env or flag for AX per-frame deadlines, truncation, or set-of-marks.
- ZDR: visual processor and annotated artifacts run only when ZDR is Off (observation.rs:672-674, 686-697). Live observations are still returned under ZDR, so any new settle or snapshot data follows the same rule.
- Model Plane image limits: `BROWSER_SCREENSHOT_MAX_BYTES` 6 MiB (http_routes.rs:1987). No detail hint, downscale or budget exists for planner pixels; the planner is text-only (llm_planner.rs).

### 6. Surprises

- `observation.rs` captures a full-viewport PNG on every observation (653-670), even when the planner never sees pixels. That is a cost driver the card does not mention.
- The JSON's gap text "ScreenshotAnnotated has no producer" is FALSE (row 18).
- The JSON's text-snippet byte slice is a real panic risk (`&trimmed[..500]` at observation.rs:2141). It is outside this item's scope but sits in the snapshot path the item edits.
- `observation_from_wire` drops console and network summaries and telemetry (quarry_agent.rs 407-474). The card's "carry page events to the model" needs this mapping and a planner text section.
- Settle does not exist for agent actions. `wait_for` and fixed sleeps are the only waits (chromiumoxide.rs 2800-2826, 3050, 3062; observation.rs 444-446).
- `DomSummary.click_map` has no production consumer in the Model Plane, SPA or gateway (the only Model Plane hit is a test name at llm_planner.rs:517). Its alignment with snapshot refs is tested only in quarry-runtime (observation.rs 2670-2714).
- The UI acts through CSS selectors (browser-client.ts 3-17), so AX refs and warnings reach the Model Plane first. Co-browsing UI needs an action vocabulary change, as the card says.
- Uncommitted edits in http_routes.rs [M] touch the browser visual summary (2106-2112), the same route that would receive image budget work.
- Side note: UI sessions cap at 12 steps (gateway browser.rs:485). The AI loop cap of 5 is unverified.

<!-- COMPLETE -->
