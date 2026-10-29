# Xero MCP to CoreSystem mapping

Status: proposed integration contract, **not deployed**. This is a narrow
mapping from the native Xero v2 MCP server to CoreSystem's current Model,
Ingestion, Data, Control, and Frontend boundaries. It does not modify
CoreSystem or assert that its registration UI can use the Xero server today.
The local server contract is in [XERO_MCP.md](XERO_MCP.md).

## Existing CoreSystem contracts

| Boundary | Current contract and implication |
| --- | --- |
| Model Plane HTTP | `apps/Model Plane/rust/services/model-gateway/src/mcp_http.rs` uses one Streamable HTTP JSON-RPC endpoint, `initialize`, `notifications/initialized`, paginated `tools/list`, and `tools/call`; it handles JSON/SSE and `Mcp-Session-Id`. `mcp_jsonrpc.rs` advertises protocol `2024-11-05`. Test this exact handshake against `rmcp` before registration. |
| Registry | `runtime_registries.rs` validates org-scoped HTTP registrations: public HTTPS, or an explicitly allowlisted private HTTP host; URL has no credentials/query/fragment; exact `tool_allowlist` has 1-64 names; raw bearer token is rejected. Discovered names are exposed as `mcp__<server_id>__<tool>`. Xero's 158 tools cannot all be allowlisted on one registration. |
| Model API | `http_routes.rs` exposes `/v1/mcp/servers` and `/v1/mcp/servers/connect` behind verified org/admin context. The connect flow is OAuth discovery/PKCE or no-auth; it is not a general raw-bearer handoff. A dedicated managed-secret/credential adapter is required for Xero MCP's service bearer. |
| Execution | `tool_loop.rs` denies `mcp__*` in inline chat; `apps/Model Plane/rust/services/execution-core/src/capability_policy.rs` fails closed on unbound dynamic MCP tools. Registry/catalog visibility alone does not permit agent execution. Add an explicitly authorized execution binding and test read/extract policy before advertising this as usable. |
| Ingestion Plane | `apps/Ingestion Plane/integration-corev2/internal/providers/catalog.go`, `internal/store/types.go`, and `internal/actions/service.go` have provider/connection/action boundaries but no Xero adapter yet. Connections are org-scoped and hold provider key, connector type, tenant mapping, capabilities/scopes, and encrypted credentials. |
| Action receipts | `integration-corev2/internal/api/server.go` accepts action requests with `operation`, `params`, `body`, `writeAttestation`, and `idempotencyKey`; write actions require a signed attestation. Xero MCP exposes GETs and raw extraction, not Xero financial writes. Its raw job status is not yet a CoreSystem action receipt. |
| Data Plane | `integration-corev2/internal/handoff/dataplane.go` uses a short-lived, scoped token and `POST /v1/documents` for handoff. Do not send large Xero responses through a model result or call that handoff directly from MCP until a reviewed adapter maps raw provenance, policy, and retention. Data Plane owns durable knowledge. |
| Frontend/Control | `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/mcp.rs` and `src/shared/api/mcp-client.ts` proxy org-scoped MCP administration. `src/shared/context-packs/context-pack.ts` keeps shared context redacted. Neither a tenant UUID in a prompt nor an MCP tool argument is an org authorization grant. |

These are source-level contracts from the current CoreSystem checkout, not a
promise that its in-progress Model Plane is production-released. Recheck the
files at implementation time, especially protocol and execution gates.

## Recommended registration slice

Register **only the eight compact Xero tools** initially, or those plus a
small, reviewed set of pinned GETs; stay under the registry's 64-name limit.
Set `XERO_MCP_TOOL_MODE=compact` for that eight-tool surface. The compact
catalog lets the agent discover operations without exposing 150
tool definitions to every run. Never use a wildcard allowlist. A server-side
catalog entry still needs per-client and per-tenant authorization on every
call. Tool annotations such as `readOnlyHint` are descriptive, not policy.

The following is a **mapping manifest proposal**, not a payload accepted by
CoreSystem today:

```yaml
schema_version: 1
provider_key: xero
server_name: xero-v2
model_plane:
  transport: streamable-http
  endpoint: https://<approved-internal-host>/mcp
  protocol_compatibility: "2024-11-05" # local wire tests passed; Core client still to validate
  tool_allowlist:
    - xero_list_connections
    - xero_list_operations
    - xero_warehouse_overview
    - xero_query
    - xero_read_result
    - xero_extract
    - xero_get_job
    - xero_cancel_job
  execution_name: mcp__<server_id>__<tool>
credential_adapter:
  owner: CoreSystem-managed token broker # to implement
  input: verified org/user/connection authorization
  output: short-lived service credential bound to client_id and tenant_ids
  forbidden: raw token in registration, prompt, tool args, or browser
connection_mapping:
  owner: Ingestion Plane integration-corev2 # to implement
  provider_key: xero
  external_tenant_id: Xero tenant UUID
  org_binding: verified CoreSystem org to approved Xero tenant(s)
  scopes_and_entitlements: per connected tenant, rechecked on use
data_handoff:
  owner: Data Plane adapter # to implement
  source: private raw object + sidecar + run manifest
  authorization: scoped short-lived Data Plane token
  policy: preserve tenant/org, sensitivity, provenance, and ZDR
```

The Xero HTTP server currently accepts static bearer clients configured with
`XERO_MCP_CLIENTS_JSON` or one `XERO_MCP_TOKEN`. Its middleware must derive
the MCP `client_id`, tenant UUID allowlist, `allow_extract`,
`allow_sensitive`, `allow_warehouse`, and `zero_data_retention` from the credential, never from
model-supplied JSON. A Core token broker must issue and rotate a credential
bound to a verified Core org/connection, map that connection to the exact
Xero UUID(s), and avoid exposing Xero OAuth refresh/access tokens to Model
Plane or the browser. CoreSystem presently rejects raw `server.token`, so a
direct UI/API registration of this bearer-protected endpoint is **not** a
working integration. Loopback `127.0.0.1:5003` is also not reachable from a
separate host/container; deployment needs an approved HTTPS or explicitly
allowlisted private route, with TLS, host/origin review, and network policy.

## Action, receipt, and data mapping

| Xero capability | Proposed CoreSystem representation | Required guard |
| --- | --- | --- |
| Pinned GET or `xero_query` | Read-only MCP tool returning bounded `data` plus source `meta`; `xero_read_result` for a tenant/client-scoped slice. | Verify Core org -> connection -> Xero tenant mapping before Xero call. Treat Xero content as untrusted. |
| `xero_list_connections` / Identity `getConnections` | Discovery, filtered to the credential's allowed tenant(s); do not turn app-wide Identity data into org membership. | Filter by selected tenant before caching/returning; no cross-org enumeration. |
| `xero_warehouse_overview` | Tenant-scoped staging count overview with no record payload. | Enable only for a verified org-to-tenant mapping; MCP service identity needs least-privilege BigQuery metadata/data read and job creation. |
| `xero_extract` | Explicitly approved asynchronous raw-data operation with idempotency key, destination, and plan. | Permit only where policy allows persistence and the destination's IAM/security domain matches the data. Starting a job is not a success receipt. |
| `xero_get_job` / `xero_cancel_job` | Content-free job status and cancellation intent; future Core action receipt may reference job/run ID and manifest outcome. | Client/tenant binding, best-effort cancellation, partial-output reconciliation. Do not imply a signed Core `writeAttestation` exists. |
| GCS raw/sidecar/manifest | Future Ingestion/Data Plane adapter maps private raw provenance into documents or warehouse staging, using scoped Data Plane token. | Separate ordinary, sensitive, document, and PKCE-token buckets; enforce ZDR and loader-specific IAM. |

The Xero API side is GET-only. `xero_extract` writes to the connector's raw
sink, not to Xero, but is still a persistent data operation that needs explicit
policy and user approval. Future Core action registration must define whether
it is an action, a background job, or both, and map its idempotency/job status
to Core receipts without inventing a Xero financial-write attestation. Do
not flatten a failed/truncated page run into a successful warehouse receipt.

For GCS extraction, keep `GCS_PREFIX=raw/xero`, provision the private bucket,
restricted sensitive/document buckets and their loaders/datasets, and keep
the PKCE token store bucket separate. Follow [operations](OPERATIONS.md) and
the [extended API runbook](XERO_EXTENDED_RUNBOOK.md) before a Data Plane
handoff. The MCP result cache is in memory and expires; it is not a durable
data source. Under zero-data-retention, no handle or extraction persistence
should occur.

## Integration gates

1. The native crate's stdio/HTTP wire tests and local `2024-11-05` handshake
   have passed, including paginated discovery and client isolation. Next run
   the actual Model Plane HTTP client against this server and its approved
   credential delivery path; no end-to-end Core execution is claimed here.
2. Implement Core-managed service credential delivery, rotation, and verified
   org-to-Xero-tenant mapping; preserve per-client extraction, sensitivity,
   and retention policy. Do not paste the current static bearer into the MCP
   registry or expose it in the frontend.
3. Register an eight-tool exact allowlist through the approved HTTPS/private
   endpoint. Enable dynamic MCP execution only after Model Plane's inline and
   execution-core gates have an explicit capability binding and audit trail.
4. Add an Ingestion Plane Xero provider/connection adapter and a separate
   Data Plane raw/sidecar/manifest adapter. Define job-to-action receipt
   semantics, data classification, replay, and ZDR before warehouse claims.
5. Validate one non-production tenant end to end: denied cross-tenant access,
   missing scopes, 429/retries, partial pages, cancellation, sensitive/binary
   access, bucket isolation, loader counts, and model-visible provenance.

Until these gates pass, use the Xero MCP server locally under a trusted host;
do not describe it as registered with, or executable by, CoreSystem agents.
