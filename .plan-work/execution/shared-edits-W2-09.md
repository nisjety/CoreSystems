## WS25-S5
shared-edit services/execution-core/src/capability_policy.rs: in trusted_capability_id add an arm `"result_query" => "cap.result.query",` next to the `"knowledge_search" => "cap.retrieval.query",` arm (about line 1512). Without it a result_query call fails closed at GrpcCapabilityPolicy.

## WS25-S5
shared-edit services/execution-core/tests/tool_class_registry_parity.rs AND go/services/capability-core/internal/registry/capability_tools_seed_test.go: both read the capability_tools seed only from 0016. Once the arm above lands, requirement 1 of the parity test fails for result_query unless both readers also parse migration 0020's seed row (the one-line `('global', 'cap.result.query', 'result_query', 'read', 'none', NULL, 'seed')` format is matched by the existing regex).

## WS25-S5
shared-edit apps/Model Plane/deploy/docker-compose.yml, execution-core block: optional passthrough of EXECUTION_RESULT_HANDLES. Default off without it.

## WS25-S5
shared-edit fix owner for services/model-gateway/src/sse.rs:1692 (skills_truncated and skills_dropped out of scope). The gateway's --lib tests cannot compile until this is fixed, so the gateway side of tool_result_handles is unverified.

