## EP-5-S5
shared-edit apps/Frontend Plane/verevonv3/src/shared/actions/model-access.ts: after line 344 (`  'chat.open_deck_slide': exempt('ui_state_only', 'read'),`) insert `  'chat.present_deck': exempt('ui_state_only', 'read'),`. The map is exhaustive by type, so typecheck and action-registry.test.ts need this row.

## EP-5-S9
shared-edit apps/Frontend Plane/verevonv3/src/styles/global.css: after line 28467 (the closing brace of `.verevon-deck__card[aria-current='true']`), insert a blank line and then /* A card with its slide's picture (EP-5 S9): the picture sets the height and the bullet lines give way to it. */ followed by the blocks `.verevon-deck__card--picture { aspect-ratio: auto; }`, `.verevon-deck__card--picture .verevon-deck__line { display: none; }` and `.verevon-deck__thumb { display: block; width: 100%; height: auto; border-radius: 4px; }`, each on its own lines. Without this a card picture draws at its intrinsic size.

## EP-5-S9
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the execution-core service's environment block, insert after line 1538 (`      CODE_INTERPRETER_ENFORCEMENT_REPORT: ${CODE_INTERPRETER_ENFORCEMENT_REPORT:-false}`) the line `      OFFICE_PREVIEW_PAGES: ${OFFICE_PREVIEW_PAGES:-3}` with six spaces of indent; the default 3 changes nothing.

## EP-7-S6
shared-edit apps/Model Plane/rust/services/model-gateway/src/lib.rs: after the line `pub mod goal_judge;` add `pub mod goal_unattended;`

## EP-7-S6
shared-edit apps/Model Plane/rust/services/model-gateway/src/thread_goal_routes.rs: in goal_body(), in the json! `data` object after `"autoContinueAvailable": crate::goal_auto::available()` add `, "unattendedAvailable": crate::goal_unattended::available()` so the SPA can show the away form

## EP-7-S6
shared-edit apps/Frontend Plane/verevonv3/apps/gateway/src/domains/agent_actions.rs create_cron: before inject_personal_schedule_create_context, refuse (422 typed `goal_schedule_personal_only`) when body.task_template has a `goal_id` key and cron_create_names_explicit_space(&body) is true. A goal-bound schedule must run in the personal Space, never in a room thread.

## EP-9-W1
shared-edit apps/Model Plane/rust/services/model-gateway/src/lib.rs: add `pub mod search_provenance;` between `pub mod schema_check;` and `pub mod security_events;` (the new module does not compile into the crate without it).

## EP-9-W1
shared-edit apps/Frontend Plane/verevonv3/src/shared/chat-nodes/types.ts: in the ConversationNode union, after the closing `    }` of the `kind: 'source-scope'` member and before the `/**` that opens the lifecycle-notices comment, add a one-line doc comment and `  | { kind: 'search-provenance'; provenance: import('@/shared/api/chat-client').ChatSearchProvenanceEvent }`.

## EP-9-W1
shared-edit apps/Frontend Plane/verevonv3/src/shared/chat-nodes/derive.ts: in deriveConversationNodes, between the `if (!waiting) { ... kind: 'source-scope' ... }` block and the `// Shown while the turn waits too:` comment, add `if (!waiting && turn.searchProvenance) { nodes.push({ kind: 'search-provenance', provenance: turn.searchProvenance }) }` with a short comment.

## EP-9-W1
shared-edit apps/Frontend Plane/verevonv3/src/features/chat/lib/settled-fold.ts: in FOLDED_KINDS add `  'search-provenance': false,` directly after `  'source-scope': false,` (the record is exhaustive, so the build fails without it).

## EP-9-W1
shared-edit apps/Frontend Plane/verevonv3/src/features/chat/components/use-chat-controller.ts: after the `onSourceScopeApplied` handler and before `onGrounding:`, add `onSearchProvenance: (event: Parameters<NonNullable<ChatStreamHandlers['onSearchProvenance']>>[0]) => { setState((s) => { const turn = s.turns.find((t) => t.id === assistantId); if (turn) turn.searchProvenance = event }) },` in the same multi-line style as its neighbour. Without it the frame is parsed and dropped.

## EP-9-W2
shared-edit apps/Model Plane/rust/services/model-gateway/src/runtime_registries.rs: in fn do_run_org_policy (about line 3413) add the line `        web: None,` after `        policy_digest: String::new(),` inside the mp_contracts::model_plane::v1::OrgPolicyContext literal; it has no ..Default::default(), so the new field OrgPolicyContext.web breaks it (compile blocker).

## EP-9-W2
shared-edit apps/Frontend Plane/verevonv3/src/shared/actions/model-access.ts: after the line `  'org.set_agent_policy': exempt('human_identity_or_security', 'external_write'),` add `  'org.set_web_policy': exempt('human_identity_or_security', 'external_write'),` (MODEL_ACCESS is Record<ActionId, ...>, so typecheck and the registry tests need it).

## EP-9-W2
shared-edit scripts/tests/zdr-contract-sync.test.sh: line 122, change the org-agent-policy-schema.json digest from 631e4457738183453b33dbcc98348a639dbbf463fb346785244cf2b4f0973fbe to e0c445e1d27045782e6f72ce456fb9c7bf60476c5f7c87f57f3a90febd085719 (the .sha256 beside the schema is already refreshed in this patch).

## EP-9-W2
shared-edit apps/Frontend Plane/verevonv3/src/features/customize/AgentModesSection.tsx (needed before the web switch is used): in function saveOrgAgentPolicy (about line 238) change the parameter type `settings: OrgAgentPolicySettings` to `settings: Record<string, unknown>`; in save() (about line 391) change `await saveOrgAgentPolicy(doc.revision, settings)` to `await saveOrgAgentPolicy(doc.revision, { ...doc.settings, ...settings })`. AgentModesSection.test.tsx still matches because its STORED.settings holds the same five keys.

## EP-9-W2
shared-edit apps/Frontend Plane/verevonv3/src/features/customize/AgentModesSection.tsx (optional, cosmetic): in settingLabel add cases 'web.mode' returning i18n.tr('Nettilgang for agentene', 'Web access for agents') and 'web.paid_providers' returning i18n.tr('Betalte søketjenester', 'Paid search services') before the default branch, so the effective table shows labels rather than raw keys.

## EP-9-E4
shared-edit apps/Model Plane/rust/services/execution-core/src/main.rs: right after `info!("execution-core starting");` insert `\n    // EP-9 E4: shell-rule proposals narrow WS13-S7's proposals, so they refuse to start without them.\n    mp_contracts::shell_rule::shell_rule_proposals_checked()?;` (ShellRuleSwitchRefused implements std::error::Error, so it works with anyhow `?`).

## EP-9-E4
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the execution-core service environment add `APPROVAL_SHELL_RULE_PROPOSALS_ENABLED: ${APPROVAL_SHELL_RULE_PROPOSALS_ENABLED:-false}` and `APPROVAL_RULE_PROPOSALS_ENABLED: ${APPROVAL_RULE_PROPOSALS_ENABLED:-false}`. Execution-core now reads the parent switch for the startup check; today only model-gateway has it.

## EP-9-C1
shared-edit apps/Control Plane/org-core/internal/http/service_auth.go: (1) in the first const block after `quarryEdgePrincipal       = "quarry-edge"` add `sessionCorePrincipal      = "session-core"`; (2) in orgServiceScopeAllowlist after the "org:web-policy:read:any" entry add `// Read-only access to one org's retention window and active legal holds, through POST /internal/orgs/:orgId/retention/check and nothing else (EP-9 C1). Held by session-core alone, whose sweeper drops every held thread.` + `"org:retention-policy:read:any": {},`; (3) in validateServiceCredential after the org:web-policy:read:any pin add `// The check decides which threads the retention sweeper may delete, so it is pinned to the sweeper's principal (EP-9 C1).` `if scope == "org:retention-policy:read:any" && credential.Principal != sessionCorePrincipal { return fmt.Errorf("scope %s is not valid for principal %s", scope, credential.Principal) }`; (4) in organizationAPIScopes before `default:` add: `// Retention window and legal holds (EP-9 C1), on behalf of a signed user only.` `case len(rest) == 2 && method == http.MethodGet && (rest[1] == "retention" || rest[1] == "legal-holds"): return []string{"org:read:self"}` / `case len(rest) == 2 && method == http.MethodPut && rest[1] == "retention": return []string{"org:settings:write:self"}` / `case len(rest) == 2 && method == http.MethodPost && rest[1] == "legal-holds": return []string{"org:settings:write:self"}` / `case len(rest) == 4 && method == http.MethodPost && rest[1] == "legal-holds" && rest[3] == "release": return []string{"org:settings:write:self"}`; (5) in internalOrganizationScopes after the web-policy case add `case len(rest) == 3 && rest[1] == "retention" && rest[2] == "check" && method == http.MethodPost: return []string{"org:retention-policy:read:any"}`. retention_handlers_test.go depends on all five.

## EP-9-C1
shared-edit apps/Control Plane/docker-compose.yml (ORG_CORE_SERVICE_CREDENTIALS, ~line 620): add "org:retention-policy:read:any" to the session-core principal's scopes array, after "org:agent-policy:read:any". Apply only together with the service_auth.go allowlist edit, otherwise org-core refuses to boot.

## EP-9-C1
shared-edit apps/Control Plane/org-core/cmd/server/main.go: import internal/retention. After server.SetBrowserSites(...) add `// Retention policy and legal holds (EP-9 C1); read by session-core's sweeper. No thread retention reader yet: thread holds are placed without the ZDR receipt check.` `var retentionPublisher retention.AuditPublisher; if natsClient != nil { retentionPublisher = natsClient }` `retentionService := retention.NewService(retention.NewRepository(db), nil, retentionPublisher)` `server.SetRetention(retentionService)` `if natsClient != nil { go runRetentionAuditOutbox(ctx, retentionService, 5*time.Second) }`. Add func runRetentionAuditOutbox, a copy of runSIEMAuditOutbox's shape that calls service.FlushAuditOutbox(ctx, 100) with 'retention audit outbox' log text. The typed-nil guard matters because a nil *nats.Client inside the interface is non-nil.

## EP-9-C1
shared-edit apps/Model Plane/rust/crates/mp-contracts/src/zdr_refusal.rs FEATURES: after the thread_goal Feature add `// EP-9 C1: org-core refuses a thread legal hold on a ZDR or waived thread with legal_hold_nothing_retained: nothing is retained, so there is nothing to hold.` `Feature { key: "legal_hold", label: "a legal hold on a thread", },`. Then regenerate the golden (MP_UPDATE_GOLDEN=1 cargo test -p mp-contracts zdr_refusal) and its .sha256, refresh the vendored copies (FE src/shared/api/contracts/zdr-refusals.json + sha, go/pkg/zdrrefusal/zdr-refusals.json, the Quarry fixture) and the GOLDENS pin in scripts/tests/zdr-contract-sync.test.sh. My hand edit of api/contracts/zdr-refusals.json must equal the regenerated output.

## EP-9-C1
shared-edit apps/Frontend Plane/verevonv3/src/shared/actions/model-access.ts after the 'org.set_model_entitlement' line: `'org.set_retention': exempt('human_identity_or_security', 'external_write'),` `'org.place_legal_hold': exempt('human_identity_or_security', 'external_write'),` `'org.release_legal_hold': exempt('human_identity_or_security', 'external_write'),`. MODEL_ACCESS is a Record<ActionId>, so typecheck fails without these. Then the guard regenerates the agent-action-admission golden and every copy of it.

## EP-9-C1
shared-edit docs/ZDR.md (LF) feature table: add a row `| **Legal holds (EP-9 C1)** | Thread hold on a ZDR or waived thread refused | org-core answers 422 legal_hold_nothing_retained (details.feature legal_hold) once a thread retention reader is wired; until then a thread hold is placed. Org holds on a ZDR org are allowed. Records hold ids, numbers and a hold reference only. Built dark behind ORG_RETENTION_POLICY_ENABLED (off). |`. Add no zdr-posture-matrix row.

## EP-11-S1
shared-edit apps/Frontend Plane/verevonv3/src/shared/actions/model-access.ts: after the line `  'learning.undo': exempt('agent_cannot_self_govern', 'destructive'),` insert `  'skills.save_from_message': exempt('agent_cannot_self_govern', 'context_write'),`. MODEL_ACCESS is typed over ActionId, so typecheck and action-registry.test fail without it.

## EP-11-S1
shared-edit apps/Frontend Plane/verevonv3/docker-compose.yml: in the gateway service environment block, after the line `      SPACE_THREAD_FORK_API: ${SPACE_THREAD_FORK_API:-false}`, insert `      # EP-11 S1: members may author private skills (BFF). Only the literal true enables it; off, the admin-only answer stands.` and `      SKILL_MEMBER_AUTHORING: ${SKILL_MEMBER_AUTHORING:-false}`. The BFF flag belongs to this FE stack compose: apps/Model Plane/deploy/docker-compose.yml has no BFF service, so the batch's listed file is not where it goes.

