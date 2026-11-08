## WS20-S1
shared-edit apps/Model Plane/rust/services/model-gateway/src/grpc.rs (impl SessionCore for MockSessionCore, line 3749): add async fn record_scheduled_run_result(&self, _: Request<mp_contracts::model_plane::v1::RecordScheduledRunResultRequest>) -> Result<Response<mp_contracts::model_plane::v1::RecordScheduledRunResultResponse>, Status> { Err(Status::unimplemented("record_scheduled_run_result not used")) }. Without it model-gateway --lib does not compile (E0046).

## WS20-S1
shared-edit apps/Model Plane/rust/services/model-gateway/tests/usage_meter_contract.rs (impl SessionCore for ScriptedSession, line 256): same stub, returning unimplemented_rpc("record_scheduled_run_result") in that file's style.

## WS20-S1
shared-edit apps/Model Plane/deploy/docker-compose.yml: pass SESSION_CORE_SCHEDULED_RESULT_PERSIST through to session-core and execution-core (default unset/false).

## WS20-S1
shared-edit apps/Model Plane/docs/ZDR.md: a scheduled run's answer is persisted only when SESSION_CORE_SCHEDULED_RESULT_PERSIST is true, refused for zdr and zdr_waived runs; receipt and fire status are numeric or status only.

