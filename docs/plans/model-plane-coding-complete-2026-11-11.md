# Model Plane coding-complete report (waves 1-5, 2026-11-11)

Branch `mp-w345`, cut from main `031e7965d`. Code only: every new behaviour is dark behind a default-off switch (strict literal `true`) unless a row says otherwise. Nothing here is tested or live-proven; after each batch only the compile-only check ran (Rust `cargo check --all-targets`, Go `build` + `vet`, TypeScript `tsc6` against the baseline). The verification phase owes the tests, the generated artifacts (FUGEN) and the live proofs listed in `docs/plans/model-plane-verification-plan-2026-11-11.md`. Owner decisions are in `docs/plans/model-plane-open-decisions.md`.

## 1. Summary

| Status | Units |
|---|---|
| built | 274 |
| superseded | 11 |
| proof_only | 3 |
| partial | 2 |
| closed_later | 2 |
| already_built | 1 |

Status words: built = code written and compile-checked; already_built = the code was there; partial = part built, the rest carried by a follow-up unit; superseded = replaced by another unit; closed_later = closed by a later unit; proof_only = only tests or a live proof owed; blocked = the writer stopped, re-run later; not built = written as a unit, not built in this phase (budget stop or waits on FUGEN).

## 2. Batches and commits

| Batch | Commit | Units |
|---|---|---|
| B01 | 80261d66b | 16 |
| B02 | 7676e30ac | 19 |
| B09 | a66c48326 | 12 |
| B10 | 349abd38c | 20 |
| B11 | 4b0cf580a | 15 |
| B12 | 5d5096414 | 16 |
| B13 | 63f62cb0f | 18 |
| B14 | 1c5394660 | 19 |
| B15 | 0120a8e84 | 16 |
| B16 | d5f89aab0 | 15 |
| B17 | 211b2a404 | 14 |
| B18 | 68e247254 | 3 |
| B03a | db96e60e7 | 10 |
| B03c | 8c1247ed7 | 19 |
| B04y | 60d20e799 | 15 |
| B06m | e8d2964b3 | 7 |
| B07m | 547a78454 | 5 |
| B08x | 4729b8145 | 19 |
| B11s | 8c43757d4 | 12 |
| B11t | 2db962669 | 7 |
| X-settled | - | 18 |

## 3. Units per wave

### W2 (66 units: built 64, partial 1, superseded 1)

| Unit | Status | Batch | Title |
|---|---|---|---|
| F-f02-10 | built | B13 | session-core: a scheduled run binding is refused when the run is zero-retention or waived |
| F-f02-18 | built | B13 | browser-broker PgStore: a read-only parent cannot mint a sensitive approval |
| F-f03-18 | built | B13 | Memory write policy: the person origin is accepted only from a person-bound credential (or the one trusted checker) |
| F-f05-01-executestep-space-search | built | B13 | ExecuteStep withholds a Space-scoped knowledge_search instead of searching the whole organisation |
| F-f06-01 | built | B13 | model-gateway: the thread workspace routes check thread ownership with Session Core before any sandbox RPC |
| F-f06-19 | built | B13 | Resumed and continued Do runs obey the org's lockdown and read-only mode cap, not only the pause |
| F-f06-21 | built | B14 | Standalone browser runs are admitted against the org's pause, lockdown and read-only mode cap |
| F-f10-29 | partial | B13 | scheduled Do lane: the caller and the run request carry the run's real ZDR and waiver, so the retention refusal can fire |
| FU-26-executestep-space-list-refuse | built | B12 | ExecuteStep refuses knowledge_list_documents for a Space-scoped step using ExecuteStepRequest.space_id |
| P0-D2-1149 | built | B11 | Waived approval writes: create and decide refuse a waiver credential before metadata is stored |
| P0-D2-460 | built | B11 | Graph grounding forwards the Space read decision on scoped turns and skips org-wide graph when no decision exists |
| P0-D2-461 | built | B11 | Follow-up turns on an existing thread resolve the thread's Space server-side when the client omits space_ref |
| P0-D2-480 | built | B11 | Capability-core memory writes for user and org scope go through the Session Core content gate before the INSERT, UPDATE or DELETE |
| P0-D2-481 | built | B11 | Orchestrator consolidated-memory writes pass the Session Core content gate before letta-bridge IndexMemory |
| P0-D2-509 | built | B11 | Person-written org-scope skills require the org admin role in Session Core, as registry_apis does |
| P0-D2-769 | built | B11 | knowledge_list_documents refuses or scopes to the org when a Space-scoped Do run calls it |
| R2-OD-12-a-versions-refusals | built | B14 | Message versions: session-core refuses a waived caller typed, the gateway passes catalogue refusals through, the branch route joins the durable-mutati |
| R2-OD-12-b-matrix-rows | built | B14 | Posture matrix: two message-version enforcement rows, the consumers' row counts, and the ZDR.md boundary row |
| W2-SWITCH-REG-a | built | B11s | Register switches of partial rows in the compose file and the Control flag registry |
| W2-WS13-S6-bff | built | B14 | BFF: pass-through routes for thread approval grants (list, revoke) and rule proposals (list, confirm, dismiss) |
| W2-WS13-S6-gw-grants | built | B15 | model-gateway: list a thread's approval grants and revoke one, owner-bound; reads and the narrowing revoke work with the switch off |
| W2-WS13-S6-spa-grants | built | B14 | SPA: grant client, approvals.list_grants / approvals.revoke_grant contracts, and the thread's active grants with Revoke |
| W2-WS13-S7-grant-digest | built | B15 | Standing thread/run grants bind to the pinned MCP definition digest: drift means no match, a re-decision rebinds |
| W2-WS13-S7-gw-proposals | built | B16 | model-gateway: list, confirm and dismiss rule proposals (claimed server-side confirm writes the capability-core rule), Always offer projected |
| W2-WS13-S7-producers-a | built | B14 | Approval cause contract: capability_id on ApprovalCause, CallFacts builder, and Always offered only for the pause a confirmed rule lifts |
| W2-WS13-S7-producers-b | built | B15 | execution-core: pause causes carry the verified effect class, pinned tool digest and (only for a rule-liftable pause) the capability id |
| W2-WS13-S7-proposal-store-a | built | B15 | session-core store: proposals carry their capability id and policy link, a confirm claims the row first, owner-bound list/get/resolve |
| W2-WS13-S7-proposal-store-b | built | B16 | session-core: ApprovalRuleProposalService list/get/resolve for the owner; confirm needs the switch, dismiss does not; waived callers cannot resolve |
| W2-WS13-S7-rule-expiry | built | B16 | capability-core: a sweeper writes one content-free tool_policy_rule_expired row per lapsed allow |
| W2-WS13-S7-rule-use-a | built | B15 | EvaluatePolicyRequest.run_retention: execution-core states the run's retention to capability-core |
| W2-WS13-S7-rule-use-b | built | B16 | capability-core: a rule-relaxed call writes a content-free tool_policy_rule_used row; a person's rule never relaxes a retention-bound run |
| W2-WS13-S7-spa-decide | built | B15 | SPA: the chosen scope and Always reach the decide route, and the card offers Always when the server says so |
| W2-WS13-S7-spa-proposals | built | B15 | SPA: rule proposal contracts and client through the gateway, and the settings section that lists, confirms and dismisses them |
| W2-WS13-S7-switches | built | B14 | Register the WS13 approval switches (scope grants, rule proposals, approval audit, rule audit, shell rules, two SPA flags) in compose and the flag reg |
| W2-WS13-S7-user-erasure | built | B16 | session-core: a person's erasure removes their standing approval grants and rule proposals |
| W2-WS13-S7-waived-decided | built | B13 | session-core: no approval_decided audit row for a decision on a zero-retention or waived run |
| W2-WS13-S7-zdr-row | built | B17 | ZDR.md: one boundary row for approval standing grants and rules, written after the code it describes |
| W2-WS14-S5-a | built | B13 | mp-contracts: a namespaced input-pause reference, the resume answer window, a strict resume answer codec and the fold extension keys for offered tools |
| W2-WS14-S5-b1 | built | B16 | session-core store: spend a resume answer once, inside its window, on a live run, renewing the recovery deadline; a skip is spendable; a lapsed answer |
| W2-WS14-S5-b2 | built | B16 | session-core: accept a resume question only behind its paused input fold (namespaced reference), behind AGENT_ASK_USER_RESUME_ENABLED; compose and ZDR |
| W2-WS14-S5-b3 | built | B14 | session-core loop checkpoints: an input-pause fold records the thread position, a claim refuses it once the thread moved, and a claim against another  |
| W2-WS14-S5-c1 | built | B16 | execution-core fold writer: every fold records the screening taint and the turn's connector selection, and can carry the offered tool definitions |
| W2-WS14-S5-c2 | built | B16 | execution-core: in-place approval resumes and park resumes restore the screening taint and the connector selection from the fold, never looser; an abs |
| W2-WS14-S5-d | built | B17 | Approval stage-1 continuations carry the taint and the connector selection through the retained descriptor (additive ApprovedAuthority fields, next fr |
| W2-WS14-S5-e1 | built | B16 | execution-core building blocks for the input pause: resume delivery rule, the answer as a tool result, PauseFold::save_input, RunStatus::AwaitingInput |
| W2-WS14-S5-e2 | built | B17 | execution-core: ask_user with resume delivery pauses the run as awaiting_input, fold first, then the resume question; falls back to next_turn only on  |
| W2-WS14-S5-f1 | built | B17 | execution-core pure checks for a question continuation: the target question, the input-fold check, the claim classification, the stage decision and th |
| W2-WS14-S5-f2 | built | B17 | execution-core: a question continuation claims the input fold, spends the answer once and continues the paused loop with it; when it cannot, it wins t |
| W2-WS14-S5-g | built | B17 | model-gateway: the question form of /continue (read the question, record the answer only if still requested, then resume) and the awaiting_input relay |
| W2-WS14-S5-h | built | B17 | BFF relay of the question form of /continue behind an AppState flag, and the action contract (chat.run.continue question form, closed refusals, a ques |
| W2-WS14-S5-i | built | B17 | SPA: a resume question's card answers through chat.run.continue's question form and streams the same run; awaiting_input; the answer is sent as the ne |
| W2-WS14-S6 | built | B12 | Ask loop: offer and dispatch ask_user (end-turn, next_turn delivery) behind MODEL_GATEWAY_ASK_USER_ASK_MODE_ENABLED, default off; ZDR and waived turns |
| W2-WS17-S9-a | built | B12 | Stream endpoint serves regenerate and edit on versioned threads behind MODEL_GATEWAY_MESSAGE_VERSIONS_ENABLED |
| W2-WS17-S9-b | built | B12 | SPA: pass server version badge and version-switch handler to the answer row in ChatPage |
| W2-WS18-S5-a | built | B12 | Do runs emit the Space knowledge receipt (retrieval, not_entitled or not_searched) after connected |
| W2-WS18-S5-b | superseded | X-settled | Refuse org-wide knowledge.list_documents in Space-scoped Do runs (typed refusal) |
| W2-WS19-S8-a | built | B11t | Name the mount path for skill bundles in the bundle footer and in the code_interpreter description |
| W2-WS19-S8-b | built | B11s | Register SKILL_BUNDLE_MOUNT_ENABLED in the Control Plane flag registry |
| W2-WS23-S3-a | built | B11t | Give the egress hold after a flagged connector read its own cause code, tainted_egress, in the closed approval vocabulary |
| W2-WS23-S3-b | built | B11s | Validate connection_id on the write path (execute_action) with the same guard the read paths use |
| W2-WS23-S3-c | built | B11s | Declare PROVIDER_READ_TOOL_ENABLED in compose, .env.example and the flag registry, default off |
| W2-WS23-S4-a | built | B12 | Record the selected connector ids on the run record (counts-free, ids only), never under ZDR or a waived run |
| W2-WS23-S6-a | built | B11s | Carry connectorScope through the pending chat launch so a Home-composer pick survives the handoff |
| W2-WS24-S4-a | built | B11t | Serialise the first Space-workspace setup per run so two concurrent calls cannot orphan a lease or a directory |
| W2-WS26-S2-a | built | B11s | Declare CONTENT_SAFETY_NORMALIZE in compose and the flag registry |
| W2-WS26-S2-b | built | B11s | Stop flagging valid emoji subdivision tag sequences as obfuscation.tag_block |

### W3 (91 units: built 86, proof_only 3, superseded 2)

| Unit | Status | Batch | Title |
|---|---|---|---|
| F-f08-12 | built | B13 | auth-core: API-key exchange refuses a banned key owner on both exchange paths |
| F-f10-01 | built | B13 | propose_shell_rule skips waived runs as well as ZDR runs |
| F-f10-04 | built | B13 | model-gateway fetch_url re-checks the page's final URL against the org's domain rules before the model reads it |
| F-f10-05 | built | B13 | model-gateway gRPC WebSearch drops hits on hosts the org's domain rules do not allow |
| F-f10-16 | built | B13 | session-core skill versions: a reader never sees a Space skill's history through an empty or foreign owner match |
| FU-01-skip-release | built | B11 | Skip on a held browser page releases it: denied approval id on continue + model reads the closer |
| FU-02-action-registry | built | B18 | Action registry hygiene: handler arms for every new action, final static count, stale schema comment |
| FU-03-gdpr-order | built | B10 | GDPR user erasure: hold check first, user_anonymize reaches profile + personal memory, content-free audit row |
| FU-09-zdr-catalogue-docs | built | B11t | ZDR catalogue follow-through: named_agent row, SPA feature labels, Quarry matrix count |
| FU-10-judge-evidence | built | B11 | Goal judge evidence: read a run's tool-call names and status by run id |
| FU-25-skip-continuation-retained-authority | built | B12 | A skipped-site continuation runs under the pause's retained authority and tools, never the /continue placeholders |
| FU-28-skip-authority-refusal-token | built | B14 | Register skip_authority_unavailable in the continue refusal lists (gateway, BFF, SPA) |
| FU-29-extract-structured-final-url-gate | built | B14 | extract_structured checks the final URL against the org's domain rules before the extraction inference |
| FU-30-zdr-spend-explicit-kind | built | B16 | ZDR spend rows travel as an explicit content-free kind that the publisher and cost-core admit by name, never as zdr=false |
| GAP-W3-EP-1-S5-timeout-copy | built | B11s | SPA: Norwegian/English copy for the terminal failure code browser_help_timed_out in the chat run panel and the assistant browser agent |
| P0-D3-956 | built | B11 | Org skill PATCH and DELETE in capability-core require the owner or an org admin, not any member |
| R2-OD-58-a-judge-principal | built | B13 | Control principal train: a dedicated persistent goal-judge principal for session-core with two narrow scopes |
| R2-OD-58-b-session-judge-gate | built | B14 | session-core: verdict writes and judge reads admit only the goal-judge principal with its own scopes, and refuse ZDR or waived threads before any read |
| R2-OD-58-c-gateway-judge-credential | built | B15 | model-gateway: mint the judge credential from the goal-judge principal, refuse a ZDR-scoped mint, never judge a turn whose thread check did not answer |
| R2-OD-59-a1-switch-attribution-helper | built | B13 | GDPR thread memory, part 1: the erasure switch registered dark, and the Space attribution loop extracted into one helper with no behaviour change |
| R2-OD-59-a2-thread-memory-erasure | built | B14 | GDPR thread memory, part 2: thread-hold check before any store, then erase the person's own thread memory and their part of Space-thread memory in the |
| R2-OD-59-a3-zdr-doc-row | built | B15 | GDPR thread memory, part 3: the ZDR.md agent_memory row names the thread-bound erasure |
| W3-EP-1-S2-skip | superseded | X-settled | Skip this site (denied browser_human_assist) releases the held page and tells the model browser_site_skipped |
| W3-EP-1-S5-timeout | proof_only | X-settled | End a run whose browser_human_assist takeover approval timed out with the known cause browser_help_timed_out |
| W3-EP-10-S7-a | built | B01 | already_always_ask uses the typed error envelope and gets SPA copy |
| W3-EP-11-S3-a | built | B01 | Pin skill versions on Do runs in execution-core and recover the pinned version |
| W3-EP-11-S3-b | built | B01 | orchestrator-core: feedback ratings and quarantine keyed by skill version |
| W3-EP-11-S7-a | built | B01 | Register /learn in the composer slash menu |
| W3-EP-13-S2-a | built | B01 | Model picker shows record v2: context window, preview chip, folded 'Eldre modeller' group |
| W3-EP-13-S5-a | built | B01 | ApiError keeps array details so per-row invalid_role_table issues reach the role editor |
| W3-EP-13-S8-a | built | B01 | Gemini enablement gate, code half: compose wiring off, release-gate Vertex check, posture export, ZDR row |
| W3-EP-14-S10-a | built | B01 | RoutingPolicy candidate flow in inference-core: client methods, PUT→202 candidate, promote route |
| W3-EP-14-S4-a | built | B01 | Run ledger: rounds from turn timing and a virtualised ledger list |
| W3-EP-14-S6-grant | built | B01 | Control registry: grant inference-core session:request-snapshots:write (persistent audience) |
| W3-EP-2-S7-fe | proof_only | X-settled | Composer capability deep_research_knowledge and enabling Deep research under Organization only |
| W3-EP-2-S8-ui | built | B02 | Mount the wide-research entry, send wideResearch on the turn and wire per-item retry |
| W3-EP-3-S5-reload | built | B01 | Relay context_refs and research_plan message metadata on thread reload and show the read-only plan card |
| W3-EP-4-S1-export | built | B01 | PDF export prints inline widgets and MCP app frames as their code, never the live iframe |
| W3-EP-4-S11-href | built | B01 | Open only http(s) search result URLs from the dashboard search panel |
| W3-EP-5-S3-deckguard | built | B01 | update_artifact refuses an artifact whose kind is deck |
| W3-EP-5-S7-ui | built | B02 | Mount DeckTemplatePicker in the deck view and composer and send deckTemplate on the turn |
| W3-EP-6-P3-a | built | B01 | P3 slice a: erase a named agent's memory on binding revocation (EraseAgentMemory RPC + BFF wiring) |
| W3-EP-6-P3-b | built | B02 | P3 slice b: 'What this agent remembers' list/delete with the named_agent ZDR refusal key |
| W3-EP-6-P3-c | built | B02 | P3 slice c: Space agent panel 'What this agent remembers' (FE + action contracts) |
| W3-EP-6-P3-d | built | B02 | P3 slice d: a named agent's thread list (filter-only on ListThreads) |
| W3-EP-6-S10-bff | built | B02 | BFF strips client agent_roles on every turn and composes the Space role catalogue for Do turns |
| W3-EP-6-S11-route | built | B01 | Admitted owner-actions read route: capability-core + model-gateway proxy |
| W3-EP-6-S11-ui | built | B02 | BFF route, client and AgentRunConsole use of the admitted-action set |
| W3-EP-6-S7-convex | proof_only | X-settled | Gate the public agent knowledge/retrieval mutations and mirror published fields onto the agents row |
| W3-EP-6-S8-preview | built | B02 | Builder draft preview panel, agentPreview send field, BFF switch on AppState and compose line |
| W3-EP-6-S9-do | built | B03a | Carry the agent tool allowlist to Do runs (RunAgentRequest 26) and enforce it in execution-core |
| W3-EP-6-S9-pin | built | B02 | Writer for spaceAgentBindings.pinnedVersion: Convex mutation, BFF route and action contract |
| W3-EP-6-S9-runmeta | built | B02 | Write runs.metadata.agent {ref, version, digest} at managed run start |
| W3-EP-7-S3-a | built | B02 | session-core goal verdicts: migration, RecordGoalVerdict RPC and the user override in UpdateThreadGoal |
| W3-EP-7-S3-b | built | B02 | Wire the goal judge after a settled completed goal turn and record the verdict |
| W3-EP-7-S3-c | built | B02 | GoalBar verdict chip, judge reasons and Mark done / Reopen |
| W3-EP-7-S5-a | built | B02 | RUN_ATTENTION sweep emits goal-stopped items (reason GOAL) |
| W3-EP-7-S5-b | built | B03a | Thread list goal_status through MG and BFF, sidebar goal chip and rail 'goal' reason |
| W3-EP-7-S6-a | superseded | X-settled | Model-gateway internal judge-turn route for unattended goal fires |
| W3-EP-7-S6-b | built | B03c | Orchestrator JudgeGoalTurnActivity, registration, service principal and compose lines |
| W3-EP-7-S6-c | built | B02 | Controller GoalAwayView wiring into GoalBar ('Keep working while away') |
| W3-EP-7-S6-d | built | B02 | Refuse Run now for goal-bound schedules and label them in the automations hub |
| W3-EP-8-S10-a | built | B03a | EraseAgentMemory RPC in session-core (Control-decided, per Space-agent binding) |
| W3-EP-8-S10-b | built | B03a | Erase agent memory on binding revocation: model-gateway route + BFF revoke call |
| W3-EP-8-S4-gdpr | built | B02 | GDPR user-subject erasure deletes the standing profile (and the user's personal memory) |
| W3-EP-8-S7-a | built | B02 | user-core issues model.space.memory.manage decisions (POST /api/v1/internal/spaces/memory-decision) |
| W3-EP-8-S7-b | built | B03a | Space memory tab: action contracts, BFF action dispatchers, mount in room details |
| W3-EP-8-S8-a | built | B02 | Personal first-run: route the onboarding settings handlers and save the role on the first turn |
| W3-EP-9-W3-a | built | B03a | org-core: let model-gateway and execution-core read browser-site resolve (org:browser-site:read:any) |
| ZDR-DEF-Z-06b-ledger-zdr-spend | built | B15 | Model Plane: ZDR spend is recorded in the ledger as ids, counts and amounts only |
| ZDR-Q-ERASE-request-snapshots | built | B12 | Session Core: user-subject erasure removes that user's request snapshots, content-free counts only |
| ZDR-Z-03a-org-core-posture-answer | built | B14 | org-core: compute the effective org ZDR posture once (matrix org rows) and serve it on the posture route and the org record |
| ZDR-Z-03b-auth-core-effective-posture | built | B14 | Auth Core: read org-core's effectiveZdr instead of computing intent AND entitlement, behind a switch |
| ZDR-Z-03c-bff-one-posture-reader | built | B15 | Gateway BFF: every org-posture read goes through zdr::org_posture, which reads org-core's effective posture, behind an AppState switch |
| ZDR-Z-03d-conversation-core-posture-reader | built | B14 | conversation-core: read org-core's effective posture instead of the raw interactiveRetention flag, behind a switch |
| ZDR-Z-03e-quarry-preflight-posture-reader | built | B17 | quarry-edge schedule preflight: read org-core's effectiveZdr instead of intent AND entitlement, behind a switch |
| ZDR-Z-05a-typed-refusals | built | B12 | Model Plane: web search and file output under ZDR become typed refusals, not silent drops |
| ZDR-Z-05b-spa-refusal-copy | built | B12 | SPA: render every ZDR refusal code, including the new feature refusals, in Norwegian and English |
| ZDR-Z-12-13-ic-capability-ack | built | B15 | integration-corev2: finish the capability record and acknowledgement ledger for the E2 read (posture fields, text minimum, strict switch, retention sw |
| ZDR-Z-14-auth-core-decision | built | B16 | Auth Core: POST /api/retention/zdr-decision with the ZDR_WAIVER_ENABLED kill switch, dark |
| ZDR-Z-14b-auth-core-waiver-mint | built | B16 | Auth Core: the user token routes stamp zdr_waiver from a verified x-retention-decision on every user audience except browser-broker and sandbox-manage |
| ZDR-Z-14e-auth-core-e2-client | built | B15 | Auth Core: the E2 client that reads one connection's ZDR posture from integration-corev2 as the session user |
| ZDR-Z-14s-auth-core-decision-signer | built | B15 | Auth Core: the dedicated retention-decision signer (RS256, file keys by key id, never in the JWKS), fail closed when absent |
| ZDR-Z-17-thread-retention-receipt | built | B14 | Session Core: a thread carries a retention receipt, and a waived append is checked against it |
| ZDR-Z-19a-bff-decision-client | built | B16 | Gateway BFF: the retention-decision client and the one turn_retention entry (policy token, decision call, Space resolution), dark |
| ZDR-Z-19b-bff-turn-waiver | built | B17 | Gateway BFF: decide the turn's retention first, mint every bearer of a waived turn with the decision, never a browser or sandbox bearer |
| ZDR-Z-19c-bff-ack-proxy | built | B16 | Gateway BFF: proxy integration-corev2's ZDR assessment and acknowledgement routes with the caller's own bearer, behind ZDR_WAIVER_UI_ENABLED |
| ZDR-Z-19d-spa-zdr-client | built | B17 | SPA: the connection ZDR API client, the action contracts and their registration, and the VITE_ZDR_WAIVER_UI type, dark |
| ZDR-Z-19e-bff-waived-continuation | built | B17 | Gateway BFF: a continuation of a waived run asks Auth Core for a fresh decision and relays it on the continuation's bearers |
| ZDR-Z-20-to-Z-21b-spa-waiver-ui | built | B17 | SPA: acknowledgement dialog, composer chip, per-answer retention line and the admin acknowledgement record, behind VITE_ZDR_WAIVER_UI |
| ZDR-Z-32-zdr2-flag-and-contract-wiring | built | B17 | Register the zdr2 switches in the flag registry and the vendored matrix and catalogue copies in the contract sync script |

### W4 (66 units: built 59, closed_later 1, partial 1, superseded 5)

| Unit | Status | Batch | Title |
|---|---|---|---|
| FU-04-quote-prefix | built | B10 | Widget quote chip prefixes the composer draft instead of replacing it |
| FU-07-plugin-killlist | built | B11t | Plugin kill list: read route, BFF pass-through, durable state and an admin lift action |
| FU-08-image-audit-cache | built | B10 | generate_image audit category and cached image tool-policy read |
| FU-12-bearer-link | built | B11t | MCP probe's bearer_required notice links into the stored-token connector flow |
| FU-13-snapshot-grant-cleanup | superseded | X-settled | Thread delete and org purge revoke a shared snapshot's Control view grants |
| FU-14-mcp-secret-ref-projection | built | B11 | Static-bearer MCP servers survive a gateway restart: capability-core projects the binding reference |
| FU-15-snapshot-membership | built | B10 | Control authz check: a shared snapshot is readable only by a current org member |
| FU-18-dryrun-connection-binding | built | B10 | Trigger dry run checks the webhook event came from the trigger's own connection |
| FU-19-custom-rest-connection-route | superseded | X-settled | integration-corev2: create a custom_rest connection bound to a credential binding |
| FU-21-thread-delete-delegated-credential | built | B12 | Thread delete forwards the owner's delegated credential so snapshot grants are revoked |
| FU-22-control-org-purge-grants | partial | B12 | user-core erases an org's resource grants (incl. snapshot view grants) on its own org purge |
| FU-27-user-core-org-erasure-consumer | built | B13 | user-core consumes the org erasure fan-out and deletes the org's resource grants (DeleteGrantsForOrg) |
| GAP-W4-BYO-1-turn-connection | built | B12 | Persist model_connection_id on a turn so regenerate, edit and retry never reach a platform provider |
| GAP-W4-BYO-2-ic-get-delete | built | B11 | Integration Core: list and delete customer-gateway connections (metadata only, admin-gated) |
| GAP-W4-BYO-5-cost-customer-billed | built | B11t | Classify customer-gateway calls as customer-billed in CostBasis (usage only, no platform spend) |
| GAP-W4-BYO-hidden-infer | built | B13 | Infer: refuse a hidden model id as model_hidden before any provider call |
| GAP-W4-BYO-picker | built | B12 | AiModelPicker: optional Your gateway group for customer-gateway connections, hidden for ZDR orgs |
| GAP-W4-BYO-policy-expected-version | built | B11s | Router policy PUT: read expected_version and answer 409 on a version mismatch |
| GAP-W4-CC-8-custom-rest-route | built | B11 | Integration Core: create a custom_rest connection from an active credential binding (admin only, flag-gated) |
| GAP-W4-P6-spill-symlink-residual | built | B11s | Spill: re-verify the spill directory and file after create_new so a swapped directory is refused |
| GAP-W4-P6-spill-workspace-filter | built | B12 | Lease release: never upload an unfiltered Space workspace; skip ZDR and waived runs before any upload |
| GAP-W4-PL-1-gateway-source-pins | built | B11s | Plugin registration through the gateway: require source_kind and source_pin when the plugin loader is on |
| W4-BYO-1 | built | B03c | Platform catalogue overlay in RoutingPolicy (hide, order, aliases) |
| W4-BYO-2 | built | B03c | Router policy page: catalogue overlay editor |
| W4-BYO-3 | built | B03c | Per-org catalogue overlay and role table (narrow-only) in session-core, read by inference-core |
| W4-BYO-4 | built | B03c | integration-corev2: customer-gateway model connection and inference broker |
| W4-BYO-5 | built | B03c | inference-core: customer-gateway provider selected by opaque model_connection_id |
| W4-BYO-6 | built | B03c | Gateway + SPA: add a customer gateway and pick it in the model picker |
| W4-CC-1 | built | B03a | MCP server probe route (no save) in model-gateway + BFF |
| W4-CC-2 | closed_later | X-settled | SPA probe-and-test add flow with a tool picker |
| W4-CC-3 | built | B08x | integration-corev2: credential bindings store and admin API (sealed at rest) |
| W4-CC-4 | built | B08x | integration-corev2: seal and redeem routes for credential references with audit |
| W4-CC-5 | built | B08x | model-gateway: static-bearer MCP servers via a credential binding reference |
| W4-CC-6 | built | B08x | integration-corev2: custom REST connector operation on the actions surface |
| W4-CC-7 | built | B09 | Model Plane: custom REST connector tool binding (read/write classes, approval on writes) |
| W4-CC-8 | superseded | X-settled | BFF + SPA: credential binding admin and custom connector add flow |
| W4-IMG-1 | built | B03c | inference-core: strict provider choice for image generation (no chain walk) |
| W4-IMG-2 | built | B03a | cost-core: per-image price rows and image usage costing (no seeded prices) |
| W4-IMG-3 | built | B04y | generate_image as a typed tool in the chat tool loop |
| W4-IMG-4 | built | B03a | SPA: generated-image tool card shows model and provider |
| W4-IO-1 | built | B04y | Attributed image-result strip under web-search answers |
| W4-IO-2 | superseded | X-settled | Widget selection-to-context over the postMessage bridge |
| W4-M2 | built | B03c | SPA frames an MCP app with the server-built CSP (srcdoc) |
| W4-P3 | built | B03c | BFF route for a run's background-process output (sandbox bearer + Space read decision) |
| W4-P4 | built | B03c | Computer panel in ChatLiveRunPanel: browser / terminal / files tabs with a live process tail |
| W4-P5 | built | B03a | Optional PTY for process_start (model-facing exec sessions) |
| W4-P6 | built | B03c | process_read output spill to a non-durable workspace file |
| W4-PL-1 | built | B03c | capability-core: plugin supply-chain pins (source, digest, kill list) |
| W4-PL-2 | built | B03c | capability-core: plugin manifest loader and admin install/uninstall |
| W4-PL-3 | built | B03c | Org-curated plugin catalogue and kill-list routes (capability-core + BFF) |
| W4-PL-4 | superseded | X-settled | SPA: plugin catalogue and install-consent dialog |
| W4-R2-b | built | B03c | Per-action brief in the chat rail's browser timeline |
| W4-R3-a | built | B04y | Browser walkthrough (filmstrip of screenshot refs) in RunProofBundle |
| W4-R3-b | built | B04y | Run Console proof panel renders the browser walkthrough filmstrip |
| W4-SH-1 | built | B03c | BFF proxy and SPA entry for importing a thread export |
| W4-SH-2 | built | B03c | Foreign export adapters (ChatGPT, Claude) in session-core thread import |
| W4-SH-3 | built | B03c | Control: session-core as owner service for conversation_snapshot grants |
| W4-SH-4 | built | B08x | session-core: share a snapshot with org members through Control grants; revoke removes grants |
| W4-SH-5 | built | B08x | session-core + MG: a granted org member reads a shared snapshot |
| W4-SH-6 | built | B08x | BFF + SPA: share dialog for snapshots and the shared-snapshot viewer page |
| W4-T3 | built | B08x | Bind an event trigger to a schedule template (what a fire runs) |
| W4-T4 | built | B08x | Start a budgeted run for each authorized trigger fire (scheduled-run path, origin trigger) |
| W4-T5 | built | B08x | Deterministic wake gate over the fetched webhook payload |
| W4-T6 | built | B08x | Trigger dry run: evaluate the wake gate on one event without firing |
| W4-T7 | built | B09 | model-gateway + BFF routes for event triggers |
| W4-T8 | built | B09 | Automations hub: event triggers section (create, pause, dry run) |

### W5 (70 units: already_built 1, built 65, closed_later 1, superseded 3)

| Unit | Status | Batch | Title |
|---|---|---|---|
| FU-05-share-action | built | B10 | ShareDialog submits through the library.share action |
| FU-06-sidechat-usercontent | already_built | B10 | Side chat question is UserContent |
| FU-11-generated-artifacts | built | B18 | Regenerate generated artifacts: agent-action-admission golden + MP copies + sha pins, Python proto bindings, contract pins |
| FU-16-quarry-teach-docs | built | B10 | Quarry teach-mode procedure draft route: OpenAPI entry and error reasons |
| FU-17-wiki-review-flag-wiring | built | B10 | Wire VITE_WIKI_PROPOSALS_REVIEW into the verevonv3 compose file and env example |
| FU-20-coordinator-resume-offer | superseded | X-settled | Coordinator mode keeps the full delegate catalogue across a browser help/takeover resume |
| FU-23-org-agents-paused-presentation | built | B11s | Present org_agents_paused on HTTP refusals and on continue/resume |
| FU-24-stream-priority-tier-pricing | built | B14 | Streamed answers report the served speed tier so priority pricing and the ledger agree |
| GAP-W5-069-a-waived-thread-refusal | built | B12 | Side chat: refuse a thread that holds a ZDR or waived run before any transcript read or model call |
| GAP-W5-071-b-coordinator-resume | built | B11 | Coordinator resume: keep the complete offer in the pause snapshot and re-apply coordinator narrowing on resume |
| P0-D2-698 | built | B11 | Per-run cost and token caps take the smaller of the request cap and the org ceiling, never the larger |
| R2-OD-40-a-failure-classes-bench | built | B14 | inference-core: closed provider failure vocabulary and a bounded per-provider-and-model bench board (pure module) |
| R2-OD-40-b1-switches-guard-anchor | built | B14 | FallbackChain, part 1: two dark switches, the user-route guard and the same-tier anchor for fallback hops |
| R2-OD-40-b2-providers-benched-status | built | B15 | inference-core: the typed ProvidersBenched error and its providers_benched wire kind in the status contract |
| R2-OD-40-b3-chain-benching | built | B15 | FallbackChain, part 3: skip benched platform providers, record outcomes per provider and model, and refuse typed providers_benched ahead of the postur |
| R2-OD-40-c-dataplane-status-cases | built | B18 | Data Plane inference clients: decide the regenerated infer-statuses cases (providers_benched, model_hidden, image_model_unavailable) in all three tabl |
| W5-069-a | built | B04y | Side chat: read-only, unpersisted model-gateway route over a thread snapshot |
| W5-069-b | built | B04y | Side chat BFF proxy, action contract and chat-client call |
| W5-069-c | built | B04y | Side chat panel and spin-off draft chip in the chat page |
| W5-070-a | built | B09 | Deferred tool index for the Do loop (tool_search over untrusted/provider/MCP tools) |
| W5-071-a | built | B09 | Typed per-item results for research fan-out (validated result_schema) |
| W5-071-b | superseded | X-settled | Coordinator mode: top-level run offered only read and subagent tools |
| W5-071-c | built | B04y | Fleet panel rewired to lineage + per-child cancel |
| W5-073-a | built | B04y | RouteNote carries the intent decision's mode, complexity and reason |
| W5-073-b | built | B04y | Routing decision on the usage frame, assistant metadata and the Details popover |
| W5-073-c | built | B04y | Model record successor for deprecated models, advertised on /v1/models |
| W5-074-a | built | B04y | Server-derived action target on EvaluatePolicyRequest |
| W5-074-b | built | B08x | capability-core org permission rules table and tighten-only evaluation |
| W5-074-c | built | B08x | Admin CRUD for org permission rules (audited) + BFF + Customize section |
| W5-075-a | built | B08x | mp-telemetry field allow-list layer for exported spans and JSON logs |
| W5-075-b | built | B06m | UserContent newtype that refuses to log, adopted on the new W5 routes |
| W5-076-a | built | B06m | Mount the changed-files card under each answer |
| W5-077-a | built | B06m | Chat shortcuts as registered commands (palette + shortcut sheet) |
| W5-077-b | built | B06m | Double-Escape stops the live chat run |
| W5-077-c | built | B06m | Palette actions for the current chat thread (archive, star, mark unread) |
| W5-078-a | built | B04y | Role-aware limit notices for budget and rate limits |
| W5-078-b | built | B06m | Shared shell toast manager mounted in CoreShell |
| W5-078-c | built | B07m | Chat connection pill with hysteresis |
| W5-079-a | built | B04y | Registry schema: stage, killSwitch, agentAccess, orgOverridable + register the W5 switches |
| W5-079-b | built | B04y | org-core per-org flag values store and routes |
| W5-079-c | closed_later | X-settled | BFF org flags read + SPA reader that ANDs org value with the build switch |
| W5-080-a | superseded | X-settled | Chat library action contract and registry entries |
| W5-081-a | built | B08x | Quarry: redacted procedure draft from a run's human takeover receipts |
| W5-081-b | built | B06m | session-core: 'teach' skill origin as an owner-held proposal |
| W5-081-c | built | B09 | model-gateway: draft a skill from a procedure draft and file it as a teach proposal |
| W5-081-d | built | B10 | 'Save as skill' after a browser takeover |
| W5-083-a | built | B09 | agents.paused org policy setting enforced at Do admission, resume and continue |
| W5-083-b | built | B10 | Customize hub toggle for pausing agent runs |
| W5-086-a | built | B09 | Review-as-run BFF proxy, action contract and client |
| W5-086-b | built | B09 | Review card in the artifact panel |
| W5-087-a | built | B08x | web_bundle artifact: validate and inline a multi-file static bundle into one HTML artifact |
| W5-090-a | built | B09 | BFF: transcribe an audio upload into a Data Plane transcript document |
| W5-090-b | built | B09 | 'Transcribe to document' on audio attachments |
| W5-091-a | built | B08x | finspo-core: sensitivity-label-aware visibility narrowing |
| W5-092-a | built | B09 | model-gateway council route: N-model fan-out + disagreement synthesis |
| W5-092-b | built | B10 | Council BFF proxy and Compare models panel |
| W5-093-a | built | B10 | promote-on-use: ZDR/waived/Space gate, untrusted provenance, cited-only counting |
| W5-093-b | built | B10 | Durable promote-on-use counter in session-core |
| W5-093-c | built | B07m | Data-source catalogue in the knowledge/retrieval tool descriptions |
| W5-094-a | built | B08x | wiki-store: create-page proposal kind and pending-proposal listing |
| W5-094-b | built | B08x | Knowledge: wiki proposals review view (BFF + SPA) |
| W5-095-a | built | B10 | inference-core: explicit speed tier per model record, decoupled from Quick |
| W5-095-b | built | B10 | model-gateway speed field, pricing multiplier and composer toggle |
| W5-096-a | built | B10 | Report a problem: consented correlation ids in the feedback widget |
| W5-096-b | built | B07m | Run Console: download support bundle with correlation id |
| W5-096-c | built | B07m | Content-free long-task beacon |
| W5-097-a | built | B10 | Shadow action reviewer (observe-only) in the Do loop |
| W5-098-a | built | B07m | Environment pill and org accent with contrast solver |
| W5-098-b | built | B10 | Reasoning translation route (model-gateway + BFF) |
| W5-098-c | built | B10 | Translate reasoning in the Details popover |

## 4. Audit items (F-t3)

Every F-t3 id in the audit, with the units that carry it and the recorded reasons where something is not code. "Coded" means every unit that names it is built; "partly" means some are not built yet; "reason only" means no unit names it and an inventory not-gap reason or a decision entry explains it; "check" means the ledger names neither, so the H pass must look at it.

| Item | State | Units | Not-gap reasons / decisions |
|---|---|---|---|
| F-t3-001 | before this branch: built (section 8) |  |  |
| F-t3-002 | before this branch: partly (section 8) |  |  |
| F-t3-003 | coded | W4-CC-7 |  |
| F-t3-004 | reason only |  | W5-075 org analytics and customer-owned telemetry export: The card treats this as a Control Plane-owned egress with consent and audit. It needs the EP-9 egress catalogue (068) and  |
| F-t3-005 | before this branch: partly (section 8) |  |  |
| F-t3-006 | before this branch: built (section 8) |  |  |
| F-t3-007 | before this branch: built (section 8) |  |  |
| F-t3-008 | reason only |  | W5-085 sandbox computer extensions: The plan says demand evidence is thin (audit 10.3). It also needs F-t3-008 and a billing decision. |
| F-t3-009 | reason only |  | W5-070 WebMCP page tools / background jobs: Quarry consuming WebMCP rests on documentation only (audit F-t3-089 note). Background jobs depend on F-t3-009 process registry semantics; W5-087 phase 1 build in Space sandbox on a per-run isolated origin, and screenshots through Quarry: Needs per-run isolated preview origin infrastructure plus F-t3-009/F-t3-045 phas |
| F-t3-010 | before this branch: built (section 8) |  |  |
| F-t3-011 | before this branch: built (section 8) |  |  |
| F-t3-012 | before this branch: built (section 8) |  |  |
| F-t3-013 | before this branch: built (section 8) |  |  |
| F-t3-014 | before this branch: built (section 8) |  |  |
| F-t3-015 | before this branch: built (section 8) |  |  |
| F-t3-016 | before this branch: built (section 8) |  |  |
| F-t3-017 | before this branch: partly (section 8) |  |  |
| F-t3-018 | before this branch: built (section 8) |  |  |
| F-t3-019 | before this branch: built (section 8) |  |  |
| F-t3-020 | before this branch: built (section 8) |  |  |
| F-t3-021 | before this branch: built (section 8) |  |  |
| F-t3-022 | before this branch: built (section 8) |  |  |
| F-t3-023 | before this branch: built (section 8) |  |  |
| F-t3-024 | reason only |  | F-t3-035 desktop viewer: Blocked on F-t3-024 desktop backend (not buildable, see below).; F-t3-024 C1 typed computer connector: Built: 5fbbe2c79 (computer_action.rs, capability-core 0038, ZDR matrix row) + 6f5dd93ec W4-C backstop, behind EXECUTION_DO_COMPUTER_ENABLED.; F-t3-024 C2+ computer backend (sandbox desktop, guest executor, coordinate fallback): Blocked on DEC-22: the microVM/desktop backend needs the WS22-S8 spike ADR (provider, cost, gu; named in the open decisions |
| F-t3-025 | before this branch: built (section 8) |  |  |
| F-t3-026 | before this branch: partly (section 8) |  |  |
| F-t3-027 | before this branch: partly (section 8) |  |  |
| F-t3-028 | before this branch: partly (section 8) |  |  |
| F-t3-029 | before this branch: built (section 8) |  |  |
| F-t3-030 | before this branch: built (section 8) |  |  |
| F-t3-031 | before this branch: partly (section 8) |  |  |
| F-t3-032 | before this branch: built (section 8) |  |  |
| F-t3-033 | before this branch: built (section 8) |  |  |
| F-t3-034 | before this branch: built (section 8) |  |  |
| F-t3-035 | partly (4 of 5) | W4-P3, W4-P4, W4-P5, W4-P6, W4-REG-1 | F-t3-035 P1 viewer registry: Built: a9b084e72. The registry, viewers and action contract are on main, but the ToolCallCard mount was lost in merge 6a1ae082e, so it is restored by W; F-t3-035 P2 process output read (MG + sandbox-manager): Built: 980861a72 (MG /v1/runs/:run_id/processes/:pid/output, ReadProcessOutputRequest.run_id=7), intact on main. BFF/SPA wir; F-t3-035 desktop viewer: Blocked on F-t3-024 desktop backend (not buildable, see below). |
| F-t3-036 | before this branch: built (section 8) |  |  |
| F-t3-037 | before this branch: partly (section 8) |  |  |
| F-t3-038 | before this branch: built (section 8) |  |  |
| F-t3-039 | before this branch: partly (section 8) |  |  |
| F-t3-040 | before this branch: partly (section 8) |  |  |
| F-t3-041 | reason only |  | W5-090 meeting transcript pull (Teams/Zoom) and video generation/understanding: Meeting pull needs new provider operations outside the frozen docs/actions-surface-operations.md con |
| F-t3-042 | coded | W5-073-a |  |
| F-t3-043 | reason only |  | F-t3-057 sandbox injecting proxy (043(c)): This is F-t3-043 (EP-9, W3 item, SANDBOX_CREDENTIAL_INJECTION), not this W4 item. Its custody path is shared with W4-CC-3/CC-4. |
| F-t3-044 | partly (2 of 3) | W4-IO-1, W4-IO-2, W4-REG-1 | F-t3-044 widget core, bridge, prompt notice: Covered by EP-4-S1, S2, S3 (done). |
| F-t3-045 | coded | W4-M2 | W5-087 phase 1 build in Space sandbox on a per-run isolated origin, and screenshots through Quarry: Needs per-run isolated preview origin infrastructure plus F-t3-009/F-t3-045 phas; F-t3-045 elicitation transport, question origin, MCP Apps resource/host/tool-call, origin ADR: Covered by EP-4-S4, S5, S6, S8, S9, S10, S11 (done), plus W4-D M1a/M1b (0a419e35c) an; F-t3-045 dedicated user-content origin, viewer-bound connectors, app KV store, hosted runtime with network: Deferred by the EP-4-S11 ADR. D3-461 was decided at its recommendation ( |
| F-t3-046 | coded | W4-SH-1, W4-SH-2, W4-SH-3, W4-SH-4, W4-SH-5, W4-SH-6 | F-t3-046 export, redacted replay, import (verevon format), snapshot store, share-model ADR: Covered by EP-12-S9..S15 (done). The import UI, foreign formats, Control grants, viewer ; F-t3-046 public links, cross-org sharing, live shared threads, fork from a snapshot: Forbidden by the share-model ADR (org-only, view-only snapshot) and DEC-40. Doing them needs a ; named in the open decisions |
| F-t3-047 | coded | W4-T3, W4-T4, W4-T5, W4-T6, W4-T7, W4-T8 | F-t3-047 T1 trigger record, T2 consumer and fire authorisation: Built: 729dd8599 (session-core 0113, event_triggers.proto) and d017cfd83 (0114 event_trigger_fires, capability-core ; F-t3-047 blueprints and consent-first suggestions: The audit card says to defer them. No design.; F-t3-047 mail-received triggers: Only webhook_received exists in Ingestion, and the event_triggers.event_type CHECK is closed. The card says mail triggers need an Ingestion change ; named in the open decisions |
| F-t3-048 | before this branch: built (section 8) |  |  |
| F-t3-049 | before this branch: partly (section 8) |  |  |
| F-t3-050 | coded | W4-R2-b, W4-R3-a, W4-R3-b | F-t3-050 R1 replay scrubber: Built: b7740b301 (BrowserChrome.tsx + features/browser/replay/*), intact on main, behind VITE_BROWSER_REPLAY_SCRUBBER.; F-t3-050 R2 action brief (Run Console): Built: 580e32237 (AgentRunConsole.tsx, browser-action-brief.ts), intact on main. Chat rail half is W4-R2-b.; F-t3-050 continuous walkthrough video clip: Audit says build the filmstrip from stored screenshots first (W4-R3-a/b); a continuous clip needs a Quarry recording plus a retention an; named in the open decisions |
| F-t3-051 | coded | W4-IMG-1, W4-IMG-2, W4-IMG-3, W4-IMG-4 | F-t3-051 Do-loop generate_image: Needs the audit's step 3 decision on a durable home for binary artifacts, because Do runs are durable and unattended. That decision is not among th; F-t3-051 edit endpoint + mark tool, C2PA signing: Sequenced after the binary-artifact-home decision (steps 3-5). C2PA also needs a Control Plane signing certificate, which is exter; F-t3-051 Gemini image provider: EP-13-S8 is blocked-external on owner Gemini prices and the DEC-40 residency ruling. Gemini media is out of EP-13.; named in the open decisions |
| F-t3-052 | before this branch: built (section 8) |  |  |
| F-t3-053 | before this branch: built (section 8) |  |  |
| F-t3-054 | before this branch: built (section 8) |  |  |
| F-t3-055 | before this branch: partly (section 8) |  |  |
| F-t3-056 | before this branch: built (section 8) |  |  |
| F-t3-057 | coded | W4-CC-1, W4-CC-2, W4-CC-3, W4-CC-4, W4-CC-5, W4-CC-6, W4-CC-7, W4-CC-8 | F-t3-057 sandbox injecting proxy (043(c)): This is F-t3-043 (EP-9, W3 item, SANDBOX_CREDENTIAL_INJECTION), not this W4 item. Its custody path is shared with W4-CC-3/CC-4. |
| F-t3-058 | before this branch: built (section 8) |  |  |
| F-t3-059 | coded | W4-PL-1, W4-PL-2, W4-PL-3, W4-PL-4 | F-t3-059 cross-vendor / foreign-layout adapters, webhook hooks: The audit says to defer adapters until the internal manifest is stable. Webhook hooks are deferred by WS25 D2 until  |
| F-t3-060 | before this branch: built (section 8) |  |  |
| F-t3-061 | before this branch: built (section 8) |  |  |
| F-t3-062 | before this branch: partly (section 8) |  |  |
| F-t3-063 | before this branch: built (section 8) |  |  |
| F-t3-064 | reason only |  | F-t3-064 public Agent API core: Covered by EP-12-S1..S8 (origin on every run, auth-core key exchange, v1 contract, api_key principal in session-core and inference-core, read/contro; F-t3-064 API-started Do or browser runs and approvals over the API: Out of EP-12 (D5 Ask-only). The 'unattended approval contract' is named as a W4 design that does not exist, and ; F-t3-064 mail-task pipeline wiring (gateway mail_task_pipeline=None): No design. Running an unattended turn in the user's identity has no approved contract (mail_task.rs and mail_t; named in the open decisions |
| F-t3-065 | coded | W4-BYO-1, W4-BYO-2, W4-BYO-3, W4-BYO-4, W4-BYO-5, W4-BYO-6 | F-t3-065 cloud-role BYO (phase 3): The audit puts it last. It needs per-cloud attestations that an admin form cannot assert (DEC-40). No design.; F-t3-065 Gemini/Vertex: EP-13-S6/S7 (built), with S8 blocked-external on the owner.; named in the open decisions |
| F-t3-066 | coded | W5-071-c |  |
| F-t3-067 | before this branch: built (section 8) |  |  |
| F-t3-068 | before this branch: built (section 8) |  |  |
| F-t3-069 | coded | W5-069-a, W5-069-b, W5-069-c | W5-069 rewind with undo preview: Built in 430a3226c (SetActiveLeafRequest.exact, BFF undo-rewind, SPA confirm/undo). Only tests/live proof owed; the unwired version switcher is the |
| F-t3-070 | coded | W5-070-a |  |
| F-t3-071 | coded | W5-071-a, W5-071-b, W5-071-c |  |
| F-t3-072 | coded | R2-OD-40-a-failure-classes-bench, R2-OD-40-b1-switches-guard-anchor, R2-OD-40-b2-providers-benched-status, R2-OD-40-b3-chain-benching |  |
| F-t3-073 | coded | W5-073-a, W5-073-b, W5-073-c | W5-072 resilient routing: The plan marks the subscription fallback as clashing with ownership-matrix rule 17. It needs an explicit reversal and also F-t3-073. The task lists it as  |
| F-t3-074 | coded | W5-074-a, W5-074-b, W5-074-c |  |
| F-t3-075 | coded | W5-075-a, W5-075-b |  |
| F-t3-076 | coded | W5-076-a |  |
| F-t3-077 | coded | W5-077-a, W5-077-b, W5-077-c |  |
| F-t3-078 | coded | W5-078-a, W5-078-b, W5-078-c |  |
| F-t3-079 | coded | W5-079-a, W5-079-b, W5-079-c |  |
| F-t3-080 | coded | W5-080-a |  |
| F-t3-081 | coded | W5-081-a, W5-081-b, W5-081-c, W5-081-d |  |
| F-t3-082 | before this branch: not built (section 8) |  |  |
| F-t3-083 | coded | W5-083-a, W5-083-b |  |
| F-t3-084 | before this branch: not built (section 8) |  |  |
| F-t3-085 | before this branch: not built (section 8) |  |  |
| F-t3-086 | coded | W5-086-a, W5-086-b |  |
| F-t3-087 | coded | W5-087-a |  |
| F-t3-088 | before this branch: not built (section 8) |  |  |
| F-t3-089 | reason only |  | W5-070 WebMCP page tools / background jobs: Quarry consuming WebMCP rests on documentation only (audit F-t3-089 note). Background jobs depend on F-t3-009 process registry semantics; F-t3-064 channel adapters / CLI, F-t3-089: Channel Plane is docs-only (CLAUDE.md, DEC-40). Forbidden to build. |
| F-t3-090 | coded | W5-090-a, W5-090-b |  |
| F-t3-091 | coded | W5-091-a |  |
| F-t3-092 | coded | W5-092-a, W5-092-b |  |
| F-t3-093 | coded | W5-093-a, W5-093-b, W5-093-c |  |
| F-t3-094 | coded | W5-094-a, W5-094-b |  |
| F-t3-095 | coded | W5-095-a, W5-095-b |  |
| F-t3-096 | coded | W5-096-a, W5-096-b, W5-096-c |  |
| F-t3-097 | coded | W5-097-a |  |
| F-t3-098 | coded | W5-098-a, W5-098-b, W5-098-c |  |

## 5. Units written but not built in this phase

These units have a reviewed spec in the programme plan folder and wait for the next phase (budget stop at the weekly usage line, or they run after FUGEN).

| Unit | Wave | Status | Title |
|---|---|---|---|
| (none) | - | - | Every adopted unit is built, superseded or carried by a built follow-up. |

Also not built, for the next phase: W3-EP-4-S7 MCP elicitation through a resume question (the owner granted its credential scope on 2026-11-11, DEC-F08-05; its reviewed design is kept with the ws14 scope drafts), and the owner answers that need new work (verification plan section 7, items O-01 to O-28).

Triage F units written but not built (195):

| Unit | Size | Sensitive | Title |
|---|---|---|---|
| F-f01-01 | M | - | audit-core: chain catch-up honours an activation grace window so a rolling deploy cannot strand rows as legacy_unchained |
| F-f01-02 | M | - | audit-core: thin old chain attestations to the newest per UTC day, keeping the newest overall |
| F-f01-03 | M | zdr | audit-core: enforce the content-free details guard at ingest (redact, never reject) on the chained and unchained paths |
| F-f01-04 | S | authz,cross-plane | auth-core: capability-core tokens for org owners and admins carry capability:admin so verifiedIsAdmin can be true |
| F-f01-05 | S | - | rollback-rehearsal.sh: refuse env files whose values start with a quote before docker run reads them literally |
| F-f01-06 | S | zdr | zdr-live-proof.sh: org-scoped counters so other traffic cannot fail the after phase |
| F-f01-07 | M | - | acceptance-live-metrics.sh: measure the live signals the acceptance thresholds name and compare them to approved targets |
| F-f01-08 | S | zdr | model-gateway: count idempotency claims and capacity refusals, and evict the oldest completed entry before refusing new sends |
| F-f01-09 | M | zdr | Queued mid-run input: the done frame names the messages the run never took, and the SPA resends only those |
| F-f01-10 | S | - | model-gateway: a duplicate send waits briefly for the original to become attachable instead of failing at once |
| F-f01-11 | S | - | model-gateway: admission fails closed with model_entitlement_unavailable when GetModelAccess cannot be reached |
| F-f01-12 | S | zdr | model-gateway attachment policy: hosted image URLs must be https and on an operator allowlist |
| F-f01-13 | M | zdr | Per-thread memory mode: ThreadSummary exposes it and model-gateway accepts it on the presentation update |
| F-f01-14 | M | zdr | Frontend: action contract, gateway pass-through and the chat toggle for per-thread memory mode |
| F-f01-15 | M | cross-plane | session-core org policy: refresh new orgs first and in parallel, and refuse to boot fail-closed without org-core credentials |
| F-f01-16 | S | zdr | session-core user erasure also removes the person's own skills, skill revisions and skill versions |
| F-f01-17 | S | - | verify-skill-revision-attribution.sh: fail when skill revisions were logged as unattributed after the cutover |
| F-f01-18 | S | authz | Plan todo recitation: owner check in one SQL statement, list function no longer public, and a bounded block |
| F-f01-19 | S | - | Chat SPA: flush pending delta batches before the snapshot is written on hide, pagehide and unmount |
| F-f01-20 | S | - | Chat CSS: a turn with an open action menu is exempt from content-visibility containment |
| F-f01-21 | S | - | inference-core: the Codex catalogue lists effort levels when the wide ladder is on |
| F-f01-22 | M | migration,zdr | capability-core: a Postgres counter store for the learning review budget, so cost controls can run |
| F-f01-23 | M | zdr | capability-core: a deterministic PII and secret screen installed as learning.Screen |
| F-f01-24 | M | - | Chat SPA: paged opens, older pages and saved-answer adoption read the paged transcript route so citations and grounding survive |
| F-f01-25 | M | - | Chat SPA: disable actions on local-only leading turns until loaded, and reset the paging window when a truncation reaches it |
| F-f01-26 | S | zdr | inference-core openai: send prompt_cache_key only when it has the derived shape (32 lowercase hex, optional :sub) |
| F-f01-27 | M | zdr | model-gateway: send the stable-prefix tier plan on Anthropic routes so inference-core can cache the standing blocks |
| F-f02-01 | M | - | Office verifier check.py: capped streamed part reads, one render deadline, font false-positive fix |
| F-f02-02 | M | - | capability-core cron: wait for session-core migration 0056 instead of failing every fire insert |
| F-f02-03 | S | - | session-core Dreaming: scheduled_run answers are never mined for memory |
| F-f02-04 | M | - | Budget guard: measure each org cap over its own reset window, not the widest |
| F-f02-05 | M | zdr | sandbox-manager thread workspace: refuse a Space-named thread lease whatever the switch, and refuse ZDR at read and commit |
| F-f02-06 | S | - | session-core archive_threads: THREAD_ARCHIVED idempotency key per archive instance |
| F-f02-07 | S | - | SPA artifact renderer registry: reactive version signal so a runtime register re-renders the open panel |
| F-f02-08 | M | - | SPA sidebar: server-paged active thread list behind VITE_CHAT_SIDEBAR_SERVER_PAGING |
| F-f02-09 | S | - | Register the dark switches of this register chunk in docker-compose and the Control flag registry |
| F-f02-11 | M | zdr | Result handles: grounding and GDPR data category follow the parked result, not the handle envelope |
| F-f02-12 | M | zdr | Result handles: park by declared tool class, and only when result_query would be allowed |
| F-f02-13 | M | authz | capability-core: content-free audit rows for schedule contract created, renewed, refused and expired |
| F-f02-16 | M | authz,migration | org-core: member removal and user self-erasure delete browser site choices; the broker's resolve checks active membership |
| F-f03-01 | M | authz,cross-plane | user-core: browser_entitled joins the entitlement catalogue and is served to org-core on an internal floor read |
| F-f03-02 | M | authz,cross-plane | org-core: browser-sites resolve accepts space_id, reads the Space floor from user-core and applies ApplySpaceFloor, failing closed |
| F-f03-03 | M | authz,cross-plane | browser-broker and execution-core send the Space with the site-policy resolve call, behind BROWSER_SPACE_FLOOR_ENABLED |
| F-f03-05 | S | zdr | Scheduled step: the model answer never travels back to the orchestrator or into Temporal history |
| F-f03-06 | S | - | Scheduled agent lane: a run-level token ceiling ends the run with a typed failure |
| F-f03-07 | S | zdr | Scheduled agent lane: web_search is withheld once the run has read external content |
| F-f03-08 | M | authz,zdr | Skill bundles: manifests follow skill visibility, an upload needs its skill, a skill delete removes its bundles, and ZDR.md names the store |
| F-f03-09 | M | authz,cross-plane | Model action view: an unavailable row no longer ends the list early, and execution-core resolves N pinned actions instead of one hard-coded  |
| F-f03-10 | M | authz,cross-plane | user-core: wire RUN_ACTION_TARGETS and the v2 model action view, mode-gate the run-action decision lane, bind the schema hash |
| F-f03-11 | M | - | SPA sidebar: pass thread state and the search-hit opener to the organizer, style the results, and list starred chats from the server |
| F-f03-12 | M | authz | Approval causes carry step_up_required for the v1 step-up set, and a step-up cause is never auto-granted by the chat posture |
| F-f03-13 | M | - | Approval decisions record step_up_verified from the gateway's verified fresh claim |
| F-f03-14 | M | authz | Step-up gate also covers the model-gateway gRPC ApproveApproval and the Space member decide route |
| F-f03-15 | S | credentials,cross-plane | Gateway BFF: POST /api/v1/auth/step-up proxies the password re-check to Auth Core |
| F-f03-16 | M | - | Approval carries a content-free decision record (source, scope, rule, step-up) on every read |
| F-f03-17 | S | zdr | ZDR field table: space_append_context is honoured (it only narrows retrieval), with the ZDR.md field row |
| F-f03-19 | S | - | Memory write policy: the credential rule recognises credential-shaped values, not prose that happens to follow a keyword |
| F-f04-01 | S | - | model-gateway: a cold or unreachable cost-core costs one catalogue wait, not one per provider call |
| F-f04-02 | M | money | model-gateway: the thread title, follow-up chips and unchosen word-range candidates are metered like every other call of the turn |
| F-f04-03 | M | money | model-gateway: a tool phase that fails still states and ledgers the spend of the rounds it already ran |
| F-f04-04 | M | money | model-gateway: the compaction summary call and the deep-research plan, ledger and synthesis calls are metered |
| F-f04-05 | M | - | usage frame states a mixed-basis turn explicitly and the message cost line says what it covers |
| F-f04-06 | S | - | SPA: usage.progress events update the cost meter only and never become trace or work-feed rows |
| F-f04-07 | S | money | run ceiling: an unpriced metered call ends the tool phase when a USD cap is set and enforcement is on |
| F-f04-08 | M | money | Do run token cap, part 1: RunAgentRequest carries the run cap and the gateway stamps min(request, org) |
| F-f04-09 | M | money | Do run token cap, part 2: execution-core ends the run at the next round boundary once the cap is spent |
| F-f04-10 | S | - | Declare MODEL_GATEWAY_BUDGET_WINDOW and register the WS27 cost switches in compose and the flag registry |
| F-f04-11 | M | - | forced web search runs the inline injection pass with the turn's own classifier budget |
| F-f04-12 | M | credentials,authz,cross-plane,zdr | execution-core: a service-bound inference credential lets the gateway classifier run for Do screening, never for ZDR or waived runs |
| F-f04-13 | M | zdr,authz | Do screening: a failed verdict call follows the org's fail mode, and a ZDR or waived Do run still withholds a deterministic flag under enfor |
| F-f04-14 | S | - | mp-contracts provider reads: decode file content_base64, keep Google calendar titles and SharePoint site names |
| F-f04-15 | M | - | execution-core Do loop uses mp_contracts::provider_reads for the read tool schema, shaping and rendering |
| F-f04-16 | M | authz | Ask loop: after a flagged connector read, web tools are withdrawn for the rest of the turn |
| F-f04-17 | L | authz,zdr,migration | owner document listing is offered only while its admission row is enabled with the pinned schema hash |
| F-f04-18 | M | zdr | ZDR catalogue: workspace_tools and owner_action feature keys, a ticket-tool gate, and waived rows in the feature table |
| F-f04-19 | S | zdr | browser run start under ZDR answers the typed refusal, not a bare 401 |
| F-f04-20 | M | migration | session-core attention sweep: re-check the item still waits in the insert, and keep a durable high-water mark |
| F-f04-21 | S | migration | session-core builds the message-tree indexes CONCURRENTLY before migration 0061 runs on a populated database |
| F-f04-22 | M | - | message versions: editing the first question of a thread moves the active leaf to the thread root |
| F-f04-23 | S | - | built-in Office skills are found by whole words, not substrings of a name or keyword |
| F-f04-24 | M | authz | gateway execute_action: dispatcher arms for the five audit.* actions |
| F-f04-25 | M | authz,zdr | gateway execute_action: dispatcher arms for approvals.decide and the four chat.* actions |
| F-f04-26 | M | authz,zdr,cross-plane | gateway execute_action: dispatcher arms for integrations.browse_connection, integrations.fetch_connection_content, mcp.approve_tool and tool |
| F-f05-02-action-surface-ratchet | S | - | Action-surface contract test: replace the stale whole-set equality with a ratchet that fails only on new undispatched ids |
| F-f05-03-attention-confirm-rpc | M | authz,zdr | session-core: a service-authorized read that says whether one attention item still waits on its owner |
| F-f05-04-attention-notify-confirm-backoff | M | zdr,cross-plane | capability-core attention notifier: confirm the item still waits, drop definitive refusals, back off transient ones, register the switches |
| F-f05-05-scheduled-outcome-emitter | M | zdr | session-core: a finished scheduled run writes one SCHEDULED_RUN_OUTCOME outbox event, in the same transaction |
| F-f05-06-scheduled-outcome-subject-check | S | authz | capability-core: a scheduled-outcome notice goes only to the schedule's own creator, verified in capability-core's own table |
| F-f05-07-thread-attention-precedence | S | - | SPA: a thread row that carries the server's attention state uses it; the legacy run-status map is only the fallback |
| F-f05-08-loop-guard-result-aware | M | - | Loop guard: a repeated call whose result keeps changing is progress, not a loop (polling a process, re-reading a changing resource) |
| F-f05-09-push-after-queued-delivery | M | zdr | notification-core: the queued DeliveryWorker hands a submitted native notice to the same Web Push hook the synchronous path uses |
| F-f05-10-push-org-erasure-consumer | M | cross-plane,zdr | notification-core: purge an organisation's Web Push subscriptions when org-core's GDPR org erasure arrives |
| F-f05-11-push-org-erasure-control-wiring | S | cross-plane | Control Plane: provision the notification-core-org-erasure durable and its NATS grants |
| F-f05-12-sw-visible-notice | S | - | Service worker: optionally show a silent, self-closing notice when a window is visible, so every push shows one |
| F-f05-13-thread-sandbox-bearer-optional | S | authz | execution-core: a thread-workspace run without a sandbox bearer degrades to an ephemeral workspace instead of being refused |
| F-f05-14-workspace-save-report | M | - | Thread workspace: report skipped paths and give a failed call the save state |
| F-f05-15-bearer-expiry-guard | M | credentials,authz | execution-core: a delegated sandbox or browser bearer near its expiry is handled before the call, not by a mid-call auth error |
| F-f05-16-overlay-tombstones-sandbox-manager | L | migration,zdr | sandbox-manager: a Space overlay can carry deletions (tombstones) and Promote honours them with compare-and-swap |
| F-f05-17-overlay-tombstones-execution-core | M | zdr | execution-core: a Space run records the files it deleted as tombstones in its overlay |
| F-f05-18-delegable-tools-capability-core | M | - | capability-core: GET /api/v1/cron/delegable-tools lists the read tools an owner may approve for an unattended schedule |
| F-f05-19-delegable-tools-gateway-bff | S | - | model-gateway and BFF: pass GET /cron/delegable-tools through |
| F-f05-20-delegable-tools-spa | M | - | SPA: load the delegable read tools for the schedule consent card and the renewal dialog |
| F-f05-21-fork-onforked-spa | S | - | SPA: a successful 'Fork from here' opens the copy and refreshes the chat list |
| F-f05-22-fork-count-session-core | M | - | session-core: ListThreads reports how many forks a thread has, and the delete-leaves-forks test exists |
| F-f05-23-fork-delete-notice-bff-spa | S | - | BFF and SPA: the delete-chat confirmation says how many forks keep their own copy |
| F-f05-24-held-office-flush | M | zdr | model-gateway: an Office file held back by the structural check is delivered, marked failed, when the turn ends without a repaired version |
| F-f05-25-broker-readonly-durable | M | migration,authz | browser-broker: the durable grant store persists read_only, so a non-ZDR read-only grant can be issued |
| F-f05-26-browser-observe-attest-readonly | S | - | Browser health: cap.browser.observe is attested only when the broker can actually issue a durable read-only grant |
| F-f05-27-broker-site-ask-reason | S | - | browser-broker: a site-policy refusal says which hosts are 'ask' (the person can allow) and which are 'deny' |
| F-f05-28-browser-session-opened-event | M | - | execution-core: a Do run's browser.open emits a content-free browser_session_opened orchestration event |
| F-f05-29-reject-plan-contract | S | - | Action contract chat.reject_plan before the UI operation (registry entry and model-access row) |
| F-f05-30-browser-act-level-grant | S | authz | execution-core: with Do browser act on, browser.open asks for an act-level grant and falls back to read level where the site allows read onl |
| F-f05-31-browser-act-risk-evidence | M | zdr | execution-core: keep the last observation in memory so a ref-bound browser.act is judged by its element's label |
| F-f06-02 | M | zdr,migration | sandbox-manager: durable blob-erasure tombstone with sweeper retry, and erasure that keeps running with the workspace switch off |
| F-f06-03 | M | zdr,cross-plane | BFF: deleting a thread (or all threads) first erases its sandbox workspace with the person's own sandbox bearer |
| F-f06-04 | M | authz | model-gateway: every reader of org hook rules awaits ensure_org_hooks first, and durable mode with no snapshot fails closed |
| F-f06-05 | S | - | execution-core: a post-tool deny classifies the call by its declared tool class when the class gate is on |
| F-f06-06 | M | - | SPA: render the 'Workspace: kept / not kept / conflict' chip from sandbox_state frames |
| F-f06-07 | M | zdr,authz | recall_conversations honours the current thread's memory mode: GetMemoryControls takes an optional thread id |
| F-f06-08 | M | zdr | Approval cause vocabulary: browser_site_consent (host and max_level) across contracts, model-gateway projection and the SPA |
| F-f06-09 | M | authz,cross-plane | BFF: deciding a browser_site_consent approval with 'remember' writes the member's site preference from the approval record |
| F-f06-10 | M | authz | SPA: action contracts for the site-consent decision, the consent card mounted on the approval row, remember sent to the decide route |
| F-f06-11 | M | migration | org-core: browser site policy and preference writes enqueue content-free audit events through an outbox |
| F-f06-13 | M | - | Knowledge receipt tells the truth: 'searched' only when a search ran, honest modes for assembled context and for follow-ups with append retr |
| F-f06-14 | S | - | SPA: a project thread opens on the same route the first time and every time, decided by the server session |
| F-f06-15 | M | zdr | BFF: GATEWAY_CHAT_PROJECTS_ENABLED refuses project starts when off and the composer capabilities advertise chat_projects; ZDR row for projec |
| F-f06-16 | M | - | SPA: turn actions that call origin=chat endpoints are withdrawn on project threads |
| F-f06-17 | M | - | composer-capabilities advertises agent_modes, the SPA shows the mode picker only when it is on, and AGENT_MODES is read as the strict litera |
| F-f06-20 | S | credentials | BFF AG-UI stream and resume forward the caller's capability bearer, as native chat does |
| F-f07-01 | M | - | SPA: answering or skipping an ask_user question stays on its own thread, and an answer whose send was not accepted goes back to the composer |
| F-f07-02 | S | zdr | model-gateway: the goal-judge route is registered with a literal path and has a classified space-surface row, so the non-GET route lint pass |
| F-f07-03 | S | - | SPA: the step strip of an approved-plan execute turn knows the plan length, so it counts 1..N of N |
| F-f07-04 | M | - | Prompt and tool manifest: pin the named prompt constants and the tool definitions that sit outside the three prompt files and the offer list |
| F-f07-05 | S | - | inference-core Gemini adapter: a malformed function call is a typed error, and tool-call ids are unique across rounds |
| F-f07-06 | M | - | SPA: a send that was not confirmed keeps its client message id, so the resend of the same text is the same transcript row |
| F-f07-08 | M | - | inference-core routing diff: price facts come from the cost-core catalogue, and an unreachable catalogue is reported, never assumed |
| F-f08-01 | S | - | user-core PUT /api/v1/settings/ai merges only the fields the body names |
| F-f08-02 | S | migration | session-core migration: validate the four agent_memory scope constraints where no row violates them |
| F-f08-03 | S | authz | Convex agents:update refuses name and model once an agent serves a published version |
| F-f08-04 | S | - | Read-only report of user-scope memory rows that cannot be proven to be outside a Space (run before switch-on) |
| F-f08-05 | S | - | web_search relevance gate: a batched search keeps a hit that answers any one of its queries |
| F-f08-06 | S | zdr,cross-plane | retrieval-engine: refuse the public Cohere rerank endpoint for tenant retrieval behind a switch |
| F-f08-07 | M | authz | Effective-policy panel: match the real item keys, show lists, and send member counts as redacted + count |
| F-f08-08 | M | zdr,authz,cross-plane | session-core: a manager's delete of a Space memory row writes a content-free audit event |
| F-f08-09 | S | - | user-core: the Space sandbox egress policy event records the tier and hosts that changed |
| F-f08-10 | M | credentials,authz,cross-plane | integration-corev2: raw token lease binds the consumer to its own key and no longer pre-authorises model-plane |
| F-f08-11 | M | zdr,authz | IndexMemoryRequest carries the run id so an agent's own note is attributed by run, not inferred |
| F-f08-13 | M | money | Agent API POST /runs accepts an Idempotency-Key so a client retry does not start a second run |
| F-f08-14 | S | money | Agent API run start refuses when the org has a spend cap that API-key turns cannot be checked against |
| F-f08-15 | M | - | Name the single-source prompt blocks (memory, profile) so the request inspector can tell their source |
| F-f08-16 | L | zdr | Split the authored instructions into tagged sections (platform, org, Space, persona, agent) for Ask turns and runs |
| F-f08-17 | S | zdr | Model picker data-use line behind a default-off switch until counsel approves the copy |
| F-f09-01 | M | authz | execution-core: name the verified person to capability-core for provider read/execute calls even with the tool class gate off |
| F-f09-02 | M | - | execution-core: keep the run's browser page across any approval pause, with the same wait-out expiry as a takeover |
| F-f09-03 | S | - | execution-core: a login form Quarry names non_signal also raises the sign-in help request when the new switch is on |
| F-f09-04 | S | authz | BFF: the wide-research estimate route forwards the capability bearer so an enforced org cap can be read |
| F-f09-05 | S | - | model-gateway: refuse document references on a Do turn with a typed error instead of admitting and dropping them |
| F-f09-06 | S | zdr | session-core: back-fill artifact_revisions.source_message_id when the run's answer message is saved |
| F-f09-07 | L | authz,credentials,cross-plane | model-gateway: read skill bundle blobs with a service credential after the person's own manifest read, so the bundle read rule can be switch |
| F-f09-08 | M | migration | capability-core: list a bundle as a deck template only after ValidateDeckTemplate passed, with a switch-on re-validation of existing bundles |
| F-f09-10 | M | money | model-gateway: settle the goal turn from the dispatch task the moment the executor answers, not from the follower stream |
| F-f09-11 | L | migration,money,zdr | goal budgets: debit the usage of a goal run's /continue segments (session-core top-up RPC, idempotent per segment) |
| F-f09-12 | M | - | session-core: ledger a failed or cancelled goal-bound scheduled fire as an unattended goal turn when its fire is settled |
| F-f09-13 | M | authz | capability-core: deny the public-web capabilities (and, behind a second switch, the browser capabilities) when the org's web.mode is not liv |
| F-f09-14 | S | authz | mp-contracts shell_rule: read-only program table plus an argument screen, so a proposed shell rule never covers a command that writes or run |
| F-f10-02 | M | zdr,authz,cross-plane | session-core: a service read of whether a thread retains anything (SessionRetentionService.GetThreadRetention) |
| F-f10-03 | M | zdr,authz,cross-plane | org-core: a ThreadRetention client over session-core's GetThreadRetention, so a hold on a nothing-retained thread is refused |
| F-f10-06 | L | migration,authz | org-core: the 30-day org purge cron skips a held org, and every refused erasure leaves a durable audit row |
| F-f10-07 | M | zdr | session-core: a refused erasure (legal hold) leaves a content-free audit row in session_audit_outbox |
| F-f10-08 | L | zdr,authz,cross-plane | sandbox-manager: a service-callable thread workspace erase for the retention sweeper |
| F-f10-09 | M | zdr,cross-plane | session-core: a real WorkspaceEraser for the retention sweeper over sandbox-manager's service erase |
| F-f10-10 | L | migration,zdr | retention sweeper: record each deletion in the same transaction so a crashed pass still counts against the daily cap and leaves a run row |
| F-f10-11 | M | zdr,authz | /learn: session-core states the thread's origin and retention, and capability-core refuses a Space, agent-run or waived thread itself |
| F-f10-13 | M | zdr | thread export: run summary shows token totals and model from the usage ledger, and unknown values are null, not zero |
| F-f10-14 | S | - | capability-core: with AUTHORED_KINDS_ENABLED off, skill list and get still exclude templates and presets |
| F-f10-15 | S | - | capability-core: SKILL_BUNDLE_RETENTION reads as off unless SKILL_VERSIONS_API is on |
| F-f10-17 | L | authz | execution-core Do runs honour the org's ext.skills: the gateway states it on the run, execution-core filters injection and recovery |
| F-f10-18 | L | authz,cross-plane | user-core: issue a Control-signed model.space.skill.write decision for a Space member |
| F-f10-19 | L | authz,cross-plane | capability-core: verify the Space skill write decision before a Space skill is created, changed or deleted |
| F-f10-20 | S | zdr | background dreaming never mines a key-owned thread |
| F-f10-21 | L | zdr,authz | execution-core ExecuteStep side-call lane: an MCP app call never creates an approval, never touches the run's snapshot and never depends on  |
| F-f10-22 | M | authz | session-core admits an api_key caller to StartManagedRun and to StreamRunEvents, owner-bound, and stamps origin api |
| F-f10-23a | M | authz,cross-plane | Control + cost-core: model-gateway service principal may call cost-core budget check (scope cost:budget:check), nothing else |
| F-f10-23b | L | money,authz,cross-plane | Agent API key turns pass cost-core budget check through model-gateway own service credential before the model call |
| F-f10-24 | S | - | deep research connector search sends keyword terms, not the whole question, as the provider query |
| F-f10-25 | S | authz | an org mode cap of read_only or lockdown becomes the read-only autonomy rung, so sandbox egress is held at off |
| F-f10-26 | M | zdr,authz | session-core stores a scheduled run's contract as exact text and serves it, with the run's retention, on the scheduled-step reads |
| F-f10-27 | M | authz,cross-plane | user-core: the scheduled step decision binds the contract digest and expiry read from the prepared authority, never from the orchestrator's  |
| F-f10-28 | M | authz | execution-core: the scheduled step verifier recomputes the stored contract's digest and expiry on every step |
| F-f10-29 | M | zdr | scheduled Do lane: the caller and the run request carry the run's real ZDR and waiver, so the retention refusal can fire |
| F-f10-30 | S | zdr | zdr-posture-matrix: a scheduled_do_run row, refused for every ZDR and waived cell |
| F-f10-31 | S | - | browser-broker: the site-policy cache key includes the Space before any Space id is forwarded |
| F-f10-32 | S | - | browser SSE approval_state_changed projects the allowlisted decision provenance |
| F-f10-34 | M | zdr | session-core publishes a content-free RUN_APPROVAL_DECIDED envelope on the run-event subject |
| F-f10-35 | M | zdr,authz | Do browser.console is served from the Do browser session with session-held ids and a schema that names none of them |

Depth-register triage (step F): 207 F units kept after the merge, 12 built in this phase (the ones that close a live authorization, ZDR or org-policy hole); the other 195 are written and wait for the next phase. Each carries the owner's answer to its decision where one applies (OWNER DECISION note).

## 6. Switches added in this phase (93)

All default off unless the registry entry carries an onDefaultReason. Registered in `apps/Control Plane/flag-registry/flag-registry.json` (rules checked statically after every batch).

| Switch | Default | Purpose |
|---|---|---|
| AGENTIC_SPACE_RETRIEVAL_ENABLED | false | WS18-S5: a Do run in a project forwards its Space retrieval decision to execution-core so knowledge_search is Space-scoped. Off, org-wide. Deploy execution-core (reads x-space-retrieval-authorization) first. Only the literal true. |
| APPROVAL_AUDIT_EVENTS_ENABLED | false | WS13-S3: the approval decision audit event, a content-free approval_decided outbox row and its provenance record. Off by default; only the literal true. |
| APPROVAL_RULE_PROPOSALS_ENABLED | false | WS13-S7 'Always' rule proposals and their list, confirm and dismiss routes. Needs APPROVAL_SCOPE_GRANTS_ENABLED, and TOOL_POLICY_ENFORCED plus TOOL_POLICY_RULE_AUDIT_ENABLED on capability-core. Only the literal true. |
| APPROVAL_SCOPE_GRANTS_ENABLED | false | WS13-S6 standing grants. Session-core refuses to boot with it on and APPROVAL_AUDIT_EVENTS_ENABLED off (K3-2). Keep off until execution-core holds approval:grant_use (D-WS13-5) and grants are digest-bound (W2-WS13-S7-grant-digest). |
| APPROVAL_SHELL_RULE_PROPOSALS_ENABLED | false | EP-9 E4: shell-rule narrowing of WS13-S7 rule proposals. Stays off (D-WS13-6). Refused at startup unless APPROVAL_RULE_PROPOSALS_ENABLED is on. |
| APPROVAL_WAIVER_REFUSE | false | D2-1149: an approval create or decide from a waived run's credential is refused before any approval metadata is stored (DEC-32: waived counts as ZDR for new stores). Off keeps today's behaviour. Only the literal true. |
| AUTH_CORE_ZDR_EFFECTIVE_POSTURE_ENABLED | false | ZDR-Z-03b: Auth Core reads the effective ZDR posture that org-core computes (effectiveZdr) instead of keeping its own plan-lapse rule. Off keeps the stored-intent rule. Only the literal true turns it on. |
| CAPABILITY_PERMISSION_RULES_ENABLED | false | F-t3-074 org permission rules: execution-core names a call's target and capability-core applies the org's deny and ask rules to it. Only the literal true turns it on; off, no target is sent and no rule is read. |
| CONNECTOR_SCOPE_RECORD_ENABLED | false | WS23-S4: a Do run records the ids of the connected accounts selected for the turn on its run record (run_context.connector_scope), ids only; never for a ZDR or waived run. Off, nothing is recorded. Only the literal true; read at start. |
| CONTENT_SAFETY_NORMALIZE | true | Normalised injection scan: tighten-only, adds a NOTE and an audit event. Only a falsey word (0, false, no or off) opts out and restores the sixteen-phrase path. session-core's write gate ignores it. |
| CONVERSATION_ZDR_EFFECTIVE_POSTURE_ENABLED | false | ZDR-Z-03d: conversation-core's org-core reader uses the effective posture (retention_posture.effective_zdr) instead of the raw interactiveRetention.zdr flag. Only the literal true turns it on. |
| COST_CORE_IMAGE_PRICING_ENABLED | false | Prices image_generation usage per image from image_price_rates. No row is seeded, so an unpriced image is recorded with no image cost and flagged. Off, image events record as ordinary usage. Only the literal true turns it on. |
| EVENT_TRIGGER_RUN_ENABLED | false | EP-7 T4 trigger starter: an authorised fire of a trigger naming a cron schedule becomes one scheduled run, re-authorised by Control and bounded by run, hourly and org limits. Needs EVENT_TRIGGER_FIRE_ENABLED. Only the literal true. |
| EVENT_TRIGGER_WAKE_GATE_ENABLED | false | EP-7 W4-F T5: the wake gate in capability-core. A fire of a trigger with a wake filter reads its webhook payload from integration-corev2 and needs a match. Off, stored filters are ignored. Only the literal true turns it on. |
| EXECUTE_STEP_SPACE_SEARCH_WITHHOLD | false | D2-771: ExecuteStep withholds a Space-scoped knowledge_search (typed PROJECT_KNOWLEDGE_UNAVAILABLE, before any Data Plane call) unless x-space-retrieval-authorization is forwarded. Off, org-wide. Only the literal true. |
| EXECUTION_COORDINATOR_MODE_ENABLED | false | Coordinator Do runs get only read and subagent delegation tools, so effects go through subagents. Never for ZDR, waived or continued runs; the gateway forwards the flag only while on. Only the literal true turns it on. |
| EXECUTION_CORE_LEASE_RELEASE_FILTER | false | W4-P6: lease release never uploads an unfiltered Space workspace: a ZDR or waived run's workspace is discarded before any upload, other runs upload only write-through-eligible files. Off, unchanged. Only the literal true; read at start. |
| EXECUTION_CORE_SKILL_PINS | false | EP-11 S3: a Do run records the skill versions it injected, and reattach_skill reads the pinned version. Never for ZDR or waived runs. Off reads the current skill row. Only the literal true; read at start, restart to change. |
| EXECUTION_CUSTOM_REST_TOOLS | false | W4-CC-7 custom REST tools on a turn with a selected connection: Do offers read and write, Ask offers read; writes pause for approval; capabilities start unavailable until attested; ZDR and waived refused. Only the literal true. |
| EXECUTION_DEFERRED_TOOL_INDEX_ENABLED | false | W5-070: the Do loop's deferred tool index. Off, a run gets its whole catalogue. On, a catalogue over EXECUTION_DEFERRED_TOOL_INDEX_THRESHOLD offers its MCP and client tools once tool_search reveals them. Only the literal true turns it on. |
| EXECUTION_PROCESS_OUTPUT_SPILL | false | W4-P6 process_read spill: writes a background process's output beyond one page into a workspace file under .tmp, never uploaded. Only the literal true turns it on. Refused on zero-retention or waived runs. Read once at start. |
| EXECUTION_PROCESS_TTY_ENABLED | false | S4.2 process_start tty: a background process on a pseudo-terminal (120x40, TERM=dumb), one output stream. Off gives plain pipes; ZDR and waived tty requests are always refused. Only the literal true; read at start. |
| EXECUTION_SHADOW_REVIEWER | false | W5-097-a shadow reviewer: one tool-free inference judges an effectful Do call; the verdict is only counted and logged, never read by permissions, hooks or approvals. ZDR and waived runs skipped. Only the literal true. |
| FEEDBACK_SKILL_VERSIONS | false | EP-11 S3 part b: keys feedback ratings by skill version and quarantines a skill only when its newest rated version is below the bar. Only the literal true turns it on; off pools every rating per skill id as before. |
| GATEWAY_ZDR_EFFECTIVE_POSTURE_ENABLED | false | ZDR-Z-03c: the BFF's one org-posture reader answers from org-core's effective_zdr instead of parsing the stored intent. Off keeps the intent-only rule. Only the literal true turns it on. |
| INFERENCE_BYO_GATEWAY_ENABLED | false | F-t3-065 phase 2: a call naming a customer-gateway connection is served by that gateway alone through Integration Core, never a platform provider; never ZDR, billed to the customer. Off refuses it. Only the literal true. |
| INFERENCE_CATALOGUE_OVERLAY | false | F-t3-065 phase 1: the platform catalogue overlay (hide, order, model-id aliases) for the listing and pinned requests; it does not change how a model is served. Off ignores a stored overlay. Only the literal true. |
| INFERENCE_GEMINI_EFFORT_ENABLED | false | EP-13: sends thinkingConfig on Gemini requests and lists effort levels for Gemini models. Only the literal true turns it on; off, a Gemini request is unchanged. |
| INFERENCE_OPENAI_PROMPT_CACHE_KEY | false | WS02-S10: sends the provider prompt-cache routing key to the first-party OpenAI endpoint only, never on a ZDR request. Only the literal true turns it on. |
| INFERENCE_ORG_OVERLAY | false | W4-BYO-3 per-org overlay (hide, order, role candidates; narrow-only) read from session-core and applied after the platform overlay; entitlement and ZDR, privacy and residency floors still apply. Needs SESSION_ORG_ROUTING_OVERLAY. |
| INFERENCE_PROVIDER_BENCHING | false | Benches a failing platform provider and model for a bounded time, never routes a platform request to a user subscription route, and refuses typed when all are benched. Only the literal true. |
| INFERENCE_SAME_TIER_FALLBACK | false | Keeps every fallback hop of a call at or above the privacy tier of the first eligible provider. Only the literal true. |
| INFERENCE_SPEED_TIER_SPLIT | false | Speed tier is its own choice, not Quick's. On, a request takes priority with speed_tier only where the model record declares it. Off, Quick selects priority as before. Only the literal true enables it. |
| INTEGRATION_ZDR_EFFECTIVE_POSTURE_ENABLED | false | D-ZDR-10: integration-corev2 reads the organisation's effective ZDR posture (retention_posture.effective_zdr) from org-core instead of the stored intent. Only the literal true turns it on. |
| MCP_SECRET_REF_AUTH_ENABLED | false | W4-CC-5: a static-bearer MCP server redeems its integration-corev2 credential binding for each call. Only the literal true turns it on; off, such a registration is refused. A ZDR or waived turn never redeems one. |
| MEMORY_GATE_CAPABILITY_WRITES | false | D2-480: capability-core user, org and global memory saves ask session-core CheckMemoryWrite before the INSERT (updates when content changes). Refused 403, unreachable 503; delete never gated. Policy per CONTENT_WRITE_POLICY. Only true. |
| MEMORY_GATE_ORCHESTRATOR_WRITES | false | D2-481: consolidated memory entries ask session-core CheckMemoryWrite before letta-bridge IndexMemory; refused entries skipped and counted, unreachable writes nothing, ZDR or unattested refused first. Only the literal true. |
| MODEL_GATEWAY_BYO_GATEWAY | false | W4-BYO-6 customer gateway on a chat turn: accepts an opaque customer-gateway connection id, text-only and never zero retention. Only the literal true turns it on. |
| MODEL_GATEWAY_CAP_MIN | false | D2-698 per-run cost and token caps: the smaller of the request cap and the org ceiling applies, so a caller can only tighten the org limit. Off, a request cap replaces the org ceiling. Only the literal true turns it on. |
| MODEL_GATEWAY_DATA_SOURCE_CATALOGUE | false | F-t3-093 (b) data-source catalogue: the chat prompt names the sources this turn's tools reach, with use-when and cost hints; a ZDR turn lists only ZDR-allowed ones. Off, the prompt is unchanged. Only the literal true turns it on. |
| MODEL_GATEWAY_EVENT_TRIGGERS | false | EP-7 W4-T7: the event trigger routes on model-gateway (/v1/event-triggers and the dry run). Off, every route answers 404. Only the literal true turns it on. |
| MODEL_GATEWAY_GOAL_JUDGE_ENABLED | false | EP-7 S3 completion judge: judges a settled, completed goal turn and records its verdict. Needs MODEL_GATEWAY_THREAD_GOALS_ENABLED. Only the literal true turns it on. |
| MODEL_GATEWAY_GOAL_UNATTENDED_ENABLED | false | EP-7 S6 unattended goals: off, a schedule template naming a goal is refused and the internal judge route answers 404. Needs MODEL_GATEWAY_THREAD_GOALS_ENABLED and MODEL_GATEWAY_GOAL_JUDGE_ENABLED. Only the literal true turns it on. |
| MODEL_GATEWAY_GRAPH_SPACE_SCOPE | false | WS18-S2: graph grounding forwards the Space read decision on a scoped turn and never reaches the org-wide graph without one (scoped turns lose graph entities until Data Plane enforces it). Off, unchanged. Only the literal true. |
| MODEL_GATEWAY_IMAGE_TOOL | false | W4-IMG-3 generate_image as a typed chat tool: one image per call, four per reply, served by strict provider choice and priced per image. Never offered on a ZDR or waived turn. Only the literal true turns it on. |
| MODEL_GATEWAY_LIFECYCLE_EVENTS | false | WS15-S1/S3: counts-only lifecycle frames (retry_scheduled and friends) on the chat and run streams. Ephemeral, and carries no text. Only the literal true turns it on. |
| MODEL_GATEWAY_MCP_PROBE | false | MCP server probe (POST /v1/mcp/servers/probe): a server's auth requirement and tools, saved nothing. Off, the route is a 404. Only the literal true turns it on. |
| MODEL_GATEWAY_MODEL_COUNCIL_ENABLED | false | F-t3-092 model council: two to four models answer one question with no tools, then a synthesis of agreement and disagreement. Cost is estimated first. Nothing stored. Off, the route is a 404. Only the literal true. |
| MODEL_GATEWAY_NAMED_AGENT_THREADS | false | EP-6 P3: a thread listing takes the agent filter (one named agent's threads). Off, the parameter is ignored. Only the literal true turns it on. |
| MODEL_GATEWAY_REASONING_TRANSLATE_ENABLED | false | F-t3-098 reasoning translation of one shown text to nb or en (POST /v1/ai/reasoning-translate), never stored or fed back to the model; ZDR and waived turns refused. Off, the route is a 404. Only the literal true. |
| MODEL_GATEWAY_REVIEW_RUN_ENABLED | false | F-t3-086 review as a run: a read-only reviewer over one text, answered on the inline path and stored nowhere. Off, the route is a 404. Only the literal true, in any case, turns it on. |
| MODEL_GATEWAY_ROUTING_DECISION_ENABLED | false | W5-073-b: the answer's verevon mode, complexity and reason on usage frames, the usage envelope, message metadata and the Routed line. Closed tokens only; needs INFERENCE_ROUTE_NOTE. Only the literal true turns it on. |
| MODEL_GATEWAY_SIDE_CHAT_ENABLED | false | F-t3-069 side chat: a read-only question about one thread's transcript, answered and never stored (POST /v1/threads/:id/side-chat). Off, the route is a 404. Only the literal true turns it on. |
| MODEL_GATEWAY_SPEED_TIER | false | Chat Fast chip: a turn that asks for speed priority sends the priority tier on each model call where the model declares it, priced with PRIORITY_TIER_COST_MULTIPLIER. Off, no call names a tier. |
| MODEL_GATEWAY_TEACH_MODE_ENABLED | false | F-t3-081 teach mode: one run's steps are drafted into a disabled skill proposal for the owner (POST /v1/teach/skill-draft). Off, the route is a 404. Only the literal true, in any case, turns it on. |
| MODEL_GATEWAY_WEB_BUNDLE_ARTIFACTS | false | W5-087: create_artifact accepts kind web_bundle, a multi-file static page the gateway checks and inlines into one html artifact (no import, export, modules or outside URLs). Off, the kind is unknown. Only the literal true. |
| MODEL_GATEWAY_ZDR_LEDGER_ROWS | false | DEF-Z-06b: a ZDR turn runs the budget check before its provider call and publishes one content-free ledger row (ids, counts, amount) after. Off, ZDR spend stays unledgered. Owner call under DEC-18(d). Only the literal true. |
| MP_TELEMETRY_FIELD_ALLOWLIST | false | F-t3-075 telemetry allow-list: JSON logs and exported span attributes keep only the names on TRACE_FIELDS and count the rest as scrubbed_fields. Off, telemetry is unchanged. Only the literal true turns it on. |
| ORCHESTRATOR_GOAL_JUDGE_ACTIVITY_ENABLED | false | EP-7 S6 orchestrator-core: a goal-bound scheduled fire asks model-gateway for the goal's completion judge once it completes. Off, such a fire is not judged. Needs MODEL_GATEWAY_GOAL_UNATTENDED_ENABLED. Only the literal true turns it on. |
| OWNER_LIST_SPACE_REFUSE | false | D2-769: a Space-scoped Do run is refused knowledge_list_documents with a typed error before any Data Plane call. Off, the listing stays org-wide. Enable before MODEL_ACTION_ADMISSION=table with Spaces. Only the literal true. |
| PLUGIN_LOADER_ENABLED | false | Plugin supply-chain pins (F-t3-059). On: create needs a commit-SHA or archive-digest source, a killed digest is refused, and enabling needs a pin and an install. Off: inert CRUD. |
| PROMOTE_ON_USE_DURABLE | false | Keeps the promote-on-use counter in session-core per org, user and URL hash, so it survives restarts. A failed count never promotes. No effect unless PROMOTE_ON_USE_ENABLED is true. Only the exact value true enables it. |
| PROMOTE_ON_USE_SIGNAL | fetch | Which grounded use promote-on-use counts: fetch counts each successful page fetch; cited counts a page only when the final answer cites its URL (exact value cited). No effect unless PROMOTE_ON_USE_ENABLED is true. |
| PROVIDER_READ_TOOL_ENABLED | false | WS23-S3: read_provider_action is offered only for a connection the person selected, and is refused in Space and under ZDR. Only the literal true turns it on. |
| QUARRY_EDGE__ZDR_EFFECTIVE_POSTURE_ENABLED | false | ZDR-Z-03e: quarry-edge's schedule preflight reads org-core's effectiveZdr instead of intent AND entitlement; a missing field is unknown and the fire is refused. Off keeps the old rule. Only the literal true turns it on. |
| ROUTING_POLICY_PROMOTION_REQUIRED | false | EP-14 S10: an inference-core routing-policy PUT records a candidate (202 with its offline diff) and POST .../candidates/:id/promote makes it live. Needs SESSION_CORE_ROUTING_CANDIDATES_ENABLED. Only the literal true. |
| SESSION_CORE_GDPR_THREAD_MEMORY_ERASURE | false | A user erasure also erases memory learned in the person's own threads and their part of Space-thread memory. Off, those rows stay until the thread is deleted. Only the literal true enables it. |
| SESSION_ORG_ROUTING_OVERLAY | false | W4-BYO-3: Get and SetOrgRoutingOverlay on session-core over org_routing_overlays; off answers UNAVAILABLE. Writes need an org admin and refuse ZDR callers. Pair with INFERENCE_ORG_OVERLAY. Only the literal true. |
| SESSION_PROOF_WALKTHROUGH_ENABLED | false | F-t3-050: GetRunProofBundle carries the run's browser walkthrough as screenshot references, never bytes. Off, or for a ZDR or waived run or an unreadable receipt, the section is omitted. Only the literal true enables it. |
| SESSION_SNAPSHOT_GRANTS_ENABLED | false | W4-SH-4/5: share snapshots to org members through Control view grants (share, unshare, list, read after a grant check). Off answers UNAVAILABLE; ZDR and waived refused. Needs the thread-share switches. Only the literal true. |
| SESSION_THREAD_IMPORT_FOREIGN_ENABLED | false | EP-12 S15 foreign readers: ImportThread also reads a ChatGPT or Claude conversations.json. Only the literal true, and only while SESSION_THREAD_IMPORT_ENABLED is on. |
| SESSION_THREAD_RECEIPT_ENFORCE | false | Z-17: CreateThread stamps the retention receipt; AppendMessage refuses a caller whose posture differs (zdr_posture_mismatch) before any write; a waiver needs the same connection and acknowledgement. No receipt = persistent. Only true. |
| SKILL_BUNDLE_MOUNT_ENABLED | false | WS19-S8: the switch mounts a turn's skill bundles read-only at /skills for code_interpreter. Off means no mounts. Only the literal true turns it on. |
| SKILL_ORG_ADMIN_CHECK | false | D2-509: Session Core requires the org admin role (verified admin, org:admin or :admin scope) for a person writing an org-wide skill; others get the typed skip org_scope_requires_admin. Off keeps today. Only the literal true. |
| SKILL_ORG_OWNER_CHECK | false | D3-956: Capability Core requires an org admin (verified scope admin, org:admin or :admin) to change or delete an org-scoped skill, not any member of the org. Off keeps today's member access. Only the literal true enables it. |
| SPACE_THREAD_SCOPE_RESOLVE | false | D2-461: a follow-up turn on an existing thread without space_ref resolves the thread's Space server-side and mints the scoped decision, or refuses typed; never an org-wide search. Off, unchanged. Only the literal true. |
| SUBAGENT_TYPED_RESULTS_ENABLED | false | Typed per-item results for wide research: checks each item's answer against the call's result_schema. Only the literal true enables it. |
| TOOL_POLICY_RULE_AUDIT_ENABLED | false | WS13-S7 rule audit: tool_policy_rule_used rows (W2-WS13-S7-rule-use-b) and the tool_policy_rule_expired sweeper (W2-WS13-S7-rule-expiry). An unstated run counts as retention_bound. Refs D2-1124, D2-1125, D2-1126. |
| USER_CORE_ORG_ERASURE_CONSUMER_ENABLED | false | user-core deletes a purged organisation's resource grants when org-core fans out the organisation erasure. Only the literal true subscribes. Needs the user-core-org-erasure durable from audit-core first. |
| VERTEX_GEMINI_ENABLED | false | EP-13 S6-S8: registers the Vertex AI Gemini provider. EU-resident only on an EU location, not ZDR-capable (ZDR turns skip it), unpriced until its cost-core row lands; the release gate checks its config. Only the literal true. |
| VITE_AGENT_RESULT_FEEDBACK | false | Thumbs and reason chips on a durable run's result, an agent run or a scheduled run, rated by run id. Off, the control renders nothing. Only the literal true turns it on. |
| VITE_APPROVAL_RULE_PROPOSALS | false | WS13-S7: the settings section to confirm or dismiss rules proposed from Always. Built by W2-WS13-S7-spa-proposals. Only the literal true turns it on. |
| VITE_APPROVAL_SCOPE_GRANTS | false | WS13-S6: the thread's active approval grants, with Revoke. Built by W2-WS13-S6-spa-grants. Only the literal true turns it on. |
| VITE_CHAT_CHANGED_FILES | false | F-t3-076 changed-files card under each answer. Off, no answer shows a card. Only the literal true turns it on. |
| VITE_CHAT_DOUBLE_ESCAPE_STOP | false | F-t3-077 double-Escape stop in chat. Off, a single Escape keeps its blur. Only the literal true turns it on. |
| VITE_CHAT_READER_POLISH | false | F-t3-098 reader polish: the answer text size, a dyslexia-friendly font and transcript spacing. Off, nothing is written and the transcript renders as before. Only the literal true turns it on. |
| VITE_CHAT_SPEED_TIER | false | Chat Fast chip: shown only on a model that declares the priority tier, and the turn then asks for speed priority. Off, the composer shows no chip and sends no speed. Only the literal true turns it on. |
| VITE_SHELL_TOASTS | false | F-t3-078 shell toasts: short notices that outlive the panel that raised them. Presentation only. Off, showing a toast does nothing. Only the literal true turns it on. |
| VITE_ZDR_WAIVER_UI | false | ZDR-Z-19d: the SPA's connection ZDR acknowledgement UI and its client. Off, nothing is read from the gateway and nothing is shown. Only the literal true turns it on. |
| ZDR_ACKNOWLEDGEMENT_ENABLED | false | ZDR-Z-13: the connection ZDR acknowledgement routes in integration-corev2. Off, the routes do not exist. Only the literal true turns it on. |
| ZDR_TYPED_REFUSALS_ENABLED | false | Z-05a: on a ZDR or waived turn, web search and file or artifact writes are typed zdr_feature_not_supported refusals (web_search, artifact_output) before any call, not silent drops. Off, unchanged. Only the literal true. |
| ZDR_WAIVER_ENABLED | false | ZDR-Z-14: the per-connection ZDR waiver issuer. Off refuses every waiver with zdr_connection_not_supported. Only the literal true turns it on. |
| ZDR_WAIVER_UI_ENABLED | false | ZDR-Z-19a: the BFF asks Auth Core for a retention decision on a subscription chat turn and mounts the connection ZDR routes. Off, no decision is asked and no route is mounted. Only the literal true turns it on. |

## 7. Depth-register rows closed in code (96)

Rows of `docs/plans/model-plane-depth-register.md` that a built unit names as its source. The register itself is not edited by this phase; a row is closed in code here and owes its proof in the verification phase. The step F triage (569 open P0/P1 code rows) is summarised in the open decisions, section 3.5.

| Row | Units | Batch commit |
|---|---|---|
| D0-019 | W2-WS23-S3-a | 2db962669 |
| D1-615 | P0-D3-956 | 4b0cf580a |
| D2-109 | W2-WS26-S2-a | 8c43757d4 |
| D2-110 | W2-WS26-S2-b | 8c43757d4 |
| D2-113 | W2-WS26-S2-a | 8c43757d4 |
| D2-219 | F-f02-10 | 63f62cb0f |
| D2-298 | F-f02-18 | 63f62cb0f |
| D2-354 | W2-WS14-S5-a, W2-WS14-S5-c1, W2-WS14-S5-c2, W2-WS14-S5-d | 211b2a404, 63f62cb0f, d5f89aab0 |
| D2-355 | W2-WS23-S3-b | 8c43757d4 |
| D2-357 | W2-WS23-S3-a | 2db962669 |
| D2-358 | W2-WS23-S3-c | 8c43757d4 |
| D2-460 | P0-D2-460 | 4b0cf580a |
| D2-461 | P0-D2-461 | 4b0cf580a |
| D2-462 | P0-D2-460 | 4b0cf580a |
| D2-463 | P0-D2-461 | 4b0cf580a |
| D2-480 | F-f03-18, P0-D2-480 | 4b0cf580a, 63f62cb0f |
| D2-481 | P0-D2-481 | 4b0cf580a |
| D2-482 | P0-D2-481 | 4b0cf580a |
| D2-483 | F-f03-18, P0-D2-480, P0-D2-509 | 4b0cf580a, 63f62cb0f |
| D2-509 | P0-D2-509 | 4b0cf580a |
| D2-510 | P0-D2-509 | 4b0cf580a |
| D2-539 | W2-WS23-S4-a | 5d5096414 |
| D2-540 | W2-WS14-S5-a, W2-WS14-S5-c1, W2-WS14-S5-c2, W2-WS14-S5-d | 211b2a404, 63f62cb0f, d5f89aab0 |
| D2-577 | R2-OD-12-a-versions-refusals, R2-OD-12-b-matrix-rows | 1c5394660 |
| D2-580 | R2-OD-12-a-versions-refusals, R2-OD-12-b-matrix-rows | 1c5394660 |
| D2-611 | W2-WS24-S4-a | 2db962669 |
| D2-620 | W2-WS17-S9-a | 5d5096414 |
| D2-623 | W2-WS17-S9-a | 5d5096414 |
| D2-625 | W2-WS17-S9-a | 5d5096414 |
| D2-626 | W2-WS17-S9-b | 5d5096414 |
| D2-633 | W2-WS23-S6-a | 8c43757d4 |
| D2-639 | W2-WS19-S8-a | 2db962669 |
| D2-640 | W2-WS19-S8-a | 2db962669 |
| D2-646 | W2-WS19-S8-b | 8c43757d4 |
| D2-698 | P0-D2-698 | 4b0cf580a |
| D2-699 | P0-D2-698 | 4b0cf580a |
| D2-769 | FU-26-executestep-space-list-refuse, P0-D2-769 | 4b0cf580a, 5d5096414 |
| D2-770 | W2-WS18-S5-a | 5d5096414 |
| D2-771 | F-f05-01-executestep-space-search | 63f62cb0f |
| D2-773 | F-f05-01-executestep-space-search | 63f62cb0f |
| D2-775 | W2-WS18-S5-a | 5d5096414 |
| D2-776 | F-f05-01-executestep-space-search | 63f62cb0f |
| D2-976 | F-f06-01 | 63f62cb0f |
| D2-977 | F-f06-01 | 63f62cb0f |
| D2-981 | F-f06-01 | 63f62cb0f |
| D2-982 | F-f06-01 | 63f62cb0f |
| D2-1114 | F-f06-19 | 63f62cb0f |
| D2-1117 | F-f06-19 | 63f62cb0f |
| D2-1119 | F-f06-21 | 1c5394660 |
| D2-1122 | W2-WS13-S7-producers-a, W2-WS13-S7-producers-b | 0120a8e84, 1c5394660 |
| D2-1123 | W2-WS13-S7-grant-digest | 0120a8e84 |
| D2-1124 | W2-WS13-S7-rule-use-b | d5f89aab0 |
| D2-1125 | W2-WS13-S7-rule-expiry | d5f89aab0 |
| D2-1126 | W2-WS13-S7-rule-use-a, W2-WS13-S7-rule-use-b, W2-WS13-S7-zdr-row | 0120a8e84, 211b2a404, d5f89aab0 |
| D2-1127 | W2-WS13-S6-bff, W2-WS13-S7-gw-proposals, W2-WS13-S7-producers-a, W2-WS13-S7-producers-b, W2-WS13-S7-proposal-store-a, W2-WS13-S7-proposal-store-b, W2-WS13-S7-spa-decide, W2-WS13-S7-spa-proposals | 0120a8e84, 1c5394660, d5f89aab0 |
| D2-1149 | P0-D2-1149, W2-WS13-S7-waived-decided | 4b0cf580a, 63f62cb0f |
| D2-1150 | W2-WS13-S7-waived-decided | 63f62cb0f |
| D2-1151 | W2-WS13-S6-bff, W2-WS13-S6-gw-grants, W2-WS13-S6-spa-grants | 0120a8e84, 1c5394660 |
| D2-1154 | W2-WS13-S6-bff, W2-WS13-S6-gw-grants, W2-WS13-S6-spa-grants | 0120a8e84, 1c5394660 |
| D2-1158 | W2-WS14-S5-a, W2-WS14-S5-c1, W2-WS14-S5-e1, W2-WS14-S5-e2, W2-WS14-S5-f1, W2-WS14-S5-f2 | 211b2a404, 63f62cb0f, d5f89aab0 |
| D2-1160 | W2-WS14-S5-a, W2-WS14-S5-b2, W2-WS14-S5-e2 | 211b2a404, 63f62cb0f, d5f89aab0 |
| D2-1162 | W2-WS14-S5-a, W2-WS14-S5-b1, W2-WS14-S5-f1, W2-WS14-S5-f2 | 211b2a404, 63f62cb0f, d5f89aab0 |
| D2-1168 | W2-WS14-S5-b1 | d5f89aab0 |
| D2-1178 | P0-D2-1149 | 4b0cf580a |
| D3-230 | P0-D2-461, P0-D2-480, P0-D2-481 | 4b0cf580a |
| D3-254 | W3-EP-6-S11-route | 80261d66b |
| D3-257 | W3-EP-6-S11-route | 80261d66b |
| D3-263 | W3-EP-14-S6-grant | 80261d66b |
| D3-267 | W3-EP-8-S4-gdpr | 7676e30ac |
| D3-300 | W3-EP-7-S3-b | 7676e30ac |
| D3-301 | W3-EP-7-S3-a | 7676e30ac |
| D3-310 | W3-EP-7-S5-a | 7676e30ac |
| D3-347 | W3-EP-8-S7-a | 7676e30ac |
| D3-356 | W3-EP-8-S8-a | 7676e30ac |
| D3-383 | W4-CC-3 | 4729b8145 |
| D3-413 | W3-EP-6-P3-a, W3-EP-8-S10-a | 80261d66b, db96e60e7 |
| D3-433 | W3-EP-6-S10-bff | 7676e30ac |
| D3-457 | W3-EP-4-S11-href | 80261d66b |
| D3-464 | F-f08-12 | 63f62cb0f |
| D3-508 | W3-EP-4-S1-export | 80261d66b |
| D3-536 | W4-SH-3 | 8c1247ed7 |
| D3-566 | W3-EP-6-S9-runmeta | 7676e30ac |
| D3-567 | W3-EP-6-S9-do | db96e60e7 |
| D3-568 | W3-EP-6-S9-pin | 7676e30ac |
| D3-579 | W3-EP-10-S7-a | 80261d66b |
| D3-647 | FU-01-skip-release, FU-25-skip-continuation-retained-authority | 4b0cf580a, 5d5096414 |
| D3-665 | GAP-W3-EP-1-S5-timeout-copy | 8c43757d4 |
| D3-680 | W3-EP-3-S5-reload | 80261d66b |
| D3-687 | W3-EP-2-S8-ui | 7676e30ac |
| D3-845 | W3-EP-5-S3-deckguard | 80261d66b |
| D3-912 | W3-EP-7-S6-b | 8c1247ed7 |
| D3-940 | F-f10-01 | 63f62cb0f |
| D3-956 | P0-D3-956 | 4b0cf580a |
| D3-974 | F-f10-04 | 63f62cb0f |
| D3-975 | F-f10-05, FU-29-extract-structured-final-url-gate | 1c5394660, 63f62cb0f |
| D3-1147 | F-f10-16 | 63f62cb0f |

## 8. Audit items built before this branch (waves 1-2 and earlier W3 work)

Items that no unit of this branch names, mapped against the build plan ledger and the earlier wave records by the H pass.

| Item | Wave | State | Evidence |
|---|---|---|---|
| F-t3-001 | W1 | built | WS02 slices in .plan-work/execution/queue.json: 6 (5 done, 1 partial); code in main at the cut: WS02-S5 033ebf675, WS02-S6 bc4cd7b2, WS02-S7 0d64d9b9, WS02-S3 7b0b7d62. Owed: a rebuilt-image run with the switch MODEL_GATEWAY_RUN_CONTEXT=full (queue notes on WS02-S6 and S7). Open depth rows: 5 (3 at  |
| F-t3-002 | W1 | partly | WS03 slices in .plan-work/execution/queue.json: 9 (7 done, 2 partial); code in main at the cut: WS03-S9 f8f1bfd3, WS03-S8 c09d71992. Open: S8-B, the recovery worker parking an active fold at its deadline, is split off and waits on an owner design decision (register D1-546, plan build ledger "Still o |
| F-t3-005 | W1 | partly | WS01 slices in .plan-work/execution/queue.json: 9 (4 done, 3 blocked-external, 2 partial); code in main at the cut: WS01-S6 db07dc0fa, WS01-S5 c6a0ff7c4, WS01-S7.code ae4f8b936, WS01-S13.code 998bc7db9. Code is built (attestation lifecycle, check script, Z-60 pass 1 runner); the Z-40 provider attest |
| F-t3-006 | W2 | built | WS13 slices in .plan-work/execution/queue.json: 9 (5 partial, 4 done); code in main at the cut: WS13-S1 709785fa, WS13-S2 771f2f60, WS13-S3 3a7a2cfe2, WS13-S4 9c60f9f61. The grant, rule-proposal and Always chain was finished by 18 branch units W2-WS13-S6 and S7 (B13 to B17); step-up (S5) is built wi |
| F-t3-007 | W1 | built | WS07 slices in .plan-work/execution/queue.json: 4 (2 done, 2 partial); code in main at the cut: WS07-S7 d1e53e998, WS07-S8 9cf1fc466, WS07-S9 5696d09eb, WS07-S10 1f556c62c. Owed: the four ignored DB tests and migration 0064 on a real database (register D1-648, D1-649); the S8 and S9 review rounds le |
| F-t3-010 | W2 | built | WS23 slices in .plan-work/execution/queue.json: 17 (7 partial, 7 blocked-external, 2 done, 1 optional-deferred); code in main at the cut: WS23-S1.code 261812a41, WS23-S2 b1c4f67ce, WS23-S3.code d4d714f0b, WS23-S4.code 02af253f3. Reads, Ask-mode reads and attested writes are code-complete behind defa |
| F-t3-011 | W2 | built | WS14 slices in .plan-work/execution/queue.json: 7 (4 partial, 2 done, 1 optional-deferred); code in main at the cut: WS14-S3 defaa28b, WS14-S1 feca82e4, WS14-S4 fbd24f8cb, WS14-S2 d7eba9ba1. Same-run resume (WS14-S5) was finished by branch units W2-WS14-S5-a to i (B13 to B17) and ask_user in the Ask |
| F-t3-012 | W2 | built | WS17 slices in .plan-work/execution/queue.json: 9 (7 partial, 2 done); code in main at the cut: WS17-S1 b4c022a05, WS17-S2 077ef5b69, WS17-S3 3edb6166d, WS17-S7 636d3182d. Message tree, versions and fork are behind switches that default off; the S1 to S9 review rounds left shared-file fixes for the  |
| F-t3-013 | W2 | built | WS18 slices in .plan-work/execution/queue.json: 6 (5 partial, 1 done); code in main at the cut: WS18-S1 2b6beb20a, WS18-S2 6f89a0d98, WS18-S3 63626b042, WS18-S4 e587f166e. The P0 Space-authority rows D2-460, D2-461 and D2-769 were closed in code by branch units P0-D2-460, P0-D2-461 and P0-D2-769 (B1 |
| F-t3-014 | W2 | built | WS19 slices in .plan-work/execution/queue.json: 11 (9 partial, 1 done, 1 optional-deferred); code in main at the cut: WS19-S2 b790795a2, WS19-S4 5eed75298, WS19-S1 4e573f6f4, WS19-S6 d926af6e8. WS19-S10 (Do-loop parity and Do file delivery) is optional-deferred pending the WS24/WS03 persistence and  |
| F-t3-015 | W2 | built | WS16 slices in .plan-work/execution/queue.json: 7 (5 partial, 2 done); code in main at the cut: WS16-S1 dc4826dae, WS16-S2 e1ccfad3c, WS16-S3 5d1477245, WS16-S4 8c0d93d41. WS16-S7 (flip defaults, retire legacy) is partial by design; the S5 review fixes were left for the lead (queue note); live shell |
| F-t3-016 | W1 | built | WS04 slices in .plan-work/execution/queue.json: 9 (5 done, 4 partial); code in main at the cut: WS04-S1 b0cb039a, WS04-S2 2cf7fe317, WS04-S4 8930af52, WS04-S3 8a11dd9ca. All paths are behind switches that default off; WS04-S9 (flip defaults, evidence) is partial by design and the two-replica proof s |
| F-t3-017 | W1 | partly | WS08 slices in .plan-work/execution/queue.json: 11 (4 partial, 3 done, 3 blocked-external, 1 optional-deferred); code in main at the cut: WS08-S1 d43b211ef, WS08-S2.code 50d1907b5, WS08-S3.code f106a331f, WS08-S4.code ddd133116. Open: WS08-S8 durable reference uploads and OCR is optional-deferred (n |
| F-t3-018 | W2 | built | WS20 slices in .plan-work/execution/queue.json: 10 (6 partial, 3 done, 1 optional-deferred); code in main at the cut: WS20-S1 d642fb252, WS20-S2 9cc42cdc6, WS20-S3 9419e6525, WS20-S4 0a3a21909. WS20-S9 owner-bound reads is a gated design only (needs a DEC-02 reversal); branch unit F-f10-29 (schedule |
| F-t3-019 | W3 | built | EP-6 slices in .plan-work/execution/queue.json: 12 (6 partial, 5 done, 1 blocked-external); code in main at the cut: EP-6-S1 1eb6b8fab, EP-6-S2 2731ac318, EP-6-S3 100fb141e, EP-6-S4 aa7fcea79. Phase 1 and phase 2 code is committed, dark; the flip and live proof (EP-6-S6) are an owner step; branch un |
| F-t3-020 | W1 | built | WS07 slices in .plan-work/execution/queue.json: 6 (4 partial, 2 done); code in main at the cut: WS07-S1 fe24d0a17, WS07-S2 7e375c3f8, WS07-S5 3bff5e8dc, WS07-S6 be0a279c1. Owed: live checks; WS07-S5 noted stale Go bindings for MemoryService.GetMemoryControls at commit time (queue note). Open depth r |
| F-t3-021 | W1 | built | WS09 slices in .plan-work/execution/queue.json: 10 (6 done, 4 partial); code in main at the cut: WS09-S1 9e86dd5d1, WS09-S2 e25af2985, WS09-S3 7f1a4612, WS09-S5 dcdd8118a. Owed: the three-run baseline in chat-perf-budget.json and Playwright before/after (WS09-S1 partial); the paged transcript (S8, S |
| F-t3-022 | W1 | built | WS02 slices in .plan-work/execution/queue.json: 9 (4 done, 3 partial, 2 blocked-external); code in main at the cut: WS02-S5 033ebf675, WS02-S2 9325d03f8, WS02-S9 427d05abb, WS02-S11 c34299956. Owed: live cache proofs (WS02-S8 and S10 external halves are blocked-external); S9 and S11 tests had not be |
| F-t3-023 | W1 | built | WS10 slices in .plan-work/execution/queue.json: 9 (4 partial, 4 done, 1 blocked-external); code in main at the cut: WS10-S2 beec17bbf, WS10-S1 f60e32b88, WS10-S3 c2c140210, WS10-S5 8c4f7c601. Owed: the shadow soak and ZDR-TEST leg of the live proof (WS10-S7 blocked-external); open owner decision on  |
| F-t3-025 | W2 | built | WS24 slices in .plan-work/execution/queue.json: 5 (4 partial, 1 done); code in main at the cut: WS24-S5 fa03cca07, WS24-S6 a97d61cbc, WS24-S7 ae8cee3a1, WS24-S8 868a46bc6. Admissions are tables behind switches; WS24-S9 proofs are partial; each further action admission is separate owner-plane work (a |
| F-t3-026 | W3 | partly | EP-1 slices in .plan-work/execution/queue.json: 6 (5 done, 1 blocked-external); code in main at the cut: EP-1-S1 9e6729a6b, EP-1-S2 1c8506be4, EP-1-S3 99b6d0d01, EP-1-S4 99b6d0d01. Open: point-and-comment steering is not designed and waits for the F-t3-031 annotation part (EP-1 design, out-of-scope  |
| F-t3-027 | W3 | partly | EP-2 slices in .plan-work/execution/queue.json: 5 (3 done, 1 partial, 1 blocked-external); code in main at the cut: EP-2-S5 fd6913e8c, EP-2-S6 ff0b539a4, EP-2-S7 1dcf842ea, EP-2-S10 303b156d3. Open: mid-run steering of deep research was demoted by the plan and is not built (EP-2 design, out-of-scope |
| F-t3-028 | W2 | partly | WS27 slices in .plan-work/execution/queue.json: 13 (7 partial, 4 done, 1 blocked-external, 1 optional-deferred); code in main at the cut: WS27-S1 6545a1ef9, WS27-S2 adce30755, WS27-S3 7666aa184, WS27-S4 3490ecc16. Open: WS27-S11 (plan-card upper bound, the pre-run estimate for Do plans) is gated on  |
| F-t3-029 | W1 | built | WS11 slices in .plan-work/execution/queue.json: 14 (7 done, 5 blocked-external, 1 todo, 1 partial); code in main at the cut: WS11-S1.code e7c4c1fd1, WS11-S2.code fd7c0433, WS11-S3.code 4b1da1f3, WS11-S4.code e25af2985. Owed: the P0 probe transcripts and live proofs per route (the WS11-S1 to S5 exter |
| F-t3-030 | W2 | built | WS24 slices in .plan-work/execution/queue.json: 5 (5 partial); code in main at the cut: WS24-S1 fe44ea0e3, WS24-S2 3dca31ce7, WS24-S3 c94362859, WS24-S4 457b044c1. WORKSPACE_TOOLS is off by default; at commit time WS24-S0 had not delivered the do_tool_catalogue.json fixture and S4 left an availabili |
| F-t3-031 | W3 | partly | EP-3 slices in .plan-work/execution/queue.json: 5 (5 done); code in main at the cut: EP-3-S2 947e5678f, EP-3-S3 a881b5dce, EP-3-S4 26d1a6c0a, EP-3-S6 a55d94a70. Open: annotation (point-and-comment) and collaborative documents are out of EP-3 and need their own design, Application Plane realtime and  |
| F-t3-032 | W2 | built | WS25 slices in .plan-work/execution/queue.json: 7 (4 partial, 2 done, 1 blocked-external); code in main at the cut: WS25-S7 f26461ee8, WS25-S10 1cb410cbd, WS25-S6 fb9edf4ff, WS25-S11 d40c70296. WS25-S12 (flag flips, retire the dead hook_configs table) was deferred by the lead on 2026-11-01 (W2-44);  |
| F-t3-033 | W2 | built | WS26 slices in .plan-work/execution/queue.json: 11 (6 partial, 3 done, 2 blocked-external); code in main at the cut: WS26-S2.code 433514a05, WS26-S3 08cfdf99d, WS26-S9 1e2f5743d, WS26-S1.code c8bdea206. Live proofs are external (WS26-S1, S2); the memory and skill write-path P0 rows D2-481 and D2-509 |
| F-t3-034 | W3 | built | EP-14 slices in .plan-work/execution/queue.json: 4 (3 done, 1 partial); code in main at the cut: EP-14-S4 5ba61031d, EP-14-S5 463b415cb, EP-14-S6 0578f4c21, EP-14-S7 fb8aa3c8b. Request snapshots are dark; S4 (run ledger view) is partial and branch unit W3-EP-14-S4-a added the virtualised list (B01); |
| F-t3-036 | W3 | built | EP-2 slices in .plan-work/execution/queue.json: 2 (2 done); code in main at the cut: EP-2-S8 4470e2862, EP-2-S9 566035344. Dark behind switches; the SPA entry was mounted by branch unit W3-EP-2-S8-ui (B02); the flip and live proof (EP-2-S11) are external. Open depth rows: 8 (4 at P0/P1) in docs/plan |
| F-t3-037 | W3 | partly | EP-7 slices in .plan-work/execution/queue.json: 7 (3 done, 3 partial, 1 blocked-external); code in main at the cut: EP-7-S1 b99446228, EP-7-S2 e4e2bee21, EP-7-S3 e992af1da, EP-7-S4 6def508f5. Open: unattended continuation in the user delegated identity (EP-7-S7) is design-only and gated on DEC-02 /  |
| F-t3-038 | W3 | built | EP-5 slices in .plan-work/execution/queue.json: 10 (8 done, 1 partial, 1 blocked-external); code in main at the cut: EP-5-S1 43be4d365, EP-5-S2 b9be9c8ac, EP-5-S3 4df350df0, EP-5-S4 8ac485975. Ask-loop only: decks in Do runs wait for WS19-S10; the template picker was mounted by unit W3-EP-5-S7-ui (B |
| F-t3-039 | W1 | partly | WS01 slices in .plan-work/execution/queue.json: 6 (4 done, 1 partial, 1 blocked-external); code in main at the cut: WS01-S8 80dff45f1, WS01-S9 99d3b9bb7, WS01-S10 59a37b72d, WS01-S11 9b1f81d0b. Tooling is built (signed-manifest gate, rollback rehearsal script, migration policy, runbook); the signed  |
| F-t3-040 | W3 | partly | EP-14 slices in .plan-work/execution/queue.json: 3 (2 partial, 1 done); code in main at the cut: EP-14-S8 a22c66e24, EP-14-S9 fc1b848ab, EP-14-S10 237920f20. Open: EP-14-S9 has no execution-core replay suite for the Do loop (queue note, required fix not done); experiment ids and user-path budgets we |
| F-t3-048 | W3 | built | EP-14 slices in .plan-work/execution/queue.json: 3 (3 done); code in main at the cut: EP-14-S1 fd023c69f, EP-14-S2 db2263f14, EP-14-S3 8609bbc57. All three slices are done; reasoning summaries and find-in-collapsed became separate items (EP-14 design). Open depth rows: 10 (7 at P0/P1) in docs/plans/ |
| F-t3-049 | W2 | partly | WS15 slices in .plan-work/execution/queue.json: 9 (5 partial, 2 done, 2 optional-deferred); code in main at the cut: WS15-S1 dbf8f5c48, WS15-S2 b3448a370, WS15-S4 5ba7c9807, WS15-S3 2663d19aa. Open: WS15-S8 (persisted counts-only notes) and WS15-S9 (connector incident ring) are optional-deferred (D4 |
| F-t3-052 | W3 | built | EP-3 slices in .plan-work/execution/queue.json: 3 (2 done, 1 partial); code in main at the cut: EP-3-S1 20a27838f, EP-3-S5 8068cb77f, EP-3-S7 e4afacb2a. Typed context refs (S5) are dark behind MODEL_GATEWAY_CONTEXT_REFS_ENABLED and the reload relay was added by unit W3-EP-3-S5-reload (B01); slash-te |
| F-t3-053 | W3 | built | EP-2 slices in .plan-work/execution/queue.json: 2 (2 done); code in main at the cut: EP-2-S1 13c2e954e, EP-2-S4 2a529fbb3. The claim ledger and provenance stamp are built; the paid per-claim support check and web authority labels were deferred by the audit card (EP-2 design). Open depth rows: 9 (2 a |
| F-t3-054 | W3 | built | EP-2 slices in .plan-work/execution/queue.json: 2 (2 done); code in main at the cut: EP-2-S2 56ff246c8, EP-2-S3 afc28e9dd. Built dark; the flip and live proof (EP-2-S11) are external. Open depth rows: 4 (2 at P0/P1) in docs/plans/model-plane-depth-register.md. |
| F-t3-055 | W3 | partly | EP-9 slices in .plan-work/execution/queue.json: 3 (2 done, 1 partial); code in main at the cut: EP-9-W1 886817552, EP-9-W2 cada6ea49, EP-9-W3 615bbbf2d. Open: cached-only mode (needs a Quarry mode that does not exist) and the provider pin (no router hook) are reserved keys, not built (EP-9 design);  |
| F-t3-056 | W3 | built | EP-10 slices in .plan-work/execution/queue.json: 9 (8 done, 1 partial); code in main at the cut: EP-10-S1 077dd9454, EP-10-S2 02b6553c4, EP-10-S3 dcb19c75d, EP-10-S4 dab915110. Calendar event cards are out (the frozen actions contract has no calendar write); live proof and flag flips (EP-10-S10) are |
| F-t3-058 | W3 | built | EP-11 slices in .plan-work/execution/queue.json: 10 (7 done, 2 partial, 1 blocked-external); code in main at the cut: EP-11-S1 5c4c5854a, EP-11-S2 482b4a885, EP-11-S3 c9d5bc1fa, EP-11-S4 2360de5fa. A project lock pinning a whole Space to fixed skill versions is deferred; /learn and version pinning w |
| F-t3-060 | W3 | built | EP-8 slices in .plan-work/execution/queue.json: 4 (3 partial, 1 done); code in main at the cut: EP-8-S2 4042af00f, EP-8-S6 60a753c13, EP-8-S7 dff9c6b2c, EP-8-S10 1af8ebad4. Space and agent scopes are dark (SESSION_SPACE_MEMORY_ENABLED, SESSION_AGENT_MEMORY_ENABLED); erase-on-revoke and the agent mem |
| F-t3-061 | W3 | built | EP-8 slices in .plan-work/execution/queue.json: 6 (5 done, 1 partial); code in main at the cut: EP-8-S1 18251a1cd, EP-8-S3 5c4672e11, EP-8-S4 104968eea, EP-8-S5 75b97b7fd. A deterministic profile ships; LLM synthesis of the profile is deferred (OD4 / DEC-30); the EP-8-S1 gateway compile was owed at  |
| F-t3-062 | W3 | partly | EP-9 slices in .plan-work/execution/queue.json: 6 (6 done); code in main at the cut: EP-9-C1 802b59ffb, EP-9-C2 116099222, EP-9-C3 ee1dd89bf, EP-9-C4 0a18451a2. Open: regime tags (owner vocabulary D-C3), the signed release pipeline (production keys), the typed telemetry registry and share links are  |
| F-t3-063 | W2 | built | WS25 slices in .plan-work/execution/queue.json: 6 (6 partial); code in main at the cut: WS25-S4 ffb738224, WS25-S5 6ce0f6ba6, WS25-S1 44e612070, WS25-S2 31e1ca3c3. Loop guard, durable hooks and handles are behind default-off switches (HOOKS_DURABLE and others); the unit tests the WS25-S3 row names h |
| F-t3-067 | W1 | built | WS12 slices in .plan-work/execution/queue.json: 11 (6 done, 3 partial, 2 blocked-external); code in main at the cut: WS12-S1 cbdff1b5, WS12-S6 283af93e, WS12-S7.code 94ce1952c, WS12-S10 520033b9e. Owed: CI runs on the integration branch need a push and the owner default-branch change (WS12-S1, S1.ex |
| F-t3-068 | W3 | built | EP-9 slices in .plan-work/execution/queue.json: 3 (2 partial, 1 done); code in main at the cut: EP-9-T1 6b55dfe24, EP-9-T2 34d6f7bb9, EP-9-T3 8e4e0c40c. The egress catalogue (T1) and effective-policy panel (T3) are written dark and marked partial; the effective-policy origin read (T2) is done; live  |
| F-t3-082 | W5 | not built | W5-082 voice input upgrade: Plan section 3.3.4 says to schedule it only when a customer asks; it needs streaming speech-to-text in inference-core and an EU provider with a ZDR attestation, which has no design (inv2/not-gaps.md; open decision OD-41 in docs/plans/model-plane-open-decisions.md, default |
| F-t3-084 | W5 | not built | W5-084 per-Space environment specs and prebuilt images: Plan section 3.3.4: demand evidence is thin (audit 10.3), do not schedule without a concrete user need, and it needs the EP-9 egress work (E3, E5). Open decision OD-42 in docs/plans/model-plane-open-decisions.md (default: wait). Not in queue.js |
| F-t3-085 | W5 | not built | W5-085 sandbox computer extensions: Plan section 3.3.4: demand evidence is thin (audit 10.3); it also needs the agent browser (F-t3-008) and a billing decision. Open decision OD-42 in docs/plans/model-plane-open-decisions.md (default: wait). Not in queue.json; recorded in inv2/not-gaps.md. |
| F-t3-088 | W5 | not built | W5-088 code edit quality on demand: Plan section 3.3.4: demand evidence is thin; post-write diagnostics (H11) stay in wave 6 under the recorded owner decision OD18 and apply_patch is dropped for the subscription route. Open decision OD-43 in docs/plans/model-plane-open-decisions.md (default: keep H1 |

