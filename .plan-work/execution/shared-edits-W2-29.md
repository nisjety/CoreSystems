## WS27-S4
shared-edit apps/Frontend Plane/verevonv3/src/features/chat/components/ChatPage.tsx: add the line `          meter={activeRunTurn()?.meter}` after line 787 (`toolCalls={activeRunTurn()?.toolCalls}`, inside the `<StepsPanel` element), and the line `        meter={activeRunTurn()?.meter}` after line 1216 (`runId={liveRunId()}`, inside the `<ChatLiveRunPanel` element). Without these lines neither the Work panel nor the live rail receives a meter.

## WS27-S4
shared-edit apps/Frontend Plane/verevonv3/playwright.config.ts: in `const CHAT_WORKSPACE_SPECS` change the regex to `/chat-(?:workspace|voice-mode|transcript-performance|attachment-gates|cost-meter)\.spec\.ts/` so tests/e2e/chat-cost-meter.spec.ts runs in the chat project. Otherwise it falls into the e2e project, which needs the setup login.

## WS27-S4
shared-edit apps/Model Plane/deploy/docker-compose.yml line 374: `MODEL_GATEWAY_USAGE_PROGRESS: ${MODEL_GATEWAY_USAGE_PROGRESS:-false}` becomes `MODEL_GATEWAY_USAGE_PROGRESS: ${MODEL_GATEWAY_USAGE_PROGRESS:-true}`. Apply ONLY after the live proof below passes, as the S4 DoD requires.

## WS16-S1
shared-edit apps/Model Plane/rust/crates/mp-orchestration/src/proto_shim.rs: line 638, change `ProtoEvent::QuestionAsked(_) | ProtoEvent::QuestionAnswered(_) => {` to `ProtoEvent::QuestionAsked(_) | ProtoEvent::QuestionAnswered(_) | ProtoEvent::RunUsageProgress(_) => {`. This reuses the existing ShimError::UnsupportedEvent arm and fixes the E0004 at line 515. It is WS27-S9's tag-29 change; WS27-S9 decides whether the usage mirror should map the event instead.

## WS16-S1
shared-edit apps/Model Plane/rust/services/model-gateway/src/state.rs (CRLF): (a) add `attention_service_client::AttentionServiceClient,` to the `use mp_contracts::model_plane::v1::{...}` block at the top; (b) after line 108 (`pub message_tree_client: MessageTreeServiceClient<Channel>,`) add `/// Session Core's Attention service on the session_client endpoint (WS16-S1).` and `pub attention_client: AttentionServiceClient<Channel>,`; (c) in AppState::new(), after line 424 (`learning_client: LearningGovernanceServiceClient::new(session_channel.clone()),`) add `attention_client: AttentionServiceClient::new(session_channel.clone()),`; (d) in from_env(), after line 595 (the `)?);` closing `let learning_client`) add a block `let attention_client = AttentionServiceClient::new(Self::lazy_channel(` / `"SESSION_CORE_URL",` / `"SESSION_CORE_ADDR",` / `"http://localhost:9091",` / `)?);` in the file's multi-line layout; (e) after lines 797 and 851 (both `state.learning_client = learning_client;`) add `state.attention_client = attention_client;`.

## WS16-S1
shared-edit apps/Model Plane/rust/services/model-gateway/tests/support/offline_downstreams.rs (CRLF): add `attention_service_client::AttentionServiceClient,` to the `use mp_contracts::model_plane::v1::{...}` block (starts line 32), and after line 73 (`state.learning_client = LearningGovernanceServiceClient::new(offline_channel());`) add `state.attention_client = AttentionServiceClient::new(offline_channel());`. offline_downstreams_contract.rs fails until this line exists.

## WS16-S1
shared-edit apps/Model Plane/deploy/docker-compose.yml, session-core service, after line 934 (`      SESSION_CORE_MESSAGE_SEARCH_ENABLED: ${SESSION_CORE_MESSAGE_SEARCH_ENABLED:-false}`) add `      # WS16-S1: attention state on thread listings, ListAttention and MarkThreadSeen. Off by default; only true turns it on.` and `      SESSION_CORE_ATTENTION_ENABLED: ${SESSION_CORE_ATTENTION_ENABLED:-false}`. The switch is read by attention::enabled() through mp_contracts::loop_fold::switch_from.

## WS27-S9
shared-edit apps/Model Plane/rust/crates/mp-orchestration/src/proto_shim.rs: lines 638-640 (CRLF file, keep CRLF). Replace line 638, `            ProtoEvent::QuestionAsked(_) | ProtoEvent::QuestionAnswered(_) => {`, with these three lines, and keep lines 639 (`                return Err(ShimError::UnsupportedEvent);`) and 640 (`            }`) as they are: `            ProtoEvent::QuestionAsked(_)` / `            | ProtoEvent::QuestionAnswered(_)` / `            | ProtoEvent::RunUsageProgress(_) => {`. The one-line form is 113 columns, over rustfmt's 100. No rustfmt.toml exists in the worktree, and rustfmt 1.9.0 produces exactly these three lines (probe checked).

## WS27-S9
shared-edit apps/Model Plane/deploy/docker-compose.yml: after line 1264 (`      EXECUTION_EFFECT_LEDGER_ENABLED: ${EXECUTION_EFFECT_LEDGER_ENABLED:-false}`), add `      EXECUTION_RUN_USAGE_PROGRESS: ${EXECUTION_RUN_USAGE_PROGRESS:-false}` with a one-line comment saying it is off by default and only the literal true enables it. The key is not present yet. The file is LF. MODEL_GATEWAY_USAGE_PROGRESS is already passed through at line 374.

