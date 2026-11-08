## WS14-S5
shared-edit apps/Model Plane/rust/services/session-core/src/grpc.rs: import run_question_consume_service_server::RunQuestionConsumeServiceServer from mp_contracts::model_plane::v1; clone the OrchestrationGrpc handle before the OrchestrationCoreServiceServer::with_interceptor call; add .add_service(RunQuestionConsumeServiceServer::with_interceptor(<clone>, auth.clone())) beside ApprovalScopeGrantServiceServer. Until this lands ConsumeRunQuestion answers UNIMPLEMENTED.

