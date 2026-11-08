## WS23-S3.code
shared-edit apps/Model Plane/go/services/capability-core/migrations/0016_capability_tool_classes.up.sql: insert after line 158 (the list_provider_actions row, file is LF, row ends with a comma): `    ('global', 'cap.tool.provider.read', 'read_provider_action', 'read', 'none', NULL, 'seed'),`. Then re-run `cargo test -q -p execution-core --test tool_class_registry_parity` (expect 6 passed).

## WS23-S3.code
shared-edit apps/Model Plane/rust/crates/mp-contracts/src/approval_cause.rs and apps/Model Plane/api/contracts/approval-causes.json: add CauseCode::TaintedEgress with wire token tainted_egress to the closed vocabulary; update the a_token_outside_the_vocabulary_is_unclassified test (line ~335-340 asserts tainted_egress is unknown) and the golden pin (line ~409). After that, the implementer (or the lead) switches apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs line ~3743 from CauseCode::RiskyEffectUnderAsk to CauseCode::TaintedEgress and adds a test that the held outcome carries that cause. Not done in this round.

