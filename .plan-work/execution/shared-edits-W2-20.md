## WS26-S4
shared-edit session-core/src/main.rs: after `mod memory_grpc;` (line 30) add `mod write_policy;` (still missing)

## WS26-S4
shared-edit session-core/src/letta_adapter.rs (IndexMemoryRequest literal near line 617): add `origin: String::new(),` (still missing)

## WS26-S4
shared-edit model-gateway/src/http_routes.rs (create_memory literal, line 10677): it now reads `origin: String::new(),`; the earlier instruction was `origin: "user".to_owned(),`. Scope is unaffected, because topic USER maps to scope user and only org scope is refused for non-user origins, but a person's own correction is labelled model_tool in the refusal log and counter. Lead to choose.

## WS26-S4
shared-edit session-core/Cargo.toml [dependencies]: add `regex = "1"` (still missing; regex 1.13.0 is already in Cargo.lock)

## WS26-S4
shared-edit apps/Model Plane/deploy/docker-compose.yml: session-core environment `CONTENT_WRITE_POLICY: ${CONTENT_WRITE_POLICY:-off}` near line 932 (not applied; the owner chooses the rollout default)

## WS27-S5.code
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the model-gateway service's environment block, after the MODEL_GATEWAY_EFFORT_LADDER line (line 341 at this run), add the comment `# WS27-S5: a ZDR answer's usage frame states its cost in-stream, numbers only, nothing stored (no ledger row, no usage envelope). Off by default; only the literal true turns it on.` and the line `MODEL_GATEWAY_ZDR_STREAM_COST: ${MODEL_GATEWAY_ZDR_STREAM_COST:-false}`, both indented to match the block.

## WS27-S5.code
shared-edit apps/Model Plane/docs/ZDR.md: replace the **Metering** row (line 102 at this run; the old row begins '| **Metering** | A paid add-on should be metered | **Gap.** The ZDR path returns before the budget check and emits no usage frame') with: '| **Metering** | A paid add-on should be metered | **Split.** In-stream cost: built behind MODEL_GATEWAY_ZDR_STREAM_COST (default off). When it is on, a ZDR answer's usage frame states its cost and cache counts, priced from the answer's counts and the model that served it. Numbers only: nothing is stored, no usage envelope is published and no ledger row is written. **Gap, recorded (DEF-Z-06b):** the ledger. A ZDR turn still returns before the budget check and is not ledgered, so budget ceilings and admin cost dashboards do not cover ZDR spend. |'

