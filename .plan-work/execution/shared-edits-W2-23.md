## WS26-S8
shared-edit docs/plans/model-plane-depth-register.md: record the WS26-S8 depth_findings above.

## WS19-S7
shared-edit apps/Model Plane/rust/crates/mp-contracts/src/lib.rs: add `pub mod skill_bundle;` between `pub mod scheduled_contract;` and `pub mod skill_recovery;`. Required before any cargo build or test of model-gateway or mp-contracts; without it the gateway fails with the four unresolved skill_bundle references.

## WS19-S7
shared-edit apps/Model Plane/rust/services/model-gateway/src/prompt_context.rs (recommended for prompt-cache order, line ~415): change `const CONDITIONAL_TOOLS: [&str; 2] = ["reattach_context", "web_search"];` to `[&str; 3] = ["reattach_context", "web_search", "skill_file"];` and add `skill_file` to the doc comment above it (once a matched skill has a bundle). conditional_tools_last runs in sse.rs only when cache_options.stable_prompt_order is on; without this, a turn that offers skill_file shifts every tool definition after it.

## WS19-S7
shared-edit apps/Model Plane/go/services/capability-core/migrations/0016_capability_tool_classes.up.sql (or a new migration with the next free number, integrator's call): add `('global', 'cap.skill.summarize', 'skill_file', 'read', 'none', NULL, 'seed'),` to the capability_tools VALUES list beside the reattach_skill row, to give the WS05 declared class read. Inert until WS05-S4 reads classes.

