# W2-CP-E2a status: org-core key API (KEK/DEK wrap, unwrap, rewrap, rotate, destroy) (lane d, branch w23/lane-d)

## Built / partly
Built: the service, the store, the migration, the six routes and the tests. Partly: wiring in `cmd/server/main.go` and the reviewed admin/destroy scopes (see Not done).

Checked first. None of WS17.E2 existed. There was no `internal/orgkeys`, no key migration and no `/keys` route. The only pieces present were the `org:keys:wrap:any` and `org:keys:unwrap:any` allowlist entries and their compose holder, quarry-edge, from W2-CP-R13-2. The scope mapping was missing too: `serviceScopesForRequest` mapped nothing to these scopes.

Migration `030_org_key_encryption_keys.up/down.sql` (LF, like 025-029). The queue named it 028, but 028 and 029 were taken.
- `org_key_encryption_keys`: PK `(org_id, kek_version)`, FK `organizations(id) ON DELETE CASCADE`, `master_key_id`, `wrapped_kek` (60 bytes), and `state` (`active`, `retired` or `destroyed`). A partial unique index allows one active KEK per org. A CHECK requires that a destroyed row holds no key material. Version 0 is the tombstone a destroy leaves when the org never had a KEK. RLS as in 023. `REVOKE ALL` is followed by `GRANT SELECT, INSERT, UPDATE` (no DELETE: the cascade runs as the owner).
- `org_key_master_canaries`: one sealed known plaintext per master id. Owner-only, no RLS.
- `org_key_audit_outbox`: the shape of 027. The subject is pinned to `verevon.audit.v2.control.org-core.org_key_custody`. A CHECK refuses DEK, KEK or context keys in the payload. RLS. `GRANT SELECT, INSERT`.
- Down: drops all three. The file warns that the down migration shreds every org.

`internal/orgkeys` [NEW, CRLF]
- `crypto.go`: AES-256-GCM with a random 12-byte nonce. Output is `nonce || ct`, the pattern of `integration-corev2/internal/crypto/vault.go`. Every open failure returns one error.
- `masterkey.go`:
  - `ORG_CORE_KEYS_ENABLED` and `Enabled`.
  - `MasterKeyProvider` with `EnvMasterKeys`, read from `ORG_CORE_KEY_MASTER_KEYS` (JSON `{"v1":"<b64 32 bytes>"}`) and `ORG_CORE_KEY_MASTER_ACTIVE`. It rejects bad ids, keys that are not 32 bytes, all-zero keys and duplicate keys, and no error message echoes a key.
  - `ValidateConfig`, the boot assertion.
  - `checkCanaries`: every stored canary must open under the key of its id. A missing or wrong key gives `ErrUnavailable`. A configured id with no canary gets one, written insert-if-absent and then re-verified.
- `store.go`: the `Store` interface, the typed errors (`ErrOrgNotFound`, `ErrNoKey`, `ErrDestroyed`, `ErrInvalidContext`, `ErrUnavailable`), `KEKRecord`, the content-free `auditPayload`, and `MemoryStore` for host tests (`DeleteOrg` simulates the cascade).
- `pgstore.go`: `PostgresStore` over `*database.DB`. Org-scoped work runs in `WithOrgScope` (RLS). Creation, rotation and destroy take `pg_advisory_xact_lock(hashtextextended('org-core.orgkeys:'||org))`. Each change writes its audit intent in the same transaction. `FlushAudit` is the claim, publish and mark loop of the other outboxes, on the owner pool.
- `service.go`: `Service` with these operations:
  - `GenerateDataKey`: creates the KEK lazily on first use.
  - `Unwrap`: returns `current_kek_version`. Successful unwraps are counted, not audited.
  - `Rewrap`: the DEK never leaves org-core.
  - `Rotate`: the old KEK is retired and still unwraps.
  - `Metadata`: no key material.
  - `Destroy`: crypto-shred. It does not need the canary.
  - `FlushAudit`, `SetAuditPublisher` and `NewServiceFromEnv`.
  - Binding: DEK AAD is `quarry.profile|org=<org>|profile=<id>` and KEK AAD is `org-core.kek|org=<org>|v=<n>|master=<id>`. The key id is `oc:<n>`. `Ready()` runs the canary check once and retries every 5 s after a failure.
  - Audited: KEK created, KEK rotated, keys destroyed, and denied unwraps (`context_mismatch`, `key_not_found`).

`internal/http` (CRLF)
- `keys_handlers.go` [NEW]: these routes, with their scopes:
  - `POST /internal/orgs/:orgId/keys/data-keys` (wrap): 201 `{data:{key_id, kek_version, alg:"A256GCM", dek_b64, wrapped_dek_b64}}`.
  - `POST .../data-keys/unwrap` (unwrap): `{data:{key_id, dek_b64, current_kek_version}}`.
  - `POST .../data-keys/rewrap` (wrap).
  - `POST .../rotate` (admin).
  - `GET .../keys` (admin).
  - `DELETE .../keys` (destroy).

  Details:
  - Request body: strict JSON, no unknown fields, 8 KiB cap.
  - Every answer is `Cache-Control: no-store`, in typed envelopes.
  - Errors: 404 `org_not_found` / `key_not_found`, 410 `key_destroyed`, 422 `invalid_context`, 400 `invalid_body`, 503 `keys_unavailable`.
  - Bodies are never logged. A 503 logs the op, the org and the error text, which never holds key material.
  - Also in this file: `SetOrgKeys(*orgkeys.Service)` and `ValidateOrgKeysConfig(getenv)`.
- `server.go` [C, in the file set]: `keysOn`/`orgKeys` fields, `keysOn = orgkeys.Enabled(os.Getenv)`, and `s.registerOrgKeyRoutes(internal)`. Routes exist only when the flag is on.
- `service_auth.go` [C, NOT in the file set, deviation]: without this the routes are unreachable, because the middleware 403s an unmapped path. Two changes:
  - The six path-to-scope mappings in `internalOrganizationScopes`.
  - A validator pin: `org:keys:wrap:any` and `org:keys:unwrap:any` are valid only for `quarry-edge`, as the W2-CP-R13-2 depth row asked.

  `service_auth_test.go` was NOT touched. Its existing asserts still hold: quarry-edge may hold wrap and unwrap, and `org:keys:destroy:any`, `rotate:any` and `read:any` fail startup.

Tests written (host unless stated):
- orgkeys: `TestGenerateUnwrapRoundtrip`, `TestUnwrapWithWrongOrgOrProfileContextFails` (wrong profile, foreign org with and without its own v1, tampered blob, bad contexts, unknown purpose, denied-unwrap audit count), `TestUnknownOrgIsNotFound`, `TestDestroyMakesUnwrapGone` (including the empty-org tombstone), `TestHardDeleteCascadeRemovesKeys` (memory), `TestRotateThenRewrapKeepsDEK`, `TestCanaryFailureIsUnavailable` (wrong key, missing key id, first-use canary write), `TestLoadEnvMasterKeysRejectsBadConfigWithoutEchoingKeys`, `TestAuditPayloadsCarryNoKeyMaterial`, `TestParseKeyID`, `TestMigrationIsIdempotent`, `TestMigrationPinsCascadeShredAndLeastPrivilege`, `TestMigrationNumberIsFree`.
- orgkeys, DSN-gated on `CONTROL_LIFECYCLE_TEST_DATABASE_URL` (the lifecycle fixture, like the WS7.H tests): `TestPostgresRoundtripRotateAndDestroy` (including cross-org under RLS), `TestPostgresHardDeleteCascadesKeyRows` (`gdpr_hard_delete_organization`), `TestPostgresMigrationIsIdempotent` (re-applies the file twice in a transaction that is rolled back), `TestPostgresCanaryRejectsWrongMasterKey`.
- http: `TestOrgKeysRoutesAbsentWhenDisabled`, `TestOrgKeysRoutesMountedWhenEnabled`, `TestOrgKeysGenerateUnwrapRoundtrip`, `TestOrgKeysUnwrapWithWrongContextFails`, `TestOrgKeysScopesGateEachRoute`, `TestOrgKeysDestroyMakesUnwrap410`, `TestOrgKeysRotateThenRewrapKeepsDEK`, `TestOrgKeysCanaryFailureReturns503`, `TestOrgKeysWithoutServiceAnswer503`, `TestOrgKeysNoDEKOrWrappedDEKInLogs`, `TestValidateOrgKeysConfig`.
  - `TestOrgKeysScopesGateEachRoute` covers: the gateway, auth-core and billing-core get 403 on all six routes; quarry-edge gets 403 on the three admin routes; it checks the path-to-scope table and the pin.
  - `TestOrgKeysNoDEKOrWrappedDEKInLogs` captures `log` and gin output across every route and the 503 path.
  - The admin and destroy handlers are exercised through a stand-in engine. Their scopes are not allowlisted, so no registry credential reaches them.

## Commits (lane d)
- b95b78ed5 feat(org-core): orgkeys KEK/DEK custody service and migration 030 (WS17.E2a)
- 046b8059e feat(org-core): /internal/orgs/:orgId/keys routes behind ORG_CORE_KEYS_ENABLED (WS17.E2a)
- (this file and the depth rows: docs commit after these)

## Code only, not compiled
No go build, vet or test was run. The integration step runs the compile guard. `gofmt -l -e` was run on LF copies of the new files: there are no syntax errors and they are gofmt-clean. The lane files are CRLF, like the other org-core http files.

## Flag and default
`ORG_CORE_KEYS_ENABLED` defaults to `0`. With it off no `/keys` route is registered (404). With it on but no `SetOrgKeys`, every route answers 503 `keys_unavailable`. Master keys: `ORG_CORE_KEY_MASTER_KEYS` and `ORG_CORE_KEY_MASTER_ACTIVE`, the compose passthroughs from W2-CP-R13-2, not minted.

## Assumed decisions
- X-8 default: the master keys come from env now (`EnvMasterKeys`), with a KMS later behind `MasterKeyProvider`. org-core owns custody in `internal/orgkeys`.
- Migration number 030 (the gap note). The plan's 024 and the queue's 028 were both taken.
- The admin scope is `org:keys:admin:any` (rotate and GET metadata) and the destroy scope is `org:keys:destroy:any`, as the plan names them. Neither is in the allowlist. The existing test calls `destroy:any` unreviewed, and no compose principal holds either, so these three routes are 403 for everyone until a reviewed registry change. The service-level code and tests are complete.
- The boot assertion refuses with the flag on in every mode, not only release mode, when the master keys do not parse. This is stricter than the plan and matches `ValidateSIEMDestinationsConfig`.
- Destroy is terminal. Afterwards data-keys, unwrap, rewrap and rotate answer 410, and a second DELETE answers 410 (the erasure saga treats 404 and 410 as done). An org with no KEK gets a version-0 tombstone, so no fresh KEK can be minted after a shred. There is no un-shred path.
- Cross-org unwrap answers 404 `key_not_found` when the foreign org lacks that version, else 422 `invalid_context` (the AEAD binds the org). This replaces the 403/404 that the R13-2 depth row suggested; the blob is never opened either way.
- Every open failure (wrong org, purpose or profile, or tampering) gives one answer: 422 `invalid_context`. A KEK that will not open under its master key is a custody fault and gives 503.
- Canary: a stored canary whose master id is no longer configured also makes the service unavailable (fail closed). Retiring a master id therefore needs all its KEKs rewrapped first. That operation is not built (depth row).
- Audit: one subject (`..._org_key_custody`) whose `event` names the change. `user_id` is `service:<principal>` and `actor_role` is `service`. Profile ids are not put in audit payloads.
- The successful-unwrap counter is an in-process atomic (`UnwrapCount`), not a Prometheus metric.

## MP asks
None. No Model Plane change is needed.

## Not done (outside the file set) / not verified
- `cmd/server/main.go` wiring is not done, because the file is not in the set. The lead adds this at merge:
  ```go
  if err := httpserver.ValidateOrgKeysConfig(os.Getenv); err != nil { log.Fatal(err) }
  if orgkeys.Enabled(os.Getenv) {
      svc, err := orgkeys.NewServiceFromEnv(db, os.Getenv)
      // fatal on err
      svc.SetAuditPublisher(natsClient) // when present
      server.SetOrgKeys(svc)
      // plus an orgKeys.FlushAudit(ctx, 100) ticker beside the SIEM one
  }
  ```
  Until then the flag-on routes answer 503.
- The reviewed allowlist entries and pins for `org:keys:admin:any` and `org:keys:destroy:any`, and the holder (operator and/or the U41b erasure caller). Adding them also needs `TestOrgKeysAndWebPolicyScopesAreMachineOnly`, the compose holder test and the compose rows changed together.
- The escrow and custody runbook, master key minting and the restore drill (plan DoD) are not done.
- Nothing was compiled or run. The Postgres tests have never run. `pgx` simple-protocol multi-statement `Exec` is assumed in `TestPostgresMigrationIsIdempotent`.
