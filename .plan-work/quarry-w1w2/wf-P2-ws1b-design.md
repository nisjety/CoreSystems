# WS1.B design note: DeriveRunGrant and the delegated browser bearer

Unit P2-ws1b, 2026-10-18, branch quarryv2-next. Written before coding.

## Shape

- Proto: `rpc DeriveRunGrant(DeriveRunGrantRequest) returns (DeriveRunGrantResponse)`.
  Request `parent_grant_id=1, session_key=2, org_id=3, allowed_domains=4, purpose=5, ttl_seconds=6`;
  response `grant_id=1, endpoint=2, expires_at=3, allowed_domains=4, parent_grant_id=5, kind=6, zdr=7`
  (plan numbers). Go regenerated only; Rust is generated at build by mp-contracts.
- Broker: `authz.Authorize` gets a method case needing `browser:write` for services (users pass, as for
  Acquire); the org pre-check covers `DeriveRunGrantRequest`. `grant.Repository.Derive(ctx, DeriveParams)`
  replaces the `ErrDeriveUnsupported` stub. The server handler checks request shape, calls Derive with the
  verified org and `OwnerFilter(principal)`, records telemetry outcome `derived`.
- model-gateway `browser_run.rs`: validate the caller's grant as today; kind `session` derives a run grant
  keyed on the run's own `run_key` (hoisted so the same key names the managed run and the grant), kind
  `run` or empty keeps the exact-equality path, `sensitive_approval` is 400.
- Frontend gateway: `BROWSER_GRANT_ENFORCEMENT=off|permissive|strict` in config.

## Rules of Derive, and why each holds

1. **Parent lookup is scoped by org and owner.** `GetScoped(parent, verifiedOrg, ownerFilter)`. A grant id
   from another tenant or another user is `NotFound`, indistinguishable from a forged id, so ids cannot be
   probed across tenants. The derived grant's owner is the parent's owner, never the caller's claim.
2. **Parent must be active, of kind `session`, with no sensitive actions.** Revoked or expired parents fail
   in `GetScoped`. A `run` parent is refused so derivation cannot chain (each hop could otherwise reset the
   clock or the key); a `sensitive_approval` is single-use authority and must never seed a reusable grant.
   (`ErrParentNotDerivable`, FailedPrecondition.)
3. **ZDR follows the parent.** The child copies `parent.Zdr`. If the verified principal is ZDR and the parent
   is not, derivation fails with `ErrZdrPostureChanged`: a ZDR caller must not obtain a non-ZDR (persistable)
   grant by deriving from an older non-ZDR session, and the child can never be less private than the parent.
4. **Domains never widen.** An explicit list is normalized like any grant, and each host must equal a parent
   host or be a dot-suffixed subdomain of one (`api.example.com` under `example.com`; `badexample.com` and
   `example.com.evil.net` are not). Otherwise `ErrDeriveWidensParent` (PermissionDenied). The static
   deny-list policy runs again on the child (defence in depth).
5. **Wide-human-browse guard.** An empty request inherits the parent's list, but if that list is longer than
   `BROWSER_BROKER_MAX_RUN_DOMAINS` (8) it is refused unless the caller names an explicit subset, so an
   agent run never silently inherits a broad human session. An explicit list is also capped at 8.
6. **R-16, stricter from day one.** A derive from any derivable parent kind other than `session` (the future
   `human_browse`) requires an explicit non-empty host list of at most 8. Today `session` is the only
   derivable kind, so this is a guard in the kind table, tested directly.
7. **Lifetime never extends.** `expires = min(now + ttl, parent.ExpiresAt)`, ttl 0 = broker default, clamped
   to the broker max. If fewer than 30 s remain the derive is refused rather than minting a grant that dies
   mid-step. Revoking the parent cascades to the child (existing `RevokeScoped`).
8. **Idempotent on (parent, session_key), and a replay cannot widen.** A second call with the same key and
   the same canonical hosts returns the same grant unchanged (same id, same expiry: a replay does not
   refresh the clock). Different hosts are `AlreadyExists`. A revoked or expired child is not resurrected.
9. **Purpose must be `ai_run`.** Any other value is InvalidArgument, so later purposes (`attach`, WS8.C) are
   opt-in additions rather than accidental passes.
10. **Run grants count toward the per-owner active cap**, checked atomically with the insert, so a caller
    cannot mint unbounded grants with fresh session keys.

## Approval parent

`CreateScopedSensitiveApproval` explicitly accepts a `session` or `run` parent (kind table), still refusing
an approval parent and any parent with actions.

## model-gateway status map for derive

PermissionDenied 403 `browser grant is not authorized`; FailedPrecondition and AlreadyExists 409
`browser_grant_not_derivable`; InvalidArgument 400; Unavailable/DeadlineExceeded 503; anything else 403
(fail closed). The derive response is re-checked: kind `run`, parent id echoed, canonical hosts, each host
inside the parent's policy, ZDR not lowered.

## Out of scope here

WS1.A2 (registry row, Quarry service token: X-1), WS1.C (Postgres; `Derive` SQL later), the L2 legs.
Gateway files outside this unit (main.rs, shared.rs, audience_tokens.rs) are recorded as not done if the
enforcement cannot land without them.

## Review round 2 (2026-10-18): limits made explicit
- Approvals are bound like derives: hosts within the parent's, expiry capped at the parent's, under-lock re-read.
- Run grants do not count toward the per-owner cap (20 sessions); they are bounded per parent (8 live).
- run_key is minted per POST /v1/browser/runs: a client retry is a new run with a new run grant. model-gateway
  revokes its derived grant when the start fails after the derive and when the run completes or is cancelled.
- The session grant is not renewed: it lives for the broker TTL (15 min default, 1 h max). An AI run on an older
  session gets 409 browser_session_grant_inactive; renewal needs a Quarry endpoint to swap a live run's grant id.
- Quarry start_run binds a validated grant to the user (owner_id), refuses a ZDR grant on a non-ZDR run, and lets
  a grant's hosts replace the caller's in every mode. Production effect still waits for Quarry's service
  credential (WS1.A2, X-1).
