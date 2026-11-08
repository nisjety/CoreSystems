# WS06 audit tamper-evidence: code-map sweep (F-t3-004)

Scope: WS06-audit-tamper-evidence (audit hash chain, compliance read API, SIEM export). Item F-t3-004, the AFTER-phase-0 part. Phase 0 belongs to WS01.
Date: 2026-10-14. Read-only sweep; this file is the only output.
Trees: worktree `C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b` at HEAD afb83928 (clean). Live tree `C:/dev/CoresSystem` at the same HEAD; `.plan-work/live-tree-status.txt` has 325 entries.
Audit doc (read-only): `C:/Users/ImaFernandesDaCosta/AppData/Roaming/Claude/scratch-workspaces/5e23d905-dd91-4518-8ff4-252efcbe0520/b5ff8b3a-437c-4499-9a7f-469de994f319/scratch-2026-10-12-69a4ee/03-model-plane-chat-and-agents-audit.md`
Legend: MATCH = behaviour present where cited. DRIFT = present at other lines (new range given). ABSENT = not found. Live: clean = no status line; M = modified; ?? = untracked (live only). Line numbers are worktree lines unless marked "live".

## 1. Card and audit-doc context

- Card F-t3-004 at audit doc lines 385-401 (anchor 384, next anchor 402). Tier Priority; Verevon today Partial; Effort L; Plane Model Plane (session-core outbox) + Control Plane (audit-core, compliance API); Wave W1; Confidence Medium.
- Phase 0 (ops closure): deploy session-core publisher fix and requeue migration (postmortem 0002); canary-replay 4,992 stuck governed-action events; add alert rules on the terminal-row gauge; schedule "the two other broken audit paths the postmortem lists".
- After phase 0: per-org hash chain with retention checkpoints and a verifier in audit-core's insert path; scoped compliance read API with time-range export; SIEM fan-out consumer configured through org-core.
- Other audit-doc locations: 9.1 wave row line 1537 (W1, "L (was M)", deps: nats-provisioner stream manifest, F-t3-039). Appendix B row 1822. Appendix C 1947-1955 (reference-side: Claude's HMAC audit.jsonl is local and same-user forgeable; no reference hash-chains a hosted store; the combination is a Verevon synthesis). Appendix D line 2121 (M to L). 9.4 dependency line 1704 (F-t3-039 is a dependency of F-t3-004; F-t3-004 is a prerequisite of F-t3-062 and F-t3-068). Other dependants: F-t3-062 (1602), F-t3-075 (1636), F-t3-096 (1657). Risk and open question: 1718, 1747 (whether the audit outbox fix is deployed is an operator fact to check live).

## 2. Card citations: worktree and live verdicts

| # | Card id | Cited path:lines | Claim | Worktree verdict | Live | Corrected path:lines |
|---|---|---|---|---|---|---|
| 1 | F-t3-004 | apps/Model Plane/rust/services/session-core/src/audit_publisher.rs:432-480 | "producer fixes are in the repo" | DRIFT. 432-489 is the status-gauge code (MATCH for the gauges). The subject/event producer fix is at 135-179 and 298-340. | clean | 432-489 (gauges); 135-179 (event name, wire subject); 298-340 (body, publish) |
| 2 | F-t3-004 | migrations/0043_audit_outbox_requeue.sql:1-60 (resolved: apps/Model Plane/rust/services/session-core/migrations/0043_audit_outbox_requeue.sql) | requeue migration for terminal rows | MATCH. 75 lines; ALTER 28-31; function 33-73. REVOKE at 75 is outside the cited range. | clean | 1-75 |
| 3 | F-t3-004 | commit b9ed9197 | the fix commit | MATCH. `git cat-file -t` = commit; subject "fix(model-plane): deliver session audit events to audit-core" (2026-10-08); reachable from HEAD. Touches audit_publisher.rs (+1158), grpc.rs (+430), 0043, nats-provisioner audit_observability.go, deploy/docker-compose.yml, deploy/nats.conf, postmortem 0002. | n/a | n/a |
| 4 | F-t3-004 | apps/Model Plane/docs/postmortem/0002-session-audit-trail-never-delivered.md:98-117 | publisher fix built, not deployed; rows not replayed | MATCH. 98-112 publisher, requeue and metrics bullets; 114-117 test list. "Not deployed" stated at 4-7 and 93; replay not done at 6-7. | clean | 93-117 (publisher side, tests); status 3-7; replay 165-244; follow-ups 282-291 |
| 5 | F-t3-004 | apps/Control Plane/audit-core/internal/api/api.go:106-116 | route mount, internal auth | MATCH (mount block 105-117) | clean | 105-117 |
| 6 | F-t3-004 | apps/Control Plane/audit-core/internal/api/api.go:197-225 | internal GET /v1/audit, row cap | DRIFT (minor). listAudit is 197-222; 223-225 is blank and the next handler. | clean | 197-222 |
| 7 | F-t3-004 | apps/Control Plane/audit-core/internal/store/store.go:85-140 | per-row sha256 payload_hash for replay idempotency | MATCH. payload_hash = sha256(json.Marshal(ev)) at 88-92; INSERT ... ON CONFLICT DO NOTHING at 94-106; conflict check at 113-139. No chain column exists. | clean | 64-139 (InsertAuditFromStream and conflict resolver) |
| 8 | F-t3-004 | store.go:225-325 | hard-delete Purge; list cap | MATCH. Purge 243-269 (DELETE by ingested_at at 250-253 and 259-262). ListAudit 299-345. Cap nuance: a limit <=0 or >500 is reset to 100 (300-302), not clamped to 500. | clean | 243-345 |
| 9 | F-t3-004 | "gateway audit.rs org-scoped proxy" (no line cited): apps/Frontend Plane/verevonv3/apps/gateway/src/domains/audit.rs | org-scoped read proxy | MATCH. Org comes from the session (50); no client org parameter (34-35); proxy call 92-103. | clean | 1-105 |

Live check: `git --no-optional-locks status` for each path above shows no status line. audit-core, org-core, gateway, nats-provisioner, the postmortem and migration 0043 are clean. The session-core migrations directory shows only ?? entries (0044, 0045, revert/).

## 3. Card claims without a line citation

| Claim (card text) | Verdict | Evidence |
|---|---|---|
| "no alert-rule file exists" | MATCH | Grep of worktree *.yml/*.yaml for `alert:`, `rule_files`, `groups:` finds only apps/Data Plane v2/infra/prometheus/alerts.yml, prometheus.yml and .github/dependabot.yml. Postmortem 0002:286-288 and trust-claims doc:112 say the same. |
| terminal-row gauge exists | MATCH | audit_publisher.rs:444-445 (pending, terminal); 474 (terminal_by_subject{subject_class}); counters at 752, 759. |
| "reuse the unique indexes on (source_bus, source_producer, event_id)" | MATCH | apps/Control Plane/audit-core/internal/store/migrations/006_v2_producer_authority.sql:18-20 (partial unique index). Related: 003 line 16 (source_subject index), 002 line 11 (jetstream inbox index). |
| "service-credential model with Principal, Scopes and Planes" | MATCH | apps/Control Plane/audit-core/internal/api/service_auth.go:34-44 (serviceCredential: Principal 35, Scopes 38, Planes 39; serviceAuthorization 42-51). Scope validation 103-120. |
| "4,992 stuck governed-action events" | MATCH | postmortem 0002:22-24 (2,515 tool_action + 2,477 reservations); 0043:5. |
| "events arrive from several buses" | MATCH | subscriber.go:27 (auditSubject verevon.audit.v2.>); extra buses via AUDIT_EXTRA_NATS_BUSES (apps/Control Plane/docker-compose.yml:826); HTTP ingest POST /v1/audit (api.go:112, 278-316). |
| "per-org ordering needs a serialised insert path" | ABSENT (no serialisation exists) | store.go:94-106 is one INSERT ... ON CONFLICT DO NOTHING. Grep of audit-core for pg_advisory, FOR UPDATE, advisory, SERIALIZABLE: no store matches (Go mutexes only in cmd/server/extra_nats.go:223-287 and api/service_auth.go:375-388). |
| "hash chain with retention checkpoints / signed head per pruned segment" | ABSENT | No prev_hash or chain column in schema.sql:11-33 or migrations 002-006. Signature or HMAC code in audit-core is only in service_auth.go (hmac.Equal at 200; verifyAuditReadDelegation at 283); none signs audit rows or heads. |
| "compliance query API with time-range export" | ABSENT as export; list exists | GET /v1/audit with since/until (api.go:204-211); store filters on ingested_at (store.go:319-320), limit <=500. Gateway GET /api/v1/audit (audit.rs:30). No export route (api.go:109-116 lists only /audit, /usage, /usage/summary). |
| "export is a new egress path that must be declared" | ABSENT (no declared-egress file found) | Grep of docs/ for "egress catalogue", "catalog", "registry", "declared egress", "inventory": no match. |
| "SIEM fan-out consumer configured through org-core" | ABSENT | git grep -i siem over tracked files: docs only (docs/evaluations/AUDIT_PLATFORM_SOC2.md:414, 429; apps/Model Plane/MODEL_PLANE_ROADMAP.md:417; apps/Infra Plane/docs/SECURITY.md) plus an eval corpus. Org-core grep for webhook/siem/export/fan-out matches GDPR code only. |
| "Model Plane only delivers via outbox and NATS, never touches audit-core's DB" | MATCH | audit_publisher.rs:345-347 NatsAuditPublisher holds a JetStream context only. Grep of apps/Model Plane (non-docs) for audit_events, AUDIT_CORE, audit-core: 12 files matched. Read: deploy/nats.conf (grants), deploy/docker-compose.yml (env), model-gateway auth.rs:1290 and gdpr_nats.rs:28 (comments). cost-core consumer files matched by name only and were not read. No SQL against audit_events seen. |
| "Phase 0 is S-sized, needs no new code" | MATCH for code; deploy is operator work | Publisher, requeue and metrics are built and tested at HEAD (postmortem 0002:93-112). Deploy and replay steps at 138-244. |

## 4. Affected-file map

Tests are listed by name where cheap. "Live" = clean, M or ?? (live-only lines are marked).

### 4a. Model Plane producer (session-core, Rust crate `session-core`)

| Path | Role | Approx lines | Live | Existing tests |
|---|---|---|---|---|
| apps/Model Plane/rust/services/session-core/src/audit_publisher.rs | outbox drainer; event naming; wire subject; body builder; status gauges; published-row purge; requeue function tests | consts 34 (SUBJECT_MODEL_TOOL_ACTION), 37, 40, 43 (AUDIT_SUBJECT_PREFIX); event id 125; event name 135; subject 148-155; resolve_wire_subject 165-179; body 298-329; publish_tool_action 334-340; NatsAuditPublisher 345; MAX attempts 382 (=20); start loop 403-430 (dispatch 408-417; status gauges 418-420; purge tick 421-426); gauges 432-489; purge 492-500 (published >7 days, 500 per tick); SUBJECT_CLASSES 608-614; outbox_subject_class 624-631 | clean | Unit: durable_audit_outbox_migration_and_transaction_contract (796), audit_wire_subject_follows_the_payload_event_not_the_stored_subject (972), audit_bodies_and_subjects_match_the_committed_audit_core_wire_fixtures (1038), outbox_subject_classes_are_bounded_and_carry_no_tenant_token (1098), publish_failures_are_classified_into_bounded_reasons (1131), a_failure_is_terminal_exactly_at_the_attempt_budget (1155), terminal_failures_are_counted_per_subject_and_logged_without_content (1302), requeue_migration_gives_terminal_rows_a_documented_way_back (1392), tool_action_event_name_is_phase_specific (1870), publishes_tool_action_with_audit_core_shape (1904). Ignored (need DATABASE_URL, some also NATS): requeue_function_returns_only_terminal_unpublished_rows_of_the_subject (1421-1422), outbox_drain_publishes_every_row_kind_through_a_provisioned_broker (1548-1549), requeued_terminal_rows_are_delivered_once_the_cause_is_fixed (1684-1685). |
| apps/Model Plane/rust/services/session-core/src/grpc.rs | writes tool-action outbox rows and intents; enqueue_inline_tool_audit; AuditOutbox wiring | enqueue_inline_tool_audit 3597-3648 (subject from audit_subject_for_event at 3632-3635; INSERT ... ON CONFLICT (event_id) at 3636-3639); enqueue_tool_action_audit 3522-3569 (complete-step path; INSERT at 3557 binds the constant SUBJECT_MODEL_TOOL_ACTION; the event-derived subject is used only by enqueue_inline_tool_audit at 3632-3635); reserve_tool_action_inner 3650 (intents INSERT 3666); finalize_tool_action_inner 3732 (intents UPDATE 3762); enqueue calls 3707, 3783; AuditOutbox::new 7148; gRPC impls reserve_tool_action 4667, finalize_tool_action 4684; thread-scoped DELETE on session_audit_outbox 385 and session_tool_audit_intents 485 (context not read) | M: 861 changed lines. Live adds a waived-run start audit enqueue at live 3219-3227. Only one audit-matching comment line in the live diff. | inline_tool_audit_reservation_and_finalization_are_idempotent_in_postgres (11228; ignore attribute not read); outbox read helpers 11479-11530 |
| apps/Model Plane/rust/services/session-core/src/learning_events.rs | learning-event rows also written to session_audit_outbox | INSERT 278; module doc 14-17 | M (77 lines; retention-gate wording and test strings) | learning_events.rs:519 (asserts INSERT ordering) |
| apps/Model Plane/rust/services/session-core/src/gdpr.rs | erasure deletes outbox rows by payload org_id | summary fields 163, 204; DELETE 431-432 | clean | gdpr.rs:628 (summary fixture) |
| apps/Model Plane/rust/services/session-core/src/gdpr_nats.rs | cross-plane GDPR consumer; durable pull consumer on AQENCIA_CONTROLPLANE not yet provisioned (owner: audit-core provisioner) | doc 18-33 | clean | not determined |
| apps/Model Plane/rust/services/session-core/src/main.rs | module wiring | `mod audit_publisher;` 12 | M (+1: adds `mod retention_receipt;`) | n/a |
| apps/Model Plane/rust/services/session-core/src/auth.rs | not inspected for audit lines | n/a | M (commit b9ed9197 added +153 here) | not determined |
| apps/Model Plane/rust/services/session-core/migrations/0012_audit_outbox.sql | session_audit_outbox table, pending index, claim UPDATE | index 16-17; claim UPDATE 35 | clean | sqlx on start |
| apps/Model Plane/rust/services/session-core/migrations/0013_tool_audit_intents.sql | session_tool_audit_intents table and index | table 1; index 20-21 | clean | n/a |
| apps/Model Plane/rust/services/session-core/migrations/0016_outbox_run_lifecycle_subjects.sql | replaces session_audit_outbox subject CHECK | 26-30 | clean | n/a |
| apps/Model Plane/rust/services/session-core/migrations/0043_audit_outbox_requeue.sql | requeue columns and requeue_session_audit_terminal(); REVOKE from PUBLIC | 28-75 | clean | audit_publisher.rs:1392, 1421-1422 |
| apps/Model Plane/rust/services/session-core/tests/fixtures/audit-core-wire/tool_action_final.json, tool_action_reserved.json | wire fixtures written by session-core builder, decoded by audit-core | 21 lines each (commit stat) | clean | audit_publisher.rs:1038; audit-core session_core_wire_fixtures_test.go:40, 69, 147, 170 |
| apps/Model Plane/go/services/nats-provisioner/audit_observability.go | Model Plane provisioner owns VEREVON_CONTROL_OBSERVABILITY and audit-core durables | comment 14-15; stream name 42; 98 lines | clean | manifest_contract_test.go: TestManifestProvisionsTheAuditObservabilityStreamAndDurables (142), TestEveryJetStreamPublisherSubjectGetsAPubAck (171), TestAuditCoreDurableReceivesWhatSessionCorePublishes (245), TestObservabilityTopologyMatchesAuditCoreProvisioner (672), TestComposeGatesSessionCoreOnTheProvisionerAndSharesTheBrokersCredential (815). main_test.go:113-115, 220-222, 309-311 skip without NATS_URL. |
| apps/Model Plane/go/services/nats-provisioner/main.go | provisionTopology entry point; NATS_PASSWORD | env 234 | clean | main_test.go |
| apps/Model Plane/go/services/nats-provisioner/publishers_registry_test.go | registry of JetStream publishers | 249 lines (commit) | clean | enforced via manifest_contract_test.go:418, 448, 548, 565, 621 |
| apps/Model Plane/deploy/nats.conf | broker users and grants | session-core publish `verevon.audit.v2.model.session-core.>` 45; audit-core-model user 165; JS API and ACK grants on VEREVON_CONTROL_OBSERVABILITY durables 169-172; DLQ subjects 173-174 | clean | loaded by manifest_contract_test.go (postmortem:116-117) |
| apps/Model Plane/deploy/docker-compose.yml | services and gating | nats-provisioner 80; session-core 712-807 with depends_on nats-provisioner service_completed_successfully at 798-799; MODEL_NATS_PROVISIONER_PASSWORD 59, 89; otel-collector 178-190 (Prometheus exporter 187) | clean | manifest_contract_test.go:815 |
| apps/Model Plane/docs/postmortem/0002-session-audit-trail-never-delivered.md | cause, deploy order, replay SQL, rollback, follow-ups | 1-291 | clean | none |

### 4b. Control Plane audit-core (Go, module github.com/triodelab/controlplane/audit-core)

| Path | Role | Approx lines | Live | Existing tests |
|---|---|---|---|---|
| apps/Control Plane/audit-core/internal/store/store.go | insert paths, conflict resolver, usage, Purge, ListAudit | InsertAudit 43; InsertAuditFromSource 52; InsertAuditFromStream 64; payload_hash 88-92; INSERT 94-106; resolveAuditConflict 113-139; InsertUsage 142-219; validateStreamIdentity 221-226; Purge 228-269; ListAudit 299-345 (since/until on ingested_at 319-320) | clean | store_test.go: TestPurge_RejectsNonPositiveRetention (21), TestPurge_DeletesOnlyExpiredRows (74), TestLogicalAuditIdentityIsBoundToProducerAndRejectsConflictingReuse (243), TestLogicalUsageIdempotencySurvivesNewStreamSequence (318), TestSummariseUsage_AggregatesAndOrdersByTotalCost (381). All gated by TEST_DATABASE_URL (32-34, 75-77). ListAudit has no direct test. |
| apps/Control Plane/audit-core/internal/store/session_core_replay_test.go | replay is exactly-once; three unique indexes exist | test 26; gate 27-29; index names 116 | clean | TestReplayedSessionCoreAuditEventIsStoredExactlyOnce (26) |
| apps/Control Plane/audit-core/internal/store/schema.sql | baseline audit_events and usage_events | audit_events 11-33 (ingested_at 13, occurred_at 14, details JSONB 23, ip_address 26, source_* 28-31, payload_hash 32); indexes 35-43; usage_events 45-65 | clean | jetstream_inbox_contract_test.go:9-104 (text checks) |
| apps/Control Plane/audit-core/internal/store/migrate.go | embeds schema.sql as 001_init plus 002-006; ledger table | embed 12-28; list 36-43; Migrate 54-70 | clean | none |
| apps/Control Plane/audit-core/internal/store/migrations/002_jetstream_inbox.sql | JetStream inbox identity | uq_audit_events_jetstream_inbox 11 | clean | jetstream_inbox_contract_test.go:9 |
| apps/Control Plane/audit-core/internal/store/migrations/003_logical_control_audit_idempotency.sql | payload_hash column; source_subject unique index | 12; 14-16 | clean | jetstream_inbox_contract_test.go:43 |
| apps/Control Plane/audit-core/internal/store/migrations/004_logical_usage_idempotency.sql, 005_usage_source_identity.sql | usage identity | n/a | clean | jetstream_inbox_contract_test.go:73 (004) |
| apps/Control Plane/audit-core/internal/store/migrations/006_v2_producer_authority.sql | source_producer CHECK and producer-scoped unique index (latest embedded) | CHECK 11-16; uq_audit_events_source_producer_event 18-20 | clean | jetstream_inbox_contract_test.go:95; session_core_replay_test.go:116 |
| apps/Control Plane/audit-core/migrations/001_init.sql | not embedded by migrate.go | whole file | clean | none; use not determined |
| apps/Control Plane/audit-core/internal/api/api.go | HTTP routes and handlers | Mount 105-117 (/healthz 106, /readyz 107, /v1 group with internalAuth 110, GET/POST /audit 111-112, /usage 113-114, /usage/summary 115); ingestUsage 119-153; internalAuth 179-188; listAudit 197-222; listUsage 224; ingestAudit 278-316 (scope and producer check 289-294; InsertAuditFromSource 295; 409 conflict 298-303) | clean | api_test.go: TestIngestAuditExactRetryAndConflictContract (132), TestIngestAudit_MissingKey_Returns401 (178), TestIngestAudit_WrongKey_Returns401 (293), TestIngestAudit_MissingOrgID_Returns400 (309), TestIngestAudit_MissingPlane_Returns400 (331), TestIngestAudit_MissingEvent_Returns400 (354), TestIngestAudit_ValidPayload_ReachesStore (376), TestIngestAuditRejectsProducerThatDoesNotMatchAuthenticatedPrincipal (406) |
| apps/Control Plane/audit-core/internal/api/service_auth.go | service credential registry, scopes, request authorisation, v3 audit-read delegation | serviceCredential 34-44; registry 63-120; ingestion-writer rule 145-155 (gateway needs audit:read:self; integration-corev2 needs audit:write pinned to ingestion); requiredAuditScope 203-212; authorizeServiceRequest 225; verifyAuditReadDelegation 283; delegation nonce cache 375-388 | clean | service_auth_test.go: TestParseServiceCredentialRegistryFailsClosed (23), TestValidateRequiredServiceCredentialRegistry (46), TestAuthorizeAuditReadRequiresV3DelegationAndExactTenant (61), TestAuthorizeAuditReadRejectsTamperingExpiryAndReplay (91), TestAuthorizeAuditWriterPinsPrincipalPlanePolicy (137) |
| apps/Control Plane/audit-core/internal/events/events.go | AuditEvent wire type and validation | struct 34-50 (Details map[string]any at 45; ip_address 48; user_agent 49); Validate 75-98 (no Details check); DecodeAudit 152 | clean | events_test.go: TestDecodeAuditRequiresStableProducerIdentityAndOccurrenceTime (56), TestDecodeAuditRejectsInvalidStableIdentity (91) |
| apps/Control Plane/audit-core/internal/subscriber/subscriber.go | JetStream consumer; authority check; dead-letter | subject comment 4; auditSubject 27; planeSubject 131; audit authority check 183; eventAuthorityMatches 287-295; authority_mismatch dead-letter 297-304 | clean | authority_test.go: TestEventAuthorityMatchesExactV2Subject (5), TestEventAuthorityRejectsLegacyAndForgedSubjects (30); subscriber_security_coverage_test.go: 36, 71, 174, 195; subscriber_integration_test.go:45 (broker-backed, no t.Skip found) |
| apps/Control Plane/audit-core/internal/provisioner/provisioner.go | Control Plane provisioner: stream constants and shared runtime | StreamName 17 (VEREVON_CONTROL_OBSERVABILITY); ControlSharedStreamName 24 (AQENCIA_CONTROLPLANE); ProvisionControlSharedRuntime 436; gdprDocumentsConsumerConfig 555 | clean | provisioner_test.go: TestProvisionControlPlaneRuntimeResourcesIsIdempotent (167), TestProvisionControlSharedRuntimePreservesLegacyGDPRSubjectsDuringMigration (453) |
| apps/Control Plane/audit-core/cmd/server/main.go | boot, env, retention sweep | NATS_URL required 207-212; AUDIT_RETENTION_DAYS default 365 at 246-253; EXTRA_NATS_URLS rejected 257-258; runRetention 157-171; 24h interval 152 and ticker 176-181 | clean | main_test.go: compose-contract checks at 49, 83, 119, 134, 787; env tests 1728-1830 |
| apps/Control Plane/audit-core/cmd/server/extra_nats.go | named extra-bus manager (AUDIT_EXTRA_NATS_BUSES) | mutex use 223-287 | clean | main_test.go:1728-1830 |
| apps/Control Plane/docker-compose.yml | audit-extra-nats-provisioner; audit-core service | audit-extra-nats-provisioner 791-802 (second copy of MODEL_NATS_PROVISIONER_PASSWORD at 802); audit-core 806; AUDIT_EXTRA_NATS_BUSES 826 | clean | main_test.go:119, 134 |
| apps/Control Plane/docker-compose.production.yml | audit-core service | 199 | clean | not determined |

### 4c. Control Plane org-core (config owner per card; no SIEM code)

| Path | Role | Approx lines | Live | Existing tests |
|---|---|---|---|---|
| apps/Control Plane/org-core/internal/org/gdpr_erasure_fanout.go | existing cross-plane fan-out precedent | 5-13 (subject verevon.gdpr.erasure.requested) | clean | org-core GDPR tests (gdpr_handlers_test.go, deletion_ledger_postgres_test.go) |
| apps/Control Plane/org-core/migrations/ | outbox precedents; latest file | 014_plan_change_outbox, 017_gdpr_audit_outbox, 019_interactive_retention_outbox; latest 020_interactive_retention_outbox_org_id_check.up.sql | clean | org-service.yml:103 (CI job) |

### 4d. Gateway and Frontend (Frontend Plane)

| Path | Role | Approx lines | Live | Existing tests |
|---|---|---|---|---|
| apps/Frontend Plane/verevonv3/apps/gateway/src/domains/audit.rs | org-scoped read proxy for GET /api/v1/audit | route 30; require_session 31; org from session 50; forwards event, user_id, since, until, limit 70-85; proxy 92-103 | clean | none in file. Gateway cargo tests run in CI (verevonv3-ci.yml:113, 141). |
| apps/Frontend Plane/verevonv3/apps/gateway/src/main.rs | merges audit router | 99; test AppState 329 | clean | n/a |
| apps/Frontend Plane/verevonv3/apps/gateway/src/domains/mod.rs | module export | 7 | clean | n/a |
| apps/Frontend Plane/verevonv3/apps/gateway/src/config.rs | AUDIT_CORE_URL | field 38; default http://audit-core:8187 at 182 | clean | n/a |
| apps/Frontend Plane/verevonv3/apps/gateway/src/upstream.rs | shared helpers used by audit.rs | authorized_org_id 324; proxy_json 348; audit_core_url 364 | clean | n/a |
| apps/Frontend Plane/verevonv3/src/shared/api/audit-client.ts | SPA client for /api/v1/audit | comments 6, 14; listAudit query 91-105 | clean | audit-client.test.ts |
| apps/Frontend Plane/verevonv3/src/features/settings/components/TrustCenterSection.tsx | Trust Center consumer of tool_action aggregates; "Brukt av AI?" column | import 10; comment 30; column header 300 | clean | SettingsPages.test.tsx; WorkspaceSettingsPage.zdr-gate.test.tsx; WorkspaceSettingsPage.negative-render.test.tsx; in-product-trust-copy.test.ts (names not read) |
| apps/Frontend Plane/verevonv3/src/shared/actions/types.ts | action result carries audit identifiers | 40 (auditId); 44-45 (auditEventId) | clean | action-execution-summary.test.ts; action-client.test.ts |
| apps/Frontend Plane/verevonv3/docker-compose.yml | AUDIT_CORE_URL env | 118 | clean | n/a |
| apps/Ingestion Plane/docker-compose.yml | AUDIT_CORE_URL env | 441 | M (diff not inspected) | n/a |

### 4e. Docs, contracts and owner docs

| Path | Role | Lines | Live | Notes |
|---|---|---|---|---|
| docs/compliance/evidence/trust-claims-pending-reverification.md | gate for the "Used by AI?" claim (section 6) | 97-112; condition 4 at 112 requires a Model Plane alert rule | clean | n/a |
| apps/Model Plane/MODEL_PLANE_ROADMAP.md | roadmap item "Enterprise audit: tamper-evident retention, export/SIEM, legal hold" | 417 | clean | n/a |
| apps/Model Plane/MODEL_PLANE_STATUS.md | status lists enterprise audit export and retention as open | 448 | clean | n/a |
| apps/Model Plane/docs/decisions/ledger.md | settled decisions | no entry on hash chain, SIEM or compliance API (grep) | clean | check before proposing a rejected approach (Model Plane CLAUDE.md) |
| apps/Model Plane/docs/ZDR.md | ZDR suppression of NATS audit envelopes | 95 ("zdr: true envelopes are suppressed by natsx") | M (64 lines; no audit-matching lines) | n/a |
| apps/Model Plane/docs/capability-ownership-matrix.md | one-owner rulings | no audit capability row; 53 = thread/message timeline owned by session-core outbox | clean | n/a |
| apps/master-ownership-matrix.md | owner rule | 29 | clean | n/a |
| apps/Control Plane/CONTROL_PLANE_OWNERSHIP.md | Control audit ownership and feature flag | 34, 49 (audit log flag and retention); 491-498 (audit via shared bus to audit-core) | clean | n/a |
| docs/evaluations/AUDIT_PLATFORM_SOC2.md | market context; not a contract | 414, 429 | clean | n/a |
| apps/Data Plane v2/infra/prometheus/alerts.yml | only Prometheus alert rule file in the repo (Data Plane v2) | file | clean | n/a |

## 5. Related contracts

- NATS subjects: `verevon.audit.v2.<plane>.<producer>.<event>` (events.go:3; subscriber.go:4; authority rule subscriber.go:287-295). Session-core tool actions: `verevon.audit.v2.model.session-core.tool_action` and `.tool_action_reserved` (audit_publisher.rs:34, 40, 43). Live only: `verevon.audit.v2.model.session-core.run_started_zdr_waived` (live retention_receipt.rs:34, 400-402).
- Stream and durables: VEREVON_CONTROL_OBSERVABILITY (audit_observability.go:42; provisioner.go:17). Durables audit-core-model-v3-audit and audit-core-model-v3-usage (deploy/nats.conf:169-172; postmortem:86-87). DLQ subjects verevon.dlq.audit-core.audit and .usage (nats.conf:173-174). Control shared stream AQENCIA_CONTROLPLANE (provisioner.go:24), GDPR fan-out subject verevon.gdpr.erasure.requested (gdpr_erasure_fanout.go:13).
- audit-core HTTP: GET and POST /v1/audit; GET and POST /v1/usage; GET /v1/usage/summary (api.go:111-115). Scope mapping requiredAuditScope at service_auth.go:203-212: GET /v1/audit, /v1/usage, /v1/usage/summary need audit:read:self; POST /v1/audit and /v1/usage need audit:write. Success envelope `{data, meta, error}` (api.go:218-221). Conflict envelope `{error:{code,message}}` (api.go:298-301).
- Gateway HTTP: GET /api/v1/audit with query event, user_id, since, until, limit (audit.rs:30, 37-43, 70-85). The client cannot pick the org (audit.rs:7-10, 34-35).
- gRPC (session-core): ReserveToolAction (apps/Model Plane/proto/model_plane/v1/sessions.proto:55), FinalizeToolAction (:59), "Inline tool audit lifecycle" messages at :649 onward. Implementations grpc.rs:4667, 4684; helpers 3650, 3732.
- Action contract: types.ts:40-45 carries auditId and auditEventId. No audit read or export action in src/shared/actions (grep).
- SSE: none for audit (apps/Model Plane/docs/sse-event-taxonomy.md has no audit match).
- Database tables: session_audit_outbox (0012; subject CHECK 0016; requeue columns 0043), session_tool_audit_intents (0013); audit_events and usage_events (audit-core schema.sql 11-65).
- Latest migrations. session-core: 0043_audit_outbox_requeue.sql (HEAD). Live adds ?? 0044_run_retention_receipt.sql, ?? 0045_zdr_waived_runs_org_index.sql and ?? revert/. Audit-core: internal/store/migrations/006_v2_producer_authority.sql (latest embedded). org-core: 020_interactive_retention_outbox_org_id_check.up.sql.
- Config and env: AUDIT_CORE_URL (gateway config.rs:182; Frontend compose:118; Ingestion compose:441). AUDIT_RETENTION_DAYS (main.go:246). NATS_URL (main.go:207). AUDIT_EXTRA_NATS_BUSES (control compose:826). AUDIT_CORE_SERVICE_CREDENTIALS (service_auth.go:152, 155). MODEL_NATS_PROVISIONER_PASSWORD (Model deploy compose:59, 89; production compose:29; control compose:802). NATS_PASSWORD (nats-provisioner main.go:234). Test env: TEST_DATABASE_URL (audit-core); DATABASE_URL and NATS_URL, NATS_USER, NATS_PASSWORD (session-core ignored tests, audit_publisher.rs:1424, 1551-1552, 1687-1688).
- Metrics (session-core): mp_session_audit_outbox_pending, _terminal, _oldest_pending_seconds, _terminal_by_subject{subject_class} (444-474); _terminal_total and _publish_failures_total{subject_class,reason} (752-759); mp_session_tool_audit_reservations_pending (486).
- CI path filters: .github/workflows/model-plane-rust-ci.yml:36, 49 (audit-core testdata triggers Model Plane Rust CI).

## 6. Test and build commands

| Area | Command | Source | Gating and notes |
|---|---|---|---|
| session-core (Rust) | `cargo test -q -p session-core --all-targets`, run from apps/Model Plane/rust. CI: `cargo test --workspace --no-fail-fast` | apps/Model Plane/CLAUDE.md (Testing); rust/Cargo.toml:15 (workspace member); services/session-core/Cargo.toml:2 (name "session-core"); model-plane-rust-ci.yml:57 (working-directory), 119 | Ignored DB and NATS tests need `-- --ignored` plus DATABASE_URL and NATS_URL/NATS_USER/NATS_PASSWORD (audit_publisher.rs:1421, 1548, 1684). User memory "Model Plane build and deploy": two cargo targets fail pre-existing on Windows; use --lib. |
| session-core live-only CI job | `cargo test -p session-core --bin session-core zdr_receipt -- --ignored --test-threads=1` | live diff of model-plane-rust-ci.yml (job "Session Core retention receipt (Postgres)") | Uncommitted. Needs Postgres. |
| nats-provisioner (Go) | `go test ./...` and `go vet ./...` in apps/Model Plane/go/services/nats-provisioner | go.mod in that directory (per-service modules; no go.work at go/ root per user memory) | NATS tests skip without NATS_URL (main_test.go:113-115). manifest_contract_test.go starts an in-process broker (postmortem:116-117). No CI workflow runs this module. |
| audit-core (Go) | `go test ./...` and `go vet ./...` in apps/Control Plane/audit-core | go.mod:1, 3 (module github.com/triodelab/controlplane/audit-core; go 1.25.0) | Postgres tests skip without TEST_DATABASE_URL (store_test.go:32-34; session_core_replay_test.go:27-29). No CI workflow runs audit-core Go tests (only testdata path filters, model-plane-rust-ci.yml:36, 49). User memory: -race needs cgo, unavailable. |
| gateway (Rust) | `cargo test --manifest-path apps/gateway/Cargo.toml --all-targets` from apps/Frontend Plane/verevonv3 | root CLAUDE.md (Build And Run); verevonv3-ci.yml:113 (working-directory), 141 (`cargo test`) | No audit-specific test. |
| Frontend (TypeScript) | `pnpm test` (node --test on gateway scripts, then vitest run); targeted `pnpm vitest run src/shared/api/audit-client.test.ts`; `pnpm typecheck` (tsc -b); `pnpm lint` (eslint .) | apps/Frontend Plane/verevonv3/package.json:11, 13, 17 | package.json is M in live (+2 demo scripts, not audit). User memory: typecheck, lint and vitest have pre-existing failures; compare failing sets against HEAD. |
| org-core (Go) | `go test ./...` in apps/Control Plane/org-core | .github/workflows/org-service.yml:103 (`go test -v -p 1 ./...`; workflow path not confirmed to be org-core) | Only if SIEM config lands here. |
| Migrations | session-core: sqlx applies on start (postmortem:147-156). audit-core: embedded migrate.go on boot (migrate.go:54-70). | postmortem 0002; migrate.go | Verify with SQL at postmortem:154-155. |
| Monitoring | No Model Plane rule file to test. Rule syntax checker command: not determined. | trust-claims doc:112 | Where mp_* metrics are scraped: not determined. |

Compose-contract tests read deploy files and will fail on text changes: apps/Control Plane/audit-core/cmd/server/main_test.go:49-134 and 787; nats-provisioner manifest_contract_test.go:815.

## 7. Owners

- Root CLAUDE.md, Architecture Rules: "Control Plane owns identity, users, orgs, billing, sessions, audit, quotas, and entitlements." "No direct database crossing between planes." "Zero Data Retention must propagate through any content-persisting boundary." Model Plane owns sessions, runs and execution. Frontend Plane owns the UI and same-origin gateway/BFF normalization.
- apps/master-ownership-matrix.md:29: "Control owns identity, authorization, canonical audit ...". Control owns the canonical audit store.
- apps/Model Plane/docs/capability-ownership-matrix.md: no audit capability row. Model Plane CLAUDE.md rule: "One canonical owner per capability; check docs/capability-ownership-matrix.md before adding anything that might duplicate an existing registry/store." Implication: a compliance store or chain must not duplicate audit-core.
- apps/Control Plane/CONTROL_PLANE_OWNERSHIP.md:491-498: audit is durable via the shared bus to audit-core.
- Service owners in code: audit-core (Control Plane); org-core (Control Plane; SIEM config owner per card); session-core outbox and publisher (Model Plane); gateway audit proxy (Frontend Plane); integration-corev2 (Ingestion Plane) holds an audit:write credential (service_auth.go:147-155).
- Dual ownership to resolve: the Model Plane nats-provisioner owns VEREVON_CONTROL_OBSERVABILITY (postmortem:85-86), and Control's audit-extra-nats-provisioner also lists the model bus (control compose:791-802; postmortem follow-up 1 at 282-285).
- Monitoring: Data Plane v2 has its own Prometheus alert file. Model Plane has none (trust-claims:112).

## 8. Uncommitted-only (live tree)

Relevant files only. Files not listed are clean in live (no status line): audit_publisher.rs, migrations 0012, 0013, 0016, 0043, gdpr.rs, gdpr_nats.rs, nats-provisioner (all), postmortem 0002, audit-core (all), org-core (all), gateway (all), audit-client.ts, TrustCenterSection.tsx, types.ts, deploy/nats.conf, Model deploy/docker-compose.yml, control docker-compose files, master-ownership-matrix.md, capability-ownership-matrix.md, CONTROL_PLANE_OWNERSHIP.md, MODEL_PLANE_ROADMAP.md, MODEL_PLANE_STATUS.md, trust-claims doc, Data Plane alerts.yml.

| Path | Live | What changed | Bears on F-t3-004 |
|---|---|---|---|
| apps/Model Plane/rust/services/session-core/src/retention_receipt.rs | ?? | Run retention class receipt. Waived-run start audit: const 34; subject 400-402; body 410-438 (ids, class, time, user_id, thread_id, waiver_ack_id; content-free by its own test); enqueue into session_audit_outbox 483-501. | Yes. New outbox event; the classifier puts it in audit_other (audit_publisher.rs:627-630). |
| apps/Model Plane/rust/services/session-core/migrations/0044_run_retention_receipt.sql | ?? | Run retention receipt columns (content not read) | Yes. Migration numbering after 0043. |
| apps/Model Plane/rust/services/session-core/migrations/0045_zdr_waived_runs_org_index.sql | ?? | Index for waived runs (content not read) | Numbering. |
| apps/Model Plane/rust/services/session-core/migrations/revert/ | ?? | Revert scripts (not read) | Rollback pattern. |
| apps/Model Plane/rust/services/session-core/src/grpc.rs | M (861 lines) | Retention and waiver plumbing. Waived-start audit enqueued in the start transaction (live 3219-3227). One audit-matching comment line. | Yes. Same file as enqueue_inline_tool_audit (worktree 3597-3648). |
| apps/Model Plane/rust/services/session-core/src/terminalization.rs | M (123 lines) | Waiver fields in the start receipt and retry check; comment that a waived start is audited in the same transaction. | Partial. |
| apps/Model Plane/rust/services/session-core/src/learning_events.rs | M (77 lines) | Retention-gate wording before the outbox insert (test string and doc). | Partial. |
| apps/Model Plane/rust/services/session-core/src/main.rs | M (+1) | Adds `mod retention_receipt;`. | Yes (module wiring). |
| apps/Model Plane/rust/services/session-core/src/auth.rs | M | Not inspected for audit lines. | Not determined. |
| apps/Model Plane/rust/crates/mp-contracts/src/retention.rs, zdr_refusal.rs; tests/zdr_retention_matrix.rs; tests/fixtures/ | ?? | Retention and ZDR refusal contracts (not read) | Low. |
| apps/Model Plane/rust/crates/mp-contracts/src/dataplane_posture.rs, lib.rs | M (67, +2) | No audit-matching lines | No. |
| .github/workflows/model-plane-rust-ci.yml | M (+50) | Adds job "Session Core retention receipt (Postgres)" running the zdr_receipt ignored tests. | Yes (test wiring). |
| apps/Model Plane/proto/model_plane/v1/sessions.proto | M (+42) | No audit-matching lines | No. |
| apps/Model Plane/docs/ZDR.md | M (64) | No audit-matching lines | Low. |
| apps/Model Plane/docs/sse-event-taxonomy.md | M (+4) | No audit-matching lines | No. |
| apps/Model Plane/api/contracts/README.md | M (+1) | Not audit | No. |
| docs/compliance/evidence/claims-vs-code.md | M (16) | Evidence wording; no edit to the "Brukt av AI?" row found in the diff | Low. |
| docs/compliance/evidence/data-flow-map.md | M (42) | Two audit-matching lines (S5 residency wording, M1 row) | Low. |
| docs/compliance/evidence/retention-and-erasure-evidence.md | M (24) | Two audit-matching lines (M1 row) | Low. |
| docs/compliance/evidence/honest-limits.md; subprocessors-from-code.md; tools/doc-codes.json | M (12; 10; +1) | No audit-matching lines | No. |
| apps/Frontend Plane/verevonv3/package.json | M (+2) | Adds demo:build and demo:probe scripts | No. |

## 9. Live-tree conflicts and phase 0 mapping

- Phase 0 deploy (card step 1) changes files that the live tree also edits: grpc.rs, main.rs, learning_events.rs, terminalization.rs, plus untracked migrations 0044 and 0045. Building session-core from the live tree would ship that uncommitted work too. The postmortem deploy order (0002:147-150) names no uncommitted work; a deploy must pick the source tree explicitly.
- Migration numbering: a WS06 migration must follow the highest number on its base: 0043 on HEAD, 0045 in live.
- Replay: the postmortem SQL filters `subject = 'verevon.audit.v2.model.session-core.tool_action'` (0002:169-175, 213-220). It does not select the live-only run_started_zdr_waived rows.
- Hash-chain insert coverage: both paths must be covered. JetStream stream insert (store.go:64, via the subscriber) and HTTP ingest (api.go:295 calling InsertAuditFromSource at store.go:52). The subscriber call site was read only at subscriber.go:183.
- Purge (store.go:243-269) deletes by ingested_at after the retention window. The card's checkpoint design needs a segment or checkpoint table; none exists.
- Erasure: session-core deletes outbox rows per org (gdpr.rs:431-432). No erasure path found in audit-core store.go or api.go (only Purge).
- Content-free rule: AuditEvent.Details is a free-form map (events.go:45) and Validate does not check it (events.go:75-98). The content-free property depends on producers. The session-core body is built at audit_publisher.rs:309-328.
- Phase 0 step mapping: (1) publisher fix: audit_publisher.rs 135-179, 298-340, 432-489 and grpc.rs 3597-3648; built at HEAD, not deployed as of 2026-10-06 (postmortem:4-7). (2) requeue migration: 0043 (1-75); applied on session-core start. (3) canary replay: postmortem:182-192 (SQL), function at 0043:33-73. (4) alert rules: none in repo; metric names at audit_publisher.rs:474; target config not determined. (5) two other broken audit paths: postmortem:266-273 (candidates rows 268 and 269; interpretation).

## 10. Not determined

- Which two "other broken audit paths" the card means. Candidates are postmortem rows 268 (audit-core Application Plane bus) and 269 (auth-core organization_membership_audit_outbox). Interpretation only.
- Whether the publisher fix and migration 0043 are deployed on any stack today. The postmortem (dated 2026-10-06) says not deployed; no live read was done.
- Where mp_* metrics are scraped. apps/Model Plane/deploy/docker-compose.yml has only an OTel collector with a Prometheus exporter on 127.0.0.1:8889 (line 187). No Prometheus server or rule file was found under apps/Model Plane.
- Whether apps/Control Plane/audit-core/migrations/001_init.sql is used anywhere. migrate.go does not embed it.
- Whether any CI runs audit-core or nats-provisioner Go tests. None was found in .github/workflows.
- How the NATS-backed subscriber tests are gated. subscriber_integration_test.go:45 has no t.Skip, and its environment was not read.
- Test coverage of the store's ListAudit method. No test name was found.
- Whether a declared-egress catalogue (F-t3-068) exists as a file. None found in docs/.
- Whether org-core has any SIEM or webhook configuration surface. None found. The org-service CI job's path was not confirmed as org-core.
- Whether the live-only changes to grpc.rs, learning_events.rs and terminalization.rs alter enqueue_inline_tool_audit or subject rules. Only three audit-matching lines were found in the diffs; the full hunks were not read.
- Whether gateway require_session and AuthenticatedUser.auth_role are an adequate gate for an admin compliance API. audit.rs has no admin-role check.
- Whether session-core auth.rs (M, +153 in b9ed9197) carries audit-related logic. Not inspected.
- Current deployed state of commit b9ed9197 and of the broker-side fix (postmortem says the broker side was deployed 2026-10-05). No live read.
