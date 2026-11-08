## WS06-S2
shared-edit apps/Control Plane/docker-compose.yml: in the audit-core service `environment:` block (near the HTTP_PORT line, about 825), add `AUDIT_CHAIN_ENABLED: "${AUDIT_CHAIN_ENABLED:-0}"` with a comment line above it, e.g. `# Per-org audit hash chain; only the literal 1 enables it (WS06-S2).` TestAuditChainSwitchShipsOff requires the `${AUDIT_CHAIN_ENABLED:-0}` default.

## WS01-S12.code
shared-edit apps/Model Plane/MODEL_PLANE_STATUS.md: lines ~54 and ~150 keep 'No candidate or rollback artifact exists yet'; add one sentence after line 54: 'The signed baseline/candidate/rehearsal operator sequence is written (docs/runbooks/release-rollback-rehearsal.md) and its scripts are unit-tested (rollback-rehearsal-test.sh, acceptance-thresholds-test.sh, coresystem-conformance-artifact-test.sh); it has not been run against real images, and runtime conformance reads blocked.'

