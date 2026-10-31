# D-B1-w1-security progress (2026-10-27)

Edits made (all uncommitted until the tests below pass), scripts in the scratchpad D-B1/e*.js:
- D1-004 fetch.rs: attempt_record emits redacted_proxy_uri (scheme://host:port) and error_class (no error text); the two other proxy-uri messages in fetch.rs redacted. 4 new tests.
- D1-039 model-gateway quarry.rs: host_class(url) and QuarryError::log_class() replace %url / %error in 7 tracing sites.
- D1-001 + D1-019(c) model-gateway browser_run.rs + state.rs: RunStartGuard (process-local) on AppState: a presented run grant backs one live run (409 browser_grant_in_use), keyed retry with a changed body is 409 idempotency_key_reused. Released in revoke_derived_run_grant.
- D1-020 gateway browser.rs close_session revokes first; dispatchers.rs run_start_headers adds an Idempotency-Key (handlers.rs passes the envelope key).
- D1-025 gateway knowledge/products.rs: one extract call, no prior_block_signals.
- D1-017 spec file: data.session.id, allowedDomains/maxSteps.
- D1-056: already closed at HEAD (browser_capacity_response and browser_session_lost_response map all five codes; tests at browser.rs the_driver_session_cap..., the_driver_page_cap..., a_lost_browser_session_is_410...). Nothing to change.
- D2-038: bootstrap_broker_db.sh at HEAD rewrites the env file with read/printf builtins and passes the password by PGPASSWORD env / psql stdin; no argv. Rotation of the live password stays an owner decision. Not changed.
- D2-012: NOT implemented, see depth/D-D-B1-w1-security.md (gateway has no service-principal caller class).

Tests to run (qtest queue): quarry-runtime fetch; model-gateway browser_run + quarry; native gateway cargo test (baseline 838).

## Final (all committed)
45176bc90 D1-004; dd1c2117a D1-020; b058d30ca D1-025; da790a951 D1-017; b42465e69 D1-039; e1515d2fd D1-001 + D1-019c.
Results: quarry-runtime --lib fetch 58 pass; model-gateway --lib browser_run 66 pass, quarry 58 pass, whole lib 1768 pass 0 fail; native gateway 841 pass 0 fail; playwright --list 12 tests; quarry cargo check --workspace --all-targets rc 0.
Model Plane test recipe: QROOT=Model Plane (whole dir), mount Frontend Plane at /Frontend Plane, apt-get update + install libprotobuf-dev, cd rust, CARGO_TARGET_DIR=/src/target (runner D-B1/mp-test.sh in scratchpad).

## Verifier round 1 (2026-10-27)
- D2-012: ff496d2fd CallerClass + effective_actor (gateway 845 pass = 841 + 4 new); producer still an owner decision.
- D1-001 durable: ValidateGrantResponse.session_key (proto 54, pb.go regenerated natively, broker populates, Go test); model-gateway refuses a presented run grant bound to another gateway run; model-gateway lib 1770 pass 0 fail; browser-broker server tests pass; buf breaking clean.
