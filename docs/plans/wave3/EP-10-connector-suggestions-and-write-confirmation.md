# EP-10 (Wave 3 part) - Connector suggestion cards and editable write confirmation

Abbreviations: `MP` = `apps/Model Plane`; `MPR` = `MP/rust`; `MG` = `MPR/services/model-gateway/src`; `EC` = `MPR/services/execution-core/src`; `SC` = `MPR/services/session-core`; `MPC` = `MPR/crates/mp-contracts`; `CC` = `MP/go/services/capability-core`; `IC` = `apps/Ingestion Plane/integration-corev2/internal`; `FE` = `apps/Frontend Plane/verevonv3`; `FES` = `FE/src`; `BFF` = `FE/apps/gateway/src`; `PROTO` = `MP/proto/model_plane/v1`. Every `path:line` below was opened in worktree `epic-shaw-4d613b` at HEAD `c18196af2` (branch `model-plane`, Waves 1 and 2 built). `[INF]` = inference, not read from code. `[verify live]` = only a running stack can prove it. Queue status = `.plan-work/execution/queue.json`; register rows = `docs/plans/model-plane-depth-register.md`.

Plan inputs: `docs/plans/model-plane-chat-build-plan-2026-10-14.md` sections 1.4, 2.1 (DEC-08, DEC-21, DEC-26, DEC-27), 3.3.0, 3.3.2 (EP-10), 3.3.3, 3.3.5, 4.3, 4.4, Appendix B rows K1-11, K3-14, K3-15, K3-16, and the WS23 section (S8 "F-t3-056 write confirmation cards build on S8"). Audit card: `C:/dev/verevon-audit-2026-10-14/03-model-plane-chat-and-agents-audit.md:1088-1098` (F-t3-056). The audit is older than the tree; where they differ, code wins and the claim is marked stale in section 2.

## 1. Scope, items resolved, owner

**Item:** F-t3-056 (recommended, Partial, audit effort L, Wave 3): in-chat connector suggestion cards with a connect step that resumes the turn, and an editable write confirmation with user-visible policy levels. Audit dependencies: F-t3-010 (WS23), F-t3-003 (WS05).

**Owner:**
- Model Plane owns the approval preview, the edit (a new frozen descriptor and fingerprint), the policy lookup in both loops, the suggestion tool and its event. session-core is the durable owner of approvals and descriptors; execution-core runs the approved call; model-gateway routes and emits.
- Ingestion Plane (integration-corev2) is unchanged at runtime. It keeps token custody, the provider call, its approval allowlist and the attested-write check (K1-11). This epic adds only a contract test there (S2).
- Frontend Plane owns the cards, the connect flow reuse, the policy panel, the BFF pass-through and the action contracts (contracts before UI, per `CLAUDE.md`).
- Control Plane: no change. Data Plane: no change.

| Slice | Tag | In | Effort | Startable now |
|---|---|---|---|---|
| S1 | [BUILD] | Full-draft preview on the approval card for mail sends (read-only) | M | yes |
| S2 | [FINISH] | One write-operations golden: exec-core catalogue = integration-corev2 approval allowlist = provider catalogue direction | S | yes |
| S3 | [BUILD] | Pure edit contract in mp-contracts: editable fields, limits, shared descriptor fingerprint and effect summary | S | yes |
| S4 | [BUILD] | Approve-with-edits in session-core, gateway and BFF (new descriptor, new fingerprint, same approval), dark | M | after S3 |
| S5 | [BUILD] | Executor accepts an edited approval on loop resume and cold delivery, and tells the model what changed | M | after S4 |
| S6 | [BUILD] | Editable card in the SPA | M | after S1, S4 |
| S7 | [BUILD] | User-visible policy levels for connected apps on the existing `tool_policies` store | M | yes (UI dark until WS05 enforce is live) |
| S8 | [BUILD] | `suggest_connector` tool in the Ask loop, `connector_suggestion` event and card | M | yes |
| S9 | [BUILD] | Resumable connect intent (SPA only) | S | after S8 |
| S10 | [FINISH] | Live proof, flag flips, ledger and docs | M + owner wait | no (blocked external) |

**Explicitly OUT:**
- F-t3-057 custom connectors (probe-and-test, secret-reference MCP auth, REST/API-key kind behind an egress proxy). It is Wave 4 in the audit (`audit:1100-1110`, W4 table `audit:1619`) and needs DEC-21 custody (ADR-0008) and EP-9 egress. Not designed here.
- Calendar event cards. integration-corev2 has no calendar write operation: the frozen contract lists `calendar.events` as a read only (`docs/actions-surface-operations.md:114, 136`). A calendar create is WS23-S9 (queue `optional-deferred`, "needs an amendment to the frozen docs/actions-surface-operations.md and incremental OAuth consent"). See D3. The preview and edit contracts are shaped so a calendar kind can be added later.
- New provider write operations of any kind (WS23-S9).
- Standing grants for provider writes. Ledger rule 4 (`MP/docs/decisions/ledger.md:100`) keeps provider writes ONCE only; this epic does not change that.
- Step-up re-verification (WS13-S5, reused unchanged).
- The `@` trigger for connectors (DEC-26: attach-menu chips first; SPACE owns `@`).
- MCP elicitation and inline widgets (EP-4).
- The shared inbox visibility fix (OD11, Ingestion/Conversation Core).
- Suggestions in the Do loop (execution-core). Recorded as a later option in D6; not in W3.
- Space threads. Connector tools and chips are refused when `space_id` is set (DEC-06); suggestion cards and edits follow that.

## 2. Verified current state

**Write pause and the frozen descriptor (code wins over the audit).**
- `pause_for_approval` (`EC/runtime_loop/agent.rs:7680-7836`) builds the descriptor first, adds the offered-tool snapshot for loop-owned pauses (7716-7730), refuses a snapshot over `MAX_SNAPSHOT_BYTES` (7725; constant 64 KiB at `EC/runtime_loop/resume.rs:265`), saves the fold's fingerprint before the approval exists (7788-7792), then calls `CreateApproval` with `continuation_mode` `"loop"` or `""`, `idempotency_key = {run}:{step}`, `expires_in_seconds: 3600`, the cause and the descriptor (7745-7782).
- `continuation_descriptor` (`agent.rs:7845-7952`) refuses ZDR (`approval_continuation`, 7853-7858), forces `permission_mode = "ask"` for owner actions (7863-7869), and computes `action_fingerprint = blake3(serde_json::to_string(scope))` where `scope = {version, run_id, org_id, user_id, step_id, action_kind, tool_name, input, permission_mode}` (7896-7908). The input is stored whole.
- session-core validates the descriptor shape only: version 1, scope ids equal the request, `action_kind = tool_call`, a 64-hex fingerprint, no secret keys (`SC/src/orchestration_grpc.rs:1040-1101`, forbidden keys 1103-1121). It does not recompute the fingerprint. Size cap `MAX_CONTINUATION_DESCRIPTOR_BYTES = 64 KiB` (`orchestration_grpc.rs:936`).
- The descriptor is encrypted into `approval_continuation_descriptors` (migration `SC/migrations/0022_encrypted_approval_continuation_descriptors.sql:4-23`: `approval_id` PK, `action_fingerprint` 64 chars, `ciphertext` up to 131072, no revision column, no immutability trigger). Insert at `SC/src/orchestration_store.rs:935-975`.
- On a grant, a delivery outbox row is written only for cold approvals; loop-owned approvals are consumed by `ConsumeApproval` (`orchestration_store.rs:1070` `decide_approval_recorded`, loop check and insert ~1121-1145).
- Loop resume: `ConsumeApprovalResponse` returns the approved call from the descriptor, fields 1-7 (`PROTO/orchestration.proto:820-840`). The restore requires the fold's pending fingerprint to match the resume fingerprint and the fold's call-argument digest to equal the consumed descriptor's digest; any mismatch is a fallback (`EC/runtime_loop/resume.rs:1222-1256`). The released call uses the descriptor's arguments, never the fold's (`resume.rs:1430-1438`). **So an edited descriptor today makes in-place restore fall back.** S5 changes that rule narrowly.
- Cold delivery re-parses the descriptor's `input` into `ProviderActionInput {connection_id, operation, params, body}` (`EC/approval_delivery_worker.rs:369-380`). An edited input needs no change there beyond a test.
- The write attestation binds `approval_id` and a `payload_sha256` computed from the payload actually sent (`EC/attestation.rs:234`, approval binding 303-305). An edited payload is attested as sent; no attestation change is needed [INF from the code shape; verify in S5 tests].

**The approval card today.**
- `ApprovalRequests` (`FES/features/chat/components/ChatMessages.tsx:2214-2330+`) shows a badge, a headline, the reason text, Approve, "Approve for this thread/run" when the cause offers a standing scope, Reject, and the WS13-S5 step-up password prompt. No payload, no edit. Reason text comes from `approvalReason` (`FES/shared/approvals/approval-copy.ts:203`).
- For a mail send the reason is `provider_effect_summary`: recipients (10 shown), subject (200 chars), the first 120 characters of `bodyText` (`EC/integration_tools.rs:1217-1277`, mail ops only, `is_mail_send` 1280-1285). K3-16 keeps that text only in `approvals.reason`, scrubbed, erased with the run.
- Gateway decide: `DecideApprovalBody {decision, reason, scope, promote}` (`MG/http_routes.rs:1954-1967`), handler `decide_approval` (`http_routes.rs:2193`) runs `continuation_gate` first (2202), reads the approval for scope checks (2215-2236). Routes at 428 (list), 444 (get), 462 (decide).
- BFF forwards a closed body (`decide_upstream_body`, `BFF/domains/orchestration.rs:~507-540`, fields `decision`, `reason`, `scope`, `promote`); routes 89-101. Anything else is refused, so S4 must add `edit` there.
- Proto: `DecideApprovalRequest` uses 1-8 (`PROTO/orchestration.proto:937-967`, next free **9**); `Approval` uses 1-16 (360-418, next free **17**); `ApprovalCause` 1-10 (451-487); `ApprovalStateChanged` 1-10 (`orchestration.proto:1305+`, next free **11**); `ConsumeApprovalResponse` 1-7 (next free **8**). `ApprovalKind` stops at 5 (214-227).

**Write classification is in three places, not two.**
- execution-core `CATALOG` with `is_write` per operation (`EC/integration_tools.rs:63-67, 74-130`; `gmail.send` 104, `mail.send` 116, plus Meta, LinkedIn, Slack writes) and `operation_is_write` (1186-1201; unknown returns `None`, treated as a write).
- integration-corev2 approval allowlist `actionRequiresApproval` (`IC/api/server.go:3751-3771+`): Microsoft `mail.send`, Slack `message.send`, Google `gmail.send`, GitHub issue writes, Notion, Shopify... An operation it does not name is not gated there (register D2-718, open P1).
- integration-corev2 provider catalogue `Direction` (`IC/providers/catalog.go:14-17`).
- No test ties the three together for writes. WS23-S2 did it for reads only (queue `partial`).

**Mail send shape.** integration-corev2 accepts a normalised body: `subject` (<= 998, header-safe), `bodyText` or `bodyHtml` (together <= 1 MiB), `to` (1-50 RFC 5322 addresses), `threadId`, `inReplyTo`, `references`, `correlationId` (`IC/actions/service.go:1858, 1945-1988, 1990-2002`). Gmail's native `{raw}` body is kept as an escape hatch (`gmailSendPayload`, ~1922-1943). Arms at `service.go:229` (`mail.send`) and 292 (`gmail.send`). No `cc`/`bcc` in the normalised shape. The frozen doc still describes the verbatim provider shapes (`docs/actions-surface-operations.md:117, 135`); the normalised path is additive in code.

**Policy levels: the store exists, the connector UI does not.**
- capability-core `tool_policies` (`CC/migrations/0017_tool_policies.up.sql`): `scope_kind` org or user, `decision` allow/ask/never, `pinned_digest` required for allow; `expires_at` only for allow (`0030_tool_policy_expiry.up.sql`). Latest capability-core migration is 0030.
- API `/api/v1/tool-policies` (`CC/internal/api/tool_policies.go:49`); active lookup reads org and the user's own rows (110-118); relaxing to allow is MCP-only (415-418: "a built-in tool's approval is never relaxed"). Tightening (ask, never) on a built-in capability is not refused by that check [verify in S7: confirm `set` accepts ask/never for `cap.tool.provider.*`].
- Engine is behind `TOOL_POLICY_ENFORCED` (WS05-S5, queue `partial`, default off, "with it off the engine never loads a policy").
- Capability ids: `read_provider_action` and `list_provider_actions` map to `cap.tool.provider.read`, `execute_provider_action` to `cap.tool.provider.execute` (`EC/capability_policy.rs:1617-1618`; seeds `CC/migrations/0016_capability_tool_classes.up.sql:157-158`, `0024_provider_read_tool.up.sql`).
- BFF already proxies `/api/v1/tool-policies` through model-gateway (`BFF/domains/mcp.rs:69-71`, upstream 379, 404); the SPA has a setter (`FES/shared/api/orchestration-client.ts:236`). The list response has no "enforced" bit (grep of `tool_policies.go` finds none), so a UI cannot tell the user whether a level is honoured.

**Suggestions and connect.** No `suggest_connector`, `connector_suggestion` or connect-intent code exists in Model Plane or the SPA (grep, none). The connect flow exists and stays in the page: `startConnectSession` (`FES/shared/api/integrations-client.ts:135-148`), `getConnectSessionStatus` (150-158), `ConnectSession` with `connectUrl`/`sessionToken` direct-OAuth fields (87-97), popup runner `runDirectOauthWindow` (`FES/shared/integrations/provider-auth-window.ts:104`), used by `InboxPage.tsx:307-311` and `CoreSidebarSupportPanel.tsx:105-109`. BFF proxies to integration-corev2 (`BFF/domains/integrations/providers.rs:23-46`). Action contract `integrations.start_connect_session` exists (`FES/shared/actions/action-registry.ts:3212`), as do `chat.set_connector_scope` (3620), `approvals.decide` (3531), `orchestration.decide_approval` (3664).
- The Ask loop already has a connector selection path: `MG/connector_reads.rs` (832 lines; `INTEGRATION_COREV2_URL` 23; `offered(enabled, zdr, space_scoped, turn)` 77), `CONNECTOR_CLASS_TOOLS` and `tools_for_connector_scope` (`MG/tool_loop.rs:5309, 5339`), `builtin_tool_defs` (5360). SSE has `follow_ups` and `user_question` as precedents for a typed card event (`MG/sse_events.rs` event names).

**Prerequisite slices and their queue status.**

| Slice | Status | Open rows that matter | Effect on this epic |
|---|---|---|---|
| WS23-S8.code (attested writes) | partial | D2-717 (rotate attestation key, P1), D2-718 (IC allowlist not pinned, P1), D2-719 (writes with empty `connector_scope`, owner decision), D2-720 (card copy and descriptor erasure, owner decision), D2-721 (K1-11 owner acceptance), D2-722 (live proof) | S1-S9 build dark; S10 live writes wait for D2-717, D2-721, D2-722. S2 closes D2-718. |
| WS23-S8 (external half) | blocked-external | live attested send on DEC-27 accounts | S10 depends on it |
| WS23-S1.code | partial | D2-172 (connector results still `OrgInternal` until the trust rows land), D2-178 | Not blocking edits; blocks S8's "use connected app" path going live (it leads to reads) |
| WS23-S4.code | partial | D2-539 (scope not persisted to `runs.metadata`), D2-540 (a resumed paused Do run carries no selection) | S5 must not depend on the selection travelling with the pause; S9 re-sends the selection explicitly |
| WS23-S5.code / S6.code | partial | D2-602 (Do loop has its own read copies), D2-604 (Ask loop no taint after flagged read), D2-636 (chips need a capability advert) | S8 uses the S6 chip state; S8's card must not appear where chips cannot |
| WS23-S7.code | done (code); S7 blocked-external | D2-668..674 (shaping fixes, live reads) | S10 only |
| WS13-S1, S2, S4 | done | WS13-S2 "live card check pending" | S1/S6 extend the same card |
| WS13-S3 | partial | approval audit events behind `APPROVAL_AUDIT_EVENTS_ENABLED` | S4 adds two fields to `ApprovalStateChanged`; ships dark with it |
| WS13-S5 | partial | step-up behind `APPROVAL_STEP_UP_ENFORCED` | S6 reuses the prompt; no change |
| WS13-S6 | partial | standing scopes | Edits are ONCE only (D1) |
| WS03-S2, S3, S6, S7 | done | WS03-S9 partial (live runner not run) | S5 changes `resume.rs`; its contract tests must stay green |
| WS05-S5 | partial | `TOOL_POLICY_ENFORCED` off | S7 builds now; its UI says "not enforced" until the switch is live |
| WS05-S1, S3 | done | | class rows exist for both provider tools |

**Stale audit claims (6).**
1. "The card offers only approve or reject, `ChatMessages.tsx:1890-1925`": the card is now at 2214+ and also offers thread/run scopes (for eligible causes, never provider writes) and a step-up prompt. Still no payload and no edit.
2. "Frozen, fingerprint-bound descriptor, `runtime_loop/agent.rs:4594-4650`": now `agent.rs:7680-7952`.
3. "Read/write classification already exists twice (`integration_tools.rs:60-71,747-769`; Capability.Direction)": it exists three times (catalogue, `actionRequiresApproval`, `Direction`) and the catalogue moved to 74-130 and 1186.
4. "Do not add an integration-corev2 client to model-gateway": overtaken. WS23-S5 added `MG/connector_reads.rs` with `INTEGRATION_COREV2_URL`. This epic still adds no new integration-corev2 call from the gateway: suggestions use the SPA's existing connect routes.
5. "User-visible policy levels are missing": the per-user allow/ask/never store and its API exist (capability-core 0017, 0030; BFF `mcp.rs:69`). What is missing is the connector-facing UI, an "enforced" signal, and the Ask loop honouring user tightenings.
6. "Editable preview for mail drafts and calendar events": there is no calendar write to confirm (frozen doc 114, 136). Calendar is out until WS23-S9.

## 3. Decisions and open questions

| # | Decision | Options | Recommendation | Gates | Proceed without? |
|---|---|---|---|---|---|
| D1 | How an edit becomes authority | (a) revise the same approval in place: new descriptor input, new fingerprint, `revision` +1, granted in the same transaction; (b) supersede: deny the old approval and create a new one with the edited input; (c) reject and ask the model to redraft | **(a).** The person is the authority and the edit is part of their grant, so grant and edit must be one atomic act. (b) creates an approval no gate raised, breaks the `{run}:{step}` idempotency key and the fold's pause reference (`resume.rs:1226`). (c) costs a model round and can drift again. Rules: only `execute_provider_action` with an editable operation; scope ONCE only; `rule_proposed` refused; the client sends the fingerprint it saw (`base_fingerprint`) and a mismatch is 409 `approval_changed`; at most 3 revisions; original fingerprint kept (digest only, no original content). New ledger entry "An edited approval is a revised descriptor, not a new approval" | S4, S5, S6 | Yes: S1, S2, S3, S7-S9 |
| D2 | Editable fields (mail) | to / subject / body; plus cc, bcc, attachments; threading headers | **`to`, `subject`, `bodyText` only.** These are the fields integration-corev2 normalises (`service.go:1945-1988`). `connection_id`, `operation`, `threadId`, `inReplyTo`, `references`, `correlationId`, `params` are frozen. `cc`/`bcc` do not exist in the normalised shape. If the draft has `bodyHtml`, editing the body sends text only and the card says "Formatting will be removed"; editing only `to` or `subject` keeps `bodyHtml`. A Gmail `raw` draft is preview-only (not editable) because its fields are inside an encoded message. Limits: 1-50 recipients, each RFC 5322-parsable; subject <= 998 chars, no CR/LF; body <= 48 KiB so the descriptor stays under the 64 KiB cap (`orchestration_grpc.rs:936`); larger edits return 422 `edit_too_large`. Adding a recipient is allowed: it is the person's own act, recorded by field name in the audit event | S3, S6 | Yes (default applies) |
| D3 | Calendar event cards | build a calendar write now / defer | **Defer.** No calendar write operation exists in integration-corev2 or in the frozen contract. Building one is WS23-S9 (owner ruling, frozen-doc amendment, incremental OAuth consent). The `ApprovalPreview.kind` enum and the edit validator take a new kind later without a breaking change | none in W3 | Yes |
| D4 | Who may read the full draft | (a) session-core decrypts the descriptor for the run owner on a pending approval; (b) copy the preview into `approvals.metadata` in plaintext; (c) show only the existing 120-char reason | **(a).** The descriptor is already retained, encrypted, owner-bound and erased with the run; (b) would put mail text in plaintext metadata, which migration 0022's header forbids; (c) is the gap the item exists to close. Read rules: caller is the run owner (`authorize_run_owner`, read intent), approval state REQUESTED, not ZDR (no descriptor exists), not waived, org must match; response is shaped by an mp-contracts function (never the raw descriptor); never logged, never put in an event | S1 | Yes: S2, S3, S7-S9 |
| D5 | User-visible policy levels | (a) per-user rows in capability-core `tool_policies` on the two provider capabilities; (b) a new per-connection preference store; (c) org-only | **(a).** Store, audit rows and BFF route already exist. Levels shown: reads "Use without asking" (no row), "Ask first" (`ask`), "Off" (`never`); writes "Always ask" (fixed, shown locked) and "Off" (`never`). Tighten only: no `allow` on a built-in (`tool_policies.go:415-418`), no write relaxation (ledger rule 4). Granularity is per capability (all connected apps) in W3; per-connection levels need new capability ids or a new key and are logged as a follow-up. The panel shows the levels as "not enforced yet" while `TOOL_POLICY_ENFORCED` is off | S7 | Yes |
| D6 | Suggestion tool shape and loop | Ask loop only / both loops; free text provider / closed list; ranking | **Ask loop only in W3; closed provider list; no ranking.** `suggest_connector {provider, reason}` where `provider` is a closed enum in mp-contracts (initially `google`, `microsoft`: the providers with connector reads, WS23-S2/S3); `reason` <= 200 chars, plain text. One suggestion per turn. Offered only when `CONNECTOR_SUGGESTIONS_ENABLED`, not ZDR, not Space, retrieval scope auto. The tool returns "shown to the person; answer without that data" and the turn continues. The card's order is the model's single pick, never paid placement (audit reference practice). Do-loop suggestions are a later option (needs a capability-core class row) | S8 | Yes |
| D7 | Where the connect intent lives | (a) SPA `sessionStorage`; (b) a server record in session-core | **(a).** The OAuth runs in a popup and the chat page stays loaded (`provider-auth-window.ts:104`), so the intent only has to survive a reload in the same tab. A server record would be a new content store (it holds the pending prompt) with ZDR and erasure duties for little gain. TTL 15 min; read and write in try/catch; a missing record degrades to a plain "Connected" state | S9 | Yes |
| D8 | What the model learns about an edit | values / field names / nothing | **Field names only**, in the tool result of the released call: "The person edited subject, body before approving; the sent version differs from your draft. Do not quote your draft as what was sent." Values are not re-injected (they are in the descriptor; the WS23-S1 persistence rule keeps write results to a verdict line) | S5 | Yes |
| D9 | Waived runs | allow edits / refuse | **Refuse** preview and edit for waived runs, as for ZDR (DEC-32 treats waived runs as ZDR for new stores; the revision columns are a new store dimension). The card falls back to approve/reject | S1, S4 | Yes (default refuses) |
| D10 | Writes with an empty `connector_scope` (register D2-719) | require a selection for writes / keep approval-only | Owner decision, not this epic's. The edit does not change it: `connection_id` is frozen in an edit, so an edit can never retarget a connection | S10 (live) only | Yes |
| D11 | Card copy keeps the 120-char opening (D2-720) | keep / recipients and subject only | Keep the reason as built; the full body is fetched on demand through S1 and is never stored again. Owner confirms the copy in S10 | S10 | Yes |

Ledger entries this epic adds (newest first, `MP/docs/decisions/ledger.md`): D1 (edited approval = revised descriptor, ONCE only), D4 (owner-only decrypted preview, never stored), D5 (connector policy levels = tighten-only user rows), D6 (Ask-loop suggestion, closed list, no ranking). Nothing in the ledger is reversed.

## 4. Slices

Commands (repo): `cd "MP/rust" && cargo test -q -p <crate> --lib` on Windows (two targets fail pre-existing; see memory "Model Plane build and deploy"), plus named `--test` targets; `cargo fmt --all -- --check`; real-PG session-core tests are `#[ignore]` and run with the DB harness (`cargo test -p session-core --bin session-core <name> -- --ignored --test-threads=1`); clean the touched package first (`cargo clean -p <crate>`, register D2-541: stale targets lied in Wave 2). Proto: in `MP/proto` run `buf lint`, `buf breaking --against proto/baseline.binpb`, `buf generate` (Go bindings under `MP/go/gen/model_plane/v1/`). SSE taxonomy: `python3 "MP/scripts/gen-sse-event-taxonomy.py" --check`. ZDR: `bash scripts/tests/zdr-contract-sync.test.sh`. Go: `cd "<service>" && GOWORK=off go test ./...`. SPA: `cd "FE" && pnpm exec vitest run <files>`, `pnpm typecheck` (use `tsc -p tsconfig.app.json`; `tsconfig.json` checks nothing), `pnpm lint`; pre-existing failures are diffed against HEAD. BFF: `cargo test --manifest-path "FE/apps/gateway/Cargo.toml" --all-targets <filter>`. Worktree files are CRLF; edits keep it.

### S1 - Full-draft preview on the approval card [BUILD]

**Goal.** Before approving a mail send, the person reads exactly what will be sent: every recipient, the full subject and the full body. Read-only. Cheapest real user-visible value: it closes most of the "feared send" gap without any new authority.

**Files.**
- `MPC/src/write_preview.rs` NEW (+ `pub mod` in `MPC/src/lib.rs`): `fn preview_from_input(tool_name, input: &Value) -> ApprovalPreviewView` that shapes the retained `execute_provider_action` input into `{kind: "mail_send" | "other", provider, operation, connection_id, to: Vec<String>, subject, body_text, has_html, is_reply (threadId or inReplyTo present), raw_only (Gmail `raw` present), editable_fields}`. `kind = other` carries only `provider`, `operation`, `connection_id` and no values. Pure, no IO. Uses the same recipient split as `EC/integration_tools.rs:1287-1300` (moved here in S3; S1 copies the 15 lines and S3 deletes the copy).
- `PROTO/orchestration.proto`: `rpc GetApprovalPreview(GetApprovalPreviewRequest) returns (GetApprovalPreviewResponse);` on `OrchestrationCoreService`. New messages, every field commented with `// EP-10 S1`: `GetApprovalPreviewRequest { string approval_id = 1; string org_id = 2; }`; `ApprovalPreview { string kind = 1; string provider = 2; string operation = 3; string connection_id = 4; repeated string to = 5; string subject = 6; string body_text = 7; bool has_html = 8; bool is_reply = 9; bool raw_only = 10; repeated string editable_fields = 11; string action_fingerprint = 12; uint32 revision = 13; }`; `GetApprovalPreviewResponse { ApprovalPreview preview = 1; }`. Additive only.
- `SC/src/orchestration_store.rs`: `read_descriptor_for_owner(pool, approval_id, org_id) -> Option<(JsonValue, fingerprint)>` that selects only a REQUESTED, unexpired approval and decrypts with the existing `DescriptorCipher` (same AAD as the insert at 959-970).
- `SC/src/orchestration_grpc.rs`: `get_approval_preview` handler next to `get_approval`: `identity`, `authorize_operation(.., "orchestration:read")`, org pin, `authorize_run_owner(.., OwnerIntent::Read)` (pattern of `create_approval`), refuse ZDR and waived callers (`approval_continuation` feature, existing refusal), return NOT_FOUND for every other case (unknown, other org, not owner, not pending, no descriptor). Never logs the descriptor or the preview.
- `MG/http_routes.rs`: `GET /v1/orchestration/approvals/:approval_id/preview` beside 444; `continuation_gate` first; flag off returns 404 `{error:{code:"preview_unavailable"}}`; response `{data:{preview}}`. `MG/grpc.rs` test double gains the RPC.
- `BFF/domains/orchestration.rs`: route `/api/v1/orchestration/approvals/{approval_id}/preview` (GET) beside 89-92, plain pass-through.
- `FES/shared/actions/action-registry.ts` (+ `action-registry.test.ts`): `approvals.preview` (ownerPlane `model`, risk `low`, read, `requiresApproval: false`) BEFORE the UI change.
- `FES/shared/api/orchestration-client.ts`: `getApprovalPreview(approvalId, signal?)` with a typed `ApprovalPreview`.
- `FES/features/chat/components/WritePreview.tsx` NEW: renders recipients, subject and body (plain text, `white-space: pre-wrap`, no HTML rendering), "Reply in an existing thread" note when `is_reply`, "Formatted mail" note when `has_html`; loading, error and "preview not available" states. Copy through `i18n.tr` (WS12 locale gates).
- `FES/features/chat/components/ChatMessages.tsx`: inside `ApprovalRequests` (2214), when `approval.cause?.tool === 'execute_provider_action'` and the cause's `action_key` names a mail send, render a "Show the full email" disclosure that fetches the preview once on open.
- Compose: `APPROVAL_WRITE_PREVIEW_ENABLED: ${APPROVAL_WRITE_PREVIEW_ENABLED:-false}` in the model-gateway block of `MP/deploy/docker-compose.yml`, and in `MP/deploy/.env.example` (lead shared edit).

**Contract changes.** Proto above (new RPC and three messages). No migration.
**Flag.** `APPROVAL_WRITE_PREVIEW_ENABLED` (model-gateway, strict literal `true`, default off). session-core serves the RPC unconditionally; the gateway decides exposure. Turned on after the S10 live check shows the preview for one Gmail and one Outlook draft.
**ZDR.** No descriptor exists for a ZDR run (`agent.rs:7853-7858`), so there is nothing to show; the route still refuses ZDR callers through `continuation_gate` and session-core refuses ZDR/waived identities. No new store, no persisted copy.
**Tests.**
- `cargo test -q -p mp-contracts write_preview`: `mail_send_preview_keeps_every_recipient_and_the_full_body`, `other_writes_carry_no_values`, `gmail_raw_draft_is_raw_only`, `reply_headers_mark_is_reply`.
- session-core unit + `#[ignore]` real-PG: `preview_is_owner_only`, `preview_refuses_other_org_as_not_found`, `preview_refuses_decided_and_expired_approvals`, `preview_refuses_zdr_and_waived_identities`, `preview_is_never_logged` (tracing capture).
- model-gateway: `preview_route_off_by_default`, `preview_route_refuses_zdr_token`, `preview_route_maps_not_found`.
- SPA: `WritePreview.test.tsx` (render, plain-text body, no HTML injection, states), `ChatMessages.approval-preview.test.tsx` (disclosure only for mail sends), `orchestration-client.test.ts`, `action-registry.test.ts`.
- BFF: `cargo test ... orchestration` with a preview route test.
- `buf lint`, `buf breaking`.
**DoD.** A Do run drafting a mail pauses; the card shows "Show the full email"; opening it shows all recipients, subject and body; a second user, another org, a decided approval and a ZDR token all get not-found or the typed refusal.
**Evidence.** Rebuilt session-core, model-gateway and BFF; second SPA dev server (5173 is the main checkout); screenshot of the open preview; the network response (redacted); session-core log showing no descriptor text. `[verify live]`
**Effort.** M, 5-7 d. **Deps.** WS23-S8.code (built), WS13-S2 (card). **Risks.** `ChatMessages.tsx` and `http_routes.rs` are hot files (edit-first rule); the descriptor key must be present on session-core (it already is for continuations).

### S2 - One write-operations golden [FINISH]

**Goal.** A write the agent can call is a write integration-corev2 gates for approval, and its provider catalogue says it writes. Closes D2-718 (an unmapped write skips both gates) before any edit path widens the write surface.

**Files.**
- `IC/actions/testdata/provider-write-operations.json` NEW: `{provider, operation, aliases[], approval_required: true, direction: "write", editable_kind: "mail_send" | "none"}` for every write arm in `service.go` that execution-core catalogues.
- `IC/actions/write_operations_golden_test.go` NEW: every row has `actionRequiresApproval(provider, op) == true` (`IC/api/server.go:3751`; move the test to `IC/api` if the function is not reachable from `actions`), every alias too, and the provider catalogue marks the capability `Direction` write; `-update` regenerates.
- `MPC/tests/fixtures/provider-write-operations.json` (vendored copy, sha256 pin) and `MPC/src/provider_writes.rs` NEW: `WRITE_OPERATIONS` table and `editable_kind(operation)`; `MPC/tests/provider_write_operations.rs` checks the table against the vendored golden.
- `EC/integration_tools.rs` test `catalog_writes_match_contract_table`: the set of `is_write: true` catalogue rows equals `WRITE_OPERATIONS`.
- `.github/workflows/integration-corev2-go.yml` (built by WS23-S2) already runs `go test ./internal/actions/... ./internal/api/...`; no change unless the test lands in another package.

**Contract changes.** Golden only. `docs/actions-surface-operations.md` is NOT edited (frozen; K3-14 sign-off pending).
**Flag.** None. **ZDR.** None.
**Tests.** `cd "apps/Ingestion Plane/integration-corev2" && GOWORK=off go test ./internal/actions/... ./internal/api/... && go vet ./...`; `cargo test -q -p mp-contracts provider_write`; `cargo test -q -p execution-core --lib catalog_writes`. Mutation check: drop one `actionRequiresApproval` arm locally and see the Go test fail (record in the PR).
**DoD.** Adding a write arm without a golden row fails Go; a golden row without a catalogue row fails Rust. Register D2-718 closed with the commit.
**Evidence.** Local test output; CI run URL when the branch is pushed (N1 CI-only evidence rule).
**Effort.** S, 2-3 d. **Deps.** none. **Risks.** the sha pin needs a `sha2` dev-dependency in `MPC/Cargo.toml` (register D2-210 has the same gap); two golden copies can drift (sha pin + CI).

### S3 - Edit contract in mp-contracts [BUILD]

**Goal.** One pure, shared definition of what may be edited and how an edited call is fingerprinted, so session-core (authority), execution-core (runner) and the SPA (form limits, mirrored) agree.

**Files.**
- `MPC/src/approval_edit.rs` NEW: `MailSendPatch { to: Option<Vec<String>>, subject: Option<String>, body_text: Option<String> }`; `fn apply_edit(original_input: &Value, patch: &MailSendPatch) -> Result<EditedCall, EditError>` (only the D2 fields; `connection_id`, `operation`, `params` and threading fields copied unchanged; `bodyHtml` removed only when `body_text` is set; rejects a Gmail `raw` input with `not_editable`; limits from D2; at least one field must change, else `no_change`); `EditedCall { input: Value, edited_fields: Vec<&'static str> }`; `EditError` closed codes `not_editable`, `invalid_recipient`, `too_many_recipients`, `invalid_subject`, `edit_too_large`, `no_change`.
- `MPC/src/approval_descriptor.rs` NEW: `fn descriptor_scope(...)` and `fn action_fingerprint(scope: &Value) -> String`, moved byte-for-byte from `EC/runtime_loop/agent.rs:7896-7908`; `EC` calls them (no behaviour change).
- Move `provider_effect_summary` and its helpers from `EC/integration_tools.rs:1210-1300` to `MPC/src/provider_writes.rs` (S2 file); `EC` re-exports, so callers and tests are unchanged.
- `MPC/tests/fixtures/approval-edit-cases.json` + `MPC/tests/approval_edit_golden.rs`: input, patch, expected input or error.
- `FES/shared/approvals/approval-edit-limits.ts` NEW: the same limits as constants, with a vitest that reads the golden fixture (vendored with a sha pin, pattern of `zdr-refusals.json`).

**Contract changes.** None on the wire. **Flag.** None (pure code).
**Tests.** `cargo test -q -p mp-contracts approval_edit approval_descriptor provider_writes`; `cargo test -q -p execution-core --lib continuation_descriptor` (the existing `continuation_descriptor_binds_the_exact_action_and_refuses_zdr`, `agent.rs:8843`, must pass unchanged) plus NEW `fingerprint_is_byte_identical_after_the_move` (a fixed descriptor and its pre-move hash); `cargo test -q -p execution-core --lib provider_effect_summary` (existing tests 1801-1868 unchanged); `pnpm exec vitest run src/shared/approvals/approval-edit-limits.test.ts`.
**DoD.** Golden cases green in Rust and the SPA; fingerprint of an unedited call identical before and after.
**Evidence.** Test output only. **Effort.** S, 3-4 d. **Deps.** S2 (for `provider_writes.rs`; can merge in either order if S3 creates the file). **Risks.** a non-identical fingerprint after the move would orphan every pending approval: the byte-identity test is mandatory.

### S4 - Approve with edits (session-core, gateway, BFF), dark [BUILD]

**Goal.** "Send with my edits" grants the approval and replaces its call in one transaction: new descriptor input, new fingerprint, new reason, revision +1. Nothing else about the approval changes.

**Files.**
- `PROTO/orchestration.proto` (all commented `// EP-10 S4`):
  - `DecideApprovalRequest.edit = 9` with NEW `message ApprovalEdit { string patch_json = 1; string base_fingerprint = 2; }`.
  - `Approval.revision = 25` (`uint32`; 0 = never edited; 17-24 are taken on branch space-w2, see 00-integration-round-b.md).
  - `ApprovalStateChanged.revision = 11`, `ApprovalStateChanged.edited_fields = 12` (`repeated string`, field names only, never values).
  - `ConsumeApprovalResponse.revision = 8`, `ConsumeApprovalResponse.original_action_fingerprint = 9` (for S5).
- `SC/migrations/0074_approval_descriptor_revision.sql` (current head `0071_run_question_consume.sql`; integrator assigns) + `SC/migrations/revert/0074_approval_descriptor_revision.down.sql`: `ALTER TABLE approval_continuation_descriptors ADD COLUMN IF NOT EXISTS revision SMALLINT NOT NULL DEFAULT 0 CHECK (revision BETWEEN 0 AND 3), ADD COLUMN IF NOT EXISTS original_action_fingerprint TEXT NULL CHECK (original_action_fingerprint IS NULL OR length(original_action_fingerprint) = 64), ADD COLUMN IF NOT EXISTS revised_at TIMESTAMPTZ NULL;`. Additive, idempotent. No content column: the original input is not kept (only its fingerprint).
- `SC/src/orchestration_store.rs`: `decide_approval_recorded` (1070) gains `edit: Option<ApprovalEdit>`. When present and the decision is GRANTED: `SELECT ... FOR UPDATE` the approval (REQUESTED, unexpired) and its descriptor; decrypt; check `base_fingerprint == action_fingerprint` else `Aborted("approval_changed")`; check `tool_name == execute_provider_action` and `editable_kind(operation) == mail_send` else `FailedPrecondition("not_editable")`; `apply_edit`; rebuild the descriptor with the new input via `mp_contracts::approval_descriptor` (keep `offered`, `permission_mode`, scope ids); size check (64 KiB); re-encrypt; `UPDATE approval_continuation_descriptors SET ciphertext, action_fingerprint, revision = revision + 1, original_action_fingerprint = COALESCE(original_action_fingerprint, <old>), revised_at = now()`; recompute `approvals.reason` with `provider_effect_summary` + `scrub::scrub_string` (K3-16); recompute `metadata.cause.call_digest` with `approval_cause::call_digest` (`MPC`); set `metadata.revision = {n, edited_fields, at}`; then the existing grant path (delivery row for cold, none for loop). One transaction. Denial ignores `edit` with `InvalidArgument("edit_requires_grant")`.
- `SC/src/orchestration_grpc.rs` `decide_approval` (3862): refuse `edit` when the caller is ZDR or the run is waived (D9), when `scope != ONCE` or `rule_proposed` (D1), when the server switch is off (`FailedPrecondition("approval_edit_disabled")`); map `EditError` codes to `InvalidArgument` with the closed code as the message; emit `ApprovalStateChanged` with `revision` and `edited_fields` (names only).
- `MG/http_routes.rs`: `DecideApprovalBody` (1954) gains `#[serde(default)] edit: Option<EditBody { patch: Value, base_fingerprint: String }>`; `decide_approval` (2193) refuses an edit with any scope other than `once` and with `promote`, forwards it, maps errors: 409 `approval_changed`, 422 with the `EditError` code, 403 `approval_edit_disabled`. Step-up (WS13-S5) applies unchanged to an edited grant.
- `BFF/domains/orchestration.rs` `decide_upstream_body` (~524-540): accept `edit {patch, baseFingerprint}` (camelCase in, snake_case out), max body 64 KiB, refuse unknown keys as today.
- `MG/grpc.rs` test double and `MG/tests/orchestration_http_test.rs`.
- Compose: `APPROVAL_EDIT_ENABLED: ${APPROVAL_EDIT_ENABLED:-false}` in the session-core, model-gateway and execution-core blocks (lead shared edit).

**Contract changes.** Proto as listed; migration `0074` in session-core. Go bindings regenerated (`buf generate`).
**Flag.** `APPROVAL_EDIT_ENABLED` (strict literal `true`, default off; read by session-core, model-gateway and execution-core). Deploy order: execution-core (S5), then session-core, then model-gateway. Off means the gateway refuses `edit` with 403 and session-core refuses it too, so approvals behave exactly as before. On only after the S10 proof.
**ZDR.** No approval exists for a ZDR run; the gateway (`continuation_gate`) and session-core refuse ZDR and waived callers. New stored data: the edited input inside the existing encrypted descriptor (same class and lifetime: removed when the approval expires, is denied or delivery settles, cascade on run delete), `revision`, `original_action_fingerprint` (a hash), `revised_at`, and field names in `metadata.revision` and the audit event. GDPR erasure: covered by `ON DELETE CASCADE` on `approval_continuation_descriptors.run_id` (0022) and `approvals` deletion; add the revised row to the erasure test (also answers the D2-720 note that erasure must cover descriptors).
**Tests.**
- session-core `#[ignore]` real-PG: `edit_and_grant_is_one_transaction` (a failure after re-encrypt leaves the approval REQUESTED and the descriptor unchanged), `edit_changes_fingerprint_and_keeps_the_original_hash`, `stale_base_fingerprint_is_approval_changed`, `two_concurrent_edits_one_wins`, `edit_refused_for_non_mail_writes`, `edit_refused_for_thread_scope_and_rule_proposal`, `edit_refused_for_zdr_and_waived`, `fourth_revision_is_refused`, `edited_reason_is_scrubbed_and_recomputed`, `revised_descriptor_is_erased_with_the_run`, `denial_with_edit_is_invalid`.
- session-core unit: `edit_error_codes_map_to_invalid_argument`, `audit_event_carries_field_names_only` (asserts no recipient, subject or body text in the event).
- model-gateway: `edit_off_by_default_is_forbidden`, `edit_with_scope_thread_is_refused`, `edit_passes_step_up_like_a_grant`, `approval_changed_is_409`.
- BFF: `decide_body_accepts_edit_and_refuses_unknown_keys`.
- `buf lint`, `buf breaking`; `cargo test -q -p session-core --lib`, `-p model-gateway --lib`.
**DoD.** With the switch on in a test stack: an edited grant stores a new fingerprint and revision 1; the old fingerprint is only in `original_action_fingerprint`; the reason names the new subject; the audit event lists `subject, body_text`; a stale edit gets 409.
**Evidence.** DB harness output; `psql` of the descriptor row (fingerprint, revision; ciphertext not decrypted in the evidence) and the approval's `metadata.revision`; the 409 body. `[verify live]` on a rebuilt stack in S10.
**Effort.** M, 6-8 d. **Deps.** S3; WS13-S3 (audit event, partial: fields ship dark with it); integrator-assigned migration number and proto tags. **Risks.** `SC/src/orchestration_store.rs` and `orchestration_grpc.rs` are large hot files; the descriptor key (`SESSION_CORE_CONTINUATION_DESCRIPTOR_KEY` per DEC-03) is needed to decrypt in the decide path, which today only the insert and consume paths use [verify the cipher's decrypt helper is reachable from the decide path].

### S5 - The executor runs the edited call and tells the model [BUILD]

**Goal.** A granted, edited approval runs the edited call on both continuation paths, and the model is told the person changed it.

**Files.**
- `SC/src/orchestration_store.rs` consume path (~1796-1818): return `revision` and `original_action_fingerprint` in `ConsumeApprovalResponse` (fields 8, 9); the `AND ($5 = '' OR d.action_fingerprint = $5)` match accepts `$5` equal to the current or the original fingerprint when `revision > 0`.
- `EC/runtime_loop/resume.rs` restore check 10/11 (1222-1256): when `consumed.revision > 0` and `APPROVAL_EDIT_ENABLED` is on, compare the fold's `pending.action_fingerprint` with `consumed.original_action_fingerprint` (not the current one) and skip the fold-digest equality for the pending call only, provided `call.name == consumed.tool_name == "execute_provider_action"`, `step_id` matches and the consumed input's `connection_id` and `operation` equal the fold call's (an edit may not retarget). Everything else in the restore stays. With the switch off, a revised approval falls back exactly as today.
- `EC/runtime_loop/mod.rs` provider-write result for a revised call: prepend the D8 sentence with the field names (read from the consumed response, not the model's arguments). The persisted step note stays the WS23-S1 verdict line plus `edited=<names>`.
- `EC/approval_delivery_worker.rs`: no logic change; NEW test `cold_delivery_runs_the_edited_input` (the descriptor's input is what `ProviderActionInput` parses, 369-380) and `cold_delivery_attests_the_edited_payload` (the `payload_sha256` of the sent body equals the edited body's).
- `EC/tests/durable_loop_contract.rs`: NEW `edited_approval_restores_in_place_with_the_edited_call`, `edited_approval_cannot_change_connection_or_operation`, `edited_approval_falls_back_when_the_switch_is_off`.

**Contract changes.** Uses S4's proto fields. **Flag.** `APPROVAL_EDIT_ENABLED` on execution-core (same name as S4). Execution-core is deployed first so it never meets a revised approval it cannot read.
**ZDR.** Continuations are refused for ZDR (`agent.rs:7853-7858`); nothing new persists beyond the field names in the step note.
**Tests.** `cargo test -q -p execution-core --lib resume` (existing restore tests stay green, including the `PendingMismatch` cases at `resume.rs:2865-2898`), the new tests above, `cargo test -q -p execution-core --test durable_loop_contract`, `--test cross_service_loop_contract`, `cargo test -q -p session-core` consume tests (`#[ignore]` real-PG for the fingerprint match).
**DoD.** Edited approval, loop mode: the run continues in place and sends the edited mail; cold mode: the worker sends the edited mail; the model's next message does not claim the original text was sent.
**Evidence.** Test output; in S10 the integration-corev2 receipt and the mailbox show the edited subject. `[verify live]`
**Effort.** M, 4-6 d. **Deps.** S4; WS03-S3/S7 (done). **Risks.** `resume.rs` is an authority boundary ("authority narrows, never widens"): the relaxation is limited to the argument digest of one call, with tool, step, connection and operation pinned; an Opus hostile review is required before merge (strict slice).

### S6 - Editable card in the SPA [BUILD]

**Goal.** The person can change recipients, subject and body on the card and approve the edited version.

**Files.**
- `FES/shared/actions/action-registry.ts` (+ test): extend `approvals.decide` and `orchestration.decide_approval` inputs with optional `edit {patch, baseFingerprint}` (patch validated by `approval-edit-limits.ts`), BEFORE wiring.
- `FES/shared/api/orchestration-client.ts` `decideApproval` (159): optional `edit`; refuses `edit` with a non-`once` scope or `promote` client-side (server refuses too); maps 409 `approval_changed` and 422 codes to `ApiError` codes.
- `FES/features/chat/components/WritePreview.tsx`: an edit mode when `preview.editable_fields` is non-empty and the gateway advertises edits (a `can_edit` boolean added to the preview response by S4's gateway when the switch is on): recipient chips with add/remove and RFC 5322 check, subject input, body textarea, live limit counters, "Formatting will be removed" when `has_html` and the body changed, "Discard edits".
- `FES/features/chat/components/ChatMessages.tsx` `ApprovalRequests`: when edits exist, the primary button reads "Send with my edits"; standing-scope buttons hide; the step-up prompt (WS13-S5) runs before the edited grant exactly as before; on 409 the card refetches the preview and keeps the person's text in a "your version" box.
- `FES/features/chat/components/use-chat-controller.ts`: pass `edit` through the decision handler.
- Locale strings via `i18n.tr` (WS12 gates).

**Contract changes.** None beyond S4. **Flag.** No `VITE_*` flag (server-controlled behaviour, `flags-and-config.md` 8.4): edit mode appears only when `can_edit` is true.
**ZDR.** Temporary chats show no approvals (none exist).
**Tests.** `pnpm exec vitest run src/features/chat/components/WritePreview.test.tsx src/features/chat/components/ChatMessages.approval-edit.test.tsx src/shared/api/orchestration-client.test.ts src/shared/actions/action-registry.test.ts`: edit mode hidden without `can_edit`; invalid recipient blocks send; unchanged form sends a plain approve (no `edit`); 409 path keeps the person's text; step-up then edited grant; keyboard-only path; narrow width. Playwright NEW `FE/tests/e2e/approval-edit.spec.ts` with a mocked API (pattern of the Space UI harness). `pnpm typecheck`, `pnpm lint`.
**DoD.** On a mocked stack and then live: change the subject, send, see the card resolve with "Sent with your edits".
**Evidence.** Second dev server; screenshots at 1280 and 375 px of preview, edit and resolved states; request body in devtools (patch, no extra keys).
**Effort.** M, 5-6 d. **Deps.** S1, S4 (S5 for live). **Risks.** `ChatMessages.tsx` hot file; WS14-S1 docked trays may move the card (render inside whatever container holds `ApprovalRequests`).

### S7 - User-visible policy levels for connected apps [BUILD]

**Goal.** A person can see and tighten how the agent uses their connected apps: reads "Use without asking / Ask first / Off", writes "Always ask (fixed) / Off". The level is honoured in both loops once the policy engine is enforced, and the UI says so honestly before that.

**Files.**
- `CC/internal/api/tool_policies.go`: the list response (`list`, 155) gains `"enforced": <TOOL_POLICY_ENFORCED>`; `set` (278) accepts `ask`/`never` with `scope=user` on `cap.tool.provider.read` and `never` on `cap.tool.provider.execute`, and refuses `ask` on `cap.tool.provider.execute` with 422 `already_always_ask` (writes are always-ask already) [verify the current `set` behaviour for built-in capabilities first]. Tests in `tool_policies_test.go`.
- `MG/connector_reads.rs` `offered` (77): also false when the caller's active user or org policy on `cap.tool.provider.read` is `ask` or `never` (the Ask loop has no pause, so "Ask first" removes the tool from the Ask loop and the composer chip shows "Ask first: available in Do mode"). The gateway reads the policy through capability-core's active-policy lookup once per turn, cached per (org, user) for 60 s [INF: confirm the gateway already has a capability-core client; if not, read through the existing tool-policies proxy path].
- Do loop: no change needed; the WS05-S5 engine applies `ask` (pause, cause `capability_policy_ask`) and `never` (deny) to `read_provider_action` and `execute_provider_action` when enforced [verify with a test in `EC/capability_policy.rs`: `user_never_on_provider_read_denies`, `user_ask_on_provider_read_pauses`].
- `FES/shared/actions/action-registry.ts` (+ test): `integrations.set_connector_policy` (risk `low`, reversible) and `integrations.list_connector_policies` (read) BEFORE wiring; both call the existing `/api/v1/tool-policies` client (`orchestration-client.ts:236`).
- `FES/features/settings/...` NEW `ConnectedAppsPolicy.tsx` in the connected-apps settings area [locate the settings page that lists connections at slice start], plus a "Change for connected apps" link on a provider approval card. Shows "Not enforced yet" when `enforced` is false.

**Contract changes.** One response field in capability-core; no migration (capability-core head stays 0030; uses `tool_policies` as is).
**Flag.** None new. Behaviour follows `TOOL_POLICY_ENFORCED` (WS05-S5, default off). The Ask-loop policy read is inert when `enforced` is false.
**ZDR.** Policy rows hold no content; they are identity-scoped settings (same as today's MCP rows). Under ZDR there are no connector tools to govern.
**Tests.** `cd "CC" && GOWORK=off go test ./internal/api/... ./internal/policy/...`; `cargo test -q -p model-gateway --lib connector_reads` (`ask_first_removes_the_read_tool_in_ask`, `off_removes_it`, `not_enforced_changes_nothing`); `cargo test -q -p execution-core --lib capability_policy`; SPA `ConnectedAppsPolicy.test.tsx`, `action-registry.test.ts`.
**DoD.** With enforce on in a test stack: "Off" for reads removes the read tool in Ask and Do; "Ask first" pauses in Do and removes it in Ask; "Off" for writes denies `execute_provider_action`; with enforce off the panel says "Not enforced yet" and behaviour is unchanged.
**Evidence.** Go and Rust test output; `[verify live]` screenshots and one Do run per level in S10.
**Effort.** M, 5-7 d. **Deps.** WS05-S5 (partial; build now, live after enforce). **Risks.** a per-turn policy read adds latency to the Ask loop (cache); capability-wide granularity may surprise a person with two Gmail accounts (D5 note in the UI copy).

### S8 - Suggestion tool, event and card (Ask loop) [BUILD]

**Goal.** When a question needs the person's own mail, calendar or files and no connection is selected, the model can suggest one; the card offers "Connect" or "Use it for this question".

**Files.**
- `MPC/src/connector_suggest.rs` NEW: `SUGGESTABLE_PROVIDERS = ["google", "microsoft"]` (closed), `SuggestArgs {provider, reason}` with validation (provider in list; reason 1-200 chars, one line, scrubbed), tool name `suggest_connector`, description text (suggest only when the person's own data is needed; never more than once per turn; never to rank vendors; never when a connection of that provider is already selected).
- `MG/tool_loop.rs`: `suggest_connector` definition in `builtin_tool_defs` (5360) behind a new filter `tools_for_connector_suggestion` beside `tools_for_connector_scope` (5339): offered only when `CONNECTOR_SUGGESTIONS_ENABLED`, not ZDR, not `space_id`, retrieval scope auto, and no connector of a suggestable provider is selected this turn. Dispatch arm: validate, emit the event once per turn (a second call returns a tool error "already suggested"), return "Shown to the person. Answer without that data and say what you could do with it."
- `MG/sse_events.rs`: new `ChatEvent::ConnectorSuggestion { suggestion_id, provider, reason }` (content family, persisted in the message parts like `follow_ups`); regenerate `MP/docs/sse-event-taxonomy.md`.
- System prompt snippet in the Ask loop prompt builder (one paragraph, only when offered).
- `FES/features/chat/components/chat-normalizers.ts`, `chat-types.ts`: accept `connector_suggestion`.
- `FES/features/chat/components/ConnectorSuggestionCard.tsx` NEW: if the provider has no active connection (`listConnections`): "Connect <provider>" runs `startConnectSession` + `runDirectOauthWindow` (existing contract `integrations.start_connect_session`); if connected but not selected: "Use <account> for this question" sets the chip selection (`chat.set_connector_scope`) and re-sends the last user message; "Not now" dismisses. Hidden in Space threads and temporary chats. On history reload the card renders as a resolved note.
- `FES/shared/actions/action-registry.ts` (+ test): `chat.accept_connector_suggestion` (composite: start connect or set scope and resend; risk `low`) BEFORE wiring.
- Compose: `CONNECTOR_SUGGESTIONS_ENABLED: ${CONNECTOR_SUGGESTIONS_ENABLED:-false}` (model-gateway block, lead shared edit).

**Contract changes.** One new SSE variant (taxonomy check). No proto, no migration.
**Flag.** `CONNECTOR_SUGGESTIONS_ENABLED` (model-gateway, strict literal `true`, default off). On after S10 shows one suggestion per provider and a connect-and-continue.
**ZDR.** The ZDR chat path has no tool loop, so the tool is never offered; the filter also checks `zdr` (defence in depth). Under ZDR nothing is emitted or persisted. Non-ZDR persistence: provider key and a model-written reason (no personal data by construction; scrubbed).
**Tests.** `cargo test -q -p mp-contracts connector_suggest`; `cargo test -q -p model-gateway --lib`: `suggestion_not_offered_without_flag`, `..._in_zdr`, `..._in_space`, `..._when_provider_already_selected`, `suggestion_emits_one_event_per_turn`, `unknown_provider_is_a_tool_error`, `reason_is_scrubbed_and_bounded`; `python3 "MP/scripts/gen-sse-event-taxonomy.py" --check`; SPA `ConnectorSuggestionCard.test.tsx`, `chat-normalizers.connector-suggestion.test.ts`, `action-registry.test.ts`.
**DoD.** Ask "what did Kari send me yesterday?" with no chip selected: the answer says it cannot see the mail and a Gmail card appears; with Gmail selected no card appears.
**Evidence.** Rebuilt model-gateway; SSE capture with the `connector_suggestion` frame; screenshots of both card states. `[verify live]`
**Effort.** M, 6-8 d. **Deps.** WS23-S5.code and S6.code (partial; chip state and selection path exist), DEC-26 (explicit selection), DEC-06. **Risks.** over-suggestion (one-per-turn cap, prompt text, eval prompts in S10); the "use it" path leads to connector reads, so it must not go live before register D2-172 (trust class) and D2-604 (Ask-loop taint) are closed.

### S9 - Resumable connect intent (SPA) [BUILD]

**Goal.** After the person connects from a suggestion card, the turn continues with the new connection selected, and a reload during OAuth does not lose the place.

**Files.**
- `FES/shared/integrations/connect-intent.ts` NEW: `saveIntent({threadId, suggestionId, provider, prompt, createdAt})`, `readIntent(threadId)`, `clearIntent`; `sessionStorage`, TTL 15 min, every access in try/catch, max one intent per thread.
- `ConnectorSuggestionCard.tsx` (S8): save the intent before opening the popup; on success poll `listConnections` until a new connection of the provider appears (max 30 s), then set the scope to that connection and re-send the saved prompt with `appendUser: false` and a visible "Continuing with Gmail connected" status line; on failure show the provider error from `provider-auth-window.ts`.
- `use-chat-controller.ts`: on thread load, if an unexpired intent exists, show the card in "Finish connecting" state with a "Continue" button (never auto-sends after a reload).
**Contract changes.** None. **Flag.** None (inert unless S8's events exist).
**ZDR.** Temporary chats never show suggestion cards, so no intent is saved. The intent holds the person's own prompt in their own tab's `sessionStorage` only.
**Tests.** `pnpm exec vitest run src/shared/integrations/connect-intent.test.ts src/features/chat/components/ConnectorSuggestionCard.test.tsx` (storage throws, TTL expiry, success resend once, reload shows Continue, no double send).
**DoD.** Connect from a card, the popup closes, the answer arrives using the new connection; reload mid-OAuth, then Continue works.
**Evidence.** Screen recording or screenshots on a second dev server. `[verify live]`
**Effort.** S, 3-4 d. **Deps.** S8. **Risks.** popup blockers (the existing runner reserves the window; reuse it).

### S10 - Live proof, flag flips, ledger, docs [FINISH]

**Goal.** Prove each switch on a rebuilt stack with dedicated accounts, then turn the switches on in order.
**Steps.**
1. Preconditions (owner and lead): DEC-27 dedicated M365 developer tenant and Gmail account connected (owner consent); K1-11 accepted (D2-721); attestation key rotated (D2-717); WS23-S8 live send proven (D2-722); D2-172 and D2-604 closed before any read path is used by S8/S9.
2. Rebuild execution-core, session-core, model-gateway, capability-core and the BFF with `.plan-work/execution/tools/recreate-with-live-env.sh`; second SPA dev server.
3. `APPROVAL_WRITE_PREVIEW_ENABLED=true`: Do run drafts a Gmail and an Outlook mail; open the preview; deny both. Evidence: screenshots, redacted response, no descriptor text in logs.
4. `APPROVAL_EDIT_ENABLED=true` (execution-core, then session-core, then gateway): edit subject and body, send; once in loop mode, once in cold mode (`AGENT_CONTINUE_AFTER_APPROVAL_ENABLED` off). Evidence: approval row `metadata.revision`, descriptor row (revision, both fingerprints), `ApprovalStateChanged` with field names, integration-corev2 receipt, mailbox screenshot showing the edited subject, the model's follow-up text. Also a stale edit (two tabs) returning 409.
5. `TOOL_POLICY_ENFORCED` (if WS05 has flipped it): one Do run per level.
6. `CONNECTOR_SUGGESTIONS_ENABLED=true`: one suggestion per provider, connect-and-continue, the already-selected case, a Space thread and a temporary chat (no card).
7. ZDR token on every new route returns the typed refusal.
8. Ledger entries (D1, D4, D5, D6), `MP/docs/capability-ownership-matrix.md` lines, `MP/docs/ZDR.md` rows, register rows closed, owner confirms the card copy (D11).
**Effort.** M, 4-6 d plus owner wait. **Deps.** S1-S9, DEC-27, K1-11, WS23-S8 live, WS05-S5 enforce (step 5 only).

**Total.** About 43-59 engineer-days (S1 5-7, S2 2-3, S3 3-4, S4 6-8, S5 4-6, S6 5-6, S7 5-7, S8 6-8, S9 3-4, S10 4-6). Two lanes can run in parallel: the write lane (S1, S2, S3, S4, S5, S6) and the suggestion/policy lane (S7, S8, S9).

## 5. ZDR clause

New stores and egress, and how each behaves under ZDR (DEC-32: waived runs count as ZDR for new stores):

| New thing | Store or egress | ZDR behaviour | Waived |
|---|---|---|---|
| Approval preview (S1) | read of the existing encrypted descriptor; no new store | Nothing exists to read; gateway `continuation_gate` and session-core refuse with `approval_continuation` | refused |
| Edit (S4) | edited input inside the existing encrypted descriptor; `revision`, `original_action_fingerprint`, `revised_at` columns; field names in `approvals.metadata.revision` and `ApprovalStateChanged` | Refused with `approval_continuation` before any read; no approval exists for a ZDR run | refused (D9) |
| Edited-call note (S5) | field names appended to the WS23-S1 verdict step note | Do runs are refused under ZDR (`EC/grpc.rs` agent run refusal) | n/a |
| Policy levels (S7) | rows in existing `tool_policies` (identity settings, no content) | No connector tools exist under ZDR; rows are not content | n/a |
| Suggestion (S8) | `connector_suggestion` event in message parts (provider key + scrubbed reason) | Tool never offered (no tool loop on the ZDR path, plus an explicit `zdr` check); nothing emitted | not offered |
| Connect intent (S9) | the person's own prompt in their browser `sessionStorage` | Temporary chats never show the card, so nothing is saved | n/a |
| Egress | none new. Sends still go through integration-corev2 with the attested write; suggestions reuse the SPA's existing connect routes | unchanged | unchanged |

Refusal behaviour: typed `{error:{code,message,details}}` with the existing `approval_continuation` feature key from `MP/api/contracts/zdr-refusals.json`; no new feature key, so `zdr-refusals.json` and its `.sha256` do not change. Approval continuations stay refused under ZDR (audit risk for this item).

Golden cases to add to `MP/api/contracts/zdr-posture-matrix.json` (re-pin the sha, re-vendor to `MPC/tests/fixtures/` and `MG/tests/fixtures/`, extend `MG/tests/zdr_posture_matrix.rs` and the `zdr_refusal_inventory.rs` tests in session-core's caller crates):
- `e_zdr_token_approval_preview_refused`
- `e_zdr_token_approval_edit_refused`
- `e_waived_run_approval_edit_refused`
- `e_zdr_turn_offers_no_connector_suggestion`

Golden for `zdr-refusals.json`: none (reuses `approval_continuation`). If the matrix owner prefers feature-level goldens for tool offers (the WS24-S9 precedent: "goldens, not posture-matrix rows"), the fourth case becomes a model-gateway golden test `connector_suggestion_absent_under_zdr` instead; the integrator decides.

## 6. Proof plan

- Unit and contract level, every slice, switches off: the named tests above, plus the existing guards that must stay green: `continuation_descriptor_binds_the_exact_action_and_refuses_zdr` (`agent.rs:8843`), the `resume.rs` restore tests, `durable_loop_contract`, `cross_service_loop_contract`, `autonomy_ladder_contract`, `provider_effect_summary` tests (`integration_tools.rs:1801-1868`), WS23-S2 read golden, `zdr-contract-sync.test.sh`, SSE taxonomy check, `buf lint`/`buf breaking`.
- DB level: session-core real-PG tests for preview, edit, consume and erasure through the DB harness (memory: WS03 durable loop progress, "DB-test harness").
- Strict-slice review: S4 and S5 change authority (what an approval releases); each gets an Opus hostile review and one fix round before merge (register standard pass 3).
- Live (S10), in order: preview, edit (loop and cold), policy levels, suggestions, ZDR refusals. Each switch flips only after its own evidence is captured and quoted in the ledger (plan 6.2 evidence rule). The live write proof uses dedicated accounts only, never production mailboxes (DEC-27).
- Regression of audit section 8 strengths: signed fail-closed capability gate (no new tool in the Do loop; Ask loop gains one non-effectful tool with an offered-set re-check at dispatch); two loops separate (the suggestion tool is Ask-only, the edit path is approval-only); honest outcomes (typed errors, "Sent with your edits", model told of edits); credential isolation (descriptor secret-key guard `orchestration_grpc.rs:1103-1121` applies to the edited descriptor; a test asserts an edit adding an `access_token` key is impossible because only three fields are writable).

## 7. Risks and what the owner must decide

**Risks.**
1. **The edit widens what an approval releases.** Mitigation: same approval, ONCE only, fields limited to `to`/`subject`/`bodyText`, connection and operation pinned in both session-core and `resume.rs`, base-fingerprint concurrency check, field names in the audit event, hostile review of S4/S5.
2. **A changed fingerprint helper orphans pending approvals.** Mitigation: S3 moves the code byte-for-byte with a fixed-vector test.
3. **Mail text exposure.** The preview decrypts personal content for display. Mitigation: owner-only, pending-only, not-found on every other case, never logged or put in events, refused for ZDR and waived runs.
4. **Prompt injection reaching a write.** An injected mail can still steer a draft; the preview and edit make the draft visible and correctable but do not detect injection. Mitigation stays WS23/WS26 (trust class, taint, `tainted_egress` cause; register D2-172, D2-353, D2-354, D2-604 must close before connector reads and suggestions go live).
5. **Policy levels shown but not enforced.** Mitigation: the `enforced` bit and "Not enforced yet" copy until WS05-S5 enforce is live.
6. **Over-suggestion or vendor steering.** Mitigation: closed provider list, one per turn, no ranking, prompt rules, Space/ZDR exclusion.
7. **Hot files.** `ChatMessages.tsx`, `http_routes.rs`, `sse_events.rs`, `orchestration_store.rs`, `orchestration_grpc.rs`, `resume.rs` are shared with WS13/WS14/WS15/WS23 follow-ups (edit-first rule, small hunks).
8. **Live proof depends on the owner.** Without DEC-27 accounts and K1-11 acceptance, S10 cannot run and no switch flips.

**Owner decisions.**
- **OD-1 (D3) Calendar event cards.** Recommendation: defer until WS23-S9 is ruled (calendar write, frozen-doc amendment, OAuth consent). Blocks nothing in W3.
- **OD-2 (D2) May the person add recipients in an edit?** Recommendation: yes (their own act, recorded by field name). Blocks the `to` field in S3/S6 only; default yes.
- **OD-3 K1-11 acceptance and key rotation (D2-721, D2-717), and DEC-27 accounts.** Blocks S10 live writes; S1-S9 build dark.
- **OD-4 (D5) Policy granularity.** Recommendation: capability-wide in W3, per-connection later. Blocks S7 scope only.
- **OD-5 (D6) Suggestion provider list and Ask-only scope.** Recommendation: Google and Microsoft, Ask loop only. Blocks S8 only; default applies.
- **OD-6 (D10, register D2-719) Writes with an empty connector selection.** Not changed by this epic; needed before S10 live writes.
- **OD-7 (D11, register D2-720) Card copy with the 120-character opening.** Recommendation: keep; confirm in S10.

Migrations (integrator assigns numbers): session-core `0074_approval_descriptor_revision` (head 0071). capability-core: none (head 0030). Proto (all additive, commented, in `PROTO/orchestration.proto`): `rpc GetApprovalPreview`, messages `GetApprovalPreviewRequest`, `ApprovalPreview`, `GetApprovalPreviewResponse`, `ApprovalEdit`; fields `DecideApprovalRequest.edit = 9`, `Approval.revision = 25`, `ApprovalStateChanged.revision = 11`, `ApprovalStateChanged.edited_fields = 12`, `ConsumeApprovalResponse.revision = 8`, `ConsumeApprovalResponse.original_action_fingerprint = 9`. New switches, all default off: `APPROVAL_WRITE_PREVIEW_ENABLED` (model-gateway), `APPROVAL_EDIT_ENABLED` (execution-core, session-core, model-gateway), `CONNECTOR_SUGGESTIONS_ENABLED` (model-gateway).
