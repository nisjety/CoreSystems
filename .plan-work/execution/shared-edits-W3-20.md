## EP-11-S2
shared-edit apps/Model Plane/rust/services/model-gateway/src/http_routes.rs: after the `.route("/v1/skills/:id", get(get_skill_proxy).patch(patch_skill_proxy).delete(delete_skill_proxy),)` block add `.route("/v1/skills/:id/versions", get(list_skill_versions_proxy))`. After delete_skill_proxy add: `/// EP-11 S2: a skill's immutable versions (capability-core GET /api/v1/skills/{id}/versions, 404 while SKILL_VERSIONS_API is off). Only the paging parameters pass.\nasync fn list_skill_versions_proxy(State(s): State<AppState>, Extension(c): Extension<Claims>, bearer: VerifiedCapabilityBearer, Path(id): Path<String>, Query(q): Query<std::collections::HashMap<String, String>>) -> Result<Json<Value>, HttpJsonError> { let mut path = format!("skills/{id}/versions"); let paging: Vec<String> = ["limit", "before_version"].iter().filter_map(|key| q.get(*key).filter(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit())).map(|v| format!("{key}={v}"))).collect(); if !paging.is_empty() { path.push('?'); path.push_str(&paging.join("&")); } proxy_to_capability_core(&s, &c, &bearer, &path, "GET", None).await }`. Import axum::extract::Query if it is not already imported. Without this route the BFF history call reaches a model-gateway 404.

## EP-11-S2
shared-edit apps/Frontend Plane/verevonv3/src/shared/actions/model-access.ts: after the line `'skills.save_from_message': exempt('agent_cannot_self_govern', 'context_write'),` add `'skills.versions.list': exempt('no_owner_executor', 'read'),`.

## EP-11-S2
shared-edit apps/Frontend Plane/verevonv3/src/shared/actions/action-registry.test.ts: in the registry count test, add the comment line `// +1 after EP-11-S2 registered skills.versions.list, the Skills page's history read.` before `expect(ids).toHaveLength(...)`, and raise that count by 1 (304 in this tree, before the batch merges).

## EP-11-S2
shared-edit apps/Model Plane/deploy/docker-compose.yml: add `SKILL_VERSIONS_API: ${SKILL_VERSIONS_API:-false}` to the environment of capability-core AND session-core.

## EP-11-S7
shared-edit apps/Model Plane/rust/crates/mp-contracts/src/zdr_refusal.rs: in FEATURES, after the legal_hold Feature entry and before the closing `];`, append:
    // EP-11 S7: /learn sends a thread's transcript to the review model and keeps what it distils, so a
    // ZDR or waived caller, and a thread holding a ZDR or waived run, is refused before any read.
    Feature {
        key: "learn_command",
        label: "learning from this conversation",
    },
    // EP-11 S1/S8: saving, editing or importing a skill is configuration a ZDR credential cannot write.
    Feature {
        key: "skill_authoring",
        label: "saving or editing skills",
    },
(Required: feature_not_supported debug_asserts the key; api/contracts/zdr-refusals.json is already edited to match and its .sha256 is now 422af9d1fe92dafeffc255fcc551dbc47dce69698d267e06e24a56fb94a2b920. Regenerate with MP_UPDATE_GOLDEN=1 if the golden diff disagrees.)

## EP-11-S7
shared-edit apps/Model Plane/go/services/capability-core/cmd/main.go: just after `api.NewMemoryHandler(pool, runClient).WithMemoryControls(memoryClient).Register(protectedMux)`, add (also importing llmreviewer is not needed):
	learnOn := api.LearnCommandAPIFromLookup(os.Getenv) && learningReviewEnabled()
	var learner *sessionreview.Learner
	if learnOn && sessionClient != nil && inferenceClient != nil && memoryClient != nil {
		learnOpts := []sessionreview.Option{sessionreview.WithMemoryControls(memoryClient)}
		if cost, costOn, costErr := sessionreview.CostControlsFromLookup(os.Getenv); costErr == nil && costOn {
			learnOpts = append(learnOpts, sessionreview.WithCostControls(cost))
		}
		learner = sessionreview.NewLearner(sessionClient, inferenceClient, os.Getenv("LEARNING_REVIEW_MODEL"), learnOpts...)
	}
	api.NewLearnCommandHandler(learnOn, runClient, learner).Register(protectedMux)
(Check memoryClient's static type is mpv1.MemoryServiceClient; WithMemoryControls takes it structurally.)

## EP-11-S7
shared-edit apps/Model Plane/deploy/docker-compose.yml, capability-core environment, after `LEARNING_REVIEW_MAX_PER_ORG_PER_DAY: ${LEARNING_REVIEW_MAX_PER_ORG_PER_DAY:-100}`:
      # EP-11 S7 /learn: a person's request to distil one thread into a skill proposal. Off unless
      # exactly true; it also needs LEARNING_REVIEW_ENABLED and a review counter (else budget_unavailable).
      LEARN_COMMAND_ENABLED: ${LEARN_COMMAND_ENABLED:-false}

## EP-11-S7
shared-edit apps/Model Plane/docs/ZDR.md: in the feature table after the `**Legal holds (EP-9 C1)**` row, add:
| **/learn (EP-11 S7)** | Refused for ZDR and waived callers and threads | model-gateway refuses a ZDR or waived token, and a thread holding a ZDR or waived run (thread reference read), with zdr_feature_not_supported, feature learn_command, before any transcript read or model call; capability-core refuses a ZDR principal and a ZDR org (memory controls reason zdr) the same way (no posture-matrix row). Proposals are disabled, owner-scoped, origin background_review, erased like WS07 proposals. Built dark behind LEARN_COMMAND_ENABLED (off). |

## EP-11-S7
shared-edit apps/Frontend Plane/verevonv3/src/shared/actions/model-access.ts: after the `'learning.undo': exempt('agent_cannot_self_govern', 'destructive'),` line, add `  'learning.learn_from_thread': exempt('agent_cannot_self_govern', 'context_write'),`

## EP-11-S7
shared-edit apps/Frontend Plane/verevonv3/src/shared/actions/action-registry.test.ts: in the id-count test, add the comment line `    // +1 after EP-11-S7 registered learning.learn_from_thread, the /learn command.` and raise `toHaveLength(N)` by 1 (304 to 305 on this base, after the other batch bumps).

## EP-11-S7
shared-edit apps/Frontend Plane/verevonv3/apps/gateway/src/domains/actions/dispatchers.rs + handlers.rs: dispatch `learning.learn_from_thread` like `learning.undo` does, calling `crate::domains::learning::learn_from_thread` with `LearnBody { thread_id: input.threadId, note: input.note }` (or tell me the dispatcher pattern and I will add it).

## EP-11-S7
shared-edit apps/Frontend Plane/verevonv3/src/features/dashboard/home/DashboardComposer.tsx: register a `/learn` command in `slashRegistry` (id `builtin:__learn`, name `learn`, aliases [`lær`, `laer`], label LEARN_ACTION.name, description no `Lær av denne samtalen: Verevon foreslår en ferdighet.` / en LEARN_ACTION.description). Register it only while `learnCommandAvailable({threadId, temporaryChat: temporaryChat(), zeroRetention, learningAllowed})` is true, with learningAllowed from the memory controls. Its run calls `requestLearnFromThread(threadId, note)` (note = the rest of the line) and shows the typed refusal; add the composer test `learn_hidden_on_temporary_chat`.

## EP-4-S8
shared-edit apps/Model Plane/deploy/docker-compose.yml: on the model-gateway service environment add `MCP_APPS_ENABLED: ${MCP_APPS_ENABLED:-false}` (EP-4 S12 flips it; default off)

## EP-12-S1
shared-edit apps/Model Plane/rust/services/session-core/src/terminalization.rs (REQUIRED for the S1 Done-when: execution-core subagents start through StartManagedRun, whose insert_managed_run_started writes no metadata). (a) In fn insert_managed_run_started, directly after the line `    let residency = super::grpc::configured_residency();` (line 580), insert this block: `    // EP-12 S1: a child run inherits its parent's origin token behind` / `    // SESSION_RUN_ORIGIN_INHERIT. A managed run is not stamped at birth, so a run` / `    // with no inherited origin keeps the {} it has always stored.` / `    let inherited_origin = crate::run_origin_stamp::inherited_origin(` / `        &mut **transaction,` / `        &request.org_id,` / `        &persisted.parent_run_id,` / `        crate::run_origin_stamp::inherit_enabled(` / `            std::env::var(crate::run_origin_stamp::INHERIT_ENV)` / `                .ok()` / `                .as_deref(),` / `        ),` / `    )` / `    .await?;` / `    let origin_metadata = match inherited_origin {` / `        Some(origin) => {` / `            serde_json::json!({ mp_contracts::run_origin::ORIGIN_METADATA_KEY: origin.as_str() })` / `        }` / `        None => serde_json::json!({}),` / `    };`. (b) In the INSERT column list (line 598) replace `                          turn_origin, trigger_message_id, agent_subject_id)` with `                          turn_origin, trigger_message_id, agent_subject_id, metadata)`. (c) In the SELECT list (line 601) replace `                $11, $12, $13, $14, $15, $16` with `                $11, $12, $13, $14, $15, $16, $17`. (d) After line 622 `    .bind(wake.map(|w| w.agent_subject_id.as_str()))` add `    .bind(origin_metadata)`. With the switch off the read is skipped and the stored value is {}, the column default.

## EP-12-S1
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the model-gateway environment, after line 374 (`      MODEL_GATEWAY_MESSAGE_VERSIONS_ENABLED: ${MODEL_GATEWAY_MESSAGE_VERSIONS_ENABLED:-false}`) add `      # EP-12 S1: the top-level origin on run projections. Only the literal true turns it on.` and `      MODEL_GATEWAY_RUN_ORIGIN_FIELD: ${MODEL_GATEWAY_RUN_ORIGIN_FIELD:-false}`. In the session-core environment, after line 1196 (`      SESSION_CORE_RUN_LEDGER_READ: ${SESSION_CORE_RUN_LEDGER_READ:-false}`) add `      # EP-12 S1: a child run inherits its parent's origin token. Only the literal true turns it on.` and `      SESSION_RUN_ORIGIN_INHERIT: ${SESSION_RUN_ORIGIN_INHERIT:-false}`. Both default off.

## EP-12-S9
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the session-core environment block, directly after the line `      SESSION_CORE_THREAD_GOALS_ENABLED: ${SESSION_CORE_THREAD_GOALS_ENABLED:-false}`, add two lines: `      # EP-12 S9: the owner's ExportThread (ThreadExportService). Only the literal true.` and `      SESSION_THREAD_EXPORT_ENABLED: ${SESSION_THREAD_EXPORT_ENABLED:-false}`

## EP-12-S9
shared-edit apps/Model Plane/api/contracts/zdr-posture-matrix.json: no row added. The refusal is keyed on the caller's bearer, which existing rows already cover. The design names e_zdr_caller_export_refused and e_waived_run_thread_export_refused; add them only if they differ from existing rows on resolver inputs. Otherwise record the refusal in docs/ZDR.md and the feature table (conversation_export).

