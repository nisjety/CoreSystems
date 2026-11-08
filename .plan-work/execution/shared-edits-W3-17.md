## EP-2-S10
shared-edit apps/Model Plane/rust/services/execution-core/src/runtime_loop/mod.rs (dispatch): insert immediately before the line `    if tool_name == result_handles::RESULT_QUERY_TOOL && result_store.is_some() {` (about line 1311, after the comment block that starts `// \`result_query\` reads a result this run held`) this block: `    // The long-page reader (EP-2-S10) answers read_page and find_in_page from the pages this run holds, as result_query answers from its own store. Off, the names fall through as before.` then `    if let Some(read) = result_handles::execute_reader_tool(tool_name, tool_input, org_id, run_id, result_store) {` then `        return match read {` then `            Ok(output) => StepOutcome::completed(output),` then `            Err(error) => StepOutcome::failed(&error),` then `        };` then `    }`.

## EP-2-S10
shared-edit apps/Model Plane/rust/services/execution-core/src/runtime_loop/mod.rs (tail, render): replace the single line `    let rendered = crate::provenance::render(&screened.output, &screened.provenance);` (about line 1696) with `    let rendered = crate::provenance::render(` then `        &result_handles::inline_text(tool_name, &screened.output),` then `        &screened.provenance,` then `    );`.

## EP-2-S10
shared-edit apps/Model Plane/rust/services/execution-core/src/runtime_loop/mod.rs (tail, park): in `let parked = result_store` (about lines 1700-1712) keep the `.filter(|_| result_handles::is_parkable_tool(tool_name))` line and replace the whole `.and_then(|store| {...})` closure (through its closing `})` before the semicolon) with: `        .and_then(|store| {` then `            let prefix = crate::provenance::render("", &screened.provenance);` then `            let label = prefix.strip_suffix('\n').unwrap_or(&prefix);` then `            result_handles::hold_for_model(store, org_id, run_id, tool_name, label, &screened.output)` then `        });`. Items 1-3 must land together; they call inline_text, hold_for_model and execute_reader_tool, which exist only in this patch.

## EP-2-S10
shared-edit apps/Model Plane/deploy/docker-compose.yml: after the line `      EXECUTION_RESULT_HANDLES: ${EXECUTION_RESULT_HANDLES:-off}` (line 1576, execution-core service) add `      # EP-2-S10: the Do loop's long-page reader (read_page, find_in_page) over held pages. Off by default; only the literal true enables it, and it needs EXECUTION_RESULT_HANDLES on.` then `      EXECUTION_WEB_READER: ${EXECUTION_WEB_READER:-false}`. apps/Model Plane/deploy/.env.example does not list EXECUTION_RESULT_HANDLES, so it needs no entry.

## EP-5-S4
shared-edit apps/Frontend Plane/verevonv3/src/shared/actions/model-access.ts: after the line `  'chat.apply_deck_template': exempt('no_owner_executor', 'context_write'),` add the line `  'chat.open_deck_slide': exempt('ui_state_only', 'read'),`. MODEL_ACCESS is exhaustive by type, so typecheck fails and the new action-registry test fails without it.

## EP-5-S3
shared-edit apps/Model Plane/rust/services/model-gateway/src/artifact_revisions.rs: in seed_from_table_heads, replace `crate::artifacts::ArtifactKind::parse(&head.kind)` with `crate::deck_tool::stored_kind(&head.kind)` so decks seeded from the revision table in `on` mode keep kind Deck

## EP-5-S3
shared-edit apps/Model Plane/docs/ZDR.md: add a row for the `deck` metadata kind (manifest JSON in message metadata `artifacts`, at most 120,000 chars, erased with the message, not stored on ZDR or waived turns, pptx never stored)

## EP-5-S8
shared-edit apps/Model Plane/rust/services/model-gateway/src/artifacts.rs (CRLF): in `struct StoredArtifact`, after the field `user_version: Option<u32>,` add:
    /// The research provenance stamp the durable thread kept beside this artifact (EP-2 S4) and the
    /// version it was stamped on. Read only by research-to-deck (EP-5 S8), and only while that
    /// version is current, so a later revision never inherits an older report's stamp.
    provenance: Option<(u32, serde_json::Value)>,

## EP-5-S8
shared-edit apps/Model Plane/rust/services/model-gateway/src/artifacts.rs: directly after `pub fn kind_of(...) { ... }` add:
    /// Keep the provenance stamp a durable artifact carried at `version` (EP-5 S8).
    pub fn remember_provenance(&self, thread_id: &str, artifact_id: &str, version: u32, provenance: serde_json::Value) {
        if self.current_version(thread_id, artifact_id).is_some() {
            self.known.entry((thread_id.to_owned(), artifact_id.to_owned())).or_default().provenance = Some((version, provenance));
        }
    }

    /// The provenance stamp of the artifact's current version, if it had one.
    pub fn provenance_of(&self, thread_id: &str, artifact_id: &str) -> Option<serde_json::Value> {
        let current = self.current_version(thread_id, artifact_id)?;
        self.known
            .get(&(thread_id.to_owned(), artifact_id.to_owned()))
            .and_then(|entry| entry.provenance.clone())
            .filter(|(version, _)| *version == current)
            .map(|(_, provenance)| provenance)
    }

## EP-5-S8
shared-edit apps/Model Plane/rust/services/model-gateway/src/artifacts.rs: in `fork_thread`, right after the `if let Some(kind) = self.kind_of(thread_id, &artifact.id) { fork.remember_kind(...); }` block add:
                if let Some(provenance) = self.provenance_of(thread_id, &artifact.id) {
                    fork.remember_provenance(thread_id, &artifact.id, version, provenance);
                }

## EP-5-S8
shared-edit apps/Model Plane/rust/services/model-gateway/src/sse.rs (CRLF, file owned by EP-5-S3 this batch): in `rehydrate_persisted_artifacts`, right after the `if let Some(kind) = crate::artifacts::ArtifactKind::parse(&text("kind")) { state.artifact_versions.remember_kind(thread_id, &id, kind); }` block add:
        // Research-to-deck reads a report's stamp back from the store (EP-5 S8).
        if crate::deck_tool::from_artifact_enabled() {
            if let Some(Kind::StructValue(stamp)) = fields.fields.get("provenance").and_then(|value| value.kind.as_ref()) {
                state.artifact_versions.remember_provenance(thread_id, &id, version, crate::deck_tool::stamp_json(stamp));
            }
        }

## EP-5-S8
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the model-gateway environment, directly below line `      MODEL_GATEWAY_DECK_TOOL: ${MODEL_GATEWAY_DECK_TOOL:-false}` add `      MODEL_GATEWAY_DECK_FROM_ARTIFACT: ${MODEL_GATEWAY_DECK_FROM_ARTIFACT:-false}`

## EP-5-S6
shared-edit apps/Model Plane/rust/crates/mp-contracts/src/deck_manifest.rs: in `pub struct DeckManifest`, after `pub next_slide: u32,` add:
    /// The brand template the deck was last built with (EP-5 S6), set by the gateway from the turn's resolved pick, never by an operation. Absent for a deck in the default theme, so its canonical text is unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<crate::deck_theme::DeckTemplateRef>,
and in `DeckManifest::empty`, after `next_slide: 1,` add `template: None,` (CRLF file)

## EP-5-S6
shared-edit apps/Model Plane/rust/services/model-gateway/src/deck_templates.rs: in tests fn `bundle()`, after `created_at: "2026-11-04T10:00:00Z".to_owned(),` add `deck_template: false,` (new proto field 7 breaks the struct literal)

## EP-5-S6
shared-edit apps/Model Plane/rust/services/model-gateway/src/sse.rs: immediately before `let rounds = crate::tool_loop::run_tool_rounds_for_model_with_attachments(` (inside `if !tool_defs.is_empty() {`) insert:
            // The turn's brand deck template (EP-5 S6): resolved once from the request's own pick and held while the tool rounds run; the model never names one. Holds nothing while a switch is off or under ZDR.
            let _deck_template = crate::deck_tool::hold_turn_template(
                &session_state,
                &session_run_for_terminal.run_id,
                &org_clone,
                &user_clone,
                capability_bearer.as_ref().map(VerifiedCapabilityBearer::as_str),
                effective_zdr,
                req.deck_template.as_ref(),
            )
            .await;

## EP-5-S6
shared-edit apps/Model Plane/go/services/capability-core/internal/api/registry_apis.go: in `(h *SkillBundlesHandler) put`, right after `bundle, err := skillbundle.Parse(body)` insert:
	if err == nil && skillbundle.DeckTemplatesFromLookup(os.Getenv) {
		// EP-5 S6: a deck template's theme, master and logo are checked before anything is stored.
		err = skillbundle.ValidateDeckTemplate(bundle)
	}

## EP-5-S6
shared-edit apps/Model Plane/go/services/capability-core/cmd/main.go: replace `grpc.UnaryInterceptor(verifier.UnaryServerInterceptor(authz.AuthorizeGRPC))` with `grpc.UnaryInterceptor(verifier.UnaryServerInterceptor(authz.AuthorizeGRPCWith(authz.BundleReadRuleFromLookup(os.Getenv))))`; and append to the capSrv chain after `WithSkillBundles(skillbundle.NewStore(pool), api.SkillBundlesAPIFromLookup(os.Getenv))` the call `.
		WithDeckTemplates(skillbundle.DeckTemplatesFromLookup(os.Getenv))`

## EP-5-S6
shared-edit apps/Model Plane/deploy/docker-compose.yml: capability-core environment (next to `SKILL_BUNDLES_ENABLED: ${SKILL_BUNDLES_ENABLED:-false}`, ~line 2125) add `SKILL_DECK_TEMPLATES_ENABLED: ${SKILL_DECK_TEMPLATES_ENABLED:-false}` and `SKILL_BUNDLE_READ_RULE_ENABLED: ${SKILL_BUNDLE_READ_RULE_ENABLED:-false}`; model-gateway environment (next to `SKILL_BUNDLE_MOUNT_ENABLED`, ~line 354) add `SKILL_DECK_TEMPLATES_ENABLED: ${SKILL_DECK_TEMPLATES_ENABLED:-false}` if S7 did not already

## EP-7-S2
shared-edit apps/Frontend Plane/verevonv3/src/features/chat/components/use-chat-controller.ts: (1) add `import { applyGoalProgress } from './GoalBar'`. (2) In the stream handler object that ends `} satisfies Partial<ChatStreamHandlers>`, after the onRetrievalNotice handler, add: `onGoalProgress: (event: Parameters<NonNullable<ChatStreamHandlers['onGoalProgress']>>[0]) => { setThreadGoalState((current) => applyGoalProgress(current, event)) },`. (3) In saveGoal, change the input type to `GoalDraft` (from './GoalDialog') and pass `...(input.maxTurns !== undefined ? { maxTurns: input.maxTurns } : {}), ...(input.tokenBudget !== undefined ? { tokenBudget: input.tokenBudget } : {})` to both updateThreadGoal and putThreadGoal.

## EP-7-S2
shared-edit apps/Frontend Plane/verevonv3/src/shared/api/thread-goal-client.ts: add `tokenBudget?: number` to SetThreadGoalInput and to UpdateThreadGoalInput. In setThreadGoal's PUT body add `...(input.tokenBudget ? { tokenBudget: input.tokenBudget } : {})`. In updateThreadGoal's PATCH body add `...(input.tokenBudget !== undefined ? { tokenBudget: input.tokenBudget } : {})`. The gateway's SetGoalBody and UpdateGoalBody already accept token_budget.

## EP-7-S4
shared-edit apps/Model Plane/rust/services/model-gateway/src/lib.rs: add `pub mod goal_auto;` between `pub mod gateway_metrics;` and `pub mod goal_judge;` (CRLF).

## EP-7-S4
shared-edit apps/Model Plane/rust/services/model-gateway/src/normalize.rs: in test fn make_request's InvokeRequest literal, after `goal_id: None,` add `goal_auto: false,`.

## EP-7-S4
shared-edit apps/Model Plane/rust/services/model-gateway/src/zdr_features.rs: (1) in FIELD_TREATMENTS after the goal_id row add `    // EP-7 S4: a pump-sent goal turn continues a goal, so a ZDR turn carrying the mark is refused like the goal.` and `    ("goal_auto", refused_feature("thread_goal")),`; (2) in invoke_field_names' destructuring after `goal_id: _,` add `goal_auto: _,`; (3) in the names vec after `"goal_id",` add `"goal_auto",`; (4) in refusal() right after the goal_id check add `    if request.goal_auto {\r\n        return feature(crate::thread_goal_routes::ZDR_FEATURE);\r\n    }`.

## EP-7-S4
shared-edit apps/Model Plane/rust/services/model-gateway/tests/zdr_feature_table.rs cases(): after the context_refs rows add `("goal_auto", with("goal_auto", json!(true)), refused("thread_goal")),` and `("goal_auto", with("goal_auto", json!(false)), None),`. goal_id and deck_template also have no rows there yet, so every_field_has_a_row_that_matches_its_treatment may already fail; add `("goal_id", with("goal_id", json!("goal_1")), refused("thread_goal")), ("goal_id", with("goal_id", Value::Null), None),` if so.

## EP-7-S4
shared-edit apps/Model Plane/rust/services/model-gateway/src/thread_goal_routes.rs goal_body: change the last line to `Json(json!({ "data": { "found": goal.is_some(), "goal": goal, "autoContinueAvailable": crate::goal_auto::available() } }))`; in goal_body_answers_found_inside_data add `assert_eq!(body["data"]["autoContinueAvailable"], false);`.

## EP-7-S4
shared-edit apps/Model Plane/rust/services/model-gateway/src/thread_goal_routes.rs goal_error: before the NotFound arm add `tonic::Code::FailedPrecondition if message == "thread_goal_pending_decision" => typed(StatusCode::CONFLICT, "thread_goal_pending_decision", "a decision waits on you in this conversation"),` and `tonic::Code::ResourceExhausted if message == "thread_goal_rate_limited" => typed(StatusCode::TOO_MANY_REQUESTS, "thread_goal_rate_limited", "this goal continued too often in the last hour"),`. S2's ClaimGoalTurn refusal mapping in sse.rs must also pass both codes through as typed stream errors before any run exists.

## EP-7-S4
shared-edit apps/Frontend Plane/verevonv3/src/shared/api/thread-goal-client.ts: in type ThreadGoal after `revision?: number` add `  /** The gateway offers Keep going on this deployment (EP-7 S4, `autoContinueAvailable` on the read). */\n  autoContinueAvailable?: boolean`; in normalizeGoalResponse replace `return normalizeGoal(record.goal)` with `const goal = normalizeGoal(record.goal)\n  return goal && record.autoContinueAvailable === true ? { ...goal, autoContinueAvailable: true } : goal`.

## EP-7-S4
shared-edit apps/Frontend Plane/verevonv3/src/features/chat/components/ComposerTrays.tsx: add `import type { GoalAutoContinueView } from '../lib/goal-pump'`; in props after `onClearGoal?: () => void` add `  /** "Keep going" on the goal bar (EP-7 S4). */\n  goalAutoContinue?: GoalAutoContinueView`; in the <GoalBar> element after `onClear={() => props.onClearGoal?.()}` add `autoContinue={props.goalAutoContinue}`.

## EP-7-S4
shared-edit apps/Frontend Plane/verevonv3/src/features/chat/components/ChatPage.tsx: in the useChatController() destructuring after `continueGoal,` add `goalAutoContinue,`; on <ComposerTrays> after `onContinueGoal={() => void continueGoal()}` add `goalAutoContinue={goalAutoContinue}`.

## EP-7-S4
shared-edit apps/Frontend Plane/verevonv3/src/shared/actions/model-access.ts: after `'chat.continue_goal': exempt('no_owner_executor', 'external_write'),` add `'chat.goal_auto_toggle': exempt('agent_cannot_self_govern', 'context_write'),`.

## EP-7-S4
shared-edit apps/Model Plane/deploy/docker-compose.yml model-gateway environment: `MODEL_GATEWAY_GOAL_AUTO_CONTINUE: ${MODEL_GATEWAY_GOAL_AUTO_CONTINUE:-false}`. The owner adds the .env.example line.

