## WS25-S7
shared-edit apps/Model Plane/api/contracts/README.md: add a table row for org-agent-policy-schema.json (authored WS25-S7; read by mp-contracts org_agent_policy and capability-core internal/api org_agent_policy; pinned by its .sha256).

## WS25-S7
shared-edit scripts/tests/zdr-contract-sync.test.sh: add to GOLDENS `org-agent-policy-schema.json` with digest 4e845cb768ad0d9df4b6cfee5af242df612b161b2e2fb5029f2da178deb270a1, in the same form as the existing entries.

## WS25-S7
shared-edit apps/Model Plane/docs/capability-ownership-matrix.md: add a row for org_agent_policy and org_agent_policy_history, owner capability-core, DEC-14 (the WS25-S0 ledger entry already exists in docs/decisions/ledger.md).

## WS25-S7
shared-edit apps/Model Plane/deploy/docker-compose.yml capability-core block: pass through ORG_AGENT_POLICY_API with default false (needed before anyone can enable it).

## WS20-S4
shared-edit apps/Control Plane/user-core/internal/http/spaces.go (issueScheduleCreateDecision, ~line 894; struct scheduleCreateDecisionRequest ~line 772): add `ContractDigest string `json:"contract_digest"`` and `ContractExpiresAt string `json:"contract_expires_at"`` to the request struct; in the spaces.IssueScheduleCreateDecision call add `ContractDigest: request.ContractDigest, ContractExpiresAt: request.ContractExpiresAt, ContractsEnabled: s.scheduleContractsEnabled`. Without this every contract create fails closed.

## WS20-S4
shared-edit apps/Control Plane/user-core/cmd/server/main.go (near httpServer.SetSpaceRepository at ~line 382): add `httpServer.SetScheduleContractsEnabled(os.Getenv("SCHEDULE_CONTRACT_ENABLED") == "true")` (strict literal true; default off).

## WS20-S4
shared-edit apps/Model Plane/go/services/capability-core/cmd/main.go (right after `cronHandler := api.NewCronHandler(pool)` near line 307, outside the verifier if/else): add `cronHandler.WithScheduleContract(os.Getenv("SCHEDULE_CONTRACT_ENABLED") == "true")`. Another session has already edited this file (ORG_AGENT_POLICY lines); add on top.

## WS20-S4
shared-edit apps/Model Plane/rust/crates/mp-contracts/Cargo.toml [dependencies]: add `sha2 = "0.10"` (the MP workspace lock already resolves sha2 0.10.9; the scratch check used =0.10.9). Required for scheduled_contract.rs digest().

## WS20-S4
shared-edit apps/Model Plane/rust/crates/mp-contracts/src/lib.rs: add `pub mod scheduled_contract;` after `pub mod run_origin;` (lib.rs is already modified by another session: org_agent_policy line). Then run `cargo test --manifest-path "apps/Model Plane/rust/Cargo.toml" -p mp-contracts` with CARGO_TARGET_DIR=C:/dev/mp-target-w2 and CARGO_INCREMENTAL=0.

## WS21-S3
shared-edit apps/Control Plane/docker-compose.yml: in the ORG_CORE_SERVICE_CREDENTIALS JSON line (the line after `ORG_CORE_SERVICE_CREDENTIALS: >-`, currently line 543), insert before the final `]`: ,{"principal":"browser-broker","audience":"org-core","token":"${BROWSER_BROKER_ORG_CORE_SERVICE_TOKEN:?BROWSER_BROKER_ORG_CORE_SERVICE_TOKEN is required}","scopes":["org:browser-site:read:any"]}. This turns green TestComposeGivesBrowserBrokerExactlyTheResolveScope and TestComposeBrokerResolveIsAdmittedByTheRealMiddleware. The gateway line needs no change.

