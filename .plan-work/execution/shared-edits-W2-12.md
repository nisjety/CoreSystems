## WS20-S6
shared-edit apps/Model Plane/rust/services/execution-core/src/lib.rs: after `pub(crate) mod scheduled_inference_auth;` add `pub(crate) mod scheduled_agent;`. Then delete the two lines `// WS20 S6. The file lives at the crate root; lib.rs should declare it there (shared edit).` and `#[path = "../scheduled_agent.rs"]` from apps/Model Plane/rust/services/execution-core/src/runtime_loop/mod.rs, change `super::scheduled_agent::` to `crate::scheduled_agent::` in runtime_loop/agent.rs (LoopScope in the run_agent_with_offered and run_agent_with_route_parkable signatures, plus the withhold/egress call sites), change `runtime_loop::scheduled_agent::` to `crate::scheduled_agent::` in grpc.rs, and re-run cargo test -p execution-core --lib scheduled_agent.

## WS20-S6
shared-edit apps/Model Plane/deploy/docker-compose.yml: pass EXECUTION_SCHEDULED_AGENT_ENABLED through, default empty (off), on both execution-core and orchestrator-core, e.g. `EXECUTION_SCHEDULED_AGENT_ENABLED: ${EXECUTION_SCHEDULED_AGENT_ENABLED:-}` next to the existing scheduled-step passthrough near lines 1135-1136. Both must agree.

## WS20-S6
shared-edit apps/Model Plane/api/contracts/zdr-posture-matrix.json (+ .sha256): add row `scheduled_do_run` refused for every ZDR and waived cell, then run bash scripts/tests/zdr-contract-sync.test.sh. Not in this slice's owned files.

