## EP-12-S7
shared-edit apps/Model Plane/rust/services/model-gateway/src/zdr_posture.rs: in the CALL_SITES table of the inference-call inventory test (beside the ("grpc.rs", "invoke", ...) rows), add `("agent_api_run.rs", "run_ask_turn", "persistent only: a ZDR key is refused in admit before any call (EP-12 D6)"),`. Without it, the test that names every `.infer(` site fails on agent_api_run.rs::run_ask_turn.

## EP-12-S12
shared-edit apps/Frontend Plane/verevonv3/src/features/chat/components/ChatPage.tsx: in the <MessageBlock> props of renderMessageRow, directly after the line `        temporaryChat={readAloudBlocked()}` (line 1286), add the line `        spaceScoped={spaceScoped()}` (8 leading spaces). spaceScoped is already destructured from useChatController() at line 166 of the same component.

