# Sandbox credential injection: custody and redemption

| Field | Value |
|---|---|
| Status | Proposed. Written in the Wave 3 build on the design's recommendation for DEC-21 and D-E4. Not ratified: the owners named under Open for owners have not ruled. This document adds no code. |
| Number | `ADR-<next>`, assigned at filing. The Model Plane ledger references up to ADR-0011, so the lead takes the next free number then. |
| Date | 2026-11-03 |
| Slice | EP-9-E5 (EP-9 Wave 3). Design: `docs/plans/wave3/EP-9-policy-egress-and-compliance.md`, sections 3 (D-E4), 4 (E5) and 5 (ZDR clause). |
| Covers | F-t3-043 credential-injecting proxy (043(c), W4); F-t3-057 secret-reference connectors (W4); F-t2-024 secret handles, custody and redemption only (the `secret://` grammar belongs to the Quarry plan). |
| Builds on | E2 tier ceilings, E3 allow-list proxy and socket bridge (D-E1), D-E2, the EP-9 ZDR clause. |

## Context

The sandbox has no network today. Execution-core claims `egress: "disabled_by_default"` and `credential_mode: "credential_free"` (`apps/Model Plane/rust/services/execution-core/src/sandbox.rs:416-417`). E3 adds an allow-list CONNECT proxy, which sees host names and not requests, so it cannot add a credential. Authenticated work (a private API, a registry with a token, a bucket) needs the secret at the moment of the request, and no run should hold it.

Verified state, by code path in this worktree. "IC" means integration-corev2.

- Custody exists for provider tokens. IC seals them with AES-GCM and binds each ciphertext to its connection id as additional data (`apps/Ingestion Plane/integration-corev2/internal/crypto/vault.go:15-53`; call sites `internal/oauth/service.go:639,793`).
- Provider access tokens leave IC through a lease route. `POST /internal/connectors/token` (`internal/api/server.go:1293`) returns the token to the caller. Authentication is the shared internal key (`internalAuth`, `server.go:191-193`). The consumer is a body field, checked by `tokenLeaseConsumerAllowed` (`server.go:2779-2791`) against `INTEGRATION_TOKEN_LEASE_CONSUMERS`, whose default (`internal/config/config.go:407`) names `model-plane`. That predicate also returns true when the list is empty. A lease row is written before the token is returned; a write failure returns 503 `token_lease_audit_failed`.
- No Model Plane caller of that route was found in non-test Rust, Go or TypeScript at this commit.
- execution-core already depends on `aws-sigv4` 1.x (`execution-core/Cargo.toml:51`) and signs with it in `src/workspace_cas.rs:31-32,290`. That client loads a MinIO access and secret key from environment variables in `from_env` (`workspace_cas.rs:70-84`). That is a second custody state for long-lived keys inside execution-core. This ADR does not change it (follow-up F1).
- The scrubber is pattern based. `scrub_string` in `execution-core/src/scrub.rs` redacts known token shapes, so it cannot find an arbitrary injected credential. The proxy must check for the exact injected bytes.
- ZDR runs write no audit rows. The WS13 approvals entry (`apps/Model Plane/docs/decisions/ledger.md:208`) says "No approval, cause, grant, denial memory or audit row exists for a ZDR run", and waived runs count as ZDR (DEC-32). The approvals v2 entry (`ledger.md:5743`) records that some session-core writes key on `caller.zdr()`, which is false for a waiver credential. So waiver must come from signed evidence, never from the caller.
- Control's signed decision already appends `space:egress` (`apps/Control Plane/user-core/internal/spaces/space_capability_decision.go:218`), so the signed-decision path can carry an injection permission.

## Decision

### 1. Where the secret lives

Only in IC, sealed at rest with the vault. It is never in Control, the Model Plane, Quarry, Data Plane v2, the sandbox image, the sandbox environment, a log or a tool result.

- Rejected: a Control secret store (the audit card's option). It would be a second custodian beside IC's vault. Control owns identity, organisations, billing and audit, not provider secrets.
- Rejected: a Quarry vault. The executor would hold long-lived secrets. The audit's F-t2-024 rule is that the Ingestion Plane owns secret resolution and the model sees only opaque references.

### 2. How a call obtains a sealed reference

A sealed reference is an AES-GCM ciphertext under a key derived from the vault key with a purpose label. The nonce is prepended and the text form is `crf1.<base64url>`. Its additional data is `v1|binding_id|org_id|connection_id|call_id|slot|host|expires_at`. It holds no secret and no token, and whoever holds it cannot change its scope. Its lifetime is `expires_at = min(call deadline + 30 s, issued_at + 15 min)`.

1. **Seal, once per call.** execution-core (EC) asks IC for one reference per binding that the call's signed decision names: `POST /internal/credential-refs/seal` (route name proposed for W4). IC verifies the decision signature, that the binding is active and attached to the Space, that the evidence is neither ZDR nor waived, and that the host is an exact binding host. Then it seals.
2. **Hold in memory.** The injecting proxy runs inside EC for that call. It keeps the references, the slot table and the sentinels in memory only. Nothing is written to disk or to a store.
3. **Redeem per request.** For each request the proxy presents the reference, the host and the call id to `POST /internal/credential-refs/redeem`. IC opens the reference, checks the expiry and the current binding status, and returns the plaintext to the proxy principal over the internal link. The plaintext never goes to the sandbox. Nothing is cached beyond the request.
4. **Principals.** The seal and redeem routes take their own principal for EC's proxy. They are not reachable with the shared internal key alone. No injection path uses `/internal/connectors/token`.

### 3. Sentinel format

The sandbox sees a sentinel in place of each secret. EC generates sentinels from the OS CSPRNG, per call and per binding.

| Use | Format | Entropy | Notes |
|---|---|---|---|
| Bearer token or API key | `vrvn-sx1-` + 40 lowercase RFC 4648 base32 characters | 200 bits | Placed in the one header the binding declares |
| AWS access key ID | `VRVNS` + 15 characters from `A-Z2-7` (20 in total) | 75 bits | Has the AWS key-ID shape. The prefix avoids `AKIA` and `ASIA`, so a future AWS scrub rule cannot mistake it for a real key |
| AWS secret access key | Same as bearer | 200 bits | Only the SDK signs with it; the proxy discards that signature |

A sentinel is a per-call handle, not a secret, because a sandbox process can read its own environment. It needs to be unique and valid only in its own slot table. A sentinel found in another slot, in a body, in a query string or in an undeclared header is refused.

### 4. Per-host injection

- A binding names exactly one host: lowercase ASCII, no trailing dot, no IP literal, no wildcard, no userinfo, port 443 only. The host must not resolve to a private, loopback, link-local or metadata address. The E3 address guard applies (planned in E3, `EC/egress_proxy.rs`), and the connection is pinned to the address it checked.
- A binding lists the methods and path prefixes it may forward. Anything else is refused (`credential_path_refused`).
- The sandbox reaches a binding over the E3 socket bridge (planned in E3, `/run/egress.sock`). It sends origin-form or absolute-form requests whose path starts with `/<slot>/`. A slot is a per-call name that maps to one binding, so a route never names a host. CONNECT requests on the same socket keep the E3 allow-list path, which never injects. One socket, two dispatchers.
- The proxy forwards `https://<host>/<rest>` after removing only the slot segment, and sets `Host` to the bound host. Plain HTTP upstream is refused for an injecting binding.
- Injection replaces only the one header the binding declares. A request without its sentinel there is refused, so the binding cannot be used as an unauthenticated relay.
- The proxy never follows a redirect on an injecting binding. A 3xx goes back to the sandbox unchanged, so the secret cannot reach a second host.
- Responses are scanned for the injected bytes across chunk boundaries with a rolling window. A match is replaced with `[redacted]`. A response that cannot be scanned is refused.
- Rejected: TLS interception with a per-run CA. The sandbox would have to trust a CA whose key sits in EC, so whoever reads that key can impersonate any host to the sandbox. The trust change would also reach the sandbox image. The base-URL route needs neither.

### 5. SigV4 re-signing scope

A binding with `auth: sigv4` fixes its service and region in IC. The request cannot choose them.

- Removed from the sandbox request: `authorization`, `x-amz-security-token`, `x-amz-date`, `x-amz-content-sha256`. Presigned requests are refused (`X-Amz-Signature`, `X-Amz-Credential` or `X-Amz-Algorithm` in the query).
- The proxy re-signs with the `aws-sigv4` crate already in execution-core. The canonical request covers the method, the path, the canonical query and these signed headers: `host` (the bound host), `x-amz-date`, `x-amz-content-sha256` (SHA-256 of the buffered body), `content-type` and `content-md5` when sent, and the `x-amz-*` headers the binding lists.
- A request whose own credential scope names another service or region is refused (`credential_scope_mismatch`).
- The body is buffered up to 64 MiB. Larger and streamed bodies are refused in W4. `UNSIGNED-PAYLOAD` is not used.
- The proxy sets the signing time itself.

### 6. Audit row

Every redemption that reaches IC writes one `store.AuditEvent` row, persisted the way `connection.action.executed` is (`internal/api/server.go:3168-3184`). Fields: `event` (`connection.credential.redeemed`), `outcome` (`allowed` or `refused`), `reason_code`, `org_id`, `connection_id`, `binding_id`, `run_id`, `call_id`, `host`, `at`. The row never carries the secret, the sentinel, the sealed reference, the path, the query or any header value.

The write fails closed. If the row cannot be persisted, IC returns 503 and the proxy refuses the request, following the `token_lease_audit_failed` pattern. Refusals the proxy decides alone (unknown slot, sentinel mismatch, redirect, presigned request) never reach IC. They appear only as host counts in the tool result, as E3 counts hosts. This is open point O4.

### 7. ZDR, waiver and run refusals

- Control leaves the injection permission out of any decision whose evidence is ZDR or waived. This is the rule E2 applies to egress, extended to waived evidence.
- IC refuses to seal for ZDR or waived evidence. It reads the flag from the signed decision, never from the caller.
- EC forces injection off for ZDR, waived, plan-mode and tainted runs. The sandbox then gets no sentinels, no slot routes and no references. An explicit request under ZDR returns the typed refusal `feature_not_supported` with `details: {"feature": "sandbox_credential_injection"}`. That feature goes into `zdr_features.rs` and `docs/ZDR.md` in W4. No row goes into `zdr-posture-matrix.json`, whose rows are keyed on resolver inputs only.
- ZDR and waived runs write no redemption row, following the approvals rule.
- Plan-mode and tainted runs are refused in addition to the slice's minimum (ZDR and waived). That matches E3's run-level refusals. It is assumption O2.
- Every refusal uses the typed envelope `{error:{code,message,details}}`, with `details` an object.

### 8. Lifecycle

Revoking a binding in IC takes effect on the next redemption. An expired Control decision stops new seals. In-flight references end at their own `expires_at`. Key rotation moves the vault version prefix from `v1:` to `v2:`. A reference sealed under a retired key is refused, and the call fails closed.

## Consequences

- W4 043(c) and F-t3-057 build on this with no new custody.
- W3 adds no code, proto, migration, fixture or golden. This document is the slice's deliverable.
- Before any W4 switch goes on:
  - distinct principals for the seal and redeem routes (the token route authenticates with the shared key alone);
  - `model-plane` removed from the default lease list unless a caller is proven, and the consumer predicate changed to fail closed on an empty list;
  - waiver read from signed evidence in IC.
- Proposed W4 switches, not created in W3: execution-core `SANDBOX_CREDENTIAL_INJECTION` (strict parse, default off, read per call) and integration-corev2 `CREDENTIAL_BINDINGS_ENABLED` (default off).

## Not decided here

- The Quarry `secret://` grammar and browser fill. This ADR gives that work its custody and redemption path only.
- The binding administration API and UI (IC and Frontend Plane, W4).
- Whether IC audits proxy-only refusals (O4).

## Proof plan for W4

- IC, Postgres: seal refused for ZDR and waived evidence; redeem refused after revoke, after expiry and for a host that is not bound; the audit row is written, and a failed write returns 503.
- EC, unit: host canonicalisation table; sentinel in the wrong slot or header refused; redirect not followed; plain HTTP refused; response scan across a chunk boundary; the SigV4 output checked against reference vectors.
- Live, on the lead's proof stack: one Do run on an injecting binding. The sandbox environment shows sentinels only, the tool result shows host counts, the IC audit row exists, and no secret appears in logs or in the tool result.

## Open for owners

- **O1, DEC-21 and D-E4.** Ratify integration-corev2 custody with sealed references and sentinels. Owners: Model Plane owner, Quarry plan, Control Plane. This ADR stays Proposed until then, and W4 043(c) and F-t3-057 stay blocked.
- **O2, mechanism and run refusals.** Confirm loopback base-URL routes over the E3 bridge, with TLS interception rejected, and plan-mode and tainted refusals beyond ZDR and waived. Owner: Model Plane owner.
- **O3, SigV4 body.** Confirm the 64 MiB cap and no unsigned payload in W4. Owner: Model Plane owner.
- **O4, refusal audit.** Decide whether proxy-only refusals reach IC's audit, given that ZDR runs write none. Owners: Model Plane owner and Control Plane (audit-core).
- **F1, existing keys.** The workspace CAS keys in execution-core (`MINIO_ACCESS_KEY`, `MINIO_SECRET_KEY`) should move under this custody or a successor ADR. Owner: Model Plane owner.

## Residual risks

- The proxy holds plaintext for the duration of one request. Injection requires this. The mitigations are no cache, no logs, the response scan and short-lived references.
- A bound host can echo the credential in an error body. The response scan covers exact bytes.
- Tainted runs are the prompt-injection case. A sandbox that can reach a bound host can make authenticated requests on the run's behalf, so they are refused (O2).
- Sentinel exfiltration to another host is harmless, because no substitution happens there.
