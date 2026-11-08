## WS20-S5
shared-edit apps/Model Plane/rust/services/session-core/src/run_service_grpc.rs: in the ResolveScheduledStepAuthorityResponse literal (about line 302) add `contract_digest: String::new(), contract_expires_at: String::new(),` after `run_status: row.run_status,`; in the ScheduledStepContext literal (about line 623) add `contract_json: String::new(), contract_digest: String::new(), contract_expires_at: String::new(),`. Without this session-core does not compile, so no session-core test can run. Once the field values are read from run metadata (contract_digest/contract_expires_at keys written by StartScheduledRun), this literal should carry those values instead of empty strings.

## WS20-S5
shared-edit apps/Control Plane/user-core/internal/spaces/decision.go: the ContractDigest (string, json contract_digest,omitempty) and ContractExpiresAt (*time.Time, json contract_expires_at,omitempty) fields on Decision, with the comment above ZeroDataRetention. Already present in the working tree; keep them in the commit alongside WS21-S8 hunks.

## WS20-S5
shared-edit Control user-core HTTP handler for POST /api/v1/internal/spaces/scheduled-run-execution-decision and /api/v1/internal/spaces/scheduled-step-decision (locate with grep for those route strings; the file was not in this slice's owned list): decode intent.contract_digest and intent.contract_expires_at (strings) into ScheduledRunIntent.ContractDigest/ContractExpiresAt (time.Parse RFC3339) and ScheduledStepIntent.ContractDigest/ContractExpiresAt, so Control issues v2 decisions. Until this lands, Control issues v1 decisions and orchestrator refuses contract-bound intents (fail closed).

