# WS01-ops-closure sweep (persisted by orchestrator from the Haiku agent return value; agent could not write the file)

Basis: worktree C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b at HEAD afb83928, clean except .plan-work/. Live tree C:/dev/CoresSystem at HEAD afb83928 with 324 status entries, matching C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/.plan-work/live-tree-status.txt. Audit doc: C:/Users/ImaFernandesDaCosta/AppData/Roaming/Claude/scratch-workspaces/5e23d905-dd91-4518-8ff4-252efcbe0520/b5ff8b3a-437c-4499-9a7f-469de994f319/scratch-2026-10-12-69a4ee/03-model-plane-chat-and-agents-audit.md

Card positions in the audit doc:
- F-t3-004: card lines 385-401. Priority, Partial, L (was M), Model Plane + Control Plane, W1, confidence Medium. Rows: 9.1 line 1537; 9.2 Phase-0 note 1677; App B 1822; App C 1947-1954; App D 2121.
- F-t3-005: card lines 403-419. Priority, Partial, M (was S), Model Plane (inference-core) + Control Plane (org switch), W1, High. Rows: 9.1 line 1538; 9.2 line 1668; App B 1823; App C 1956-1961; App D 2122.
- F-t3-039: card lines 885-895. Recommended, Partial, M, Model Plane (+ Infra Plane for deploy), W1, Medium. Rows: 9.1 line 1547; 9.2 line 1672; App B 1856. No App C or D entry.
- Other audit lines: 9.4 note 9 (1704), risk 8 (1718), open question 7 on ZDR (1730), evidence limits (1747).

## 0. Headline findings

1. F-t3-004 Phase-0 code is committed. Commit b9ed9197 "fix(model-plane): deliver session audit events to audit-core" is reachable from HEAD and touches audit_publisher.rs, migration 0043 and nats-provisioner/audit_observability.go. Deployment cannot be proven from the repo. Postmortem 0002 (dated 2026-10-06) says the publisher fix, requeue function and metrics are "not deployed" and the 4,992 rows are "not replayed" (postmortem lines 4-7, 93).
2. No alert rules exist for the Model Plane audit outbox. The metric names appear only in audit_publisher.rs, postmortem 0002 and docs/compliance/evidence/trust-claims-pending-reverification.md. The only alert rules file in the monorepo is C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Data Plane v2/infra/prometheus/alerts.yml.
3. MODEL_PLANE_STATUS.md does not track audit-outbox delivery. Its only "audit" hits (lines 11, 448) are unrelated, although CLAUDE.md says to read it before trusting a "done" claim.
4. The replay is time-sensitive. Postmortem lines 204-205 say the one old-build reservation row is deleted 7 days after publish (2026-10-12) and must be re-armed before then. Today is 2026-10-14, so the re-arm may match 0 rows and the final-read counts (postmortem 225-226: 2,516 and 2,478) may be off by one. Re-read live before any replay.
5. Live-only audit change, not in the card. Untracked retention_receipt.rs defines the subject AUDIT_SUBJECT_PREFIX + "run_started_zdr_waived" (lines 34, 398-401) and inserts the row into session_audit_outbox (lines 483-495). It is called from live grpc.rs:3218-3226 and live terminalization.rs:674. This adds a new event to the same outbox that the Phase-0 replay and audit-core acceptance must cover.
6. audit-core acceptance is generic. eventAuthorityMatches (subscriber.go:287) checks subject-last-token against the payload event. A grep found no event-name allowlist. Acceptance of run_started_zdr_waived is inferred, not tested.
7. "The two other broken audit paths" is ambiguous. Postmortem table lines 266-273 lists six rows. The concrete defect site for row 269 (auth-core membership audit outbox) is C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Control Plane/auth-core/src/auth/organization-events.plugin.ts:829 (audit.actor_resolution_dead_lettered_at?.toISOString()), present at HEAD. Row 268 conflicts with apps/Application Plane/nats.conf:89-92, which grants audit-core consumers on VEREVON_CONTROL_OBSERVABILITY. Which two to schedule is not determined.
8. F-t3-005 attestation code and the org-switch gate are committed: inference-core provider/zdr.rs, gateway settings.rs:120-163, compose 936-951. Most of the ZDR wave is uncommitted (about 20 untracked files plus about 12 modified ZDR runtime files), including the retention resolver, refusal catalogue, waiver tests, retention receipts and migrations 0044/0045. BUILD_PASS_6_8_2026-10-13.md, which holds the deploy order, is untracked.
9. Compose attestation defaults are empty, except *_ZDR_CONFIRMED=false (compose 936, 945). Whether a real provider attestation exists is an operator fact and is not determined from the repo. Release evidence row 1049 says "no external provider attestation".
10. F-t3-039 conformance gate blocks for three reasons: no candidate or rollback artifact (evidence row 1047; conformance 110-114), thresholds not approved (conformance 153-157; all 9 TSV rows are "proposed"), and a capability ledger in source_only state (conformance 130-132). MODEL_PLANE_STATUS.md lines 54 and 150 say no candidate or rollback artifact exists.
11. The revert scripts for migrations 0044 and 0045 exist only in the live tree (untracked). HEAD has no migrations/revert directory and no migration after 0043. The card's "only 0044 and 0045 have revert scripts" is true for live only.
12. F-t3-039 gaps confirmed absent: no rollback rehearsal script (grep "rehears" hits docs only), no expand/contract policy (no match under apps/Model Plane), and no error-rate or p95 thresholds (TSV has none). The card's release-artifact.sh:108-180 covers usage text and validators; subcommand bodies are at 1358-1909.
13. CI: no workflow at HEAD runs the release-* gate tests or the audit-core / nats-provisioner Go tests. The only audit-core references in .github/workflows are path filters at model-plane-rust-ci.yml:36,49. Live model-plane-rust-ci.yml adds a step running cargo test -p session-core --bin session-core zdr_receipt -- --ignored (DB-backed).

## 1. Citation verification

Verdict key: MATCH = symbol or behaviour found at the cited lines; DRIFT = found at other lines; ABSENT = not found; LIVE-ONLY = untracked (??) in live; DIFFERS-IN-LIVE = modified (M) in live.

### F-t3-004 (card 385-401)

| # | Cited (absolute) | Card claim | Worktree verdict | Live | Corrected lines |
|---|---|---|---|---|---|
| 1 | C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Model Plane/rust/services/session-core/src/audit_publisher.rs:432-480 | producer fixes and outbox gauges | DRIFT: 432-489 is record_status_metrics (gauges only); publish fix is elsewhere | clean | audit_publisher.rs 432-489 (gauges); 135-175 (tool_action_event_name, resolve_wire_subject); 334-338 (publish_tool_action); 746-765 (failure and terminal counters) |
| 2 | .../apps/Model Plane/rust/services/session-core/migrations/0043_audit_outbox_requeue.sql:1-60 (card short path, resolved by Glob) | requeue columns and function | MATCH; 76 lines; REVOKE is outside 1-60 | clean | 28-31 columns; 33-73 requeue_session_audit_terminal; 75 REVOKE FROM PUBLIC |
| 3 | commit b9ed9197 | publisher and migration committed | MATCH: in HEAD history; touches audit_publisher.rs, 0043 and nats-provisioner/audit_observability.go | committed | n/a |
| 4 | .../apps/Model Plane/docs/postmortem/0002-session-audit-trail-never-delivered.md:98-117 | "records them undeployed and unreplayed" | MATCH with drift: status at 4-7; "not deployed" at 93; 98-117 holds mechanism bullets (95-112) | clean | 4-7; 93-112; deploy steps 138-163; replay 165-244; rollback 246-259; follow-ups 280-291 |
| 5 | .../apps/Control Plane/audit-core/internal/api/api.go:106-116 | internal-auth GET /v1/audit | MATCH: /v1 group with internalAuth (109-110); GET /audit at 111 | clean | api.go 109-116 |
| 6 | .../apps/Control Plane/audit-core/internal/api/api.go:197-225 | listAudit handler | MATCH: 197-222; default limit 100 at 210 | clean | api.go 197-222 |
| 7 | .../apps/Control Plane/audit-core/internal/store/store.go:85-140 | per-row sha256 payload_hash; idempotent insert | MATCH, shifted: insertAudit 77-111 (sha256 at 92); resolveAuditConflict 113-140 | clean | store.go 77-140 |
| 8 | .../apps/Control Plane/audit-core/internal/store/store.go:225-325 | Purge hard-delete; ListAudit cap | MATCH, shifted: Purge 243 (DELETE audit_events 251, usage_events 260); ListAudit 299 with cap of 500 at 300-301 | clean | store.go 243-270; 299-301 |
| 9 | C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Frontend Plane/verevonv3/apps/gateway/src/domains/audit.rs (no line in card) | org-scoped audit proxy | MATCH: route 30; list_audit 45; org_id required 52; upstream {audit_core_url}/v1/audit at 92; 0 Rust tests | clean | audit.rs 30-92 |
| 10 | "unique indexes on (source_bus, source_producer, event_id)" (no path) | reuse | MATCH: .../apps/Control Plane/audit-core/internal/store/migrations/006_v2_producer_authority.sql:18-20 (audit_events), 34-36 (usage_events); also 003:16, 002:11 | clean | 006:18-20 |
| 11 | "service-credential model with Principal, Scopes and Planes" (no path) | reuse | MATCH: .../apps/Control Plane/audit-core/internal/api/service_auth.go:42 (serviceAuthorization), 51 (AllowsPlane), 225 (authorizeServiceRequest) | clean | same |
| 12 | "alert rules on terminal-row gauge in monitoring config" | Phase-0 deliverable | ABSENT: no Model Plane alert-rule file (Glob empty); the only alert file is apps/Data Plane v2/infra/prometheus/alerts.yml. Postmortem lines 286-288 propose the expressions | clean | none |
| 13 | "two other broken audit paths the postmortem lists" | Phase-0 scope | AMBIGUOUS; see section 7 and headline 7 | clean | postmortem 266-273 |
| 14 | "Trust Center presents tool-action aggregates" (no path) | consumer | MATCH: .../apps/Frontend Plane/verevonv3/src/features/settings/components/TrustCenterSection.tsx:300 ("Brukt av AI?" column); .../src/shared/api/audit-client.ts:105 (GET /api/v1/audit) | clean | same |

### F-t3-005 (card 403-419)

| # | Cited | Card claim | Worktree verdict | Live | Corrected lines |
|---|---|---|---|---|---|
| 15 | .../apps/Model Plane/rust/services/inference-core/src/provider/zdr.rs:1-160 | evidence-bound attestation validation | MATCH: module doc 1-55; FIELD_SUFFIXES 61-67; error enum 79-163; validate 232; from_env 344; resolve 386; unit tests from 541 | clean | zdr.rs 1-160, 232, 344, 386 |
| 16 | .../apps/Model Plane/deploy/docker-compose.yml:927-951 | fields "all default empty" | MATCH with correction: OPENAI fields 936-942; ANTHROPIC 945-951; *_CONFIRMED default false (936, 945), other fields empty | clean | 936-951 |
| 17 | .../apps/Frontend Plane/verevonv3/apps/gateway/src/domains/orgs/settings.rs:120-185 | 409 zdr_no_attested_provider | MATCH: gate doc 120-133; zdr_enable_refusal 134-163 (409 body 147-153); PATCH handler 184 | clean | settings.rs 134-163, 184-190 |
| 18 | .../apps/Frontend Plane/verevonv3/src/features/dashboard/home/DashboardComposer.tsx:1100-1113 | Temporary chat gated on attested model | MATCH in worktree: comment 1097-1105; zeroRetentionAvailable 1106-1110; flatChatModels 1111-1113. DIFFERS-IN-LIVE (M, +7/-0): memo now at 1110-1114, flatChatModels 1115-1117; the added lines wire ChatWorkPeriodPicker and the memo body is unchanged | M | live C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/src/features/dashboard/home/DashboardComposer.tsx 1101-1117 |
| 19 | "chat-retention.ts local-cache purge policy" (no line) | consumer policy | MATCH: .../verevonv3/src/features/chat/lib/chat-retention.ts | clean | same |
| 20 | .../apps/Frontend Plane/verevonv3/apps/verevon-web/plans/system-audits/BUILD_PASS_6_8_2026-10-13.md:13,40,65 | Z-41b open findings; deploy order | LIVE-ONLY: absent in worktree; present untracked at C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/plans/system-audits/BUILD_PASS_6_8_2026-10-13.md. Lines 13 (Z-41b row), 40 (Z-41b items a-g), 65 (deploy order) MATCH. Also line 3 "Nothing was committed or deployed" and line 36 (running model-gateway is revision 2f2c3e83 from 2026-10-05, never exercised live) | ?? | live 3, 13, 36, 40, 65 |
| 21 | ZDR_OPERATOR_SEED_RUNBOOK.md (no path) | reuse; Z-60 and Z-40 | MATCH: .../apps/Frontend Plane/verevonv3/docs/ZDR_OPERATOR_SEED_RUNBOOK.md. Z-60 at 3 and 94; Z-40 at 127; sections 1-8 at 5, 21, 28, 53, 84, 92, 104, 122 | clean | same |
| 22 | zdr-seed-test-org.mjs (no path) | reuse | MATCH: .../apps/Frontend Plane/verevonv3/apps/gateway/scripts/zdr-seed-test-org.mjs (+ zdr-seed-test-org.test.mjs, run by pnpm test) | clean | same |
| 23 | opt-in Playwright gate spec (no path) | reuse | MATCH: .../apps/Frontend Plane/verevonv3/tests/e2e/zdr-org-switch-gate.spec.ts. Read-only by default (lines 9-17); PATCH only with ZDR_GATE_LIVE_PATCH=1 | clean | same |
| 24 | "waiver minting (Z-15) off"; Z-14; Z-20 (no path) | gating | Z-15 and Z-16 items appear in BUILD_PASS (live). Waiver route tests exist only live: C:/dev/CoresSystem/apps/Model Plane/rust/services/inference-core/src/grpc/zdr_waiver_route_tests.rs. Z-14 minter and Z-20 wording not located | ?? | not determined |

### F-t3-039 (card 885-895)

| # | Cited | Card claim | Worktree verdict | Live | Corrected lines |
|---|---|---|---|---|---|
| 25 | .../apps/Model Plane/scripts/release-artifact.sh:108-180 | build, verify, restore, deploy; signed root manifest; rollback-locator rules | PARTIAL DRIFT: 108-180 holds usage (110-158; env gates 131-141; rollback-locator rules 143-151) and validators (160-180). Subcommand bodies: verify_artifact 1358; build_artifact 1626; restore_artifact 1763; compose_artifact 1846-1873 (deploy args 1858); dispatch 1876-1909. Root manifest: generate 583, validate 595, sign 1593; verify_signature 1169 | clean | 110-158; 131-151; 1169; 1358; 1593; 1626; 1763; 1846-1909 |
| 26 | C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/scripts/coresystem-conformance.sh:100-125 | blocks on missing candidate or rollback evidence | MATCH: 106-121 (evidence grep at 109; absent checks 110-114). Ledger gate 123-134; threshold gate 147-167 | clean | 106-121; 123-134; 147-167 |
| 27 | .../apps/Model Plane/docs/MODEL_PLANE_RELEASE_EVIDENCE.md:1047 | candidate and rollback "absent" | MATCH: row 1047 "Release artifact v3 captures immutable inputs and partitions secrets ... absent | absent". Row 1049 "One provider deployment is independently ZDR-attested ... Release blocker" links to F-t3-005. Promotion requirements 1168-1186 (rollback rehearsal step at 1177); 1263 says rehearsal is operator-controlled | clean | 1047; 1049; 1168-1186 |
| 28 | thresholds TSV (no path) | "no canary values" | MATCH: .../apps/Model Plane/docs/CORESYSTEM_ACCEPTANCE_THRESHOLDS.tsv:7-15. Nine rows, all "proposed"; no canary, error-rate or p95 rows; row 15 zdr_residual_content_bytes (target 0, owner All owner planes) | clean | 7-15 |
| 29 | "migrations 0044 and 0045 have revert scripts" (no path) | rollback scripts exist | DIFFERS: no migrations/revert at HEAD (migrations end at 0043). Live-only: C:/dev/CoresSystem/apps/Model Plane/rust/services/session-core/migrations/revert/0044_run_retention_receipt.down.sql and .../revert/0045_zdr_waived_runs_org_index.down.sql | ?? | live paths |
| 30 | postmortem 0002 "publisher fix undeployed; 4,992 unreplayed" | deploy state | MATCH (row 4) | clean | n/a |
| 31 | "error-rate, p95 latency thresholds" | to add | ABSENT in TSV (row 28) | clean | n/a |
| 32 | "script a rehearsal"; "expand/contract policy for migrations without reverts" | to add | ABSENT: no rehearsal script (grep "rehears" hits only docs: MODEL_PLANE_RELEASE_EVIDENCE.md 1177, 1263 and core-research); no expand/contract text under apps/Model Plane or scripts | clean | n/a |
| 33 | "markdown-row grep in the gate" | replace with signed-manifest check | MATCH: coresystem-conformance.sh:109 (rg on "| Release artifact v3 captures immutable inputs") | clean | 109 |
| 34 | "Infra Plane for deploy" | deploy plane | apps/Infra Plane/ exists at C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Infra Plane; not inspected | clean | n/a |

## 2. Affected-file map

Role and lines are approximate. Tests are names read from source, not run.

### F-t3-004 (audit outbox, Phase 0, audit-core)

| Path | Role | Lines | Live | Tests (line) |
|---|---|---|---|---|
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Model Plane/rust/services/session-core/src/audit_publisher.rs | outbox drainer; wire-subject routing; status gauges; failure and terminal counters; requeue | 1945 | clean | durable_audit_outbox_migration_and_transaction_contract (796); audit_wire_subject_follows_the_payload_event_not_the_stored_subject (972); audit_bodies_and_subjects_match_the_committed_audit_core_wire_fixtures (1038); terminal_failures_are_counted_per_subject_and_logged_without_content (1302); requeue_migration_gives_terminal_rows_a_documented_way_back (1392); requeue_function_returns_only_terminal_unpublished_rows_of_the_subject (1422); outbox_drain_publishes_every_row_kind_through_a_provisioned_broker (1549); requeued_terminal_rows_are_delivered_once_the_cause_is_fixed (1685); publishes_tool_action_with_audit_core_shape (1904) |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Model Plane/rust/services/session-core/migrations/0043_audit_outbox_requeue.sql | requeue columns, function, REVOKE | 76 | clean | see audit_publisher.rs 1392, 1422 |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Model Plane/rust/services/session-core/migrations/0012_audit_outbox.sql | session_audit_outbox table and pending index | ~40 | clean | see audit_publisher.rs 796 |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Model Plane/rust/services/session-core/src/grpc.rs | enqueue_inline_tool_audit (HEAD 3597); run start | 12568 | M (+817/-44); enqueue now at 3739 live | not enumerated |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Model Plane/rust/services/session-core/tests/fixtures/audit-core-wire/tool_action_final.json and tool_action_reserved.json | wire fixtures pinned by session-core, decoded by audit-core | small | clean | audit_publisher.rs 1038; apps/Control Plane/audit-core/internal/subscriber/session_core_wire_fixtures_test.go |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Model Plane/go/services/nats-provisioner/audit_observability.go | VEREVON_CONTROL_OBSERVABILITY stream (const 42) and auditObservabilityStream (54) | 4355 B | clean (committed b9ed9197) | manifest_contract_test.go:142 TestManifestProvisionsTheAuditObservabilityStreamAndDurables |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Model Plane/go/services/nats-provisioner/main.go | main (231); provisionTopology (277) | 15962 B | clean | main_test.go 11, 97, 112, 219, 308 |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Model Plane/go/services/nats-provisioner/manifest_contract_test.go | real-broker publish and ack, drift and compose-gating contract | 32043 B | clean | 142, 171, 245, 347, 418, 448, 548, 565, 621, 672, 815 |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Model Plane/go/services/nats-provisioner/publishers_registry_test.go | publisher registry rows | 12917 B | clean | used by manifest_contract_test 171, 418 |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Model Plane/deploy/nats.conf | session-core-runtime publish grant verevon.audit.v2.model.session-core.> (45); audit-core-model consumer and ack grants (165-172); password variable AUDIT_MODEL_NATS_PASSWORD (166) | — | clean | manifest_contract_test 672, 815 |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Model Plane/deploy/docker-compose.yml | session-core block (712) depends_on nats-provisioner service_completed_successfully (797-799); cost-core block (1739) same at 1795-1797 | — | clean | manifest_contract_test 815 |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Control Plane/audit-core/internal/api/api.go | /v1 routes (109-116); listAudit (197-222); ingest POST /v1/audit | 344 | clean | api_test.go (fakes; 500 expectations at 81, 379, 399) |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Control Plane/audit-core/internal/api/service_auth.go | credential registry; serviceAuthorization (42); AllowsPlane (51); authorizeServiceRequest (225) | 400 | clean | service_auth_test.go 23, 46, 61, 91, 137 |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Control Plane/audit-core/internal/store/store.go | insertAudit (77); resolveAuditConflict (113); Purge (243); ListAudit (299, cap 300) | 490 | clean | store_test.go; session_core_replay_test.go:26 TestReplayedSessionCoreAuditEventIsStoredExactlyOnce (needs Postgres) |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Control Plane/audit-core/internal/store/schema.sql and migrations/002-006 | base tables; payload_hash BYTEA (schema 30-32, 63-65); unique indexes | — | clean | session_core_replay_test.go:116 checks three indexes; jetstream_inbox_contract_test.go |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Control Plane/audit-core/internal/subscriber/subscriber.go | handleAudit (168); handleUsage (214); eventAuthorityMatches (287); deadLetter (330) | — | clean | authority_test.go 5, 30, 59, 70, 85 |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Control Plane/audit-core/internal/events/events.go | DecodeAudit (152) | — | clean | — |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Control Plane/audit-core/internal/provisioner/provisioner.go | audit-core's own provisioner: StreamName (17); Optional bus (145); model-bus branch (253) | — | clean | — |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Control Plane/audit-core/cmd/server/main.go | AUDIT_RETENTION_DAYS (247); runRetention (157); 24h sweep (152) | — | clean | main_test.go |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Control Plane/docker-compose.yml | audit-extra-nats-provisioner service (791-797) | — | clean | — |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Control Plane/nats.conf | audit-core-control consumer grants on VEREVON_CONTROL_OBSERVABILITY (82-84) | — | clean | — |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Application Plane/nats.conf | audit-core-application consumer grants on the same stream (89-92); conflicts with postmortem row 268 | — | clean | — |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Control Plane/auth-core/src/auth/organization-events.plugin.ts | membership audit outbox; defect site line 829 (toISOString on dead-letter field) | — | clean | membership-audit-outbox.postgres.spec.ts:636 |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Frontend Plane/verevonv3/apps/gateway/src/domains/audit.rs | org-scoped audit proxy (30, 45, 52, 92) | 105 | clean | 0 Rust tests |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Frontend Plane/verevonv3/src/shared/api/audit-client.ts | GET /api/v1/audit client (105); zeroDataRetention; aggregateWorkspaceActivity | — | clean | audit-client.test.ts 14, 47, 57 |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Frontend Plane/verevonv3/src/features/settings/components/TrustCenterSection.tsx | Trust Center "Brukt av AI?" column (300) | — | clean | — |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/docs/compliance/evidence/trust-claims-pending-reverification.md | claim audit-trail-not-verified-end-to-end (text 101); section 6 "Item d" (97) | — | clean | — |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/docs/compliance/evidence/claims-vs-code.md | rows T4, T11b, H3, H7 reference the claim | — | M (16 lines) | — |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Model Plane/docs/postmortem/0002-session-audit-trail-never-delivered.md | root cause, deploy, replay, rollback, follow-ups | 291 | clean | — |
| C:/dev/CoresSystem/apps/Model Plane/rust/services/session-core/src/retention_receipt.rs (live only) | waived-start audit body (398-401); enqueue (483-495); tests 628-671 | 1735 | ?? | 628; 663 the_waived_start_row_is_routed_and_published_on_its_own_subject |
| C:/dev/CoresSystem/apps/Model Plane/rust/services/session-core/src/terminalization.rs (live) | waived-start audit call (674) | — | M (+108/-15) | — |
| C:/dev/CoresSystem/apps/Model Plane/rust/services/session-core/src/main.rs (live) | adds mod retention_receipt | — | M (+1) | — |

### F-t3-005 (ZDR attestation, org gate, live proof)

| Path | Role | Lines | Live | Tests (line) |
|---|---|---|---|---|
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Model Plane/rust/services/inference-core/src/provider/zdr.rs | attestation parse; validate (232); from_env (344); resolve (386); canonical digest (510) | 932 | clean | 559-914+: complete_and_matching_attestation_validates (560); placeholder_digest_is_rejected (605); a_lapsed_approval_stops_granting_zdr (856); canonical_digest_matches_an_independently_computed_vector (914) |
| C:/dev/CoresSystem/apps/Model Plane/rust/services/inference-core/src/auth.rs (live) | fail-closed JWT verification at inference boundary, with posture claims | 1250 | M (+402/-2) | not enumerated |
| C:/dev/CoresSystem/apps/Model Plane/rust/services/inference-core/src/grpc.rs (live) | gRPC handlers; waiver route binding (Z-16) | — | M (+129/-2) | not enumerated |
| C:/dev/CoresSystem/apps/Model Plane/rust/services/inference-core/src/grpc/zdr_waiver_route_tests.rs (live) | Z-16 route-binding tests | — | ?? | not enumerated |
| C:/dev/CoresSystem/apps/Model Plane/rust/services/inference-core/tests/zdr_refusal_inventory.rs (live) | refusal inventory | — | ?? | not enumerated |
| C:/dev/CoresSystem/apps/Model Plane/rust/crates/mp-contracts/src/retention.rs (live) | pure retention resolver used by model-gateway, inference-core, execution-core, session-core | 518 | ?? | tests/zdr_retention_matrix.rs (live, ??) |
| C:/dev/CoresSystem/apps/Model Plane/rust/crates/mp-contracts/src/zdr_refusal.rs (live) | refusal catalogue generator (MP_UPDATE_GOLDEN=1) | — | ?? | not enumerated |
| C:/dev/CoresSystem/apps/Model Plane/rust/crates/mp-contracts/src/dataplane_posture.rs and lib.rs (live) | posture contract | — | M (+67; +2) | — |
| C:/dev/CoresSystem/apps/Model Plane/api/contracts/zdr-refusals.json (+ .sha256) (live) | generated refusal catalogue; digest pinned by zdr-contract-sync.test.sh | — | ?? | scripts/tests/zdr-contract-sync.test.sh |
| C:/dev/CoresSystem/apps/Model Plane/rust/services/model-gateway/src/zdr_precheck.rs (live) | connection-bound ZDR precheck (a subscription connection cannot give ZDR) | 239 | M (+58/-226) | not enumerated |
| C:/dev/CoresSystem/apps/Model Plane/rust/services/model-gateway/src/zdr_posture.rs (live) | turn posture adapter over mp_contracts resolve_turn; typed refusals | 1259 | ?? | tests/zdr_posture_matrix.rs, zdr_feature_table.rs, zdr_refusal_inventory.rs (all live, ??) |
| C:/dev/CoresSystem/apps/Model Plane/rust/services/model-gateway/src/zdr_features.rs (live) | ZDR feature table | — | ?? | zdr_feature_table.rs (??) |
| C:/dev/CoresSystem/apps/Model Plane/rust/services/model-gateway/src/sse_events.rs (live) | adds retention-class variants Zdr and ZdrWaived | — | M (+202/-1) | — |
| C:/dev/CoresSystem/apps/Model Plane/rust/services/session-core/src/auth.rs (live) | fail-closed Auth Core verification at session-core gRPC boundary | 2095 | M (+396/-0) | — |
| C:/dev/CoresSystem/apps/Model Plane/rust/services/session-core/src/retention_receipt.rs (live) | run retention class zdr / persistent / zdr_waived; waived-start audit | 1735 | ?? | in-file tests 628-671 |
| C:/dev/CoresSystem/apps/Model Plane/rust/services/session-core/migrations/0044_run_retention_receipt.sql (live) | run receipt columns | — | ?? | — |
| C:/dev/CoresSystem/apps/Model Plane/rust/services/session-core/migrations/0045_zdr_waived_runs_org_index.sql (live) | org index for waived runs | — | ?? | — |
| C:/dev/CoresSystem/apps/Model Plane/rust/services/session-core/migrations/revert/0044_run_retention_receipt.down.sql and 0045_zdr_waived_runs_org_index.down.sql (live) | revert scripts | — | ?? | none |
| C:/dev/CoresSystem/apps/Model Plane/rust/services/session-core/src/run_service_grpc.rs (live) | run service | — | M (+10/-11) | — |
| C:/dev/CoresSystem/apps/Model Plane/proto/model_plane/v1/inference.proto | PrivacyTier PRIVACY_TIER_ZDR_CONTRACTUAL=3 (93); message InferRequest (100); zdr = 9 (126) | 513+ | clean | — |
| C:/dev/CoresSystem/apps/Model Plane/proto/model_plane/v1/sessions.proto (live) | adds SessionRetentionService (counts and dates only) | — | M (+42) | — |
| C:/dev/CoresSystem/apps/Model Plane/docs/ZDR.md | ZDR design and status table; Z-00 built on HTTP paths, not gRPC; section 5.10 Z-41b | — | M (+63/-1) | — |
| C:/dev/CoresSystem/apps/Model Plane/docs/sse-event-taxonomy.md (live) | SSE event taxonomy | — | M (+4) | — |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Model Plane/deploy/docker-compose.yml | ZDR env fields (927-951) | — | clean | — |
| C:/dev/CoresSystem/scripts/tests/zdr-contract-sync.test.sh (live) | refusal catalogue digest pin; cross-plane contracts | — | M (+93/-2) | itself; CI model-plane-rust-ci.yml:95-96 |
| C:/dev/CoresSystem/scripts/tests/zdr-contract-schema.mjs (live) | schema check | — | M (+12/-12) | zdr-contract-schema.test.mjs (clean; contract-goldens.yml:74) |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Frontend Plane/verevonv3/apps/gateway/src/domains/orgs/settings.rs | ZDR switch gate; 409; PATCH handler; catalogue helpers (36, 57, 79) | 1036 | clean | 556 test helper; 606 turning_zdr_on_is_refused_while_no_provider_is_attested_and_org_core_is_never_called; 636 turning_zdr_on_goes_through_once_a_chat_provider_is_attested |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Frontend Plane/verevonv3/apps/gateway/src/onboarding/actions/organization.rs | onboarding ZDR switch (settings.rs:180 comment) | — | clean | — |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Frontend Plane/verevonv3/apps/gateway/tests/fixtures/zdr-model-catalogue-cases.json | shared gate-rule fixture | — | clean | — |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Frontend Plane/verevonv3/apps/gateway/scripts/zdr-seed-test-org.mjs | operator seed for dedicated ZDR-TEST org | — | clean | zdr-seed-test-org.test.mjs (in pnpm test) |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Frontend Plane/verevonv3/docs/ZDR_OPERATOR_SEED_RUNBOOK.md | Z-60 live proof and Z-40 operator steps | — | clean | — |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Frontend Plane/verevonv3/tests/e2e/zdr-org-switch-gate.spec.ts | read-only Playwright gate; PATCH only with ZDR_GATE_LIVE_PATCH=1 | — | clean | itself |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Frontend Plane/verevonv3/tests/e2e/browser-workspace-zdr.spec.ts | Playwright ZDR workspace | — | clean | itself |
| C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/src/features/dashboard/home/DashboardComposer.tsx (live) | Temporary chat gate (hasZeroRetentionModel) | — | M (+7) | — |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Frontend Plane/verevonv3/src/features/chat/lib/chat-retention.ts | local-cache purge policy | — | clean | — |
| C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/src/shared/api/zdr-refusals.ts (live, + zdr-refusals.test.ts) | SPA copy of refusal catalogue | — | ?? | zdr-refusals.test.ts (??) |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Frontend Plane/verevonv3/src/shared/actions/action-registry.ts | org.update_zdr (2761); zdr boolean fields (1077, 1123) | — | clean | — |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Frontend Plane/verevonv3/src/features/settings/components/WorkspaceSettingsPage.zdr-gate.test.tsx | SPA gate test | — | clean | itself |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Frontend Plane/verevonv3/src/shared/i18n/errors.ts | zdr_no_attested_provider copy | — | clean | errors.test.ts |
| C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/plans/system-audits/BUILD_PASS_6_8_2026-10-13.md (live) | pass 6-8 results; Z-41b items; deploy order | — | ?? | — |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Model Plane/docs/MODEL_PLANE_STATUS.md | status register; no candidate or rollback (54, 150); no immutable rollback (241-250) | — | clean | — |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Model Plane/docs/MODEL_PLANE_RELEASE_EVIDENCE.md | register (1047-1049); promotion requirements (1168-1186) | 1311 | clean | — |

### F-t3-039 (release safety)

| Path | Role | Lines | Live | Tests (line) |
|---|---|---|---|---|
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Model Plane/scripts/release-artifact.sh | build, verify, restore, compose, deploy; root manifest; rollback locator; release-mode gates | 1917 | clean | release-contract-test.sh, release-runtime-partition-test.sh, release-input-security-test.sh (not in CI) |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Model Plane/scripts/tests/release-contract-test.sh, release-runtime-partition-test.sh, release-input-security-test.sh | release gate tests | — | clean | self |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/scripts/coresystem-conformance.sh | read-only conformance gate; blockers | 275 | clean | coresystem-conformance-artifact.sh (clean; persisted record) |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/scripts/coresystem-cross-plane-preflight.sh | cross-plane preflight referenced by conformance | — | clean | — |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Model Plane/scripts/tests/acceptance-thresholds-test.sh | validates TSV; prints approved or proposed state | — | clean | self |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Model Plane/docs/CORESYSTEM_ACCEPTANCE_THRESHOLDS.tsv | nine threshold rows, all proposed | 16 | clean | acceptance-thresholds-test.sh |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Model Plane/docs/CAPABILITY_PROMOTION_LEDGER.tsv | capability states (source_only rows) | — | clean | scripts/tests/capability-promotion-ledger-test.sh |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Model Plane/docs/MODEL_PLANE_RELEASE_EVIDENCE.md | release register; promotion requirements; rollback rehearsal | 1311 | clean | — |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Model Plane/docs/MODEL_PLANE_STATUS.md | status: no candidate or rollback (54, 150); managed-run terminalization not release-proven (296-314) | — | clean | — |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Model Plane/scripts/compose.sh | compose wrapper; release-mode gates | — | clean | — |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Model Plane/scripts/verify-durable-layer.sh | durable-layer gate named in the evidence register | — | clean | not read |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Model Plane/rust/services/session-core/migrations/0043_audit_outbox_requeue.sql | requeue (also in F-t3-004 map) | 76 | clean | — |
| C:/dev/CoresSystem/apps/Model Plane/rust/services/session-core/migrations/revert/*.down.sql (live) | revert scripts for 0044 and 0045 | — | ?? | none |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Model Plane/docs/postmortem/0002-session-audit-trail-never-delivered.md | deploy steps 138-163; replay 165-244; rollback 246-259 | 291 | clean | — |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/.github/workflows/model-plane-rust-ci.yml | model-plane CI; live adds zdr_receipt --ignored step (+50 lines) | — | M (+50) | no release-* step at HEAD |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/.github/workflows/contract-goldens.yml | runs zdr-contract-sync and zdr-contract-schema tests | — | clean | itself |
| C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Infra Plane/ | deploy plane named by the card; directory exists | — | clean | not inspected |

## 3. Related contracts

- Protos: apps/Model Plane/proto/model_plane/v1/inference.proto (PrivacyTier at 93; InferRequest zdr = 9 at 126); sessions.proto (live only: SessionRetentionService, +42). events.proto, execution.proto, gateway.proto, capabilities.proto, browser.proto and orchestration.proto mention zdr or audit in grep hits; not read.
- SQL, session-core migrations (apps/Model Plane/rust/services/session-core/migrations/): 0001 through 0043 committed; latest committed is 0043_audit_outbox_requeue.sql. Live adds 0044_run_retention_receipt.sql and 0045_zdr_waived_runs_org_index.sql (untracked) plus revert/0044_run_retention_receipt.down.sql and revert/0045_zdr_waived_runs_org_index.down.sql.
- SQL, audit-core (apps/Control Plane/audit-core/internal/store/migrations/): 002_jetstream_inbox.sql, 003_logical_control_audit_idempotency.sql, 004_logical_usage_idempotency.sql, 005_usage_source_identity.sql, 006_v2_producer_authority.sql (latest; clean). Base DDL in internal/store/schema.sql.
- Env, ZDR (docker-compose.yml 936-951): AZURE_OPENAI_ZDR_{CONFIRMED,RESOURCE_ID,APPROVAL_REF,EFFECTIVE_DATE,REVIEW_BY,REVIEWER,DIGEST} and AZURE_ANTHROPIC_ZDR_* with the same suffixes. Digest recipe in comments at zdr.rs:36-43 and compose 933-935 (printf of five fields, sha256sum).
- Env, release (release-artifact.sh 131-141): MODEL_PLANE_RELEASE_MODE; MODEL_PLANE_ARTIFACT_SIGNING_KEY (build); MODEL_PLANE_ARTIFACT_VERIFY_KEY (verify, deploy); MODEL_PLANE_ROLLBACK_ARTIFACT_DIR; MODEL_PLANE_ROLLBACK_ARTIFACT_VERIFY_KEY (optional); MODEL_PLANE_ROLLBACK_RUNTIME_ENV_FILE; MODEL_PLANE_ROLLBACK_RUNTIME_EVIDENCE_FILE; MODEL_PLANE_COMPATIBILITY_GATES_FILE; MODEL_PLANE_RUNTIME_PUBLIC_POLICY_FILE; MODEL_PLANE_RUNTIME_ENV_FILE (compose and deploy).
- Release gate content (release-artifact.sh 143-151): compatibility gates must attest protocol, migration, live authorization, approval-continuation and a separately accepted rollback artifact. ZDR_RETENTION_PATH must be zdr-provider-route-attested or authoritative-non-zdr-policy-attested, with a non-secret SHA-256 evidence digest. The rollback locator must not be copied into the candidate.
- Env, audit-core (cmd/server/main.go): DATABASE_URL (203), NATS_URL (207), NATS_USER / NATS_PASSWORD / NATS_TOKEN (216-218), AUDIT_ALLOW_NATS_TOKEN_FALLBACK (214), HTTP_PORT (225), METRICS_PORT (232), AUDIT_CORE_SERVICE_CREDENTIALS (238), AUDIT_RETENTION_DAYS (247).
- Session-core constants (audit_publisher.rs): MAX_AUDIT_DISPATCH_ATTEMPTS = 20 (382); AUDIT_SUBJECT_PREFIX = verevon.audit.v2.model.session-core. (43); SUBJECT_MODEL_TOOL_ACTION = verevon.audit.v2.model.session-core.tool_action (34); SUBJECT_CLASSES (608). Reserved event subject verevon.audit.v2.model.session-core.tool_action_reserved (postmortem 201).
- NATS stream and durables: VEREVON_CONTROL_OBSERVABILITY (audit_observability.go:42); durables audit-core-model-v3-audit and audit-core-model-v3-usage (postmortem 90; nats.conf 169-170). Live-only waived subject verevon.audit.v2.model.session-core.run_started_zdr_waived (retention_receipt.rs:34, 398-401).
- SSE (apps/Model Plane/rust/services/model-gateway/src/sse_events.rs): privacy-tier string zdr_contractual (line 77 at HEAD). Live adds retention-class variants Zdr and ZdrWaived (+202/-1). No audit SSE event exists.
- Gateway routes: GET /api/v1/audit (apps/Frontend Plane/verevonv3/apps/gateway/src/domains/audit.rs:30). PATCH /api/v1/orgs/:id/settings (settings.rs:165-190). audit-core GET and POST /v1/audit, GET and POST /v1/usage (api.go:111-115), internal auth.
- Action contracts: apps/Frontend Plane/verevonv3/src/shared/actions/action-registry.ts: org.update_zdr (2761); zdr: z.boolean().optional() (1077, 1123).
- Refusal and posture catalogues (live-only): apps/Model Plane/api/contracts/zdr-refusals.json (+ .sha256); rust/services/model-gateway/tests/fixtures/zdr-posture-matrix.json (+ .sha256); pinned by scripts/tests/zdr-contract-sync.test.sh (digest 9c2b5458...).
- Threshold contract: apps/Model Plane/docs/CORESYSTEM_ACCEPTANCE_THRESHOLDS.tsv (header lines 3-5: values are proposals until Release/Infra and the owning plane approve; a conformance run stays blocked while proposed).

## 4. Test and build commands

Rust (crate names from Cargo.toml; run from C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Model Plane/rust):
- session-core (apps/Model Plane/rust/services/session-core/Cargo.toml:2): cargo test -q -p session-core --all-targets (command from apps/Model Plane/CLAUDE.md "Testing"). Filter: cargo test -p session-core audit_publisher.
- inference-core (.../inference-core/Cargo.toml:2; lib target inference_core at :54): cargo test -q -p inference-core --all-targets.
- model-gateway (.../model-gateway/Cargo.toml:2): cargo test -q -p model-gateway --all-targets.
- execution-core (.../execution-core/Cargo.toml:2): cargo test -q -p execution-core --all-targets.
- mp-contracts (apps/Model Plane/rust/crates/mp-contracts/Cargo.toml:2): cargo test -q -p mp-contracts --all-targets. Golden refresh writes files (dev only): MP_UPDATE_GOLDEN=1 cargo test -p mp-contracts zdr_refusal (named in zdr-contract-sync.test.sh header, live).
- Live-only CI step (model-plane-rust-ci.yml, live): cargo test -p session-core --bin session-core zdr_receipt -- --ignored --test-threads=1 (needs DB).
- Workspace: cd rust && cargo test --workspace (apps/Model Plane/CLAUDE.md).
- Windows: user memory notes two cargo targets fail pre-existing on Windows; use --lib.
- Gateway (apps/Frontend Plane/verevonv3/apps/gateway/Cargo.toml:2, package verevon-gateway-rs): cargo test --manifest-path apps/gateway/Cargo.toml --all-targets, run from apps/Frontend Plane/verevonv3 (root CLAUDE.md).

Go (module names from go.mod):
- audit-core (apps/Control Plane/audit-core/go.mod:1, module github.com/triodelab/controlplane/audit-core): cd apps/Control Plane/audit-core && go test ./... && go vet ./... Postgres-backed tests (session_core_replay_test.go) need a database; env not verified.
- nats-provisioner (apps/Model Plane/go/services/nats-provisioner/go.mod:1, module github.com/triodelab/model-plane/services/nats-provisioner): cd apps/Model Plane/go/services/nats-provisioner && go test ./... (manifest_contract_test.go starts an in-process broker from deploy/nats.conf; postmortem 114-127).
- User memory: no go.work; build per service; -race needs cgo (unavailable on this machine).

Node and TypeScript (apps/Frontend Plane/verevonv3/package.json):
- pnpm typecheck (line 17, tsc -b); pnpm lint (line 11, eslint .); pnpm test (line 13: node --test on four gateway scripts including zdr-seed-test-org.test.mjs, then vitest run); pnpm build (line 9); pnpm test:e2e (line 15, playwright test; config apps/Frontend Plane/verevonv3/playwright.config.ts).
- Targeted: pnpm vitest run src/shared/api/audit-client.test.ts src/features/settings/components/WorkspaceSettingsPage.zdr-gate.test.tsx src/shared/i18n/errors.test.ts.
- Read-only ZDR gate: pnpm test:e2e tests/e2e/zdr-org-switch-gate.spec.ts with ZDR_GATE_LIVE_PATCH unset.

Shell and contract gates:
- bash scripts/tests/zdr-contract-sync.test.sh and bash scripts/tests/zdr-contract-sync.test.sh --self-test (model-plane-rust-ci.yml:95-96 at HEAD).
- node --test scripts/tests/zdr-contract-schema.test.mjs (contract-goldens.yml:74).
- bash apps/Model Plane/scripts/tests/release-contract-test.sh; release-runtime-partition-test.sh; release-input-security-test.sh (evidence row 1047 says these pass; not wired to CI).
- bash apps/Model Plane/scripts/tests/acceptance-thresholds-test.sh.
- bash scripts/coresystem-conformance.sh [--json] (read-only; exit 2 when blocked; header lines 1-30). Persisted record: bash scripts/coresystem-conformance-artifact.sh OUTPUT_PATH.
- Release, not for this sweep: apps/Model Plane/scripts/release-artifact.sh dry-run | build | verify | restore | compose <dir> config | deploy <dir> (usage 110-158). Card rule: never hand-roll a deploy that bypasses it (apps/Model Plane/CLAUDE.md).
- Compose: cd apps/Model Plane && ./scripts/compose.sh up -d (apps/Model Plane/CLAUDE.md). Control Plane: apps/Control Plane/scripts/run-control-plane.sh (user memory: bare docker compose build exits 0 without building).

## 5. Owners

- audit-core (ingest, store, retention, service auth): Control Plane. Rule: root CLAUDE.md "Control Plane owns identity, users, orgs, billing, sessions, audit, quotas, and entitlements"; master-ownership-matrix.md line 29 "Control owns identity, authorization, canonical audit".
- session-core outbox, drainer, requeue, retention receipts: Model Plane. Rule: root CLAUDE.md "Model Plane owns reasoning, sessions/runs, inference..."; capability-ownership-matrix.md line 3 ("exactly one canonical owner") and line 53 (thread timeline and outbox owned by session-core).
- Cross-plane delivery: Model Plane publishes only through its outbox and NATS and never touches audit-core's database (card line 398; root CLAUDE.md "No direct database crossing between planes").
- Stream and durable provisioning: model-bus stream owned by Model Plane nats-provisioner since b9ed9197; the Control Plane audit-extra provisioner (apps/Control Plane/docker-compose.yml:791-797) still lists the model bus (postmortem 282-285, follow-up 1). One owner must be chosen.
- Content-free audit rows: root CLAUDE.md "Zero Data Retention must propagate through any content-persisting boundary"; card line 398 ("rows stay content-free").
- Gateway audit proxy and Trust Center UI: Frontend Plane (root CLAUDE.md "Frontend Plane owns Verevon v3 UI plus same-origin gateway/BFF normalization").
- ZDR attestation config and resolver: Model Plane inference-core (card line 416: "Attestation is Model Plane config"). mp-contracts resolver is shared across model-gateway, inference-core, execution-core and session-core (retention.rs header, live).
- ZDR org-switch gate: Frontend Plane gateway. settings.rs:176-178 says this gateway is "the only holder of org-core's org:settings:write:self, and only it can read the Model Plane catalogue". org-core stays ungated by design (settings.rs:181-182).
- Live proof and provider attestation: operator (runbook sections 4 and 7; card line 408).
- Release tooling: Model Plane (card line 886). Deploy plane: Infra Plane (card; directory exists; not in CLAUDE.md six-plane focus list).
- Threshold approval: owner planes and Release/Infra (TSV header lines 3-5). Migration expand/contract policy: session-core owner (card line 892; no policy exists).

## 6. Uncommitted-only (live tree; numbers from git --no-optional-locks diff in C:/dev/CoresSystem)

Modified (M):
- apps/Model Plane/rust/services/session-core/src/grpc.rs: +817/-44. Run-start ZDR paths including the waived durable audit (3218-3226). enqueue_inline_tool_audit moved from 3597 (HEAD) to 3739.
- apps/Model Plane/rust/services/session-core/src/terminalization.rs: +108/-15. Waived-start audit call at 674.
- apps/Model Plane/rust/services/session-core/src/main.rs: +1 (mod retention_receipt).
- apps/Model Plane/rust/services/session-core/src/auth.rs: +396/-0. Auth Core verification (header only read).
- apps/Model Plane/rust/services/session-core/src/run_service_grpc.rs: +10/-11. Not characterised.
- apps/Model Plane/rust/services/inference-core/src/auth.rs: +402/-2. JWT verification at the inference boundary with posture claims.
- apps/Model Plane/rust/services/inference-core/src/grpc.rs: +129/-2. Waiver route binding (Z-16).
- apps/Model Plane/rust/services/model-gateway/src/zdr_precheck.rs: +58/-226. Rewritten connection-bound ZDR precheck.
- apps/Model Plane/rust/services/model-gateway/src/sse_events.rs: +202/-1. Adds Zdr and ZdrWaived retention classes.
- apps/Model Plane/rust/crates/mp-contracts/src/dataplane_posture.rs: 67 lines changed. lib.rs: +2.
- apps/Model Plane/docs/ZDR.md: +63/-1. Status table and section 5.10 (Z-41b).
- apps/Model Plane/docs/sse-event-taxonomy.md: +4.
- apps/Model Plane/proto/model_plane/v1/sessions.proto: +42. Adds SessionRetentionService.
- apps/Model Plane/api/contracts/README.md: +1.
- scripts/tests/zdr-contract-sync.test.sh: +93/-2. Refusal catalogue digest pin.
- scripts/tests/zdr-contract-schema.mjs: +12/-12.
- .github/workflows/model-plane-rust-ci.yml: +50. Adds the zdr_receipt --ignored step.
- docs/compliance/evidence/claims-vs-code.md (16 lines), honest-limits.md (12), retention-and-erasure-evidence.md (24), data-flow-map.md, subprocessors-from-code.md, tools/doc-codes.json: modified; compliance evidence, not diffed.
- apps/Frontend Plane/verevonv3/src/features/dashboard/home/DashboardComposer.tsx: +7/-0. Wires ChatWorkPeriodPicker; the ZDR memo body is unchanged but shifted +4 lines.

Untracked (??), live only:
- apps/Model Plane/api/contracts/zdr-refusals.json (+ .sha256)
- apps/Model Plane/rust/crates/mp-contracts/src/retention.rs, zdr_refusal.rs, tests/zdr_retention_matrix.rs, tests/fixtures/
- apps/Model Plane/rust/services/execution-core/tests/zdr_refusal_inventory.rs
- apps/Model Plane/rust/services/inference-core/src/grpc/zdr_waiver_route_tests.rs; inference-core/tests/zdr_refusal_inventory.rs
- apps/Model Plane/rust/services/model-gateway/src/zdr_features.rs, zdr_posture.rs; tests/zdr_feature_table.rs, zdr_posture_matrix.rs, zdr_refusal_inventory.rs; tests/fixtures/zdr-posture-matrix.json (+ .sha256); tests/fixtures/auth-core/
- apps/Model Plane/rust/services/session-core/src/retention_receipt.rs
- apps/Model Plane/rust/services/session-core/migrations/0044_run_retention_receipt.sql, 0045_zdr_waived_runs_org_index.sql, revert/ (two .down.sql)
- apps/Frontend Plane/verevonv3/src/shared/api/zdr-refusals.ts and zdr-refusals.test.ts, http.zdr-refusal.test.ts, chat-client.zdr-refusal.test.ts, src/shared/i18n/errors.zdr-refusal.test.ts, src/features/chat/components/chat-normalizers.zdr-refusal.test.ts, use-chat-controller.zdr-refusal.test.ts, src/shared/api/contracts/
- apps/Frontend Plane/verevonv3/apps/verevon-web/plans/system-audits/BUILD_PASS_1 through BUILD_PASS_6_8_2026-10-13.md (6_8 is the card's cited doc)
- Out of scope but uncommitted: apps/Ingestion Plane/integration-corev2 zdr files; apps/Ingestion Plane/Quarry-v2 zdr_claim_floor.rs (M).

Deleted (D): none relevant to these items.

## 7. Not determined

1. Whether b9ed9197 and migration 0043 are deployed anywhere, and whether the 4,992 rows are still unreplayed (postmortem 4-7; no live reads possible from the repo).
2. Whether the re-arm row still exists. Postmortem 204-205 says it is purged 7 days after publish (2026-10-12).
3. Which two "other broken audit paths" the card means. Candidates from postmortem 266-273: row 268 (app-nats stream absent per postmortem, but apps/Application Plane/nats.conf:89-92 grants consumers on that stream; config and state conflict); row 269 (auth-core membership outbox; defect at organization-events.plugin.ts:829); row 272 (publish grants for mp.v1.security.* dropped by the broker; audit-adjacent); row 273 (6 managed_run_terminalization rows).
4. Whether audit-core accepts run_started_zdr_waived in practice. No allowlist was found; acceptance follows eventAuthorityMatches (subscriber.go:287). Not tested.
5. Whether the live session-core, inference-core and model-gateway changes compile or pass. Nothing was built or run.
6. Whether any alert backend (Prometheus or Alertmanager) for Model Plane exists outside the repo.
7. Whether any provider attestation exists (Z-40), whether the ZDR-TEST org exists, and whether the Z-60 live proof has run. Repo evidence says no (BUILD_PASS line 54-61; evidence 1049).
8. Whether the signing keys and the rollback artifact directories exist on the operator's machine (release-artifact.sh 133-137).
9. Threshold values beyond the nine proposed rows. The TSV has no canary, error-rate or p95 rows; owners and values are not in the repo.
10. Location of Z-14 (org_zdr minter), Z-15 waiver minter and Z-20 wording. Not located in HEAD or live files read.
11. Location of the trust-claims.ts file referenced by trust-claims-pending-reverification.md:101. Not located.
12. Whether apps/Infra Plane contains deploy scripts or an owner rule. Not inspected.
13. Whether the zdr-refusals.json digest in zdr-contract-sync.test.sh matches the live file. Not computed.
14. Whether the proto files events.proto, execution.proto, gateway.proto, capabilities.proto, browser.proto and orchestration.proto change the audit or ZDR contract. Only grep hits were seen.
15. Whether scripts/coresystem-cross-plane-preflight.sh and scripts/coresystem-conformance-artifact.sh produce valid records. Not run.
16. Whether the gateway's ZDR catalogue route and the onboarding ZDR switch (apps/Frontend Plane/verevonv3/apps/gateway/src/onboarding/actions/organization.rs) match the gate. Not read beyond the settings.rs comments.
17. Test results. All test names above were read from source, not executed.