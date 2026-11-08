## WS22-S1
shared-edit apps/Model Plane/rust/services/execution-core/src/sandbox_manager_client.rs: in the AcquireLeaseRequest literal at line 201 (the struct initializer that begins `let message = AcquireLeaseRequest {`), add the line `zdr: false,` directly after `capability_claims_json: request.capability_claims_json.to_owned(),` (line 210), with the same 12-space indentation. Without it execution-core fails with E0063 missing field `zdr`. After applying, re-run `CARGO_INCREMENTAL=0 cargo check -q -p execution-core` in apps/Model Plane/rust.

## WS22-S1
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the sandbox-manager service environment, next to `SANDBOX_MANAGER_BACKEND_ID: ${SANDBOX_MANAGER_BACKEND_ID:-}`, add these three lines with the same indentation: `SANDBOX_MANAGER_THREAD_WORKSPACE: ${SANDBOX_MANAGER_THREAD_WORKSPACE:-}`, `SANDBOX_MANAGER_THREAD_MAX_FILES: ${SANDBOX_MANAGER_THREAD_MAX_FILES:-256}`, `SANDBOX_MANAGER_THREAD_MAX_BYTES: ${SANDBOX_MANAGER_THREAD_MAX_BYTES:-67108864}`. Empty THREAD_WORKSPACE keeps the switch off.

