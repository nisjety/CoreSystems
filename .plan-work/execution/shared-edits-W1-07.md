## WS06-S3
shared-edit apps/Control Plane/docker-compose.yml: add to the audit-core environment block (near the AUDIT_CORE_SERVICE_CREDENTIALS line, ~826-833): AUDIT_CHAIN_KEY_ID: ${AUDIT_CHAIN_KEY_ID:-} ; AUDIT_CHAIN_PRIVATE_KEY_BASE64: ${AUDIT_CHAIN_PRIVATE_KEY_BASE64:-} ; AUDIT_CHAIN_PUBLIC_KEY_BASE64: ${AUDIT_CHAIN_PUBLIC_KEY_BASE64:-} ; AUDIT_CHAIN_VERIFY_KEYS: ${AUDIT_CHAIN_VERIFY_KEYS:-} ; AUDIT_CHAIN_ATTEST_INTERVAL: ${AUDIT_CHAIN_ATTEST_INTERVAL:-60s} ; AUDIT_CHAIN_PRUNE_ENABLED: ${AUDIT_CHAIN_PRUNE_ENABLED:-0} ; AUDIT_CHAIN_PRUNE_DRYRUN: ${AUDIT_CHAIN_PRUNE_DRYRUN:-0}. These lines are inert until a switch is on.

## WS06-S3
shared-edit apps/Control Plane/docker-compose.yml (and docker-compose.production.yml if it sets the environment per service): pass ENVIRONMENT to audit-core (for example ENVIRONMENT: ${ENVIRONMENT:-development}). Without it the 'chain on in production without a key fails boot' rule can never fire. Check what other Control services use for this variable. Also confirm audit-core's compose block still contains AUDIT_CHAIN_ENABLED: ${AUDIT_CHAIN_ENABLED:-0} from the S2 compose edit; main_test.go requires the :-0 defaults.

