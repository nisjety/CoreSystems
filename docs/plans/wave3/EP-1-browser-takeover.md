# EP-1 (Wave 3 part) - Human takeover and hand-back of the agent browser (F-t3-026)

Abbreviations: `MP` = `apps/Model Plane`; `MPR` = `MP/rust`; `EC` = `MPR/services/execution-core/src`; `MG` = `MPR/services/model-gateway/src`; `SC` = `MPR/services/session-core`; `PROTO` = `MP/proto/model_plane/v1`; `CON` = `MP/api/contracts`; `FE` = `apps/Frontend Plane/verevonv3`; `BFF` = `FE/apps/gateway/src`; `FES` = `FE/src`; `QE` = `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src`. Every `path:line` was opened in worktree `epic-shaw-4d613b` at HEAD `70e67c8d7` (branch `model-plane`) unless marked otherwise. `[INF]` = inference. `[verify live]` = only a running stack can prove it. Sibling Wave 3 designs used: `docs/plans/wave3/00-integration-round-a.md` (reservation table 1.1/1.2, hot-file matrix 2). Quarry plan = `docs/plans/quarry-browser-build-plan-2026-10-14.md`.

## 1. Scope, items resolved, owner

**Item:** F-t3-026 (audit: Partial, M, W3, confidence medium): let the agent ask for help when a browser run hits a login, two-factor prompt or bot check, hand the hosted browser to the person, and resume with the person's note and a fresh snapshot; human-typed input never reaches the model.

**Owner split (CLAUDE.md and overlap-quarry row 4, `.plan-work/common/overlap-quarry.md:25`):**
- Quarry v2 (Ingestion Plane) owns control mode, the initiator, the control epoch and the enforcement of `USER_TOOK_OVER`, and who may address a Quarry run. This design asks Quarry for changes (section 6, Q-1 to Q-5); it does not design Quarry code.
- Model Plane owns the pause and resume of the Do run, the `browser_human_assist` approval cause, the events, and what the model is told after hand-back.
- Frontend Plane owns the take-over and give-back controls in `ChatLiveRunPanel`, the help card, and BFF pass-through.
- Control Plane: no change. Data Plane: no change (nothing is retrieved or embedded).

| Slice | Tag | In | Startable now? |
|---|---|---|---|
| S1 | [BUILD] | Manual take over and give back in the chat live panel, end to end (contract, Do-loop pause on `USER_TOOK_OVER`, panel toggle). First user-visible value | MP half yes; FE half after D1 + WS21-S6 binding |
| S2 | [BUILD] | Agent-requested help: a strong login/CAPTCHA/consent challenge raises a help card instead of ending the browser session | Yes (dark); live after S1 |
| S3 | [BUILD] | Hand-back note and fresh snapshot; human input never reaches the model | Yes |
| S4 | [BUILD] | Hand-back hygiene: no typed values in the first observation, no reopen during takeover, login carry-over bounded by the run | Yes (MP defensive half) |
| S5 | [BUILD] | Wait budget and expiry: assist TTL, grant and bearer expiry across a long takeover, timeout behaviour | Yes (dark) |
| S6 | [FINISH] | Flip, ZDR rows, ledger, live proof | After S1-S5 and Q-1 to Q-4 |

**Explicitly OUT:**
- W4 members of EP-1: F-t3-035 (live computer panel, typed viewers, PTY exec), F-t3-050 (scrubbable replay), F-t3-024 (computer use in the sandbox desktop).
- Point-and-comment steering (second track of the 026 card): it waits for the F-t3-031 annotation part, per the audit card. Not designed here.
- Synthetic-input arbitration: dropped, as the card says; Quarry's control mode plus enforcement covers it once `QUARRY_EDGE__TAKEOVER_ENFORCEMENT` is on.
- Co-browsing in the person's own session (Quarry ADR-3 attach: Quarry plan WS8.G Model Plane attach mode, WS8.I gateway/SPA, WS9.G/H chat dock). Those are Quarry-plan slices whose Model Plane half is "sibling (b)" (`/c/dev/CoresSystem/.plan-work/quarry-w1w2/W2-queue.md:83,85`, live tree). This design does not duplicate them; see D1 for how the two paths relate.
- Takeover of the legacy durable browser-agent run (`ExecuteStep tool_name=browser_agent`, started from `/v1/browser/runs`): its `USER_TOOK_OVER` pause is specified in Quarry plan WS8.G (`PlanStepResult::PausedByUser`, `BrowserRunPaused` with `reason: user_takeover`). EP-1 shares the `reason` field with it (section 4 S1) and nothing else.
- Persistent logins across runs (WS21-S9, optional-deferred; needs Quarry F-t2-007/013/021 and DEC-21).
- Site-password vault substitution (DEC-21, Quarry F-t2-024).
- Endpoint Plane / user-desktop control (DEC-22 parks it).

## 2. Verified current state

**What a person can control today.**
- Quarry has a whole-run control route `POST /v1/agent/runs/{run_id}/browser-session/control` (`QE/agent_routes/control.rs:101-171`). Only a person may flip it (`derive_initiator(&claims, None) != Human` is 403, `control.rs:125-132`); it is compare-and-set on `expected_epoch` (409 with the current epoch, `control.rs:133-147`); a takeover pauses the run's runtime budget and give-back resumes it (`control.rs:151-159`); a timeline `Control` event is recorded for non-ZDR runs (`control.rs:160-171`, `record_browser_timeline_event` 77-97 returns early under ZDR).
- Enforcement exists inside Quarry (F-t2-013 phase 1, Quarry plan WS2.C, built): `takeover_gate` refuses a non-human request while `HumanTakeover` holds with 409 `USER_TOOK_OVER` and `details.control_mode/control_epoch/taken_over_at` (`QE/agent_routes/admission.rs:300-347`); one `skipped` receipt per epoch for non-ZDR runs (`admission.rs:352-383`); the gate is step 3 of the pinned admission chain (`admission.rs:385-406`). It refuses only when `QUARRY_EDGE__TAKEOVER_ENFORCEMENT` is on; otherwise it logs `would_reject` (`admission.rs:320-347`). `takeover_enforced()` defaults to false (`QE/run_limits.rs:33-36`).
- A service principal is always the agent; a person's token is the agent only when it says so (`QE/agent_routes/run_live.rs:84-95`).
- **Who owns a run.** Every owner check goes through `caller_owns_browser_run` (org equal and `run_actor_id == claims.actor_id()`, `QE/agent_routes/control.rs:63-71`; `RunLive::owned_by` 485-487 in `run_live.rs`). It guards the control route (`control.rs:121`), live frames (`QE/agent_routes/live.rs:148,163,210,224,282,457`), projection (`projection.rs:241-319`), receipts (`receipts.rs:15-23`), steps (`step.rs:83,95`) and close (`close.rs:23`). A service principal's `actor_id` is its signed `service_id` (`QE/auth.rs:150-157`, normalisation 164-173). A run started by a service is bound to the grant's owner only for user principals: "A service principal acts for the run's owner, so the owner rule does not apply to it" (`QE/agent_routes/lifecycle.rs:3-40`), and the actor stamped on the run is the caller (`lifecycle.rs:131`).
- **Consequence (the central finding).** Both Model Plane browser engines start their own Quarry run with execution-core's service token: the Do loop (`EC/quarry_agent.rs:816-851`, `start_run` with `bearer_auth(service token)`, called from `EC/browser_session.rs`) and the legacy browser-agent loop (`EC/browser_agent.rs:863`). Those runs are owned by `service:execution-core`. The person cannot read their frames, cannot flip their control and cannot step on them: every one of those routes answers 404 to the person's token. Quarry's own plan records the same fact (Quarry plan line 1706: "a user token cannot read a service-owned run (F6b)").
- The broker already says who a grant is for: `ValidateGrantResponse.owner_id = 11`, "The user (or service actor) the grant was issued to. A consumer may bind a run to it; it is not a credential" (`PROTO/browser.proto:219-221`). The Do loop mints its run grant under the user's delegated browser bearer (`EC/browser_session.rs:1-13`, `RunGrant` 182-189), so `owner_id` is the person [INF from the broker's mint rule; verify live].

**BFF.**
- `POST /api/v1/browser/sessions/{session_id}/control` forwards `{mode, expected_epoch}` to Quarry with the person's token (`BFF/domains/browser.rs:479-482` route, `set_control_mode` 1017-1056, `ControlBody` 334-343, `control_quarry_body` 347-353). Control authority stays in Quarry ("A BFF cache must neither authorize a hand-off ...", 1034-1035).
- The BFF's 409 `browser_human_takeover_active` (`browser.rs:951-966`) and its implicit flip for a person's action (`browser.rs:967-985`) apply only to actions sent through the BFF. The Model Plane never calls `/api/v1/browser`; it drives Quarry directly with a `browser:execute` service token (`browser.rs:270-284`, doc comment of `CallerClass`). So the BFF 409 never touches a Model Plane run.
- Frame and event routes exist and are keyed by the Quarry run id: `/frame`, `/frames/stream`, `/events/stream`, `/frames/ws` (`browser.rs:513-529`).
- `/api/v1/browser/runs/{run_id}/control` is pause/resume/stop of the durable browser-agent run, mapped by MG onto `PauseRun`/`ResumeRun`/`CancelRun` (`MG/browser_run.rs:1715-1830`), refused under ZDR (`browser_run.rs:1740-1750`). It is not a takeover.

**SPA.**
- `ChatLiveRunPanel` mounts `<BrowserChrome compact session={model()} />` with no control handler (`FES/features/chat/components/ChatLiveRunPanel.tsx:604-617`); its own pause/resume buttons call `controlBrowserAiRun` (`ChatLiveRunPanel.tsx:444-460`).
- The view model sets `sessionId: undefined` on purpose: "no gateway-created browser session backs a chat run, so every manual control in `BrowserChrome` stays disabled" (`FES/features/chat/lib/chat-run-watch.ts:449-452`); `controlMode` is hard-coded `'agent_control'` (`chat-run-watch.ts:426`); live frame URLs are null (435-437).
- `BrowserChrome` has the prop `onBrowserControlMode(mode, expectedEpoch)` (`FES/features/dashboard/home/BrowserChrome.tsx:464`) and disables the toggle without it (`BrowserChrome.tsx:677-682`).
- The panel watches the run that `liveRunId` picks: the latest assistant turn in plan mode, execute-granted or research (`FES/features/chat/components/use-chat-controller.ts:553-570`).
- `APPROVAL_CAUSE_CODES` is the closed SPA list (`FES/shared/approvals/approval-copy.ts:20-34`); an unknown code reads as `unclassified_gate` (`approval-copy.ts:49`).

**Model Plane.**
- The Do browser tools are `browser.open|observe|close` (+ `browser.act` from WS21-S5b) with server-held ids (`EC/browser_session.rs:33-58`, `BrowserSession` 140-152). A Quarry failure maps through `quarry_refusal`: challenge to `browser_blocked_by_site`, 404/410 to `browser_session_lost`, every other status (409 `USER_TOOK_OVER` included) to `browser_session_failed` (`EC/browser_session.rs:428-440`). An observation with a challenge becomes `BLOCKED` (`browser_session.rs:506-510`).
- `AgentClientError::Status{status, body}` carries no parsed code; only `Challenge` is typed (`EC/quarry_agent.rs:718-732`). A test pins that 403 `USER_TOOK_OVER` stays an untyped status error (`quarry_agent.rs:1391-1405`). Nothing in `MPR/services` treats `USER_TOOK_OVER` as a pause (repo grep: only that test).
- ChallengeSignal **is** consumed (audit claim stale): the browser-agent planner stops on a strong wall (`EC/browser_agent.rs:1065-1076` `blocked_by_bot_wall`), the planner prompt carries a typed challenge line (`EC/llm_planner.rs:106-112`, `challenge_line` 278-288), and a 403 challenge test expects `"next": "user_takeover_or_other_source"` from Quarry (`browser_agent.rs:2030-2053`). `ChallengeKind` has `captcha, waf, login, consent, rate_limit, access_denied, paywall, geo_blocked, unknown` (`browser_agent.rs:227-279`).
- The Do-loop approval pause: `pause_for_approval` mints a durable approval through session-core with a typed cause (`EC/runtime_loop/agent.rs:7685` onward; `approval_cause_for_pause` 7663-7684 sets `risk_category` for `browser.act`).
- Capability ids: `browser.open|observe|close` map to `cap.browser.observe`, `browser.act` to `cap.browser.act` (`EC/capability_policy.rs:1564-1570`); an unmapped tool id fails closed.
- Contracts: `ApprovalCause` uses fields 1-10 (`PROTO/orchestration.proto:451-485`, last `tool_digest = 10`); `ApprovalKind` ends at 5 (`orchestration.proto:214-227`; kind 6 is reserved for the Space/WS13 kind, plan 4.3 row "ApprovalKind next value"); `OrchestrationEvent` oneof uses through 29 (`orchestration.proto:1241-1289`); `BrowserRunPaused`/`BrowserRunResumed` carry only `run_id = 1, plan_id = 2` (`orchestration.proto:1407-1416`); `EVENT_TYPE_BROWSER_RUN_PAUSED = 142`, `_RESUMED = 143` (`PROTO/events.proto:177-178`). The closed cause list `CON/approval-causes.json` has ten codes, no browser hand-off code; `mp-contracts` `CauseCode` mirrors it (`MPR/crates/mp-contracts/src/approval_cause.rs:40-70`).
- ZDR: the Do browser tools are never offered or dispatched under ZDR (`CON/zdr-posture-matrix.json:799-811`, row `e_browser_do_tools_refused_under_zdr`); refusal keys `browser_run`, `browser_run_control`, `browser_tool` exist (`CON/zdr-refusals.json:168-178`).

**Prerequisite slices (queue `.plan-work/execution/queue.json`, register `docs/plans/model-plane-depth-register.md`).**

| Slice | Queue status | Rows that matter | Can EP-1 start anyway? |
|---|---|---|---|
| WS21-S5a Do open/observe/close | partial | D2-935 (no `browser_session_opened`, no event sink in the Do loop), D2-936 (no bearer expiry margin or park), D2-938 (no rebuilt-stack proof); ledger `MP/docs/decisions/ledger.md:5513`: every non-ZDR open fails closed because the broker refuses non-ZDR read-only grants | MP code yes; **no live proof of anything in EP-1 until the read-only grant policy is decided and an open works live** |
| WS21-S5b `browser.act` | partial | D2-958 (open asks read-only, so act is refused), D2-961/964 (Quarry read_only contract, K3-6) | S1-S5 do not need act; a login by the person does not go through `browser.act` |
| WS21-S6 consent card, live panel | partial | D2-1030 (`browser_site_consent` absent from cause vocabulary), D2-1033 (panel not bound; `quarry_run_id` mapping undecided), D2-1035 | S1's FE half needs D2-1033 closed (D3) |
| WS21-S9 interim login hand-off | optional-deferred | needs Quarry F-t2-007/013/021 | Not needed: EP-1 is the run-scoped hand-off; S9 is the persistent one |
| WS21-S10 flips | partial | D2-1061, D2-1063, D2-1064 | S6 waits for the Do browser flip |
| WS03-S2/S3/S7 consume, continue, in-place resume | done | - | yes |
| WS03-S8 parking | partial | D1-545/546/547 | S5's "bearer expired at hand-back" path needs the park or falls back to a refusal (S5) |
| WS13-S1/S2 cause carrier, projection | done | - | yes |
| WS14-S5 question resume | partial | D2-1158/1160 | Not used (D2 picks the approval carrier) |
| WS16-S1/S2 attention state, rail | partial | (attention derivation from approvals) | S2's rail entry is visible only when WS16-S2 is on; card works without it |
| WS16-S3/S6 typed attention events, Web Push | done | - | yes |
| Quarry WS2.C enforcement + epoch | built on `quarryv2`, present in this tree (`admission.rs`, `control.rs`) | enforcement off by default; stack evidence row in W2-queue (`WS9.I ... WS2.C (PARTLY; stack evidence not run)`, live tree `W2-queue.md:96`) | yes; enforcement must be on for S6 |
| Quarry WS8.F/WS8.H attach, lanes | built, dark (`QUARRY_EDGE__ATTACH` off, `QE/agent_routes/attach.rs:1-13`) | WS8.G (MP attach mode) and WS8.I not built | not needed under D1(a) |

**Stale claims (code wins).**
1. Audit card: "the BFF returns 409 to agent actions while a human holds control (browser.rs:690-715)" as the guard for agent runs. The 409 is at `browser.rs:951-966` and never sees a Model Plane run (MP calls Quarry directly). Quarry now enforces itself (WS2.C), but only with the switch on.
2. Audit card and plan EP-1 "first slice (S-M): wire the existing manual takeover toggle ... passes no onBrowserControlMode". True that no handler is passed, but wiring it does nothing useful: the chat run has no addressable session (`chat-run-watch.ts:449-452`) and the person's token cannot address a service-owned Quarry run (`control.rs:63-71`). The toggle needs D1 first; it is not an S slice.
3. Audit card: "Quarry's ChallengeSignal is not consumed". Consumed as a stop signal in both engines (see above); not consumed as a hand-off.
4. Audit card: "Quarry's single control-mode state already covers [arbitration]". True only with `QUARRY_EDGE__TAKEOVER_ENFORCEMENT=1` and only for a run the person can address.
5. overlap-quarry line 12 ("Quarry's step handler never reads `control_mode`"): stale; `admission.rs:325-347`.
6. Audit card citation `quarry-runtime/src/step_receipts.rs:55-95` as the control mode owner: control now lives in `QE/agent_routes/{control,admission,run_live}.rs`; not re-opened at the old path.

## 3. Decisions and open questions

| # | Decision | Options | Recommendation | Gates | Proceed without? |
|---|---|---|---|---|---|
| D1 | How a person addresses (views, takes over, steps on) the Quarry run a Model Plane Do loop started | (a) **Grant owner of record**: Quarry records the validated grant's `owner_id` as the run's beneficiary when a service principal starts a run; `caller_owns_browser_run` accepts the beneficiary for projection, frames, control, human steps and close; the agent stays the service principal. (b) **Attach (ADR-3)**: every chat turn that browses first gets a person-owned session (human_browse grant) and the Do loop attaches as a delegate tab (Quarry WS8.F/H built dark, MP WS8.G and gateway WS8.I not built, X-13 shared-jar decision open). (c) No person control; Model Plane only pauses cooperatively | **(a)** for the chat Do loop. Reasons: it reuses the whole-run control route, epoch and enforcement that already exist; the change is one owner function plus a stamped field in Quarry (S, Q-1); the person sees only a run that was minted with their own delegated bearer; no shared cookie jar (the run keeps its own ephemeral profile, so X-13/D8-6 do not apply); it works for Space runs, where the grant owner is the run initiator and other members are not owners. (b) remains the right model for co-browsing in the person's own session and is not blocked by (a). Risk of (a): it departs from ADR-3's "one long-lived user-owned run per visible session" (OQ-12) for agent-started runs; Quarry must accept that. Decider: Quarry session + owner | S1 FE half and live proof, S2 "Take over" button, S6. Not S1 MP half, S3, S4, S5 | MP halves yes |
| D2 | How a takeover is represented in the Do run | (a) approval row with a new cause `browser_human_assist` (plan K3-12 option a); (b) WS14 question with `delivery: resume` (K3-12 option b; WS14-S5 partial, D2-1160 open); (c) new `ApprovalKind` 6 | **(a)**. Approvals already pause and resume in place (WS03 S2/S7 done), surface in the attention rail (WS16 models human-assist as approvals, plan EP-1 prerequisites), carry a typed cause (WS13 done) and are single-use. Kind stays `TOOL_CALL`; kind 6 is reserved for the coordinated Space/WS13 kind (plan 4.3). (b) waits on D2-1160 and has no grant snapshot | S1, S2 | yes (low risk) |
| D3 | How the SPA learns the Quarry run id of the chat run's browser | (a) owner-only `browser_session_opened{quarry_run_id}` event on the run stream (WS21-S6 plan row); (b) BFF resolves run id to Quarry run via an MG owner read | **(a)**, as WS21-S6 already plans; never shown to the model (DEC-20 is about the model, not the owner's SPA). The id is not a credential: under D1(a) Quarry still checks the owner of record. Owned by WS21-S6 (D2-1033) | S1 FE half | MP half yes |
| D4 | What may trigger an agent-side help request | (a) Quarry `ChallengeSignal` only (system-raised: strong signal, kind `login`/`captcha`/`consent`/`waf`, recovery exhausted); (b) also a model tool `browser.request_help{reason}` | **(a) in W3.** A model tool lets page text steer the model into asking the person to type credentials into a page it chose (prompt injection); the fence limits the host but not the social engineering. Revisit (b) with F-t3-031 | S2 | yes |
| D5 | How long a person may hold the browser | approval TTL 3600 s (default) vs 1800 s vs configurable | **1800 s** default (`EXECUTION_BROWSER_ASSIST_TTL_SECONDS`, clamp 300-3600), equal to Quarry plan D8-3 "1800 s takeover idle". On timeout the model is told the person did not give control back and the browser session is closed | S5 | yes |
| D6 | May the person's hand-back note reach the model | (a) yes, typed in the give-back control, capped 500 chars, framed as the person's words; (b) no note | **(a)**. It is the person's own instruction, like a chat message. What the person typed into the page never reaches the model (S3, S4) | S3 | yes |
| D7 | Read-only grants and a person's steps | Quarry applies `read_only` to every initiator vs to the agent only | **Agent only.** A read-level run must still let the person log in by hand. Quarry's K3-6 contract test (D2-961) must say so. Decider: Quarry (Q-3) | S2 live proof on read-level sites | yes |
| D8 | Login carry-over expiry (audit: "any login carry-over grant needs an expiry") | (a) bounded by the Quarry run: ephemeral run-scoped profile, ends with the run, the grant TTL (30 min, `EC/browser_session.rs:67`) and the session runtime; (b) carry into later runs (persistent profile) | **(a).** (b) is WS21-S9 (deferred) and needs DEC-21. Note: the Quarry runtime budget is paused during a takeover (`control.rs:151-159`), so the wall time can exceed 15 min; the grant TTL still caps it | S4 | yes |
| D9 | Sign-in that crosses hosts (SSO, IdP on another domain) | (a) refused by the run fence, card says so; (b) widen the fence for the person's steps | **(a) in W3**: the fence is DEC-19's (start host + at most 8). Widening needs a DEC-19 amendment and Quarry agreement | S2 copy | yes |

Open questions that do not block any slice: whether the browser-agent engine (workspace AI run) should also adopt D1(a) before WS8.G lands (Quarry plan's call); whether the takeover should count against WS27 cost meters (it does not call a model, so no [INF]).

## 4. Slices

Flag rule for every slice: default off, read once into service state (no per-request `std::env::var`; D2-657 is the trap), boolean switches take the literal `true` (D2-658).

### S1 - Manual take over and give back in the chat live panel [BUILD] (first user-visible value)

**Goal.** While the agent browses in a chat Do run, the person presses Take over in the live panel, drives the page, presses Give back, and the run continues. Today the person cannot do this at all.

**Files and changes.**
- Contract (first): `CON/approval-causes.json` new code `browser_human_assist`, `raised_by` "execution-core browser gate: the person took the browser, or the agent asked for help", `scope_options ["once"]`; `MPR/crates/mp-contracts/src/approval_cause.rs` `CauseCode::BrowserHumanAssist` (and `ALL` length 11); `FES/shared/approvals/approval-copy.ts` add to `APPROVAL_CAUSE_CODES` and nb/en copy via `i18n.tr` ("You have control of the browser. Press Give back when you are done." / "Du styrer nettleseren. Trykk Gi tilbake når du er ferdig."); `FES/shared/actions/action-registry.ts` contract `browser_run.take_over` and `browser_run.give_back` (inputs: run id, expected epoch, optional note) before wiring.
- Proto (additive, commented): `PROTO/orchestration.proto` `ApprovalCause`: `host = 11` (shared with WS21-S6 `browser_site_consent{host}`; one field, first lander adds it), `assist_kind = 13` (closed string: `user_takeover`, `login`, `captcha`, `consent`, `waf`), `assist_origin = 14` (`person`, `agent`). 12 is left for WS21-S6 `max_level`. Integrator assigns (DEC-39); verified next free is 11.
- Proto: `OrchestrationEvent.BrowserRunPaused` `reason = 3` (closed string `user_takeover`, `human_assist`, `approval`, `budget`) and `control_epoch = 4`; same on `BrowserRunResumed` (`reason = 3`, `control_epoch = 4`). Quarry plan WS8.G/R-11 names the same `reason` field for the browser-agent engine: one definition, first lander adds it, the other reuses it. No new `EventType` (142/143 reused).
- `EC/quarry_agent.rs`: parse the error envelope into `AgentClientError::Typed{status, code, details}` (keep `Status` for bodies without a code); update the pinned test at 1391-1405 so 403/409 `USER_TOOK_OVER` is `Typed`. Add `run_control(run_id, org_id) -> {mode, epoch}` reading the Quarry projection with the service token (the service owns the run).
- `EC/browser_session.rs`: new refusal `TAKEN_OVER` (`browser_user_has_control`, "The person is using the browser. Wait for them to give it back."); `quarry_refusal` maps `Typed{code: "USER_TOOK_OVER"}` to it (not to `SESSION_FAILED`). With `EXECUTION_BROWSER_TAKEOVER_PAUSE=true`, a dispatch that gets `USER_TOOK_OVER` returns a pause request instead of a refusal; `browser.open` is refused with `TAKEN_OVER` while the run's session is held by the person (no new Quarry run may be started to get around a takeover).
- `EC/runtime_loop/agent.rs`: `pause_for_approval` with cause `browser_human_assist`, `assist_origin=person`, `assist_kind=user_takeover`, `host` = session host; tool input of the pause = the browser call that was refused (it re-runs after consume). On resume (WS03 in-place), before re-dispatch, call `run_control`: if the mode is still `human_takeover`, pause again under the same approval key (idempotent), never act.
- Event sink: emit `BrowserRunPaused{reason:user_takeover, control_epoch}` at the pause and `BrowserRunResumed{reason:user_takeover, control_epoch}` at the re-dispatch, through the Do-loop event sink that WS21-S6 adds for `browser_session_opened` (D2-935). If that sink is not there yet, S1 adds the sink (same file, same change), and WS21-S6 reuses it.
- `BFF/domains/browser.rs`: no new route. The SPA uses the existing `POST /api/v1/browser/sessions/{quarry_run_id}/control` with `expectedEpoch`. `forward_quarry_failure` already maps Quarry errors; add `browser_control_epoch_stale` no/en copy only if WS9.I has not (check `FES/shared/i18n/errors.ts` at slice start).
- `FES/features/chat/lib/chat-run-watch.ts`: `browserSessionFromChatRun` takes `sessionId` from the WS21-S6 `browser_session_opened` binding (D3), `controlMode`/`controlEpoch` from the owner projection (poll on control change, or the `/events/stream` route), and the live frame URLs (`/frames/stream`) when the switch is on; keeps `sessionId: undefined` when off (today's behaviour).
- `FES/features/chat/components/ChatLiveRunPanel.tsx`: pass `onBrowserControlMode` to `BrowserChrome` when `VITE_CHAT_BROWSER_TAKEOVER` is on and the run is not ZDR. Give back = control flip to `agent_control` with `expectedEpoch`, then decide the open `browser_human_assist` approval (granted, through the existing WS13 decide action) and continue (WS03 `/continue`). Order is safe either way: the executor re-checks control before acting.

**Flags.** `EXECUTION_BROWSER_TAKEOVER_PAUSE` (execution-core, default false); `VITE_CHAT_BROWSER_TAKEOVER` (SPA, default off). Requires `EXECUTION_DO_BROWSER_ENABLED`, `BROWSER_DO_ENABLED` and Quarry `QUARRY_EDGE__TAKEOVER_ENFORCEMENT=1`. With the execution-core flag off, `USER_TOOK_OVER` still maps to the `TAKEN_OVER` refusal (a correctness fix that ships on).

**Tests.**
- `cd "apps/Model Plane/rust" && cargo test -p mp-contracts approval_cause` (golden has 11 codes, `browser_human_assist` scope once).
- `cargo test -p execution-core --lib browser_session` (Windows: `--lib`, two targets fail pre-existing): `user_took_over_maps_to_taken_over_not_session_failed`, `takeover_pauses_with_human_assist_cause_when_switch_on`, `open_is_refused_while_person_holds_control`, `resume_rechecks_control_and_repauses_when_still_human`, `resume_after_give_back_redispatches_the_refused_call_once`, `pause_and_resume_emit_browser_run_paused_and_resumed_with_epoch`.
- `cargo test -p execution-core --lib quarry_agent`: `typed_errors_parse_code_and_details`, `user_took_over_is_typed` (replaces the pinned untyped case).
- `python3 "apps/Model Plane/scripts/gen-sse-event-taxonomy.py" --check`; `buf lint` and `buf breaking proto --against proto/baseline.binpb` (from `apps/Model Plane`).
- SPA: `cd "apps/Frontend Plane/verevonv3" && pnpm exec vitest run src/features/chat/components/ChatLiveRunPanel.takeover.test.tsx src/features/chat/lib/chat-run-watch.test.ts src/shared/approvals/approval-copy.test.ts` (toggle hidden when switch off, ZDR, or no session id; give back sends `expectedEpoch`; cause copy nb/en); `node_modules/.bin/tsc6 -p tsconfig.app.json` (diff against the pre-existing error set); `pnpm lint`.
- Playwright (local): NEW `tests/e2e/chat-browser-takeover.spec.ts`; extend `tests/e2e/browser-workspace-live-takeover.spec.ts` only for shared helpers.

**Live proof and evidence** [verify live]. Stack with Q-1 deployed and enforcement on: a Do turn opens `https://example.com`; the panel shows the live frame; Take over: projection `controlMode=human_takeover`, epoch 1; the next agent call gets `USER_TOOK_OVER`, the run shows the card and `BrowserRunPaused{reason:user_takeover}` in run events; the person scrolls; Give back: epoch 2, approval consumed once, `BrowserRunResumed`, the run completes. Negative: a second user of the same org gets 404 on the control route; `curl` the control route with `expectedEpoch: 0` after give-back gets the stale-epoch error. Evidence: run event dump, Quarry projection JSON before/after, screenshots of the three states, dev server port of the worktree (not :5173, memory "Verevon v3 worktree verification").

**Effort.** M, 5-7 d (contract+proto 1, execution-core 2-3, SPA 2, proof 1). **Depends on:** WS21-S5a live (open works), WS21-S6 binding (D3), D1 + Q-1 for the FE half and proof; WS03 S2/S7, WS13 S1/S2 (done). **Risks:** event sink ownership collides with WS21-S6 (one editor for `browser_session.rs`); the hot files `runtime_loop/agent.rs`, `action-registry.ts`, `ChatLiveRunPanel.tsx` (round-A hot-file matrix rules).

### S2 - Agent-requested help on a login or bot check [BUILD]

**Goal.** When the page the agent opened shows a strong login, CAPTCHA, consent or WAF challenge that Quarry could not recover, the run asks the person for help instead of telling the model the site is blocked.

**Files and changes.**
- `EC/browser_session.rs` `page_view` (506-510): with `EXECUTION_BROWSER_HUMAN_ASSIST=true`, a strong challenge whose kind is `login|captcha|consent|waf` and recovery exhausted (or Quarry `details.next == "user_takeover_or_other_source"` on the typed 403) returns a pause request with `assist_origin=agent`, `assist_kind=<kind>`; other kinds (`rate_limit`, `access_denied`, `paywall`, `geo_blocked`, `unknown`) keep `BLOCKED`. At most 2 help requests per run (third becomes `BLOCKED`), so a page cannot keep the run waiting.
- `EC/runtime_loop/agent.rs`: same pause path as S1 (cause `browser_human_assist`). The approval's tool input is `browser.observe{}` (after help, the run looks again).
- `FES/shared/approvals/approval-copy.ts`: per `assist_kind` copy, nb and en ("The site asks you to sign in on example.com. Take over, sign in, then give back." / "Nettstedet ber deg logge inn ..."); actions: "Take over" (flips Quarry control to `human_takeover` through the S1 control call and opens the panel) and "Skip this site" (deny; the model gets `browser_help_declined` and the session closes). Host shown from the approval record only.
- WS16 copy: attention rail label and Web Push text for the cause carry no host ("Verevon needs you in the browser"), because push text can show on a lock screen.
- D9 copy: a sign-in that leaves the fence ends with Quarry's navigation refusal; the card text says sign-in through another site is not supported yet.

**Flag.** `EXECUTION_BROWSER_HUMAN_ASSIST` (execution-core, default false). Turned on only after S1 is live-proven and Q-1 is deployed (without a Take over path the card would only offer Skip).

**Tests.** `cargo test -p execution-core --lib browser_session`: `strong_login_challenge_raises_human_assist_when_switch_on`, `strong_captcha_raises_human_assist`, `rate_limit_and_paywall_stay_blocked`, `switch_off_keeps_blocked_refusal`, `third_help_request_in_a_run_is_blocked`, `declined_help_closes_session_and_tells_model`, `assist_cause_carries_kind_origin_and_host_only`. SPA: `approval-copy.test.ts` (every `assist_kind` has nb and en copy; push copy has no host), `ChatMessages.human-assist.test.tsx` (card renders Take over and Skip; Take over calls the control action with the projection epoch).

**Live proof** [verify live]. A local fixture page served from the crawl-fixture overlay (memory: Ingestion deploy recipe) that Quarry classifies as a strong `login` challenge; the run pauses with the card; Take over, fill the fixture form, Give back; the run observes the logged-in page and continues. Evidence: run events, approval row (cause JSON), screenshots. If Quarry cannot be made to classify a fixture as `login` deterministically, prove with a unit fake and record the live gap.

**Effort.** M, 4-5 d. **Depends on:** S1. **Risks:** challenge classification false positives (a consent banner pausing a run); the 2-per-run cap and the kind list limit it.

### S3 - Hand-back note and fresh snapshot; human input never reaches the model [BUILD]

**Goal.** After give-back the model gets one fresh observation and, if the person wrote one, their note. Nothing the person typed into the page and none of the pages they visited during the takeover is given to the model.

**Files and changes.**
- `FES/features/chat/components/ChatLiveRunPanel.tsx` / the card: optional note field on Give back (max 500 chars), sent as the approval `decision_reason` (`DecideApprovalRequest.decision_reason = 4`, existing).
- `EC/runtime_loop/agent.rs` resume branch for `browser_human_assist`: before re-dispatch, one `observe` (new snapshot generation); the tool result is the page view plus a framed line `<person_note>...</person_note>` (escaped like `wrap_untrusted`, `EC/browser_session.rs:499-503`) when the note is non-empty. The note is never logged (log the length only).
- No code path in execution-core reads Quarry receipts, the owner timeline or devtools of the run (assert by test that the client exposes no such call; `QuarryAgentClient` today has `start_run`, `step`, `close_run`, `ready`, plus S1's `run_control`).

**Tests.** `resume_observes_once_before_redispatch`, `note_is_framed_and_capped`, `empty_note_adds_no_line`, `note_never_logged` (tracing capture), `client_has_no_receipt_or_timeline_reader` (compile-level: a test lists the client's public methods via a trait the test implements). Command: `cargo test -p execution-core --lib browser_session runtime_loop`.

**Live proof.** Part of S6: grep run events, transcript and execution-core logs for a marker string the tester typed into the page (expect 0 hits) and for the note marker (expect it only in the model input record, not in logs).

**Effort.** S, 1-2 d. **Depends on:** S1. **Risks:** the note is model input, so it is subject to WS26 prompt-injection framing like any user text.

### S4 - Hand-back hygiene [BUILD]

**Goal.** The first observation after a takeover does not carry values the person typed (passwords, one-time codes, usernames); the person's login lives no longer than the run.

**Files and changes.**
- `EC/browser_session.rs` `RefView::from` / `page_view`: after a hand-back (session flag `typed_by_person` set when the resume came from a `browser_human_assist` approval), drop the `text` of targets whose role is `textbox`, `searchbox`, `combobox` or `spinbutton` for every observation until the next `browser.open` (navigation to a new page by the agent clears it). The real fix is Quarry-side redaction of field values (Q-2, F-t2-021); this is defence in depth and must not be read as the guarantee.
- `EC/browser_session.rs` open path: assert the Do loop never sends `persist_profile` or a `profile_id` (`start_run` takes `profile_id: Option<String>`, `EC/quarry_agent.rs:816-831`; keep it `None`); release on run end already closes the Quarry run and revokes the grant (`release`, `browser_session.rs:551` and `release_run` 786).
- Do not reopen during takeover: covered by S1's `TAKEN_OVER` on `browser.open`.

**Tests.** `typed_field_text_dropped_after_handback`, `field_text_kept_before_any_takeover`, `agent_open_clears_typed_by_person`, `do_loop_never_requests_persistent_profile`, `run_end_closes_quarry_run_after_handback`. Command: `cargo test -p execution-core --lib browser_session`.

**Live proof.** S6 evidence item: type a marker password into the fixture form during takeover; grep the model input records and run events for it: 0 hits [verify live].

**Effort.** S, 2 d. **Depends on:** S1. **Risks:** role-based dropping can hide a field the agent must read back; accepted for the first observation window only.

### S5 - Wait budget and expiry across a long takeover [BUILD]

**Goal.** A takeover can last up to the assist TTL without breaking the run, and a timeout ends cleanly.

**Files and changes.**
- `EC/runtime_loop/agent.rs`: approval TTL for `browser_human_assist` = `EXECUTION_BROWSER_ASSIST_TTL_SECONDS` (default 1800, clamp 300-3600) instead of the 3600 default.
- Resume: the `/continue` call carries the person's click-time bearers (DEC-02 (b)); before re-dispatch, revalidate the run grant with the fresh browser bearer (`ValidateGrant`); if the grant expired (run grant TTL is 30 min, `EC/browser_session.rs:67`; broker cap `BROWSER_GRANT_MAX_TTL`), return `browser_session_lost` to the model (it may reopen; the login is lost and the copy says so).
- Timeout: on `APPROVAL_STATE_TIMED_OUT` for this cause, the model gets `browser_help_timed_out` ("The person did not give the browser back in time."), the session is released, and the Quarry run is closed (control stays with the person until close; Quarry ends the run).
- Bearer expiry while the run waits is not an issue for the pause itself (no bearer is used while paused). If WS03-S8 park is on and the resume finds no fresh bearer, park with `browser_credential_expired`; otherwise refuse with `CREDENTIAL_EXPIRED` (`EC/browser_session.rs:99-102`).

**Flag.** `EXECUTION_BROWSER_ASSIST_TTL_SECONDS` (config, not a switch).

**Tests.** `assist_ttl_defaults_to_1800_and_clamps`, `expired_grant_at_resume_maps_to_session_lost`, `timed_out_assist_releases_session_and_tells_model`, `resume_uses_click_time_bearer_not_stored_one`. Command: `cargo test -p execution-core --lib browser_session runtime_loop`.

**Live proof** [verify live]. Shorten TTL to 300 s on the evidence stack; take over and wait; the run shows the timeout, the Quarry run is gone from the owner's projection (404). Q-5 decides whether Quarry's lease survives 1800 s of takeover for a service-started run.

**Effort.** S-M, 2-3 d. **Depends on:** S1; WS03-S8 for the park branch (optional). **Risks:** Quarry lease idle sweep ending the lease during a takeover (Q-5).

### S6 - Flip, document, prove [FINISH]

- ZDR rows (section 5) and refusal key added; `zdr-contract-sync` gate green (note D2-1060: the gate was red before WS21-S10; clear or record).
- Ledger entry in `MP/docs/decisions/ledger.md` (CRLF): D1-D9 rulings as decided, the cause code, the proto fields.
- Depth register rows for anything not proven.
- Flip order on the dev stack: Quarry `QUARRY_EDGE__TAKEOVER_ENFORCEMENT=1` and Q-1's switch; `EXECUTION_BROWSER_TAKEOVER_PAUSE=true`; `VITE_CHAT_BROWSER_TAKEOVER=1`; then `EXECUTION_BROWSER_HUMAN_ASSIST=true`. Production defaults stay off until the owner says so.
- Live proof: section 6.
- Effort S, 2-3 d.

## 5. ZDR clause

**Posture.** The Do browser is refused under ZDR at offer and dispatch (`CON/zdr-posture-matrix.json:799-811`), so no ZDR run can reach a takeover or a help request. EP-1 keeps that as a requirement, not an accident: every new path refuses ZDR and waived runs explicitly.

| New store or egress | Content | ZDR behaviour | Erasure |
|---|---|---|---|
| Approval row with cause `browser_human_assist` (session-core `approvals`, cause in `approvals.metadata`) | code, assist kind, origin, host | never created (no Do browser under ZDR); the pause path checks the run's ZDR flag and refuses with feature `browser_human_assist` | with the thread and user (existing approvals erasure in `gdpr.rs`) |
| `decision_reason` holding the hand-back note | person's text, max 500 chars | never created | same as above |
| `BrowserRunPaused/Resumed` events with `reason`, `control_epoch` | ids, closed enum, number | events of a ZDR run are not persisted (events envelope `zdr`, `PROTO/events.proto:50-52`, `zdr = 13`) | with the run |
| Quarry timeline `Control` event and `skipped` receipt | Quarry-owned | Quarry writes nothing for ZDR runs (`control.rs:77-82`, `admission.rs:363-366`) | Quarry's |
| Live frames to the SPA | page pixels, live only | not stored by Model Plane or BFF; no Do browser under ZDR | none |
| Web Push text for the cause | fixed copy, no host | never sent for ZDR | none stored |

**Refusal behaviour.** If a takeover pause is somehow requested on a ZDR or waived run, execution-core refuses with `Refusal::feature_not_supported("browser_human_assist")` (the existing ZDR refusal shape) and the model gets `browser_unavailable`; no approval row is written. The SPA never shows the take-over control for a ZDR thread (`zdr` prop, read at `ChatLiveRunPanel.tsx:466`).

**Golden cases.**
- `CON/zdr-posture-matrix.json` `enforcement_rows`: `e_browser_human_assist_refused_under_zdr` (explicit ZDR bearer, expect `decision: zdr, code: explicit_zdr`); `e_browser_human_assist_refused_under_org_zdr` (org posture ZDR, expect `org_zdr`). Both reference the dispatch refusal; the loader requires the `e_` prefix (D2-1061 note).
- `CON/zdr-refusals.json`: new key `browser_human_assist`, `label_en` "browser help requests".
- Row-count assertions in `zdr_posture_matrix.rs` and `zdr_refusal_inventory.rs` change (round-A hot-file row).

## 6. Proof plan and cross-session requests

**Requests to Quarry (Quarry owns the code; this design only states the contract EP-1 consumes).**
- **Q-1 (D1a), S, blocks S1 FE half and S6.** When a service principal starts a run under a grant whose `owner_id` is a user, stamp it as the run's beneficiary (checkpoint field, survives resume). `caller_owns_browser_run` accepts the beneficiary (user principal, same org) for projection, frames, events stream, control, steps and close. The agent remains the service principal; `derive_initiator` unchanged, so the person's steps are `Human`. Behind a Quarry switch (name Quarry's; suggested `QUARRY_EDGE__BENEFICIARY_CONTROL`, default off). Tests Quarry should own: beneficiary can flip control; another org member gets 404; a service cannot act while the beneficiary holds control (409); ZDR run: control works live, nothing persists.
- **Q-2, blocks S6 default-on (not the build).** F-t2-021 field-value redaction in observations taken after a person's steps (same prerequisite WS21-S9 lists). S4 is only the Model-side guard.
- **Q-3 (D7).** `read_only` grants refuse mutating actions for the agent initiator only; K3-6 contract test states it.
- **Q-4.** `QUARRY_EDGE__TAKEOVER_ENFORCEMENT=1` on dev and evidence stacks (production default is Quarry's call) and the WS2.C stack evidence recorded.
- **Q-5 (D5).** A service-started run with a beneficiary in `human_takeover` is not idle-swept for at least 1800 s (Quarry plan D8-3 number).

**Space session.** In a Space thread only the run initiator (the grant owner) may take over; other members never see the control. Covered by Q-1's owner rule; add SPA test `space_member_sees_no_take_over` in S1.

**Live proof (S6), one evidence run** [verify live]. Preconditions: WS21-S5a open works live (read-only grant policy decided, ledger 5513), WS21-S6 binding, Q-1, Q-4.
1. Do turn opens the fixture page; panel shows live frames.
2. Manual take over; agent call refused; card shown; `BrowserRunPaused{user_takeover}`.
3. Person types a marker password into a fixture field; gives back with note "use the second result".
4. Run observes once, continues, completes.
5. Grep model input records, run events, transcript, execution-core and gateway logs for the marker password: 0 hits; the note appears only in the model input.
6. Agent-requested help on the fixture login challenge; Take over; Give back; continue.
7. Skip on a second challenge: `browser_help_declined`, session closed.
8. Second user, same org: 404 on control. ZDR thread: no control shown, no Do browser offered.
9. Timeout with TTL 300 s: `browser_help_timed_out`, Quarry run closed.
Evidence: run event dumps, approval rows (cause JSON), Quarry projections, screenshots, grep outputs, commands with exit codes, image ids of rebuilt services (execution-core, model-gateway, session-core, verevon gateway, SPA dev server, quarry-edge).

## 7. Risks and owner decisions

**Risks.**
1. **Everything live waits on WS21.** No Do-browser open succeeds live today (ledger 5513). A slip in WS21-S5a/S6/S10 moves all of EP-1's proof. Build is still possible behind switches.
2. **Authority creep.** D1(a) widens who can address a Quarry run (the grant owner, not only the starter). Mitigated: only the person the broker minted the grant for, same org, and the agent identity is unchanged. Quarry must agree; it departs from ADR-3's user-owned-run model for agent-started runs.
3. **Prompt injection and social engineering.** A page could look like a login wall to get the person to type credentials. Mitigations: system-raised help only (D4), fence-bound host shown from the approval record, 2 requests per run, model never sees typed values (S3, S4, Q-2).
4. **Credential leakage into model context.** S4 is defence in depth; the guarantee is Q-2. Do not default-on before Q-2.
5. **Expiry chain.** Grant 30 min, bearer about 300 s, Quarry lease idle, assist TTL 1800 s: a long takeover ends with a lost session and a lost login (S5 states it; Q-5).
6. **Hot files.** `runtime_loop/agent.rs`, `browser_session.rs` (also WS21-S6), `action-registry.ts`, `ChatLiveRunPanel.tsx`, `approval-copy.ts`, `zdr_posture_matrix.rs`: one slice at a time, CRLF preserved, append-only (round-A matrix rules).
7. **Two takeover models in one product.** Chat Do runs use whole-run control (D1a); co-browsing uses per-tab attach (Quarry WS8). The SPA must render both from one `BrowserSessionViewModel` (overlap-quarry row 3); S1 only fills `sessionId`, `controlMode`, `controlEpoch` and frame URLs, the fields WS9.I already reads.
8. **Effort above the audit's M.** MP+FE 16-22 engineer-days plus Quarry Q-1 (S). The audit priced the toggle as wiring; the ownership gap makes it larger.

**Owner decisions (in order of what they unblock).**
- D1 (with the Quarry session): grant owner of record vs attach for chat Do runs. Unblocks S1 FE half, S2 Take over, S6.
- D4: system-raised help only in W3 (recommended) vs a model tool.
- D7 (Quarry): read-only applies to the agent only.
- D5: 1800 s assist TTL.
- D9: no cross-host sign-in in W3.
- D2, D3, D6, D8 are recommended defaults with low risk; record them in the ledger at S6 unless the owner objects.

**Reservations for the integrator (`00-integration-round-a.md` section 1).** Migrations: none (session-core head 0071, capability-core head 0030, unchanged; `browser_human_assist` lives in approval metadata; no new capability id because no new tool name is added). Proto: `ApprovalCause.host = 11` (shared with WS21-S6), `ApprovalCause.assist_kind = 13`, `ApprovalCause.assist_origin = 14` (12 left for WS21-S6 `max_level`); `OrchestrationEvent.BrowserRunPaused.reason = 3, control_epoch = 4`; `BrowserRunResumed.reason = 3, control_epoch = 4` (`reason` shared with Quarry plan WS8.G R-11). No `EventType` value, no `OrchestrationEvent` tag. Flags (all new, default off, no clash with round A's 47): `EXECUTION_BROWSER_TAKEOVER_PAUSE`, `EXECUTION_BROWSER_HUMAN_ASSIST`, `EXECUTION_BROWSER_ASSIST_TTL_SECONDS` (config), `VITE_CHAT_BROWSER_TAKEOVER`; Quarry-owned `QUARRY_EDGE__TAKEOVER_ENFORCEMENT` (exists) and Q-1's switch.
