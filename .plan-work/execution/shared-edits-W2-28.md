## WS27-S6
shared-edit apps/Model Plane/deploy/docker-compose.yml: insert after line 374 (the MODEL_GATEWAY_USAGE_PROGRESS line, inside service model-gateway), with the same 6-space indent: `      MODEL_GATEWAY_RUN_CEILING_WARN: ${MODEL_GATEWAY_RUN_CEILING_WARN:-false}` and `      MODEL_GATEWAY_RUN_CEILING_ENFORCE: ${MODEL_GATEWAY_RUN_CEILING_ENFORCE:-false}`.

## WS24-S9
shared-edit apps/Model Plane/docs/capability-ownership-matrix.md: after the 'Per-tool side-effect class and approval floor' row (about line 62) add two rows. Row 1: 'Agent action admission (which actions a run may see, access, closed reason, side-effect class, authority)' | **capability-core** (agent_action_admissions and the signed model action view); the Frontend model-access.ts side map only classifies | Postgres agent_action_admissions; the vendored contract api/contracts/agent-action-admission.json | execution-core pins owner actions (owner_actions.rs) and reads the view; model-gateway checks Ask reads against the golden | — | ✅ one admission owner (DEC-25). Row 2: 'Do-mode workspace tools (workspace_glob, workspace_grep, workspace_list, workspace_read, workspace_edit, workspace_put)' | **execution-core** (workspace_tools.rs, workspace_edit.rs; the run's overlay) | run-keyed overlay rows in **sandbox-manager** workspace_files (no matrix row kind yet) | cap.workspace.read and cap.workspace.write gates in capability-core; the Space changes only by diff-and-upload, no PromoteWorkspace caller (DEC-24 D1) | — | ✅ owner; overlay durability is an owner decision.

## WS24-S9
shared-edit apps/Model Plane/rust/crates/mp-contracts/src/zdr_refusal.rs: after the mcp_execution Feature entry (about lines 292-295) insert Feature { key: "workspace_tools", label: "workspace files" } in the same layout. Then regenerate: cd 'apps/Model Plane/rust' && MP_UPDATE_GOLDEN=1 CARGO_TARGET_DIR=C:/dev/mp-target-w2 CARGO_INCREMENTAL=0 cargo test -p mp-contracts zdr_refusal (writes apps/Model Plane/api/contracts/zdr-refusals.json).

## WS24-S9
shared-edit apps/Frontend Plane/verevonv3/src/shared/api/zdr-refusals.ts: add `  | 'workspace_tools'` after line 48 (`  | 'mcp_execution'`), and `  workspace_tools: { no: 'arbeidsfiler', en: 'workspace files' },` after line 174. The Norwegian wording is a proposal for the owner to confirm.

## WS24-S9
shared-edit apps/Model Plane/rust/services/execution-core/src/runtime_loop/mod.rs, the workspace gate (about lines 774-786): return mp_contracts::zdr_refusal::Refusal::feature_not_supported("workspace_tools").message() for the zdr or waived case, as the connector_read branch (about lines 755-759) does; the switch-off message stays plain text. Split the condition, so the two cases do not share one message.

