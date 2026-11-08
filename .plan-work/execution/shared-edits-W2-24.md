## WS27-S2
shared-edit apps/Model Plane/deploy/docker-compose.yml: add beside line 359 in the model-gateway environment block (MODEL_GATEWAY_ZDR_STREAM_COST line): MODEL_GATEWAY_TURN_USAGE_COMPLETE: ${MODEL_GATEWAY_TURN_USAGE_COMPLETE:-false}

## WS17-S8
shared-edit apps/Model Plane/rust/services/model-gateway/src/state.rs (CRLF file: keep CRLF; apply bottom-up so the line numbers hold): (e) after the line `            state.message_search_client = message_search_client;` in both from-env branches (lines 786 and 839) add `            state.message_tree_client = message_tree_client;`; (d) after the 5-line block that starts `        let message_search_client = MessageSearchClient::new(Self::lazy_channel(` and ends `        )?);` (lines 577-581) add `        let message_tree_client = MessageTreeServiceClient::new(Self::lazy_channel(`, then `            "SESSION_CORE_URL",`, `            "SESSION_CORE_ADDR",`, `            "http://localhost:9091",`, then `        )?);`; (c) in `pub fn new()`, after `            message_search_client: MessageSearchClient::new(session_channel.clone()),` (line 419) add `            message_tree_client: MessageTreeServiceClient::new(session_channel.clone()),`; (b) in `pub struct AppState`, after `    pub message_search_client: MessageSearchClient<Channel>,` (line 105) add the doc line `    /// Session Core's MessageTree service on the session_client endpoint (WS17-S8).` and `    pub message_tree_client: MessageTreeServiceClient<Channel>,`; (a) in the `use mp_contracts::model_plane::v1::{...}` list, after `    memory_service_client::MemoryServiceClient,` (line 18) add `    message_tree_service_client::MessageTreeServiceClient,`.

## WS17-S8
shared-edit apps/Model Plane/rust/services/model-gateway/src/run_context.rs (LF file): in the `ListConversationRequest {` literal in load_history (line 516, no ..Default::default()), after the line `            space_read_decision_token: String::new(),` (line 522) add `            include_inactive: false,`. This is the proto3 default, so the bytes sent do not change.

## WS17-S8
shared-edit apps/Model Plane/deploy/docker-compose.yml: (a) in the session-core environment, after `      SESSION_CORE_MESSAGE_SEARCH_ENABLED: ${SESSION_CORE_MESSAGE_SEARCH_ENABLED:-false}` (line 910), add a WS17-S8 comment and `      SESSION_CORE_MESSAGE_VERSIONS_ENABLED: ${SESSION_CORE_MESSAGE_VERSIONS_ENABLED:-false}`; (b) in the model-gateway environment, after `      MODEL_GATEWAY_SEND_RECEIPTS: ${MODEL_GATEWAY_SEND_RECEIPTS:-true}` (line 325), add a WS17-S8 comment and `      MODEL_GATEWAY_MESSAGE_VERSIONS_ENABLED: ${MODEL_GATEWAY_MESSAGE_VERSIONS_ENABLED:-false}`. Both default off; set them together.

## WS26-S7
shared-edit apps/Model Plane/deploy/docker-compose.yml (execution-core service environment): declare CONTENT_SAFETY_SCREEN_RPC with default false. Not edited here: the file is not owned by this slice.

## WS26-S7
shared-edit apps/Model Plane/docs/CONTENT_SAFETY.md: add a Do-mode section (switch CONTENT_SAFETY_SCREEN_RPC, ZDR skip, fail-open to local, tighten-only merge, withhold before render, taint, the deviations listed above). Not edited here.

## WS19-S9
shared-edit apps/Model Plane/rust/services/model-gateway/Dockerfile: in the builder stage, after line 43 (`COPY rust/ rust/`, before `WORKDIR /app/rust`), add `COPY skills/office/ /app/skills/office/`. Required before the gateway image builds: builtin_skills.rs embeds the four SKILL.md files with include_str!, and this Dockerfile copies only rust/ and proto/.

## WS19-S9
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the model-gateway service environment, after line 320 (`SKILL_FILE_TOOL_ENABLED: ${SKILL_FILE_TOOL_ENABLED:-false}`), add a comment line `      # WS19-S9: first-party Office skills (docx, xlsx, pptx, pdf) listed for every org. Off by default; only the literal`, a second comment line `      # `true` turns it on.`, and the variable line `      BUILTIN_OFFICE_SKILLS_ENABLED: ${BUILTIN_OFFICE_SKILLS_ENABLED:-false}`.

## WS19-S9
shared-edit apps/Model Plane/rust/services/model-gateway/src/lib.rs (optional; replaces the #[path] workaround): add `pub mod builtin_skills;` after `pub mod budget;`. Then in skills.rs delete the #[path] comment, the attribute, the module line and the blank line after the mp_contracts use block; make the field `builtin_office` pub(crate); change the four `builtin_skills::` calls (SkillStore::new, list, get, score) to `crate::builtin_skills::`; and give the builtin tests a pub(crate) SkillStore constructor, because the test module stops being a child of skills and cannot set private fields. Re-run the lib tests after this.

