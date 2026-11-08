## EP-8-S5
shared-edit apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs (CRLF) fn seed_order: inside `if root {`, before the `// The boundary already refuses any other role` comment, insert:
        // The person's standing profile (EP-8 S5): context about them, so the root loop only,
        // like the thread and the memory; Context tier, ahead of the conversation.
        if !run_context.user_profile.is_empty() {
            messages.push(seed_message("system", run_context.user_profile.clone()));
        }

## EP-8-S5
shared-edit same file fn seed_messages info! call: after `memory_bytes = if root { run_context.memory_block.len() } else { 0 },` add `            profile_bytes = if root { run_context.user_profile.len() } else { 0 },`

## EP-8-S5
shared-edit same file tests: in fn full_run_context() after `memory_block: CONTEXT_MEMORY.to_owned(),` add `            user_profile: String::new(),`; in the `hostile` RunContext literal (run_context_text_cannot_change_the_permission_posture) after the memory_block line add `            user_profile: String::new(),` (both literals are exhaustive and will not compile once the proto is regenerated)

## EP-8-S5
shared-edit same file tests, after seed_order_follows_the_tiers_and_leaves_empty_parts_out add:
    #[test]
    fn seed_order_places_the_profile_after_the_instructions_root_only() {
        let context = pb::RunContext {
            user_profile: "PROFILE".to_owned(),
            ..full_run_context()
        };
        let root = seed_order("PREAMBLE".to_owned(), Some(&context), true, None, "GOAL");
        assert_eq!(
            roles_and_contents(&root),
            vec![
                ("system", "PREAMBLE"),
                ("system", CONTEXT_INSTRUCTIONS),
                ("system", "PROFILE"),
                ("user", CONTEXT_FIRST_TURN),
                ("assistant", CONTEXT_FIRST_REPLY),
                ("system", CONTEXT_MEMORY),
                ("user", "GOAL"),
            ]
        );
        let child = seed_order("PREAMBLE".to_owned(), Some(&context), false, None, "SUB-GOAL");
        assert_eq!(
            roles_and_contents(&child),
            vec![("system", "PREAMBLE"), ("system", CONTEXT_INSTRUCTIONS), ("user", "SUB-GOAL")]
        );
    }

## EP-8-S5
shared-edit apps/Model Plane/rust/crates/mp-contracts/src/run_context.rs (LF): after MAX_MEMORY_BYTES add `/// Largest standing-profile block, in bytes (EP-8 S5).\npub const MAX_USER_PROFILE_BYTES: usize = 6_144;`; add RunContextRefusal variants `/// A zero-retention run carried a standing profile.\n    ZdrUserProfile,` and `/// The profile block exceeds [`MAX_USER_PROFILE_BYTES`].\n    UserProfileTooLarge,` with tokens `Self::ZdrUserProfile => "run_context_zdr_user_profile",` and `Self::UserProfileTooLarge => "run_context_user_profile_too_large",`; in validate's `if zdr {` block after the memory check add `if !context.user_profile.is_empty() { return Err(RunContextRefusal::ZdrUserProfile); }`; after the MemoryTooLarge check add `if context.user_profile.len() > MAX_USER_PROFILE_BYTES { return Err(RunContextRefusal::UserProfileTooLarge); }`; module doc ZDR bullet: 'never history, memory or the standing profile'; tests `a_zdr_run_never_carries_a_profile` (user_profile "x" + zdr -> ZdrUserProfile, non-zdr -> Ok) and `an_oversize_profile_is_refused` (MAX_USER_PROFILE_BYTES+1 -> UserProfileTooLarge). Then replace model-gateway run_context.rs's local `MAX_USER_PROFILE_BYTES` with `mp_contracts::run_context::MAX_USER_PROFILE_BYTES`.

## EP-8-S5
shared-edit apps/Model Plane/deploy/docker-compose.yml model-gateway environment, after line `      MODEL_GATEWAY_TONE_PRESETS: ${MODEL_GATEWAY_TONE_PRESETS:-false}` add `      MODEL_GATEWAY_USER_PROFILE_PREFIX: ${MODEL_GATEWAY_USER_PROFILE_PREFIX:-false}`

## EP-8-S7
shared-edit apps/Frontend Plane/verevonv3/apps/gateway/src/domains/spaces/mod.rs: after the line `mod stream;` (module list near line 52) add `mod memory;`; in `fn module_routes()` after `.merge(stream::routes())` add `.merge(memory::routes())`.

## EP-8-S7
shared-edit apps/Frontend Plane/verevonv3/apps/gateway/src/config.rs: in `struct SpaceFeatureFlags` after the `append_retrieval` field add `/// `GATEWAY_SPACE_MEMORY_UI` (EP-8 S7): the Space memory routes (`GET /api/v1/spaces/{space_ref}/memory`, `DELETE .../memory/{memory_id}`). Only the literal `true` turns it on, never by default in development. Off answers 404 `feature_unavailable`. A rollout control, never an authorization input: Control's `model.space.memory.manage` decision decides who may list or delete.\n    pub(crate) memory_ui: bool,` and in `SpaceFeatureFlags::from_env` after `append_retrieval: ...,` add `memory_ui: env::var("GATEWAY_SPACE_MEMORY_UI").is_ok_and(|value| value == "true"),`.

## EP-8-S7
shared-edit apps/Model Plane/rust/services/session-core/src/grpc_space_memory_tests.rs ~line 851: in the `ListMemoryRequest { org_id, user_id, limit: 50, }` literal add `..Default::default()` after `limit: 50,`.

## EP-8-S7
shared-edit apps/Model Plane/rust/services/session-core/src/letta_adapter.rs ~line 751: in `ListMemoryRequest { org_id, user_id, limit, }` add `..Default::default()` after `limit,`; ~line 813: in `DeleteMemoryRequest { org_id, user_id, memory_id: memory_id.to_owned(), }` add `..Default::default()` after the memory_id line.

## EP-8-S7
shared-edit apps/Model Plane/rust/services/session-core/src/memory_control.rs ~line 1702: in `pb::ListMemoryRequest { ..., limit: 10, }` add `..Default::default()`; ~line 1758: in `pb::DeleteMemoryRequest { ..., memory_id: id, }` add `..Default::default()`.

## EP-8-S7
shared-edit apps/Model Plane/rust/services/session-core/src/gdpr_nats.rs in `handle_message`: inside the `else { return Ok(Outcome::Skipped); }` arm of `let Some(erasure) = parse_erasure_event(...)`, insert before the return: `// EP-8 S7: a per-user erasure drops that person's part of Space memory, nothing else.\n        if let Some(user) = crate::gdpr::parse_user_erasure_event(payload).map_err(|e| anyhow::anyhow!(e.to_string()))? {\n            let space = crate::gdpr::erase_user_space_memory_sources(pool, &user.org_id, &user.user_id).await?;\n            tracing::info!(org_id = %user.org_id, deleted = space.deleted, narrowed = space.narrowed, "GDPR user erasure: Space memory sources dropped");\n        }`.

## EP-8-S7
shared-edit apps/Model Plane/api/contracts/space-surface.v1.json (then regenerate the .sha256 and re-vendor the model-gateway and BFF tests/fixtures copies; run scripts/tests/zdr-contract-sync.test.sh): add four route rows. (1) id `mg:POST:/v1/spaces/:space_id/memory/list`, plane model, service model-gateway, handler `http_routes.rs::list_space_memories`, decision_type `space-memory-decision`, min_role admin, zdr_class n/a, basis path-rule, idempotency n/a, flag null, payload_allow_list [], reason `Read of Space memory under a Control model.space.memory.manage decision; POST so the decision rides in the body. A ZDR caller is answered empty by Session Core (EP-8 S7).` (2) id `mg:POST:/v1/spaces/:space_id/memory/delete`, handler `http_routes.rs::delete_space_memory_entry`, same fields, reason `Deletes are allowed under every posture; Session Core verifies the per-row Control decision (EP-8 S7).` (3) id `bff:GET:/api/v1/spaces/{space_ref}/memory`, plane frontend, service verevonv3-gateway, handler `domains/spaces/memory.rs::list_space_memory`, route_class ModelRead, decision_type space-memory-decision, min_role admin, zdr_class n/a, basis registry, flag `memory_ui`. (4) id `bff:DELETE:/api/v1/spaces/{space_ref}/memory/{memory_id}`, handler `domains/spaces/memory.rs::delete_space_memory`, route_class Config, otherwise as (3), reason `Deletes are allowed under every posture`.

## EP-8-S8
shared-edit apps/Control Plane/user-core/internal/http/server.go: inside the `settings := v1.Group("/settings")` block, directly after the line `settings.PUT("/ai", s.updateAISettings)` (three tabs of indent), add these four lines at the same indent: `// GET /api/v1/settings/onboarding - Get the personal first-run state (EP-8-S8)`, `settings.GET("/onboarding", s.getOnboardingSettings)`, `// PUT /api/v1/settings/onboarding - Update the personal first-run state (EP-8-S8)`, `settings.PUT("/onboarding", s.updateOnboardingSettings)`. Without this the two handlers are unrouted.

## EP-8-S8
shared-edit apps/Control Plane/docker-compose.yml: in the user-core service environment, directly after line 447 `TONE_PRESETS_VISIBLE: ${TONE_PRESETS_VISIBLE:-false}`, add `PERSONAL_SETUP_VISIBLE: ${PERSONAL_SETUP_VISIBLE:-false}` at the same indent (default off). The owner adds the example entry to .env.example; this slice does not edit it.

## EP-8-S8
shared-edit apps/Frontend Plane/verevonv3/apps/gateway/src/domains/actions/dispatchers.rs (a): in dispatch_settings_set_tone_preset, directly after the line `object.remove("tonePresetsAvailable");` add `object.remove("personalSetupAvailable");` at the same indent. The AI settings GET now carries the first-run flag, and the PUT must not send it back.

## EP-8-S8
shared-edit apps/Frontend Plane/verevonv3/apps/gateway/src/domains/actions/dispatchers.rs (b, part 1 of 2): insert the function below after the closing brace of dispatch_settings_set_tone_preset and before the comment line `// Reuses the real create_api_key/delete_api_key rather than re-deriving them:`. Code:
// Saves the personal first-run step (EP-8-S8). Every value is checked before the first write. The writes overwrite, and the first-run state is written last: if an earlier write fails the step stays pending, so a retry never leaves a half-saved step marked done.
pub(super) async fn dispatch_settings_complete_personal_setup(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let display_name = ticket_string(input, "displayName").trim();
    let timezone = ticket_string(input, "timezone").trim();
    let language = ticket_string(input, "language").trim();
    let tone = ticket_string(input, "tonePreset").trim();
    if display_name.chars().count() > 80 {
        return ticket_bad_request(
            "settings.complete_personal_setup takes a displayName of at most 80 characters",
        );
    }
    if !language.is_empty() && !matches!(language, "nb-NO" | "en-US") {
        return ticket_bad_request(
            "settings.complete_personal_setup takes a language of nb-NO or en-US",
        );
    }
    if !tone.is_empty() && !crate::domains::persona::is_tone_preset(tone) {
        return ticket_bad_request(
            "settings.complete_personal_setup takes a tonePreset of default, professional, friendly, direct, warm or neutral",
        );
    }
    let actor = actor_for_user(user);
    // user-core checks the time zone itself. It is the first write, so a bad zone stops the step before anything is saved.
    if !timezone.is_empty() || !language.is_empty() {
        let mut preferences = serde_json::Map::new();
        if !timezone.is_empty() {
            preferences.insert("timezone".to_owned(), json!(timezone));
        }
        if !language.is_empty() {
            preferences.insert("language".to_owned(), json!(language));
        }
        let (status, Json(resp)) = proxy_json(
            state,
            Method::PATCH,
            &format!("{}/api/v1/preferences", state.user_core_url),
            Some(Value::Object(preferences)),
            None,
            Some(&actor),
            None,
        )
        .await;
        if !status.is_success() {
            return owner_json_response_to_envelope("settings.complete_personal_setup", user, status, resp);
        }
    }
    if !display_name.is_empty() {
        let (status, Json(resp)) = proxy_json(
            state,
            Method::PATCH,
            &format!("{}/api/v1/users/me", state.user_core_url),
            Some(json!({ "name": display_name })),
            None,
            Some(&actor),
            None,
        )
        .await;
        if !status.is_success() {
            return owner_json_response_to_envelope("settings.complete_personal_setup", user, status, resp);
        }
    }
    if !tone.is_empty() {
        let url = format!("{}/api/v1/settings/ai", state.user_core_url);
        let (status, Json(mut settings)) =
            proxy_json(state, Method::GET, &url, None, None, Some(&actor), None).await;
        if !status.is_success() {
            return owner_json_response_to_envelope("settings.complete_personal_setup", user, status, settings);
        }
        if let Some(object) = settings.as_object_mut() {
            object.remove("tonePresetsAvailable");
            object.remove("personalSetupAvailable");
            object.insert("tonePreset".to_owned(), json!(tone));
        }
        let (status, Json(resp)) =
            proxy_json(state, Method::PUT, &url, Some(settings), None, Some(&actor), None).await;
        if !status.is_success() {
            return owner_json_response_to_envelope("settings.complete_personal_setup", user, status, resp);
        }
    }
    let (status, Json(resp)) = proxy_json(
        state,
        Method::PUT,
        &format!("{}/api/v1/settings/onboarding", state.user_core_url),
        Some(json!({ "personalStep": "completed" })),
        None,
        Some(&actor),
        None,
    )
    .await;
    owner_json_response_to_envelope("settings.complete_personal_setup", user, status, resp)
}

## EP-8-S8
shared-edit apps/Frontend Plane/verevonv3/apps/gateway/src/domains/actions/dispatchers.rs (b, part 2 of 2): insert directly after the function from part 1, before the same comment line `// Reuses the real create_api_key/delete_api_key rather than re-deriving them:`. Code:
// Records that the person skipped the personal first-run step (EP-8-S8). The card does not show again.
pub(super) async fn dispatch_settings_skip_personal_setup(
    state: &AppState,
    user: &AuthenticatedUser,
) -> Response {
    let actor = actor_for_user(user);
    let (status, Json(resp)) = proxy_json(
        state,
        Method::PUT,
        &format!("{}/api/v1/settings/onboarding", state.user_core_url),
        Some(json!({ "personalStep": "skipped" })),
        None,
        Some(&actor),
        None,
    )
    .await;
    owner_json_response_to_envelope("settings.skip_personal_setup", user, status, resp)
}

## EP-8-S8
shared-edit apps/Frontend Plane/verevonv3/apps/gateway/src/domains/actions/handlers.rs: (a) add `dispatch_settings_complete_personal_setup, dispatch_settings_skip_personal_setup,` to the `use super::dispatchers::{...}` list (it sits beside `dispatch_settings_set_tone_preset`); (b) in the execute match, directly after the arm `"settings.set_tone_preset" => {` ... `}` (about line 278), add `"settings.complete_personal_setup" => {` / `dispatch_settings_complete_personal_setup(&state, &user, &body.input).await` / `}` and `"settings.skip_personal_setup" => dispatch_settings_skip_personal_setup(&state, &user).await,`. The gateway parity test (apps/Frontend Plane/verevonv3/apps/gateway/scripts/action-surface-contract.test.mjs) needs both arms.

## EP-8-S8
shared-edit apps/Frontend Plane/verevonv3/src/features/chat/components/ChatPage.tsx: (a) add `import { PersonalSetupCard } from './PersonalSetupCard'` directly after the line `import { ScheduledRunsPanel } from './ScheduledRunsPanel'` (line 62); (b) in the chat switch, directly after `<Match when={!hasMessages()}>` (about line 1180) and before `<EmptyChatState`, add `<PersonalSetupCard threadId={state.threadId} />`. The card renders nothing unless PERSONAL_SETUP_VISIBLE is on and the step is pending.

## EP-9-E1
shared-edit apps/Frontend Plane/verevonv3/src/features/chat/components/ChatMessages.tsx: in ToolCallCard, insert after the closing </Show> of the `presentation().intent === 'diff'` count block (about line 2538) and before the line `<Show when={presentation().truncated && !stepStripEnabled}>`, at 8-space indent: <Show when={presentation().sandbox}>{(sandbox) => (<em class="verevon-chat-tool-call__sandbox" data-sandbox={sandbox()}>{sandbox() === 'isolated_no_network' ? i18n.tr('Isolert, uten nett', 'Isolated, no network') : i18n.tr('ukjent', 'unknown')}</em>)}</Show> (format over several lines; it is shown in both step-strip modes on purpose).

## EP-9-E1
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the execution-core environment block, after the line `      EXECUTION_SANDBOX_MAX_OPEN_FILES: ${EXECUTION_SANDBOX_MAX_OPEN_FILES:-1024}` (about line 1472, six-space indent), add `      CODE_INTERPRETER_ENFORCEMENT_REPORT: ${CODE_INTERPRETER_ENFORCEMENT_REPORT:-false}`.

## EP-9-E1
shared-edit apps/Model Plane/deploy/.env.example: add `CODE_INTERPRETER_ENFORCEMENT_REPORT=false` beside the execution-core settings (owner-owned file; the lead applies it).

## EP-9-E2
shared-edit apps/Control Plane/user-core/cmd/server/main.go: directly after the line `spaceRepo.SetHistoryWindowClaim(os.Getenv("SPACE_HISTORY_WINDOW_CLAIM") == "true")` (around line 395) add `spaceRepo.SetSandboxEgressTiers(os.Getenv("SPACE_SANDBOX_EGRESS_TIERS") == "true")` (CRLF). Without this line the switch simply stays off.

## EP-9-E2
shared-edit apps/Model Plane/docs/capability-ownership-matrix.md: note that the sandbox egress ceiling (tier + allow-list) is owned by Control user-core space_effect_policies (migration 044) and that sandbox-manager only verifies it.

## EP-9-E5
shared-edit apps/Model Plane/docs/capability-ownership-matrix.md: insert one row directly after line 72 (the 'Secret scrub / redaction' row), inside the same six-column table, CRLF: '| Sandbox credential custody, sealed references and sentinel injection (proposed, EP-9-E5 ADR) | **integration-corev2** (custody, sealing, redemption, audit record); **execution-core** (injecting proxy per call: slots, sentinels, header injection, SigV4 re-signing; enforcement only); **user-core** (signed injection permission, never for ZDR or waived evidence) | Postgres in integration-corev2 (binding rows, W4); memory in execution-core per call | execution-core proxy = enforcement; integration-corev2 = redemption authority | audit record in IC store | single custodian; proposed, not built (W4) |'

## EP-9-C5
shared-edit apps/Frontend Plane/verevonv3/src/shared/api/chat-client.ts (LF file; line numbers are before any of these edits): (1) after line 1258 `export type ModelLifecycle = 'ga' | 'preview' | 'deprecated'`, insert a blank line and: /** The data-use statements a catalogue record makes for a model (`data_use` on `/v1/models`). A field is set only when a source states it: `undefined` is "not stated", never "no". */ export type ModelDataUse = { /** Whether the provider states it trains on the inputs it receives. */ trainsOnInputs?: boolean; /** Days the provider states it keeps the inputs; `0` when it states none are kept. */ retentionDays?: number; /** `true` only when the deployment carries a verified zero-retention attestation in force. */ zdrAttested?: boolean } (one member per line, as the file style). (2) inside `export type ModelInfo = {`, after line 1299 `  price?: ModelPrice` and before the closing brace on line 1300, insert: /** What the provider states about the request data sent to this model (`data_use`). Absent when none is stated. */ dataUse?: ModelDataUse. (3) in listModelCatalogue's map, after line 3956 `        price: normalizeModelPrice(item.price),`, insert: dataUse: normalizeModelDataUse(item.data_use), (4) before line 3694 `/** A token count the record names as a positive whole number; anything else is unknown. */`, insert: /** The data-use record a catalogue entry carries. A statement the record does not make stays unset. */ function normalizeModelDataUse(value: unknown): ModelDataUse | undefined { if (!value || typeof value !== 'object' || Array.isArray(value)) return undefined; const record = value as Record<string, unknown>; const trainsOnInputs = readBool(record.trains_on_inputs); const days = record.retention_days; const retentionDays = typeof days === 'number' && Number.isSafeInteger(days) && days >= 0 ? days : undefined; const zdrAttested = readBool(record.zdr_attested); const dataUse: ModelDataUse = { ...(trainsOnInputs === undefined ? {} : { trainsOnInputs }), ...(retentionDays === undefined ? {} : { retentionDays }), ...(zdrAttested === undefined ? {} : { zdrAttested }) }; return Object.keys(dataUse).length > 0 ? dataUse : undefined } (written one statement per line in the file).

## EP-9-C5
shared-edit apps/Model Plane/rust/services/model-gateway/src/http_routes.rs (CRLF file; line numbers are before any of these edits): (1) in `struct ModelDescriptor`, after the field line `    price: Option<crate::catalogue_prices::ModelPrice>,` (line 10747), insert: `    /// What the provider states about this model's request data (`data_use` on inference-core's` / `    /// `ModelInfo`). Left out when nothing is stated, so a deploy without an attested deployment` / `    /// serves the JSON it served before (EP-9 C5).` / `    #[serde(skip_serializing_if = "Option::is_none")]` / `    data_use: Option<DataUseDescriptor>,`. (2) after the struct's closing brace (line 10748) and the blank line, before the doc comment `/// Wire-numeric → tier label` (line 10750), insert: `/// The data-use statements one model's provider makes (`data_use` on `/v1/models`). A field is` / `/// left out unless a source states it: an absent field is "not stated", never "no".` / `#[derive(Debug, Serialize)]` / `struct DataUseDescriptor {` / `    #[serde(skip_serializing_if = "Option::is_none")]` / `    trains_on_inputs: Option<bool>,` / `    #[serde(skip_serializing_if = "Option::is_none")]` / `    retention_days: Option<i32>,` / `    #[serde(skip_serializing_if = "Option::is_none")]` / `    zdr_attested: Option<bool>,` / `    #[serde(skip_serializing_if = "String::is_empty")]` / `    statement_ref: String,` / `}` and a blank line. (3) in `list_models`, in the `ModelDescriptor { ... }` literal, after the line `                price,` (line 10862), insert: `                data_use: m.data_use.map(|data_use| DataUseDescriptor {` / `                    trains_on_inputs: data_use.trains_on_inputs,` / `                    retention_days: data_use.retention_days,` / `                    zdr_attested: data_use.zdr_attested,` / `                    statement_ref: data_use.statement_ref,` / `                }),`. (4) in the test helper `fn descriptor(effort_levels: &[&str]) -> ModelDescriptor`, after the line `            price: None,` (line 15317), insert `            data_use: None,`.

## EP-9-C5
shared-edit apps/Frontend Plane/verevonv3/src/styles/global.css (CRLF file): after line 49828 `.dashboard-composer-model-price { display: block; ... }`, insert: `/* EP-9-C5: provider data-use line under a model name in the composer picker */` then `.dashboard-composer-model-data-use { display: block; margin-top: 1px; font-size: 11px; font-weight: 400; color: var(--verevon-text-muted); }`.

