## WS07-S9
shared-edit C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Model Plane/rust/services/model-gateway/src/state.rs (CRLF; clean against HEAD): (a) in the mp_contracts::model_plane::v1 use list, after the line '    inference_core_client::InferenceCoreClient,' add '    learning_governance_service_client::LearningGovernanceServiceClient,'. (b) in struct AppState, after '    pub message_search_client: MessageSearchClient<Channel>,' add the doc line /// Session Core LearningGovernance service on the same endpoint as session_client (WS07-S9). and '    pub learning_client: LearningGovernanceServiceClient<Channel>,'. (c) in AppState::new(), after '            message_search_client: MessageSearchClient::new(session_channel.clone()),' add '            learning_client: LearningGovernanceServiceClient::new(session_channel.clone()),' (session_channel is still unmoved there). (d) in from_env, directly after the 'let message_search_client = MessageSearchClient::new(Self::lazy_channel(' block (it ends with '        )?);'), add 'let learning_client = LearningGovernanceServiceClient::new(Self::lazy_channel(' with the same three string arguments (SESSION_CORE_URL, SESSION_CORE_ADDR, http://localhost:9091) and closing as that block. (e) in BOTH from_env branches, after each '            state.message_search_client = message_search_client;' add '            state.learning_client = learning_client;'.

## WS07-S9
shared-edit C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Model Plane/rust/services/model-gateway/tests/support/offline_downstreams.rs (CRLF): in the mp_contracts::model_plane::v1 import list, after '    inference_core_client::InferenceCoreClient,' add '    learning_governance_service_client::LearningGovernanceServiceClient,', and after '    memory_service_client::MemoryServiceClient,' add '    message_search_client::MessageSearchClient,'. In offline_state(), after '    state.session_client = SessionCoreClient::new(offline_channel());' add '    state.message_search_client = MessageSearchClient::new(offline_channel());' and '    state.learning_client = LearningGovernanceServiceClient::new(offline_channel());'. Needed because the contract test requires every AppState client field to be replaced here; message_search_client is already missing at HEAD.

## WS07-S9
shared-edit C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Model Plane/rust/services/model-gateway/tests/offline_downstreams_contract.rs (CRLF): in one_call_per_client!, after '            call!(state, session_client.create_thread),' add '            call!(state, message_search_client.search_messages),' and '            call!(state, learning_client.list_learning_ledger),'. Line 94: the 17 in assert_eq!(answers.len(), 17, ...) becomes 19. Line 97: 'all seventeen answered' becomes 'all nineteen answered'. Line 172, the line reading '        17,': becomes '        19,' (the field count). HEAD is 18 fields against the hard-coded 17, so this test already fails; after this change the count is 19.

## WS07-S9
shared-edit C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Frontend Plane/verevonv3/src/shared/actions/model-access.ts (LF): after the row for memory.remember (line 199, exempt('human_identity_or_security', 'external_write')) add three rows at the same two-space indent and in the same syntax: learning.accept_skill with exempt('agent_cannot_self_govern', 'context_write'); learning.reject_skill with exempt('agent_cannot_self_govern', 'context_write'); learning.undo with exempt('agent_cannot_self_govern', 'destructive'). Quote the ids the way the neighbouring rows do. The ids match action-registry.ts lines 2633, 2644 and 2655.

## WS09-S7
shared-edit apps/Frontend Plane/verevonv3/src/features/chat/components/ChatPage.tsx: on the thread element whose opening tag carries class="verevon-chat-thread" (about line 1027 in the current worktree, which other slices are editing; re-locate it by that class), add directly after the class line: data-chat-containment={import.meta.env.VITE_CHAT_CONTENT_CONTAINMENT === 'true' ? 'on' : undefined}

## WS11-S9
shared-edit internal/config/config.go (Config struct, after the CodexSubscriptionQueueTimeout field, about line 120): CodexSubscriptionWideReasoningEffort bool

## WS11-S9
shared-edit internal/config/config.go (Config literal in the loader, after the CodexSubscriptionQueueTimeout entry, about line 364): CodexSubscriptionWideReasoningEffort: codexKnobs.wideReasoningEffort,

## WS11-S9
shared-edit internal/config/config.go (codexSubscriptionKnobs struct, before unparsed []string, about line 977): wideReasoningEffort bool

## WS11-S9
shared-edit internal/config/config.go (loadCodexSubscriptionKnobs, before return knobs, about line 989): knobs.wideReasoningEffort = strings.TrimSpace(os.Getenv("CODEX_SUBSCRIPTION_WIDE_REASONING_EFFORT")) == "true"   (do not use strictEnvBool here: it uses ParseBool, which would switch on for "1")

## WS11-S9
shared-edit cmd/api/main.go (codexsubscription.Config literal, after the QueueTimeout line, about line 111): WideReasoningEffort: cfg.CodexSubscriptionWideReasoningEffort,

