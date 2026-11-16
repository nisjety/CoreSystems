# Space W3 build status (lead tracker)

Branch `space-w3` at `C:/dev/space-w3`, off main 7323ed708. Code only: no tests, no builds. Builders: Haiku 5.5, effort max.
Lock groups (one builder per group at a time): sc = session-core/model-gateway/mp-contracts/proto; ctl = user-core; run = execution-core/capability-core; dat = wiki-store/Data; gw = gateway BFF; spa = SPA; app = convex-core.

| slice | group | W3 deps | status | commit |
|---|---|---|---|---|
| WS17-S3 | sc | - | built | 0fe66b9fd |
| WS17-S4 | ctl | - | built | 7454c4c0c |
| WS17-S5 | app+gw+spa | S4 | built | c7226c02b |
| WS17-S6 | spa | S5 | partial (gw leg owed: WS17-S6b) | ae68a0e96 |
| WS17-S7 | ctl+sc+spa | S3 | built (with S7b) | baf0cf5c1 |
| WS17-S8 | sc | S3, S7 | built | 16132335c |
| WS17-S9 | ctl | - | built (with S9b, FIX-3) | 1daf75481 |
| WS18-S1 | run | - | built | 91fd7fec0 |
| WS18-S4a | run | S1 | built | 15a8b0279 |
| WS18-S2 | ctl+run | S1, S4a | built | aca539d6c |
| WS18-S3 | sc | S1 | built | f8d36b857, 56c61d5c4 |
| WS18-S4 | run | S4a, S2, S3 | built | 7134e8f07, 484bb0aa7 |
| WS18-S5 | gw+sc | S4 | built | 4eb484984 |
| WS18-S6 | spa | S5 | built | bef3fec66, 1712ac655 |
| WS18-S7 | sc+spa | S3, S6 | built | 47a910cb3 |
| WS18-S10 | run | S4, S5 | built | ecf51b0d5, 5c88b2776, 2c29a83c4 |
| WS19-S1 | dat | - | built | ca550fee6 |
| WS19-S2 | ctl | - | built | 8d1f3ed3e |
| WS19-S3 | dat | S1, S2 | built | 194343781 |
| WS19-S4 | gw | S3 | built | f4991051f |
| WS19-S5 | spa | S4 | built | 7dc143234, 179b4c8d2 |
| WS19-S6 | spa | S5 | built | 6493a8dda |
| WS19-S7 | spa | S6 | built | 818beff19 |
| WS19-S8 | dat+app | S1, S3 | built | fdb5aee90 |
| WS19-S9 | run+sc | S3, S4, WS18-S1 | built | b7f2ca6e6 |
| WS20-S1 | ctl | - | built | c814a4abc |
| WS20-S2 | sc | S1 | built | 384dce654 |
| WS20-S3 | sc+run | S2 | built | 50bdfefc0 |
| WS20-S4 | sc+gw | S3 | built | 0a5a56ddd |
| WS20-S5 | spa | S3, S4 | built | 0abde829c |

Out of scope this wave: WS20-S6..S9 (blocked by SD-23 default and sibling Quarry F-t2-001/F-t2-013), conditional WS18-S8, WS18-S9, WS19-S10.

## Bugs and issues reported by builders
(filled in as each builder reports)

### WS19-S1 (ca550fee6)
- P1: Save/Restore trust caller-stamped author; WS19-S3 must verify the decision and refuse ZDR first.
- P1: canvas purge runs on the owner pool, not WithOrgScope (plan row vs data-model paragraph conflict; followed the data model).
- P1: HardPurgeByOrg now touches canvas tables, so the migration must be deployed before the code, or org erasure fails.
- P2: 1-byte content minimum blocks "clear canvas by restoring empty" (design 3.12); WS19-S6 decision.
- P2 UNVERIFIED (not compiled): pgx scans, ON CONFLICT under RLS; down migration drops canvas text.

### WS18-S1 (91fd7fec0)
- P1: two Control lookups (golden for decisions, sibling run_action_targets.go table for views) until a joint edit.
- P1 test-gap: fake tickets.create hash in run_action_decision_test.go and owner_effect_reservation_live_test.go:178 now fails the schema-hash check; fixtures need updating in the test phase.
- P1: migration 025 CHECK (action_id='tickets.create') blocks any future Space reservation row.
- P1: Rust registry copy lives inside mp-contracts (Dockerfiles copy only rust/ and proto/).
- P1: pinned tickets.create hash is not sha256 of its params, so a hash_basis field was added.
- P1: Go/TS payload-digest parity scope needs an owner decision before WS18-S3.
- P2 pre-existing: RUN_ACTION_TARGETS has no production caller (v2 view always tickets-only); S2 to close.
- P2: several row values are builder choices (reversible flags, follow_up min_role owner, permission string).
- P2 risk: a golden load failure also refuses tickets.create decisions.

### WS17-S3 (0fe66b9fd)
- P1 deviation: no separate scope_kind column; sibling migration 0103 already holds scope_id/audience columns with `scope` as the kind (lead to confirm).
- P1 contradiction: EP-8 room recall drops the thread's own rows vs WS17 Rule R keeping both (fails safe: forgets, no leak).
- P1 decision: room recall uses exact audience-hash equality (stricter than plan's revision ceiling).
- P1 risk/owner decision: backfill permanently withholds personal rows on threads with NULL space_kind (pre-claim personal-Space notes).
- P1 test-gap: letta-bridge auth_test.go (2 tests) assert user-principal access, now refused; also UNVERIFIED that session-core always presents a service principal.
- P2: backfill = 3 UPDATEs + VALIDATE scan on agent_memory under lock; legacy room thread stops recalling its old notes.
- Fixed pre-existing bug: dedup ignored Space (rows of two Spaces could supersede each other); grounding unpartitioned.
- Compile risk: 15-field sqlx tuple in lock_rows; format! captures in label_read_sql.
- Not done: K4-13 arbiter index recreate (would break sibling ON CONFLICT); batched backfill task.

### WS17-S4 (7454c4c0c)
- P1 deviation: user-core 044 already used twice on main; migration is 045 and carries the slot-045 entitlement bits (personal_agent_admit, memory_declassify).
- P1 deviation: SPACE_AGENT_ADMISSION defaults off (plan says observe); rollout must set observe explicitly.
- P1 risk: under enforce, unadmitted/inactive agents are dropped from roster and seats deactivated; legacy ids not matching ^agent-[A-Za-z0-9_-]{6,64}$ are not backfilled; read would_skip rows in observe first.
- P1 decision: cascade B runs in every mode (removal only); sweeper only when mode != off.
- P2: RemoveMember cascade writes no membership-ledger row; observe writes duplicate would_skip rows per sync.
- P2: Convex confirm keeps ALL Space bindings pending when any agent is skipped.
- P2: P-CFG train must carry the two new gateway scopes; principal-train row names the flag under WS18-S4 (contradiction).
- P2 test-gap: routes_spaces_test, service_auth_test, entitlement_catalog_test goldens likely fail.
- UNVERIFIED (not compiled): gin route conflicts /:space_ref/agent-* vs wildcards; pgx typing of ANY($n::text[]).

### WS18-S4a (15a8b0279)
- P1 pre-existing bug FIXED: strict decode refused capability-core's degraded run views (reason_code), so a disabled tickets.create refused parked resumes (CheckpointUnavailable) and dropped the tool. Worth shipping alone.
- P2 gap (for S2): resolveModelActionView returns at the first unavailable row (model_action_view.go:243-250), dropping later admitted rows.
- P2 for S4: read `level`, refuse `ask` rows unless EXECUTION_CORE_SPACE_SELF_API_RESUMABLE; replace the actions.len()>1 guard and tickets.create-only name check with the registry; log reason_code.

### WS19-S2 (8d1f3ed3e)
- P1 test-gap: registration_state_scan_test.go (TestOnlyTheReadSurfacesResolveAnArchivedRoom) fails until its list adds ResolveSharedCanvasDecisionEvidence.
- P1 gap: since_join read carries history_floor_revision, nothing in Data enforces it (passed to WS19-S3).
- P1 risk: S3 must derive `create` op and canvas_id exactly as signed (passed to WS19-S3).
- P1 deploy order: migration 046 before code (personal decision query selects canvas_entitled).
- P2 pre-existing: org_policy_test explicitOnlyEntitlements lacks canvas, personal_agent_admit, memory_declassify.
- P2: design denial codes (canvas_not_entitled, ...) not added; built closed vocabulary used. No `collaboration` preset exists, so any preset switch turns canvas off.
- P2: untyped repo failure on evidence answers 403 (design says 503); fails closed.

### WS18-S3 (f8d36b857, 56c61d5c4)
- P1 deviation: plan supersedes design (space_run.proto, 0084, sessions.proto tags 31/32, self_api.rs); Control decision has 29 claims not 26.
- P1 interop (for WS18-S4): flatten args exactly as argument_pairs (absent fields -> len 0, untrimmed, golden argument_order); map trailer words feature_disabled, not_offered, not_authorized, run_not_active, title_owned_by_human, limit_reached, idempotency_key_reused, invalid_arguments.
- P1 deploy: Control decision key id must be in session-core's public key set.
- P1 compile risk: prost oneof paths, FromRow Option<DateTime>, strict 29-field claims struct vs Control JSON.
- Follow-up: buf generate for Go/Python/TS bindings of sessions.proto + space_run.proto; probe scope session:run-thread-action:probe needs P-CFG + execution-core session_terminal_auth.rs (WS18-S4).
- P2: run must be root+persistent (beyond design); per-run cap counts NOOP receipts.

### WS19-S3 (194343781)
- P1 OWNER DECISION: since_join rooms with SPACE_HISTORY_WINDOW_CLAIM on -> canvas refused 403 space_history_floor_unsupported (builder chose refuse over honour). Lead default: keep refuse (fail closed) until the owner decides.
- P1: one content-free head read runs before the digest check (wire body has no canvas id); writes only after digest match.
- P1 contract for WS19-S4: gateway must derive op/canvas_id identically (create + empty id when no active canvas; expected_version_id null, never "").
- P1 race: ActiveCanvasID and save in separate txs -> fix assigned to WS19-S8.
- P1 test-gap: digest matches Control by inspection only; golden vectors unasserted.
- P2 pre-existing: documents-api compose sets CONTROL_SPACE_DECISION_PUBLIC_KEYS_JSON but build script exports only KEY_ID + PUBLIC_KEY_BASE64 -> documents-api may fail closed (WS14).
- P2: no nonce check, decision replayable within 2 min (bounded by idempotency + CAS); flag-off answers typed 404.

### WS18-S2 (aca539d6c)
- P1 deviation: capability-core migration is 0045 (0027 taken; runs to 0044).
- P1 deviation: WS24's v2 (keyed by action_schema_hashes, same envelope name) already landed; adopted plan's actions[] shape, kept MODEL_ACTION_ADMISSION=table gate. WS24 owner must reconcile.
- P1 rollout gap: Space v2 offers also need MODEL_ACTION_ADMISSION=table on capability-core (off in prod), missing from the design's rollout list.
- P1 test-gap: WS24 TestModelActionViewTicketsCreateIsByteIdenticalUnderV2 fails by design; Control tests TestIssueModelActionViewIntersectsEntitlement, TestRunActionTargetTableSeedsTicketsCreateOnly, TestRunActionDecisionRejectsActionOutsideTable fail until rewritten.
- P1: edited plane-service-principals.json (P-CFG train file); parity spec + sha re-pin owed.
- P1 decision: v2 view needs allowlist entry even for tickets.create; empty allowlist = 403 (passed to WS18-S4 as "no tools").
- Fixed: RUN_ACTION_TARGETS superseded (lookups joined); resolver no longer drops rows after the first unavailable one.
- UNVERIFIED: go:embed paths, init order, whether a Space row reaches `agentic` after attestation.

### WS17-S5 (c7226c02b)
- P1 deviation: access list read from a Convex projection (Control has no entries read); a failed projection write leaves it stale and a re-save could overwrite collaborators. Needs a Control read.
- P1 OWNER DECISION: bind/room-create registers unregistered org agents at Control with the actor as owner, so the owner sweep can suspend an org agent when that person leaves.
- P1 risk: a Control-invisible org agent (e.g. suspended) shows as addable; bind then refuses.
- P1 deviation: installable list filters by Control's visible subject ids (design filter would hide agents from listed collaborators); added withdrawSpaceAgentBindingForGateway (pending bindings could not be revoked).
- P1 test-gap: gateway installable/bind tests in mod.rs (~9869-10040) and route/service-auth/features goldens fail until mocks updated.
- P1 UNVERIFIED (not compiled): proxy_json DELETE/PUT, super::valid_avatar_color, SPA createResource overload, keyed Show accessor.
- P2: route param {agent_id} not {agent_ref} (matchit conflict); my-agents/installations scan all org bindings (no index); installations now hides foreign personal agents (fixes an org-wide name leak; visible behaviour change).
- P2 product: SPA card shows both revoke and withdraw; bind dialog shows add state regardless of flag.

### WS19-S8 (fdb5aee90)
- Fixed: WS19-S3 canvas-id race (id checked in writeTx) and late-create window (per-Space advisory lock + space_canvas_purges fence; new migration 20261116112048).
- P1 deviation: purge route NOT gated by WIKISTORE_SPACE_CANVAS (flag-off must still erase; a 404 would stall Space deletion). Lead: agree.
- P1 FOLLOW-UP (Control, unassigned): no Control mint site / coordinator principal for scope data:space-canvas:delete; without it every canvas leg 403s and Space deletions stall.
- P1 deploy order: canvas migrations before wiki-store, wiki-store before Convex, set DATA_SPACE_CANVAS_DELETION_URL in the same window.
- P1 risk: purge takes org from the bearer; a bearer for another org yields a false "succeeded" erasure; coordinator mint must be org-bound.
- P2: create replay after concurrent create now canvas_head_moved; org erasure has no write fence (same as all HardPurgeByOrg tables); purge allowed for zdr tokens (erasure).
- P3 pre-existing: first-leg retrieval receipt check does not compare space_ref.

### WS20-S1 (c814a4abc)
- P1 test-gap: golden digests computed by a Python re-implementation, not Go; TestBrowserRunDigestGolden must reproduce before S2 pins.
- P1 pre-existing test drift: org_policy_test presetCoveredEntitlements stale (canvas, personal_agent_admit, memory_declassify); registration_state_scan_test fails until canvas resolver listed.
- P1 deploy order: migration 047 before the Control image.
- P2 OWNER DECISION: `browser` added to the agent_computer preset (per plan), so applying that preset enables room browser runs; canvas is in no preset.
- P2: dedicated browser resolvers instead of plan's shared-thread ones; DMs/group DMs refused; stricter host validation (no wildcards/.localhost/all-digit labels).
- P2 gap: Control accepts view_scope=room while SPACE_BROWSER_VIEW_ROOM is off; Model Plane must withhold frames (passed to WS20-S2).
- P2 pre-existing: two user-core migrations numbered 044 on main.

### WS17-S6 (ae68a0e96) PARTIAL
- Owed WS17-S6b (gw): GET/POST /api/v1/spaces/{ref}/private-panel, private_panel context feature, SPACE_PRIVATE_PANEL_UI flag, spaces.open_private_panel handler arm (action-surface-contract test flags the missing arm meanwhile); pointer-row purge on Space deletion/member removal.
- P1 gap: no path locks the private thread to sourceScope 'conversation' (first turn sets scope; no non-Space owner-thread create route). Flag must stay off until it does (SD-22 v1).
- P1 deviation: panel is a registry pane, not a SpaceAuxPane union member.
- P2 test-gap: space-pane-registry.test.ts:11 pins SPACE_PANES to the thread pane; action-registry count 386->387; _generated/api.d.ts hand-edited.
| WS17-S6b | gw+app | S6 | built | 9ce028db3, fe0b71e40 |

### WS19-S4 (f4991051f)
- P1 OWNER DECISION: replaying a first create after the canvas exists derives `save` -> 403 space_decision_digest_mismatch instead of deduplicated (first write did land). SPA must not show it as a plain failure.
- P1: added the head read the design omitted (4 upstream calls per save).
- P1 UNVERIFIED: can_write assumes Control membership?readable=1 returns data.role (informational only).
- P2: Control 400 canvas_decision_invalid relayed as 503 space_decision_unavailable (drift looks like an outage).
- P2: Edit tool strips raw CR bytes in config.rs test literals; check git diff for deletions.
- Not done: message_search.rs space_features not extended.

### WS18-S4 (7134e8f07, 484bb0aa7)
- P1 deviation: RunAgentRequest.tool_profile = 28 (25/26/27 taken); WS18-S5/model-gateway must use 28. buf generate owed.
- P1: hot-file touch sse.rs (tool_profile literal) per 4.4.1 row 16; integrator note.
- P1 PRE-EXISTING BUG (unfixed): ticket v1 view request sends action_ids under MODEL_ACTION_ADMISSION=table; Control strict decoder 400s it, so tickets.create view is unavailable in table mode.
- P1 risk: keep EXECUTION_CORE_SPACE_SELF_API_RESUMABLE=0 until WS10-S9 (no resume for ask rows).
- P1 rollout: Space tools need MODEL_ACTION_ADMISSION=table + CONTROL_SPACE_SELF_API_ACTIONS + both execution-core flags.
- P2: tickets.create dropped from v2 Space view; TOOL_CLASS_GATE=enforce unverified for Space rows; forwarded bearer assumed to be run owner's.
- Compile risk: prost oneof paths, let-else tuple in SpaceSelfApiClient::from_env, parse_arguments closure.
| W3-FIX-1 | run | - | built | ecce00c6d |

### WS17-S9 (1daf75481) PARTIAL
- P1 bug FIXED: retrieval cache key had no Space scope (filters applied after read) -> two rooms could share a cached answer for a member of both. Now partitioned by space + audience hash + revision.
- Owed WS17-S9b: model-gateway retrieval.rs consumer of chunk space_ref (crate locked to WS20-S2); gRPC retrieval label needs a proto field.
- P1 test-gap: space_scope.rs test retrieval_decision_is_a_signed_target_bound_authority_not_a_client_filter fails (payload lacks the two required claims).
- P2 deploy order: migration 20261116114344 before the new engine or every trace fails to persist.
- P2 gap: Space-scoped cache hit not re-checked against the room's current documents until expiry/org-version change.
| WS17-S9b | model-gateway retrieval.rs | S9 | built | 905bf7062 |

### WS20-S2 (384dce654)
- P1 gap: no active-run count yet (S3 must 503, never admit) -> passed to WS20-S3.
- P1 risk: golden digests only Python-computed; verifier follows Go source; reconcile Go + golden first if DigestGolden fails.
- P1 gap: frames_visible_to is a helper only; S3/S6 routes must call it -> passed to WS20-S3.
- P2 bug (Control WS20-S1): browserRunPayloadDigest hashes RAW recipient audience/privacy policy/resource refs while newEvidenceDecision writes them TRIMMED -> padded values fail closed. Control fix owed.
- P2 deviation: verifier in mp-contracts/model-gateway, not browser-broker (plan overrides design D5); duplicates session-core's v2 Ed25519 check (consolidate later); new mp-contracts deps (base64, chrono, ed25519-dalek) + hand-edited Cargo.lock.
- P2: decision replayable 2 min -> S3 keys start on idempotency key (passed).
- P3 UNVERIFIED: goal digest hashes raw goal bytes (BFF rule unconfirmed). No money cap in the decision; no key-set rotation.

### W3-FIX-1 (ecce00c6d)
- Fixed pre-existing bug: execution-core ticket_tools.rs sent action_ids (from WS24-S7 ae8cee3a1) on the v1 view under MODEL_ACTION_ADMISSION=table; Control 400'd it. Request is now {run_id, org_id}; Control input not widened.
- Test-gap: model_action_view_request_names_pinned_actions_only_with_the_table fails at runtime until rewritten.

### WS17-S6b (9ce028db3, fe0b71e40)
- P1 gap -> queued W3-FIX-2 (session-core, after WS17-S7): CreateThread ignores metadata, threads.source_scope defaults 'workspace' (0038), only the first append locks it; need a create path stamping conversation scope. Until then POST private-panel refuses creation with 501.
- P2: 501 not in design/SPA status list; SPA lacks copy for private_panel_creation_unavailable (501) / private_panel_unavailable (503); flag must stay off outside dev.
- P2 gap: room-level member removal has no Convex hook with the person's id -> pointers stay (inert).
- P2: pointer purge rides the Data delivery event (assumed for every Space deletion, UNVERIFIED).

### WS19-S5 (7dc143234, 179b4c8d2)
- P1 gap: gateway canvas.rs DATA_PLANE_CODES omits space_history_floor_unsupported (403 becomes 503) -> fix assigned to WS19-S6.
- P1 test-gap: space-tab-registry.test.ts:16 pins SPACE_TABS ids; fails until `lerret` added after `chat`.
- P2 PRE-EXISTING: ERROR_COPY.space_decision_unavailable carries posting copy ("your message is kept") while six gateway files emit that code for non-post refusals.
- P2: 15 s refresh costs a Control read decision + wiki GET per check; listSpaceCanvasVersions direct fetch skips the dev Authorization bypass.
- P2 deviation: tab registry is space-tab-registry.ts not SpaceCockpit.tsx; canvas copy via canvasErrorMessage.
| W3-FIX-3 | retrieval-engine-rs | S9b | built | d095ecfeb |

### WS17-S9b (905bf7062)
- P0 bug FIXED: Space-scoped turn with MODEL_GATEWAY_GRAPH_SPACE_SCOPE off made an UNSCOPED /v1/retrieve/graph call (org-wide graph). Now no graph leg. Overrides flag table "Off, unchanged" -> owner to confirm.
- P1 gap -> W3-FIX-3 running: gRPC Candidate.metadata is None in retrieval_svc.rs, so ALL room grounding is withheld until the Data Plane labels gRPC candidates. Do not ship S9b without FIX-3.
- P1 deviation: Space read unverified from the decision token's space_ref claim (retrieve_checked signature shared with sse.rs/deep_research.rs); safe because Data Plane verifies the same token. Follow-up: pass verified Space id.
- P1 gap: with graph switch on, graph entities/communities are not label-checked.
- P2: context-pack facts unlabelled -> scoped answers drop all of them incl. pinned memory facts; labels not on SSE citation event.

### W3-FIX-3 (d095ecfeb)
- gRPC Retrieve/RetrieveStream now label candidates metadata.space_ref from the verified authority; room grounding works once deployed.
- P1 gap -> queued W3-FIX-4 (model-gateway, after WS20-S3): gRPC GetChunks/PackContext take no Space decision; model-gateway POST /v1/retrieval/chunks and /v1/retrieval/pack forward only the bearer -> org-visible unlabelled chunks; gate/refuse for room-scoped callers (SD-22).
- P2: untrimmed signed space_ref vs model-gateway trimmed claim -> padded id drops all chunks (fails closed).

## Lead queue (follow-ups not yet assigned)
- W3-FIX-2 + W3-FIX-6: done.
- W3-FIX-9, W3-FIX-15: done.
- W3-FIX-16 running (+ agent_card forwarding + ACTION_TOOL_NAMES): windowed timeline projections (session-core space/timeline.rs select title_source/pinned_source; model-gateway space_routes/timeline.rs summary_json emit them); stale comment core.ts:151-152.
- W3-FIX-14 running (+ errors.ts duplicate keys): gateway message_search.rs space_features add "memory_declassify": flags.space_memory_declassify_api (share control hidden until then); private panel chat sends source_scope conversation on every append (first send currently refused, fail-closed); tidy private_panel_* copy; stale comment space-canvas-save.ts:135.
- W3-FIX-11 (end of wave): ZDR surface-registry rows + pins for every new W3 route (incl. POST /v1/chat/conversation-threads, /v1/memories/declassify, gateway declassify/retract, canvas, private panel, browser runs, self-api).
- W3-FIX-10: done. W3-FIX-17 running (receipts name org + Space).
- W3-FIX-4: done. W3-FIX-7 running (P0 tool-loop leak).

- W3-FIX-5: done.
| W3-FIX-4 | model-gateway retrieval | FIX-3 | built | a0aeb0d32 |

### WS20-S3 (50bdfefc0)
- P1 race -> add to W3-FIX-6 (session-core/model-gateway): per-Space cap is check-then-start; two concurrent starts can admit a third run. Needs a per-Space lock.
- P1 OWNER RULING: CountActiveRoomBrowserRuns/ResolveRunSpace rest on user credential + org, not a Control read decision or Space membership.
- P1 UNVERIFIED: run key stored verbatim in managed_run_terminalization_outbox.start_key; Control space_ref == thread space_id; session-core accepts GatewayBrowser on Space threads (mismatch breaks replays or refuses every room start).
- P1 test-gap: browser_run.rs standalone_browser_start_refuses_explicit_space_binding panics on space_context/space_browser_authority bodies.
- P1 deviation: WireObservation.challenge already exists (plan premise stale); restricted browser RPC outside the ResolveRunSpace check (room runs never use it).
- P2: result-turn format unverified; cancelled runs write no result turn; cost clamped from body not org ceiling; space_run.proto Go bindings regen owed.

### WS19-S6 (6493a8dda)
- Fixed: history-floor code relayed by gateway; private-panel 501/503 copy added; create-retry rule implemented (never plain failure).
- P1 compile risk: Solid Show callbacks, two-arg createEffect, union narrowing unverified.
- P1 for S7: restore must use its own clientSaveId prefix (passed to WS19-S7).
- P2: commit made from the index (not pathspec); errors.ts holds an uncommitted WS17-S7 hunk (memory_declassify_not_authorized) that WS17-S7 must commit.
- P2 gaps: archived rooms still show Edit (server refusal only); deduped replay can show a non-head version briefly; room switch discards draft (assigned to S7); summary field not offered.

### WS17-S7 (baf0cf5c1) PARTIAL
- Owed WS17-S7b (model-gateway, after WS18-S5/FIX-4): POST /v1/memories/declassify + DeclassifyMemory call (gateway route 404s until then; keep flag off); retract route + SPA retract control + model-gateway delete path; 3 MemoryService mocks need declassify_memory (grpc.rs:4181, http_routes.rs:23015, tests/e2e_invoke_chain_test.rs:12514) or test builds fail.
- P1 OWNER DECISION: a declassifier removed from the room or demoted to viewer cannot retract; their shared copy stays (D-5 assumed owner-only).
- P1: memory.proto Go bindings not regenerated.
- P2: retraction removes the shared row for every live share of identical text; describe() in errors.ts shadows new copy when details.reason present; goldens to re-pin (catalogue Decisions, admission golden, registry 382->383).
- UNVERIFIED: sqlx tuple decode, ON CONFLICT across two unique keys, repeated $4 bind.
| W3-FIX-7 | model-gateway tool_loop.rs | FIX-4 | built | 7f1d9ff12 |

### W3-FIX-4 (a0aeb0d32)
- model-gateway /v1/retrieval, /chunks, /pack, /sources refuse 403 space_scope_unsupported when x-space-decision is present. No in-tree caller reaches chunks/pack today.
- P0 -> W3-FIX-7 running: inline tool loop knowledge_search / graph / wiki / contradictions send no Space decision on room turns (org-wide reads; bypass S9b guard and SD-22).
- P1: knowledge_list_documents refused on Space turns only if OWNER_LIST_SPACE_REFUSE=true (in FIX-7).
- P1 residual: Claims has no Space field; a caller omitting the header is not detected (Control follow-up: verified Space claim on token).
- P2: Data Plane gRPC GetChunks/PackContext still apply no decision; GET /v1/retrieval/traces/:id unguarded.
| WS17-S7b | model-gateway+gw+spa | S7 | built (inside bef3fec66) | bef3fec66 |

### WS18-S5 (4eb484984)
- P1 deviation: sse.rs got 17 added lines (not one call-site line) -- integrator approval needed under 4.4.1 row 16.
- P1 risk: client-named `space_self_api` stripped on known chat entry points only; sweep other BFF paths forwarding chat bodies before SPACE_AGENT_SELF_API_API goes on.
- P1: compose lines for MODEL_GATEWAY_SPACE_SELF_API and SPACE_AGENT_SELF_API_API not added (default off when unset); add with rollout.
- P1 test-gap: agent-action-admission.json golden and action-registry.test.ts Space count (~1812) fail until updated.
- P2 decision: `agentic` granted to require_confirmation self-API bindings (design 3.8) -- review vs WS10 ceilings.
- P2 risk: model-gateway trusts space_self_api from the body with no BFF-attested marker (relies on gateway being sole caller under service auth, unverified).

### WS19-S7 (818beff19)
- Restore via action with canvas-restore-<uuid>; one conflict banner with "Review differences" (existing diffText); history pages of 50; dev-bypass fix via new requestJsonPage; room-switch guard via useBeforeLeave.
- P2 follow-up (gateway, small): canvas.rs derives canvas-save- prefix for restores too; should be canvas-restore-.
- P1 risk: not type-checked; useBeforeLeave typing read from live checkout's @solidjs/router 2.0.0-next.16.
- P2: panel tests must render under a router; back/forward always ask with dirty draft; diffText up to 2000x2000 on main thread (on demand).
| W3-FIX-5 | ctl | - | built | ed3ce4fda |

### W3-FIX-5 (ed3ce4fda)
- Fixed raw-vs-trimmed digest in browser, canvas and memory-declassify decisions (goldens unchanged). Model Plane verifier already matched Go.
- Added principal control-space-deletion (audience data-plane, scope data:space-canvas:delete, persistent) to plane-service-principals.json.
- P1: PRINCIPAL-TRAIN says rows change only via weekly train -> integrator accepts/batches; scope request in depth.
- P1 gap: retrieval-leg scope data:space-binding:delete granted nowhere; one static bearer CONTROL_SPACE_DELETION_DATA_BEARER covers one org -> Convex needs per-org mint (directory-checked) else false/blocked erasures.
- P1 test-gap: plane-service-principals-policy.contract.spec.ts pins principal key set (~1312) and consumerEnv (~560).
- P1 OWNER: `persistent` retention on the new row.
- P2 PRE-EXISTING: same raw-ref digest pattern in personal_thread_decision.go (~525, ~602), scheduled_run_decision.go:77, scheduled_step_decision.go:132, schedule_create_decision.go:99.
| W3-FIX-8 | compose+execution-core owner_reads.rs | FIX-7 | built | 4fa05ba6e |

### W3-FIX-7 (7f1d9ff12)
- Room turns: knowledge_search/graph/wiki/contradictions not offered and refused at dispatch (space_scoped); knowledge_list_documents refused by default (only literal false opts out). Personal turns unchanged.
- P0 -> W3-FIX-8 running: compose + flag registry default OWNER_LIST_SPACE_REFUSE=false, so compose deployments keep the opt-out.
- P1 -> W3-FIX-8: execution-core owner_reads.rs Do-run reader refuses only on literal true.
- P1: room answers get no org knowledge until FIX-3 (Data Plane labels) deploys; knowledge_search withheld, not rerouted (no decision token in the loop).
- P1 test-gap: tool_loop.rs:13161 and :13163 rows of knowledge_list_documents_in_a_space_follows_owner_list_space_refuse_alone fail.
- P2: Space signal is a request field (space_context), not a verified claim; sse.rs:170 wrapper now dead.
| W3-FIX-2 | session-core | - | built | eacfb5919 |
| W3-FIX-6 | session-core | - | built | 643c09fcd |

### W3-FIX-2 (eacfb5919) / W3-FIX-6 (643c09fcd)
- FIX-2: CreateThread honours metadata.source_scope=conversation (locked at insert; other values refused; reused non-conversation thread refused). Callers still owed (W3-FIX-9).
- P1 risk: PrepareUserOwnedThread drops metadata -> private panel must not use it; every append to a locked thread must carry source_scope conversation (chat send path UNVERIFIED).
- FIX-6: per-Space pg_advisory_xact_lock + recount before GatewayBrowser run/outbox rows; refusal resource_exhausted trailer browser_capacity. model-gateway mapping to 429 owed (W3-FIX-9).
- P2: count uses runs.status; a stuck non-terminal run holds a slot until terminalized (sweeper UNVERIFIED).
- Verified: start key stored verbatim (closes one WS20-S3 assumption).

### W3-FIX-8 (4fa05ba6e)
- OWNER_LIST_SPACE_REFUSE defaults true in both compose files + flag registry (onDefaultReason OD-06/SD-22); execution-core owner_reads refuses unless literal false.
- P1 OWNER/RELEASE: deployment change; Space turns lose org-wide listing where the var was unset; flag-registry README says compose default changes go through owner + release.
- P1 test-gap: owner_list_space_refuse_compose_test.rs (3 fns pin :-false); owner_reads.rs 3 tests; runtime_loop/mod.rs 2; runtime_loop/agent.rs 1.
- P2: whitespace-only space_id is personal in execution-core (trim) but Space in model-gateway; sse.rs owner_list_space_refuse_enabled dead and contradicts the default.
| W3-FIX-9 | model-gateway+gw | FIX-2, FIX-6 | built | 0e4d8107c |

### WS17-S7b (code inside bef3fec66, committed under WS18-S6's message)
- model-gateway POST /v1/memories/declassify; gateway retract route + action memory.retract_from_space; SPA retract in dialog; 3 mocks fixed.
- P1 process: sibling committed the shared index; lead accepts attribution (no history rewrite).
- P1 gap -> W3-FIX-11 (registry sweep): space-surface registry rows missing for mg:POST:/v1/memories/declassify, gateway declassify + retract routes; lints every_non_get_route_is_classified and space_routes_zdr_classes fail.
- P1 gap -> SPA follow-up: nothing mounts MemoryDeclassifyDialog (no UI entry point); sharer cannot list own shares.
- P1 OWNER (carried): removed sharer cannot retract.
- P2: 503 shows posting copy of space_decision_unavailable (pre-existing copy bug).
| W3-FIX-12 | ctl | WS19-S9 | built | c41b40e31 |
| W3-FIX-13 | spa | WS17-S7b | built | e4806bcb0 |

### WS19-S9 (b7f2ca6e6)
- Golden rows space.canvas.read/write (all copies byte-identical); wiki-store run-action verifier; execution-core space_canvas_tools.rs (flag SPACE_CANVAS_AGENT_TOOLS off); trust class RoomCanvas; capability-core migration 0046.
- P1 -> W3-FIX-12 running: Control run-action path lacks canvas_entitled/kind/history-floor checks. Do NOT name canvas rows in CONTROL_SPACE_SELF_API_ACTIONS until fixed.
- P1 gap: no wiki-store attester/principal and admission CHECK accepts only owner_contract -> canvas tools dark (P-CFG + migration needed).
- P1 test-gap: Go RunPayloadDigest vs Rust payload_digest need a shared golden vector for space.canvas.write.
- P2: write row is `ask` (needs RESUMABLE after WS10-S9); several row values (phase, caps, rungs, ids) builder choices; pre-existing availability.go maps cap.tool.space.schedule.follow_up to execution-core vs WS18 capability-core.

### WS18-S6 (bef3fec66, 1712ac655)
- SPA vendored registry (byte-identical, pin 635587a2...), toggle, provenance flags, i18n.
- P1 OWNER DECISION: canvas rows sit in the same space_self_api profile; once Control's allowlist names them, the "rename and pin" switch would also offer canvas tools. Change the copy, or exclude canvas from the profile?
- P1 gap -> next model-gateway fix: ThreadSummaryResponse (http_routes.rs:15418) must forward title_source/pinned_source or the chips stay dark.
- P2: S1 sync test must also cover the SPA copy (WS19-S9 moved the pin without it).
- P2 test-gap: action-registry.test.ts still owes S5's +1.

### WS18-S10 (ecf51b0d5, 5c88b2776, 2c29a83c4)
- Proof script (not run), rollout runbook, evidence template, compose lines MODEL_GATEWAY_SPACE_SELF_API:-0 and SPACE_AGENT_SELF_API_API:-false.
- P1 OWNER: MODEL_ACTION_ADMISSION=table has no env line in apps/Model Plane/deploy (comments only) -> first rollout switch cannot be delivered from checked-in compose.
- P1: tokens phase needs an operator-captured decision token; live shapes unverified; BFF sweep for client-named space_self_api is a pre-enable gate.
- P2: production overlay should pin SPACE_AGENT_SELF_API_API off in next compose train; proof script mode 100644 (run with bash).

### WS20-S4 (0a5a56ddd)
- R2-R4 reads: session-core SpaceBrowserRead +3 RPCs (verified model.thread.read decision, removed members refused, closed allowlist projector); model-gateway GET routes; gateway browser_runs.rs; golden space-browser-events.v1.json.
- SD-23 VIOLATION -> fix assigned to WS20-S5: R4 served observation screenshots to every member as card thumbnail. Lead ruling: starter only (SD-23 already decided).
- P1 OWNER (carried from S3): CountActiveRoomBrowserRuns/ResolveRunSpace rest on credential+org only.
- P1 risk: Control decision rides upstream query string (ingress logs unverified).
- P1 UNVERIFIED: events.org_id on browser rows; RUN_COMPLETED/FAILED rows carry run_id+org_id; runs.space_id == threads.space_id.
- P1: quarry.rs touched (one additive artifact_bytes method). R1 left to WS20-S5.
- P2: view_scope not persisted (S6); hand-off event names unverified (S7); one Control call + up to 50-run host scan per poll.
| W3-FIX-15 | model-gateway+gw | WS18-S6 | built | 09473e3c0 |

### W3-FIX-9 (0e4d8107c)
- Private panel POST now creates a conversation-locked owner thread via new model-gateway POST /v1/chat/conversation-threads (ZDR first, narrowing only), records pointer in Convex; no workspace fallback.
- Browser capacity refusal -> revokes derived grant, 429 browser_capacity, no retry.
- Canvas restore key prefix canvas-restore- (cosmetic double prefix with SPA id).
- P1 gap -> W3-FIX-14: SPA first send uses default composer scope -> refused by session-core (fail closed).
- P1 gap -> W3-FIX-11: registry row for the new POST route (ZDR lint red).
- P2: deleted panel thread leaves a dead pointer (POST never re-creates).

### W3-FIX-13 (e4806bcb0)
- "Share to a room" in MemorySection, room picker, people count, writes via executeAction; fixed Space-denial branch shadowing memory refusal copy.
- P1 gap -> W3-FIX-14: gateway features report lacks memory_declassify, so the control is hidden everywhere.
- P1 gap: take-back only while the dialog is open (needs a session-core list of a person's own room copies).
- P2: viewer-only rooms not hidden (server refuses).
| W3-FIX-10 | convex+ctl config | FIX-5 | built | 678780846 |

### W3-FIX-12 (c41b40e31)
- Control run-action + v2 view now apply canvas checks (entitlement, kind, role, ZDR, archived write, since_join floor) to space.canvas.read/write only.
- P1 OWNER: space_history_floor_unsupported is outside Control's closed denial vocabulary (gateway/SPA mirror it) -- add to the golden?
- P1 deploy: ResolveSharedThreadDecisionEvidence now selects canvas_entitled -> migration 046 must deploy before this Control build or shared thread decisions fail.
- P2: viewer's agent cannot read canvas (editor floor runs first for every agent row); archived-room agent read gets generic 403; floor only when SPACE_HISTORY_WINDOW_CLAIM on.
- P2 pre-existing: human save conflict returns head metadata to since_join members.
- P2 test-gap: TestIssueModelActionViewIntersectsEntitlement expects tickets.create in v2 with no allowlist.

### W3-FIX-15 (09473e3c0)
- model-gateway ThreadSummaryResponse forwards title_source/pinned_source (closed owner|agent map); BFF relays unchanged; SPA types already match.
- P1 gap -> W3-FIX-16: windowed timeline (features.timeline_windows) projections omit the fields.
- P3 pre-existing: windowed summary_json hard-codes starred/unread false.
| W3-FIX-17 | wiki-store+convex | FIX-10 | built | ed2eb5b55 |

### W3-FIX-10 (678780846)
- Convex mints a per-org token from auth-core (/api/data-plane/internal-token, x-service-id control-space-deletion) per delivery, both legs; static bearer removed; mint failure releases for retry. Space-org check before minting (beyond brief, kept).
- principal adds data:space-binding:delete.
- Deploy: CONTROL_SPACE_DELETION_SERVICE_API_KEY (launcher mint, >=16 chars) + AUTH_CORE_URL on Convex; auth-core needs ORG_CORE_URL + ORG_CORE_SERVICE_TOKEN and durable audit; recreate auth-service after the row lands; until then deliveries retry (fail closed).
- P1 -> W3-FIX-17: canvas receipt names no org.
- P1 test-gap: plane-service-principals-policy.contract.spec.ts pins the row to one scope.
- P2: Space whose org org-core no longer lists retries hourly forever (no dead-letter); Model/Ingestion legs may share the static-bearer defect (UNVERIFIED).
| W3-FIX-16 | sc timeline+mg+exec | WS18-S7 | built | bfd9dbf29 |

### WS18-S7 (47a910cb3)
- post_card: session-core card write (plain text, audience-stamped, card_run_id, typed metadata.entry, ids-only event, no memory extraction), reply slot rule, MessageEntry.agent_card = 4, execution-core offer (phase 1, dark), SPA AgentCardEntry.
- P1 OWNER DECISION: capability-core admission row for post_card is `declared`; offering it needs a capability-core migration (0047) flipping it to enabled. Left dark.
- P1 -> W3-FIX-16: model-gateway ThreadMessageEntry drops card title/tone/fields; ACTION_TOOL_NAMES omits post_card.
- P1 deviation: assistant rows ARE extracted into memory (design says user only); cards excluded on both paths.
- P1: proto field added; buf generate owed; posts.rs literal fix outside lock.
- P2: WS18-S6 toggle copy says rename+pin only; card writes emit no MESSAGE_APPENDED (live via polling); LinkPreviewCards unregistered.
| W3-FIX-14 | spa + 1 gw line | WS20-S5 | partial | b528cef90 |

### WS20-S5 (0abde829c)
- R1 start route (flag, ZDR 412 first, Control browser-run + thread-append decisions, parent grant, relay); SD-23 fix: evidence image starter-only (403 browser_evidence_withheld otherwise); action spaces.browser_run.start; SPA start panel/card/pane/evidence; flags browser_run + browser_card (SPACE_BROWSER_CARD off in prod); i18n.
- P1 OWNER: new-thread start refused (400 thread_required) because Control's thread-append decision needs an existing thread (departs from design D2): create-then-start, or Control change?
- P1 OWNER: parent grant is issue_session_grant(HumanBrowse) (no hosts), not revoked on success (lives to broker TTL); 3 helpers in gateway browser.rs made pub(crate).
- P1 UNVERIFIED: session-core owner subject id == gateway user.user_id (else starter screenshots withheld; fails closed).
- P2: errors.ts PRE-EXISTING duplicate keys (TS error) -> W3-FIX-14; share_view refused; card in thread footer not timeline row; requiresApproval:true on the action (UI behaviour unverified).

### W3-FIX-17 (ed2eb5b55)
- Canvas purge receipt carries org_id; Convex checks org_id + space_ref on canvas leg and space_ref on retrieval leg; mismatch releases.
- P1 deploy order: wiki-store (and current retrieval-engine) before Convex, or deletions stall (fail closed).
- P2 (carried): 30 s claim lease in spaces.ts now covers the token mint round trip -> slow auth-core lets a second worker re-claim. Follow-up in spaces.ts.
- P3: padded space_ref stalls (exact compare).

### W3-FIX-16 (bfd9dbf29)
- Windowed timeline carries title_source/pinned_source; model-gateway forwards agent_card (closed tone map); ACTION_TOOL_NAMES has post_card (inert until capability-core admits it).
- P2: owner|agent map duplicated (provenance_token private); core.ts:155 stale comment; prost shapes for agent_card unverified.
- P3 pre-existing: windowed summary_json hard-codes starred/unread false.

### W3-FIX-14 (b528cef90) PARTIAL
- Chat controller forces conversation scope on known private-panel threads (ids remembered per tab in sessionStorage); gateway features report memory_declassify; neutral space_decision_unavailable copy; dead private_panel_creation_unavailable removed.
- P1 gap (follow-up): new tab/device/cleared storage -> first send on an empty private thread uses workspace and is refused (fail closed). Durable fix: thread-level source_scope on the chat read (gateway + model-gateway).
- P2: no TS duplicate-key error actually exists (WS20-S5 premise wrong; CANVAS_SHARED_COPY is a separate table). Chat source picker stays unlocked on empty private thread.
| W3-FIX-11 | registries | all | built | 73c20a042 |

### WS17-S8 (16132335c)
- Control memory-personal-context decision + route; session-core owner-only personal answer when grant present; model-gateway seam + 1 sse.rs line; gateway injector (flag SPACE_PERSONAL_MEMORY_TURN_API off in prod, ZDR 412 first); SPA composer checkbox (resets each send).
- P1 OWNER: reuses memory_declassify_entitled bit (plan names none) -- confirm or own bit.
- P1 deviation: grant rides gRPC metadata, not SearchMemoryRequest fields 8/9 (would break 5 literals).
- P1 hotspot approval: sse.rs 1 line + test literal; session_flow.rs field + 2 fallback literals (row-16 hotspot).
- P2: recall_memory tool, compaction recall, get_context_assembly get no grant (fail closed); new-thread turn gets no personal memory; no consumed-decision record (one-per-turn relies on random idempotency + 2 min life); filter_forgotten not applied to personal rows (UNVERIFIED).
| W3-FIX-18 | gw personal_agents.rs | FIX-11 | built | e37f48567 |

### W3-FIX-11 (73c20a042)
- 26 rows added to space-surface.v1.json (245 -> 271), fixtures byte-identical, pins 384b5dd0...; dataplane.rs handler names fixed.
- P1 -> W3-FIX-18: POST /api/v1/me/agents persists free text with no ZDR guard (registered refuse).
- P1 test-pin: scripts/tests/zdr-contract-sync.test.sh:235 still pins 2c5921b0... -> contract-goldens, space-gate, model-plane-rust-ci, browser-grant-contract fail until updated to 384b5dd0...
- P2: Control internal decision routes and wiki-store canvas routes not registered (registry has no control/data plane) -- owner decides; 13 pre-existing stale handler paths listed in depth.

### W3-FIX-18 (e37f48567)
- POST /api/v1/me/agents refuses ZDR (412) before any Convex call; other personal_agents writes are ids-only.
- P1 PRE-EXISTING (open): POST /api/v1/spaces/{ref}/agents (mod.rs create_space_agent) stores systemPrompt with no ZDR guard, registry row Config/n/a -> reclassify DurableWrite/refuse + guard.
- P1 PRE-EXISTING (open): PUT /api/v1/agents/{id}/draft (agent_actions.rs save_agent_draft) stores systemPrompt, no ZDR check, no registry row.

## Decision phase
| id | decisions | status | commit |
|---|---|---|---|
| W3-DEC-CTL1 | D7, D12, D10 (Control) | built | 6b873a411 |
| W3-DEC-CARD | D3 | built | e3ff62e22 |
| W3-DEC-ZDR | D14 | built | 840526f3c |
| W3-DEC-ZDR2 | D14 follow-up (publish + agent_actions survey) | built | 5fe670ac7 |
| W3-DEC-GRANT | D5 | built | 5e78930c8 |
| W3-DEC-NOTES | D9 | built | 53704acfc |
| W3-DEC-BROWSER | D4 + run-key fix + revoke bearer | built (revoke bearer blocked: owner) | 4ce271bc8 |
| W3-DEC-RETRACT | D1 + D9 follow-up | partial (D1 built, D9 blocked: owner) | cd61d2bd9 |
| W3-DEC-CANVAS | D2 + copy stragglers | built | 376af8b81 |
| W3-DEC-GWFIX | live ZDR posture + 403 relay | built | c78c029e8 |
| W3-DEC-GWFIX2 | browser start live ZDR + 422 relay + copy | built | 734cce02d + copy commit |
| queued | D15 registry extension, D13 runbook note, then compile pass | | |
| design first | D8 room history | awaiting owner confirmation | |

### W3-DEC-ZDR (840526f3c)
- create_space_agent and save_agent_draft refuse ZDR; registry rows reclassified/added; pins 67c6a872...
- P1 -> W3-DEC-ZDR2: publish route copies system prompt, unguarded.
- P1 test-pin: zdr-contract-sync.test.sh:235 must read 67c6a872... (will change again after ZDR2).
- P1 test-gap: create_space_agent_response tests lack org-core posture stub (503).
- P2: durable_write_refusal uses cached org posture (up to 60 s stale); org_posture_live exists -- all W3 guards inherit this.
- P2: no lint checks that DurableWrite handlers call a guard.

### W3-DEC-CARD (e3ff62e22)
- capability-core 0047 flips post_card admission to enabled (guarded UPDATE; to switch off set `retired`, not `declared`); toggle copy says rename, pin and post cards; attester already covers post_card.
- Ops: cards still need MODEL_ACTION_ADMISSION=table (core .env), EXECUTION_CORE_SPACE_SELF_API=1, SPACE_SELF_API_SESSION_CORE=1, and CONTROL_SPACE_SELF_API_ACTIONS naming space.thread.post_card with rename/pin.
- Copy stragglers -> fold into D2 SPA work: ws18.ts:29 "and later post cards"; SpacePage.tsx:1806; core.ts:1127.

### W3-DEC-CTL1 (6b873a411)
- D7 browser out of agent_computer preset; D12 migration 049 memory_personal_turn_entitled + own resolver; D10 org agents registered ownerless, sweeper only owner-visible.
- P1 deviation: migration 049 (048 reserved for WS20-S9). Deploy 049 before Control image.
- P1 decision (lead: accept; safe): org-visible registration refuses non-members 403; gateway maps registration 403 -> 503 (gateway follow-up to relay typed).
- P1 test pins: explicitOnlyEntitlements, zdrProbes (~160), TestRegisterIsOwnerDerivedAndFirstWriterWins.
- P2 gap: ownerless org agents have no org-admin edit/suspend/retire route; owner->org flip keeps owner.
- P2: full-state entitlement writes must include memory_personal_turn.

### W3-DEC-GRANT (5e78930c8)
- Parent grant narrowed to the decision's hosts, 600 s TTL (model-gateway grant route gains ttl_s, session kind only); revoked at complete/cancel/refusals; gateway already revoked on failed start.
- P1 -> W3-DEC-BROWSER: run-end revoke uses the ingress bearer (~300 s) -> refused on long runs (grants still die at TTL <= 600 s).
- P1 PRE-EXISTING -> W3-DEC-BROWSER: keyed_run_key includes the parent grant id; a retry with the same client_send_id starts a second live run.
- P2: org site policy refusal at issuance reaches SPA as 502 browser_grant_unavailable (untyped); dispatch failures that never terminalize skip the revoke (TTL bound).

### W3-DEC-NOTES (53704acfc)
- RestoreMemory (session-core, owner-only, ZDR first, room-checked under row+thread locks); model-gateway + gateway routes; action memory.restore; SPA Restore control.
- P1 -> W3-DEC-RETRACT: threads with no recorded space_kind refused (thread_kind_unknown) -- the exact D9 case; resolve via Control Space kind.
- P1 process: builder ran `git reset -q --` (mixed); lead verified index clean and all commits intact.
- P1: memory.proto rpc added -> buf generate. P2 test: action-registry.test.ts:1828 count 382->383 (plus other W3 additions).
- P2 UNVERIFIED: Letta copy of restored note not handled; lock order row->threads could deadlock vs thread-first writers.

### W3-DEC-ZDR2 (5fe670ac7)
- publish + 14 more agent_actions content routes refuse ZDR (skills, plugins, cron, event triggers); 10 left (deletes, ids, config, dry run); registry 286 rows, pins 6fb0226a...
- P1 test-gap: in-file skill/import/cron/publish tests now 503 (no org-core posture stub).
- P1 test-pin: zdr-contract-sync.test.sh:235 (digest will move again with CANVAS).
- P2 owner: registry marks skill/plugin/cron DELETE allowed under ZDR but proxy_to_capability_core refuses every non-GET for ZDR; cron PATCH row says refuse though it is config.
- P2: event_trigger_routes.rs not in the model-gateway lint file list.

### W3-DEC-RETRACT (cd61d2bd9) PARTIAL
- D1: Control publishes role_changed via the durable outbox; session-core consumer retracts a removed/demoted (<editor) member's shared copies (advisory lock, same statements as owner retract), retries + 10-min sweep; declassify re-checks revocation in-tx. member_left treated as removal.
- D9 BLOCKED (owner): session-core cannot read Control's Space kind (gateway-delegation-only route). Options: (a) Control-signed kind token relayed by gateway; (b) new authz read for session-core on owner assertion; (c) status quo: restorable after the owner's next verified append stamps the kind.
- P1 gap: a missed demotion older than the 24 h re-announce window is never retracted (sweep covers removals only); durable JetStream consumer provisioning owed.
- P2 PRE-EXISTING SECURITY BUG -> queue W3-DEC-ORDER: space_membership_nats.rs clears the revocation row on ANY active=true fact, so a stale active=true event after a removal can reopen access up to 10 min. Needs revision ordering.
- P2: shared identical text = one room row, removal deletes it for both sharers.

### W3-DEC-BROWSER (4ce271bc8)
- D4: Control signs thread_mode (new|existing); gateway creates the room thread (Control thread-decision + /v1/threads, goal as first post) before the browser decision; verifier + golden updated (old digests reproduced first).
- Run key fixed: room runs keyed by org+user+decision key; existing-thread goal append keyed by start key.
- BLOCKED (owner): run-end revoke needs a service principal with browser:write on the broker; none exists. Grants end at TTL (<=10 min). Options: grant model-gateway a broker principal with browser:write, or accept TTL.
- P1: new-thread run's goal is the thread's first post; Session Core managed start on such a thread UNVERIFIED; tie to the created thread relies on the trusted gateway.
- P2 deploy: Control + model-gateway before gateway (all browser digests changed).
- SPA start panel still sends the current thread id (new-thread entry owed).

### W3-DEC-GWFIX (c78c029e8)
- durable_write_refusal reads org posture live (org_posture_live); admit_for_bind relays Control 403 agent_access_forbidden.
- P1 -> GWFIX2: browser start zdr_gate still uses the 60 s cache.
- P2 cost: one extra uncached org-core GET per durable write on hot paths (chat, posts, files...); coalescing cache is the follow-up if load matters.

### W3-DEC-GWFIX2 (734cce02d)
- Browser start uses live ZDR posture; admit_for_bind relays 422 agent_policy_invalid.
- LEAD TODO: errors.ts agent_access_forbidden neutral copy is uncommitted in the worktree (mixed with CANVAS hunks); check after CANVAS commits.
- P2: agent_policy_invalid copy misleads on the bind route; room-create (mod.rs:4049) maps admit refusals to 502.
- P3: room reads (R2-R4) stay on 60 s cache after a ZDR switch-on (lead: accept; reads persist nothing new).

### W3-DEC-CANVAS (376af8b81)
- Own switch canvasToolsEnabled (Convex), gateway PUT .../canvas-tools (SPACE_AGENT_CANVAS_TOOLS_API off in prod), marker space_canvas_tools; tool_profile is a +-joined list (field 28); model-gateway honours marker behind MODEL_GATEWAY_SPACE_CANVAS_TOOLS; execution-core offered_under_profile default deny; SPA toggle; copy stragglers fixed; registry pins a281d2bc...
- P1 pin: zdr-contract-sync.test.sh:235 -> a281d2bc... (latest).
- P1: client-named space_canvas_tools stripped only where space_self_api is; sweep BFF chat paths before enabling.
- P1 interpretation (lead: accept): canvas switch PUT allowed under ZDR (writes a boolean); grant refused for ZDR.
- P2: compose lines for SPACE_AGENT_CANVAS_TOOLS_API / MODEL_GATEWAY_SPACE_CANVAS_TOOLS not added (per D13 cores own .env).
| W3-DEC-ORDER | stale membership fact fix | built | 9c2972423 |
| W3-DEC-REG | D15 registry + D13 runbook | built | 7878a4d59 |
| W3-DEC-WIKIZDR | wiki-store typed ZDR refusal | built | bd0308170 |
| W3-DEC-ORDER2 | removal retracts pre-removal copies even after quick re-add | built | d7a041f7a |

### W3-DEC-ORDER (9c2972423)
- Membership facts applied only if space_seq newer (session-core migration 0132 fact-order table, per-subject advisory lock); closes stale active=true reopen bug.
- P1 -> ORDER2 (lead ruling): quick re-add skipped the removal's retraction; retract copies shared before the removal regardless.
- P2: declassify revocation check not serialised with the membership consumer (argued safe, confirm in test phase).

### W3-DEC-REG (7878a4d59)
- 20 rows (14 ctl:, 6 data:) + `planes` key; registry 307 rows; pins 09b8c2f5... (zdr-contract-sync.test.sh:235 must read this).
- Runbook: MODEL_ACTION_ADMISSION per-core .env; canvas-tools and card switches added.
- P1 -> WIKIZDR: wiki-store canvas refusal zdr_persistence_forbidden not in the typed catalogue.
- Lints for ctl:/data: rows owed (test phase).

### W3-DEC-ORDER2 (d7a041f7a)
- Removal/demotion always retracts the subject's shares authorised below the fact's revision (migration 0133 adds memory_declassifications.authority_revision); retract_marks table closes the late-commit race (kept, lead accepts).
- P1 hotspot approval: 3 lines in grpc.rs + memory_grpc.rs.
- P2: shared room row kept while another member holds it (owner retract still deletes for all -- paths differ).
- P2: pre-0133 shares (NULL revision) retracted by any later removal (fails closed).

### W3-DEC-WIKIZDR (bd0308170)
- Canvas ZDR refusal now 412 zdr_durable_mutation_forbidden (details.surface room_canvas) on human and agent paths; gateway relays it; OpenAPI updated.
- P1 PRE-EXISTING: non-canvas wiki writes (wiki.go:105-107 via authctx.go:237) still answer non-catalogue 403 zdr_persistence_forbidden; SPA WikiProposalsReview.tsx:141 keys on it.
- P2: registry reason text for canvas rows still says 403 (re-pin needed); stale code in DATA_PLANE_CODES.

## Compile pass (started)
| Area | Result |
|---|---|
| Go: user-core, wiki-store-go, capability-core, letta-bridge | go build clean |
| Frontend gateway (Rust) | clean after b6edfad7e (recursion_limit 256; revoke_session_grant path) |
| Model Plane Rust (mp-contracts, session-core, model-gateway, execution-core) --lib --bins | clean |
| retrieval-engine-rs | clean |
| SPA tsc -b | clean except 33 environmental (quarry-client.ts 21, RemoteSupportPage.tsx 12: unbuilt workspace packages) after ebdc4f545 |
| Convex tsc --noEmit | clean after ebdc4f545 |
- Real bugs caught: core.ts entry shadowing crashed normalizeSpaceThreadTurns at runtime; 3 components used onMount which solid-js 2.0 rc lacks (never mounted).
- Not checked: test targets (--tests/--all-targets), clippy, lint.

## Round 2 decisions (D16-D20)
| id | decision | status | commit |
|---|---|---|---|
| W3-DEC-ZDRDEL | D20 | partial (not effective, see follow-up) | 1a717d6c7 |
| W3-DEC-ZDRDEL2 | D20 follow-up | running | |
| W3-DEC-ORGAGENT | D19 | running | |
| W3-DEC-REVOKE | D16 | running | |
| W3-DEC-KIND | D17 | running | |

### W3-DEC-ZDRDEL (1a717d6c7) PARTIAL
- Premise wrong: W3-DEC-ZDR2 never guarded gateway DELETE routes for skills/plugins/cron; only 2 registry rows (skill-draft delete, plugin-kill-list delete) needed changing to n/a.
- P1 NOT EFFECTIVE -> W3-DEC-ZDRDEL2 running: model-gateway proxy_to_capability_core refuses every non-GET under ZDR regardless, so these deletes still 412.
- P2: DURABLE_MUTATIONS comment in zdr_posture.rs already says deletes are allowed; code doesn't implement it for this proxy.
- Pin churns with every registry commit; digest for zdr-contract-sync.test.sh:235 not final until the last registry commit of the wave.

### W3-DEC-REVOKE (4e7276b5a) BUILT
- New principal model-gateway-browser-admin (browser-broker audience, browser:write); model-gateway mints its own org-scoped token, caches it, falls back to the ingress bearer when unset/refused.
- P1: NOT EFFECTIVE until the credential is deployed (compose env, principal-train acceptance) -- until then revoke behaves exactly as before (no regression).
- P1 test-gap: 3 policy spec pins in auth-core will fail until the train updates them.
- P2 OWNER NOTE: the scope is browser:write (as named in D16), so this token also authorises AcquireGrant/DeriveRunGrant for its org, not revoke-only. A narrower scope needs a broker change. Accepting as built unless told otherwise.
- P3: dispatch paths that never reach a terminal state still revoke nothing (unchanged gap).

### W3-DEC-KIND (707ff7c72) BUILT
- Control signs a 2-min claim that a Space is the caller's personal Space (new route space-kind-evidence); gateway refuses ZDR then relays it; model-gateway forwards it; session-core RestoreMemory verifies it and stamps the thread personal.
- P1 NOT WIRED -> W3-DEC-KIND2 running: SPA sends no space_ref, so the claim path is inert (fails closed, no regression).
- P1: Go/Python memory.proto bindings stale (buf generate owed).
- P2: claim replayable 2 min by same subject/Space (no nonce); route not in decision-audit table; a row linking to multiple unrecorded threads only covers one Space per claim.

### W3-DEC-ORGAGENT (e8008895d) BUILT
- Control: list/suspend/reactivate/retire ownerless org agents (settles admissions + seats in every room on suspend/retire); gateway routes + proxy; 3 actions; SPA admin section in settings; registry +8 rows (315 total), pins 7a691a2c...
- Scope: "edit" not built (would widen UpdateAgentAccess) -- LEAD DECISION: accept suspend/reactivate/retire as the delivered admin controls for this release; editing an org agent's instructions can reuse the existing UpdateAgentAccess path gated on org-admin in a later pass if wanted.
- P1 gap: agent-action-admission.json rows for the 3 new action ids not added (schemaHash needs the generator) -> admission-contract test fails until added.
- P1 PIN CHURN: registry touched by 6+ builders this round; final consolidated re-pin needed once all registry-touching builders are done (lead task, before test phase).

### W3-DEC-KIND2 (a43bd8231) BUILT — D17 complete end to end
- action-registry.ts memory.restore accepts optional spaceRef; memory-client.ts restoreMemoryInputFrom(entry); MemorySection.tsx sends it. List API already returns origin_space_ref (no gap there).
- D17 (Control-signed proof -> gateway relay -> session-core verify -> SPA wiring) is now fully wired end to end.
- P2 minor gap: room-level SpaceAgentMemoryPanel (withheld flag) has no origin_space_ref/restore control, separate from personal MemorySection. Logged, not built.
- P2: spaceRef sent even for shared-room rows (harmless, one extra Control call, still refused as before).

### W3-DEC-ZDRDEL2 (e72363eac) BUILT
- model-gateway proxy_to_capability_core now exempts the 6 removals from ZDR refusal; uninstall_plugin registry row -> n/a; pins re-synced (digest 05ab47b5...).
- P1 NOT YET LIVE -> W3-DEC-ZDRDEL3 running: capability-core (Go) authz.go still denies every non-GET for ZDR, and plugin_catalog.go deleteKill's pluginWriter independently refuses.
- P2 OWNER: waived tokens still get a route mismatch on these removals -- does D20 cover waived tokens too, or ZDR-only? (holding as ZDR-only pending answer)
- P2: org hook/permission-rule deletes correctly stay refused (they persist a revision/history row, not a pure removal).

### W3-DEC-ZDRDEL3 (f1719b9bd) BUILT, with a real P0
- capability-core authz.go + plugin_catalog.go exempt the 6 removals; pluginWriter split preserved for other callers.
- P0 FOUND: skill delete + plugin uninstall trigger a Postgres rule (0064_skill_revisions.sql) that snapshots the deleted skill's TEXT into skill_revisions, even for a ZDR org -- D20's premise ("deletion persists nothing") is false for these two.
- OWNER ANSWER: fix the snapshot, then allow all 6. W3-DEC-ZDRSKILL running: new session-core migration makes the trigger skip content under ZDR (session var set by capability-core at the DELETE call site; non-ZDR unchanged).
- P1 carried: session-core migration 0108 says ZDR credentials cannot write agent_skills at all -- D20 reverses that for deletes; both claims should be settled together (handled by ZDRSKILL).

### W3-DEC-ZDRSKILL (bab5947bd) BUILT — D20 now end-to-end for its 6 named routes
- Migration 0134 replaces agent_skills_revision_fn: ZDR posture (set by capability-core at the DELETE/UPDATE call site) -> metadata-only revision row, content_free=true; unset or non-ZDR -> unchanged snapshot (fail-safe: unset defaults to KEEPING the snapshot, never to dropping it).
- Also caught: draft discard is an UPDATE that already wrote a full-text revision on every discard -- now content-free under ZDR too (ZDRDEL3 had wrongly assumed this path was unread/unaffected).
- NEW P0, PRE-EXISTING, OUT OF WAVE SCOPE: a global plugin kill (UninstallKilled, platform-principal posture) still snapshots a ZDR org's skill text during a platform-wide kill, because it runs under the platform's posture, not each affected org's. This predates Wave 3 and D20; it is not one of the 6 routes D20 covers. NOT FIXED. Flagged to the owner as a separate follow-up (needs per-affected-org posture lookup during a global kill, or a fail-closed rule that changes non-ZDR global-kill behaviour).
- P1 follow-up: session-core's revision restore/undo reader should get a typed refusal for a content_free row instead of reusing the `truncated` marker shape.
- P2 pre-existing, unfixed: a content-only skill edit over 64 KiB records no revision at all under either posture.
