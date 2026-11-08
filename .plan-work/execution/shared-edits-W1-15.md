## WS10-S5
shared-edit apps/Frontend Plane/verevonv3/docker-compose.yml (verevon-gateway service environment): add `MODEL_ENTITLEMENT_ADMIN_ENABLED: ${MODEL_ENTITLEMENT_ADMIN_ENABLED:-false}` once, next to VEREVON_GATEWAY_CONTINUE_PARK_ENABLED. Add the same line with the value false to docker-compose.production.yml if that file lists gateway switches. Neither file is owned by this slice.

## WS10-S4
shared-edit execution-core/src/subscription_refusal.rs + runtime_loop/agent.rs (~:2791, the Err(error) arm of the infer round): for ANY run (not only pinned) read mp_contracts::model_entitlement::Refusal::from_status(&error); when Some, fail the run with a typed refusal carrying refusal.code() (model_not_entitled or model_entitlement_unavailable) in the response (code is &'static str, so Refusal::bare(code) fits), terminal failure code from Session Core's fixed vocabulary (provider_unavailable for unavailable, inference_failed for not_entitled), never latch, never park, and log only grpc code and refusal code (content-free). The contract test tests/model_entitlement_refusal_contract.rs already pins the wire this reads.

