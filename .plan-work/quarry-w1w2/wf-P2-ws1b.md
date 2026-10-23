# WS1.B hand-over (unit P2-ws1b), 2026-10-18

## Commits (branch quarryv2-next, local only)
- 0997b7c8 feat(browser-broker): DeriveRunGrant (proto, regenerated Go, authz, grant.Derive, server, errors, tests). First agent; re-verified: go vet + go test -count=1 ./... green in browser-broker.
- 6da7ffd8 feat(model-gateway): browser runs derive their own grant from a session grant.
- d26c61a5 feat(gateway): in-app browser sessions carry a BrowserBroker grant (BROWSER_GRANT_ENFORCEMENT).
- 27801ea1 feat(quarry-edge): the owner browser-session projection carries grantId.

Design note: wf-P2-ws1b-design.md.

## Changes made by the resume agent on top of the first agent's draft
- model-gateway: DeriveRunGrant Unimplemented -> 502 browser_grant_derive_unavailable (plan's rollback row), was 403.
- gateway: removed the "session:<id>" placeholder for off mode; a session without a grant is 409
  browser_session_grant_missing in every mode. Added a first-step-failure revoke test. rustfmt.

## Left
- Quarry-side grant checks + tests (owner, ZDR, domain override in every mode) need WS1.A2 / X-1.
- L1 contract test does not pin derive field numbers; L1 assertions (gateway has no session: string,
  required_browser_token) not added; L2 legs not run; nothing deployed; no Postgres Derive (WS1.C).
- execution-core browser_risk test cross_domain_goto_is_flagged_even_when_both_hosts_are_in_the_grant not added.
- Router-level "derive failure creates no thread/run" test not written (ordering is structural).
- BROWSER_BROKER_MAX_RUN_DOMAINS not set in compose; BROWSER_GRANT_ENFORCEMENT not set in compose
  (dev default permissive, prod default strict, prod refuses off).

## Traps
- model-gateway via qtest: mount QROOT="apps/Model Plane" (mp-contracts reads ../../../proto) and
  `cd rust && export CARGO_TARGET_DIR=/src/target`. The image's protoc has no well-known protos:
  set PROTOC_INCLUDE to a dir with google/protobuf/{any,duration,struct,timestamp}.proto (copied from
  ~/go/pkg/mod/github.com/bufbuild/protocompile@*/wellknownimports; scratch P2-ws1b/wkt.b64 is a tgz).
- With that mount, 5 model-gateway lib tests fail for environment reasons (4 read verevon-web fixtures
  outside the mount; zdr_precheck reads CRLF sse.rs and searches "\n}\n").
- rustfmt on Windows rewrites CRLF files as LF: convert back after formatting.
- Dev rollout: permissive mode issues a grant and Quarry validates it; if Quarry's validator is
  configured but cannot validate user grants (no service token, X-1), UI session creation fails at Quarry.

## Review round 2 (2026-10-18)
Commits: 73ad8767 (broker), 9d835979 (gateway), 33a362f6 (model-gateway), 6216f19f (Quarry), plus a plan ledger row.
Left: session-grant renewal (needs a Quarry endpoint), a client idempotency key for AI-run starts, route-level Quarry start_run
grant tests, deleting GrpcGrantValidator::acquire, an SPA i18n entry for browser_session_grant_inactive, and the per-parent run cap
as an env setting.
Trap: Quarry canonical_broker_domains requires already-canonical input (it refuses, never repairs).
