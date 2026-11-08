## EP-11-S3
shared-edit apps/Model Plane/rust/services/model-gateway/src/state.rs: (1) in the `use mp_contracts::model_plane::v1::{...}` list add `skill_version_service_client::SkillVersionServiceClient,` (after `session_core_client::SessionCoreClient, ...`); (2) after the field `pub artifact_revision_client: ArtifactRevisionServiceClient<Channel>,` add `/// Session Core's SkillVersionService (EP-11 S3: recovery reads a run's pinned skill version) on the session_client endpoint.\n    pub skill_version_client: SkillVersionServiceClient<Channel>,`; (3) in the test/default constructor after `artifact_revision_client: ArtifactRevisionServiceClient::new(session_channel.clone()),` add `skill_version_client: SkillVersionServiceClient::new(session_channel.clone()),`; (4) in from_env after the `let artifact_revision_client = ArtifactRevisionServiceClient::new(Self::lazy_channel("SESSION_CORE_URL", "SESSION_CORE_ADDR", "http://localhost:9091")?);` block add the same block as `let skill_version_client = SkillVersionServiceClient::new(Self::lazy_channel("SESSION_CORE_URL", "SESSION_CORE_ADDR", "http://localhost:9091")?);`; (5) in BOTH branches after `state.artifact_revision_client = artifact_revision_client;` add `state.skill_version_client = skill_version_client;`. tool_loop.rs pinned_skill_recovery uses `state.skill_version_client`.

## EP-11-S3
shared-edit apps/Model Plane/rust/services/model-gateway/src/deck_templates.rs: in test fn `skill(id, name) -> Skill` after `min_score: 0.0,` add `            version: 0,\n            digest: String::new(),` (Skill gained gateway.proto fields 7/8).

## EP-11-S3
shared-edit apps/Model Plane/deploy/docker-compose.yml (model-gateway environment, after the `BUILTIN_OFFICE_SKILLS_ENABLED: ${BUILTIN_OFFICE_SKILLS_ENABLED:-false}` line): `      # EP-11 S3: a run records the skill versions it injected and recovery inside the run reads the pinned version. Off\n      # by default; only the literal `true` turns it on. Needs SKILL_VERSIONS_API on in session-core for recovery reads.\n      MODEL_GATEWAY_SKILL_PINS: ${MODEL_GATEWAY_SKILL_PINS:-false}`

## EP-11-S6
shared-edit apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs (~2488, pb::ListAgentSkillsRequest): add `kinds: Vec::new(),` after `for_user_id: String::new(),` (needed to compile after proto field kinds=4).

## EP-11-S6
shared-edit apps/Model Plane/rust/services/execution-core/src/runtime_loop/mod.rs (~3798, ListAgentSkillsRequest): add `kinds: Vec::new(),` after `for_user_id: String::new(),`.

## EP-11-S6
shared-edit apps/Model Plane/rust/services/model-gateway/src/grpc.rs (~2075), src/sse.rs (~8230, fetch_skill_context) and src/tool_loop.rs (~4649): in each ListAgentSkillsRequest literal add `kinds: Vec::new(),` after `for_user_id: String::new(),`, with a comment in sse.rs: `// EP-11 S6: no kinds = skills only; a template or preset is never matched or injected.`

## EP-11-S6
shared-edit apps/Model Plane/rust/services/model-gateway/src/sse.rs tests: add `#[test] fn templates_never_injected_as_skills()`, a source-text test asserting that the fetch_skill_context ListAgentSkillsRequest literal contains `kinds: Vec::new()` and never names "template"/"preset".

## EP-11-S6
shared-edit apps/Frontend Plane/verevonv3/src/shared/actions/model-access.ts after line `'skills.versions.list': exempt('no_owner_executor', 'read'),` add: `'templates.create': exempt('agent_cannot_self_govern', 'context_write'),` `'templates.update': exempt('agent_cannot_self_govern', 'context_write'),` `'presets.create': exempt('agent_cannot_self_govern', 'context_write'),` `'presets.update': exempt('agent_cannot_self_govern', 'context_write'),` (MODEL_ACCESS is Record<ActionId,...>, so this is required for typecheck).

## EP-11-S6
shared-edit apps/Model Plane/rust/services/model-gateway/src/http_routes.rs list_skills_proxy (~8370) and get_skill_proxy: forward a `kind` query param only when it is exactly skill|template|preset (e.g. `Query(q): Query<HashMap<String,String>>`, then append `&kind={k}` to `skills?org_id=`). Today the proxy drops the query string, so ?kind=template reaches capability-core as a skills list. The SPA filters by row kind, so nothing leaks, but templates and presets never appear.

## EP-11-S6
shared-edit apps/Model Plane/deploy/docker-compose.yml capability-core environment: `AUTHORED_KINDS_ENABLED: ${AUTHORED_KINDS_ENABLED:-false}`.

## EP-11-S6
shared-edit apps/Frontend Plane/verevonv3/src/features/settings/components/SkillsSection.tsx: mount <TemplateEditor/> and <PresetEditor/> (features/skills/components) behind a 'New template' / 'New preset' button, shown only when GET /api/v1/skills?kind=template answers kinds_enabled:true.

## EP-12-S10
shared-edit apps/Model Plane/rust/services/model-gateway/src/state.rs: (a) in the `use mp_contracts::model_plane::v1::{` block insert `    thread_export_service_client::ThreadExportServiceClient,` directly above `    thread_fork_service_client::ThreadForkServiceClient,`; (b) in `pub struct AppState`, after `    pub thread_goal_client: ThreadGoalServiceClient<Channel>,`, insert the doc line `    /// Session Core's ThreadExportService on the session_client endpoint (EP-12 S10).` and the field `    pub thread_export_client: ThreadExportServiceClient<Channel>,`; (c) in `AppState::new()`, after `            thread_goal_client: ThreadGoalServiceClient::new(session_channel.clone()),`, insert `            thread_export_client: ThreadExportServiceClient::new(session_channel.clone()),`; (d) in `from_env()`, after the statement `let thread_goal_client = ThreadGoalServiceClient::new(Self::lazy_channel(...)?);`, insert `let thread_export_client = ThreadExportServiceClient::new(Self::lazy_channel("SESSION_CORE_URL", "SESSION_CORE_ADDR", "http://localhost:9091")?);` wrapped as its neighbours are; (e) in BOTH branches of `from_env()`, after each `            state.thread_goal_client = thread_goal_client;`, insert `            state.thread_export_client = thread_export_client;`. thread_export_routes.rs does not compile until (a) to (e) apply.

## EP-12-S10
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the model-gateway service environment, after the line `      MODEL_GATEWAY_THREAD_GOALS_ENABLED: ${MODEL_GATEWAY_THREAD_GOALS_ENABLED:-false}` and before the `# EP-7 S4` comment, insert `      # EP-12 S10: the owner's conversation export route (JSON and Markdown). Only the literal true.` and `      MODEL_GATEWAY_THREAD_EXPORT: ${MODEL_GATEWAY_THREAD_EXPORT:-false}`.

