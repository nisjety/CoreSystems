## EP-11-S5
shared-edit apps/Model Plane/rust/services/model-gateway/src/http_routes.rs: after `.route("/v1/skills/:id/versions", get(list_skill_versions_proxy))` add `.route("/v1/skills/:id/publish", post(publish_skill_proxy))`, `.route("/v1/skills/:id/draft", delete(discard_skill_draft_proxy))` and `.route("/v1/skills/:id/versions/:version/restore", post(restore_skill_version_proxy))` (use the routing fn names already imported in that file). After `delete_skill_proxy` add three handlers in its shape (State, Extension<Claims>, VerifiedCapabilityBearer, Path): `publish_skill_proxy` -> `proxy_to_capability_core(&s, &c, &bearer, &format!("skills/{id}/publish"), "POST", None)`; `discard_skill_draft_proxy` -> `&format!("skills/{id}/draft")`, "DELETE", None; `restore_skill_version_proxy` with `Path((id, version)): Path<(String, u32)>` -> `&format!("skills/{id}/versions/{version}/restore")`, "POST", None. Doc comment: EP-11 S5, capability-core answers 404 while SKILL_DRAFT_PUBLISH is off.

## EP-11-S5
shared-edit apps/Model Plane/rust/services/model-gateway/src/deck_templates.rs (~line 283): in the `GetSkillBundleManifestRequest { skill_id: skill_id.to_owned(), }` literal add `manifest_sha256: String::new(),` after `skill_id`. Without it the crate does not compile once the proto is regenerated.

## EP-11-S5
shared-edit apps/Model Plane/go/services/capability-core/cmd/main.go (~line 501-502): after `WithDeckTemplates(skillbundle.DeckTemplatesFromLookup(os.Getenv))` add `.WithSkillBundleRetention(skillbundle.RetentionFromLookup(os.Getenv))`. This is the gRPC server wiring; the HTTP handlers read the switch themselves.

## EP-11-S5
shared-edit apps/Frontend Plane/verevonv3/src/shared/actions/model-access.ts: after line `'skills.versions.list': exempt('no_owner_executor', 'read'),` add `'skills.draft.save': exempt('agent_cannot_self_govern', 'context_write'),`, `'skills.publish': exempt('agent_cannot_self_govern', 'context_write'),`, `'skills.draft.discard': exempt('agent_cannot_self_govern', 'context_write'),` and `'skills.version.restore': exempt('agent_cannot_self_govern', 'context_write'),`. MODEL_ACCESS is a Record<ActionId,...>, so typecheck fails without them.

## EP-11-S5
shared-edit apps/Model Plane/deploy/docker-compose.yml capability-core environment (beside `AUTHORED_KINDS_ENABLED: ${AUTHORED_KINDS_ENABLED:-false}`, ~line 2194): add a comment `# EP-11 S5: edits of accepted skills go through a draft (publish/discard/restore routes). Needs session-core 0110. Only the literal true enables.` with `SKILL_DRAFT_PUBLISH: ${SKILL_DRAFT_PUBLISH:-false}`, and a comment `# EP-11 S5: keep replaced skill bundles per version. Keep off until D2-379/D2-383 (bundle read authz) are fixed. Needs session-core 0108+0110 and capability-core 0037.` with `SKILL_BUNDLE_RETENTION: ${SKILL_BUNDLE_RETENTION:-false}`.

## EP-11-S9
shared-edit apps/Model Plane/rust/services/model-gateway/src/org_agent_policy.rs: in struct OrgPolicy, after `pub web: WebCaps,` add `    /// The org's `ext.skills` token (EP-11 S9): `none`, `approved_only` or `any`, or the schema's fail-closed `none` for a value this build cannot read. Read by `skills::held_ext_skills` only while SKILL_EXT_POLICY_ENFORCED is on.\n    pub ext_skills: String,`; in OrgPolicy::from_settings, after the `web: WebCaps { ... },` initializer add `            ext_skills: token("ext.skills"),` (skills.rs does not compile without this).

## EP-11-S9
shared-edit apps/Model Plane/rust/services/model-gateway/src/grpc.rs (~line 2072, ListAgentSkillsRequest literal): after `kinds: Vec::new(),` add `thread_id: String::new(),`

## EP-11-S9
shared-edit apps/Model Plane/rust/services/model-gateway/src/tool_loop.rs (~line 4710, ListAgentSkillsRequest literal): after `kinds: Vec::new(),` add `thread_id: String::new(),`

## EP-11-S9
shared-edit apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs (~line 2485, ListAgentSkillsRequest literal): after `kinds: Vec::new(),` add `thread_id: String::new(),`; and (~line 12908, test mock ListAgentSkillsResponse literal) after `skills: self.rec.lock().unwrap().agent_skills.clone(),` add `space_id: String::new(),`

## EP-11-S9
shared-edit apps/Model Plane/rust/services/execution-core/src/runtime_loop/mod.rs (~line 3792, ListAgentSkillsRequest literal): after `kinds: Vec::new(),` add `thread_id: String::new(),`

## EP-11-S9
shared-edit apps/Model Plane/deploy/docker-compose.yml: add `      SKILL_SPACE_SCOPE: ${SKILL_SPACE_SCOPE:-false}` to session-core (beside SKILL_VERSIONS_API ~line 1218), to capability-core (beside AUTHORED_KINDS_ENABLED ~line 2194) and to model-gateway (beside MODEL_GATEWAY_SKILL_PINS ~line 358); and add `      SKILL_EXT_POLICY_ENFORCED: ${SKILL_EXT_POLICY_ENFORCED:-false}` to model-gateway, each with a comment line: '# EP-11 S9: only the literal `true` turns it on; SKILL_EXT_POLICY_ENFORCED needs ORG_AGENT_POLICY_MODE=enforce or every authored skill is withheld (fail closed).'

## EP-11-S9
shared-edit apps/Frontend Plane/verevonv3/src/features/customize/CustomizeHub.tsx: import `{ SkillLibrary } from '@/features/skills/components/SkillLibrary'` and `Show` from solid-js; add `const skillLibraryEnabled = () => import.meta.env.VITE_SKILL_LIBRARY === 'true'`; render `<Show when={skillLibraryEnabled()}><SkillLibrary /></Show>` right after `<SkillsSection />` (the owner adds VITE_SKILL_LIBRARY to env examples).

## EP-4-S9
shared-edit apps/Frontend Plane/verevonv3/src/shared/actions/model-access.ts: directly after the line `  'chat.open_widget_link': exempt('ui_state_only', 'read'),` add `  'chat.open_mcp_app': exempt('ui_state_only', 'read'),` and `  'chat.stage_mcp_app_message': exempt('ui_state_only', 'read'),`

## EP-12-S4
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the session-core environment block, directly after the line `      SESSION_RUN_ORIGIN_INHERIT: ${SESSION_RUN_ORIGIN_INHERIT:-false}`, insert `      # EP-12 S4: admit the api_key principal on its closed method list. Only the literal true.` and `      SESSION_CORE_API_KEY_PRINCIPAL: ${SESSION_CORE_API_KEY_PRINCIPAL:-false}`

## EP-12-S4
shared-edit apps/Model Plane/docs/ZDR.md: add one refusal row: "EP-12 S4 session-core api_key principal: a ZDR api_key token is refused on CreateThread, AppendMessage, StartRun and CancelRun with zdr_feature_not_supported (feature agent_api), before any handler runs; reads are served (nothing is stored for a ZDR key); a key never carries a waiver; key-owned threads/runs live in the existing threads/runs tables (user_id = apikey:<key_id>) and are erased by the org purge (gdpr.rs)."

