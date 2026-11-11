# Organisation key custody runbook

Status: code built dark (build plan WS17.E2a in org-core, WS17.E2b in Quarry). Nothing here is enabled. Minting, escrow, the loss plan and the restore drill are person steps, and none of them has run. Checked against the tree on 2026-11-11.

This file covers the org-core master key, which wraps each organisation's key-encryption key (KEK), and the Quarry browser profiles sealed under those KEKs (key ids `oc:<kek_version>`). The Quarry side of the same provider is in `apps/Ingestion Plane/Quarry-v2/docs/RUNBOOKS.md` (WS17.E1 and the E2b notes).

## Terms

- Master value: one 32-byte key per id, held in `ORG_CORE_KEY_MASTER_KEYS` as JSON `{"v1":"<base64 of 32 bytes>"}`. `ORG_CORE_KEY_MASTER_ACTIVE` names the id that wraps new KEKs, and it is required. It is not Quarry's env master (`QUARRY_PROFILE_MASTER_KEYS`). That key wraps the DEKs of `env:` rows and has its own custody in RUNBOOKS.md.
- KEK: one per organisation and version, stored wrapped under a master id in `org_key_encryption_keys`.
- DEK: one per profile at a time. A new DEK is made on every 256th stored save. The DEK is wrapped by a KEK and kept inside the Quarry row (`quarry_profiles.envelope`), and `quarry_profiles.key_id` is `oc:<kek_version>`.

The chain is: master value, then KEK (per organisation and version), then DEK (per profile), then the snapshot.

Commands below use placeholders such as `<org>`, `<profile_id>` and `<token>`. Never paste a real value into a ticket, a chat or a log.

## 1. What is built and what blocks a flip

Each row was checked in the tree on 2026-11-11. Check it again before you act, because other units change these files.

| # | Gap | Effect now | To clear it |
|---|---|---|---|
| 1 | `cmd/server` does not call `orgkeys.NewServiceFromEnv`, `SetOrgKeys`, `ValidateOrgKeysConfig` or the audit flush. The comment in `server.go` says it does. | With `ORG_CORE_KEYS_ENABLED=1` every key route answers 503 `keys_unavailable`, and the log says `org keys: no key service configured`. | Wire the service in `cmd/server/main.go` and rebuild the image. |
| 2 | `envelope_profile_store.rs` compares a row's key id with `current_key_id()` (near lines 125 and 249). For the org-core provider that is the bare `oc:` prefix, which no row equals. | With `org_core` and mode `on`, every save mints a new DEK, and every load reseals the row. | Use `is_current_key(org, key_id)` at both sites, with tests. |
| 3 | `org:keys:admin:any` (rotate and GET metadata) is held by no principal. | Rotate and GET metadata answer 403 to everyone, so the rotation step in section 6 cannot run. | A reviewed registry change that grants a named operator principal. |
| 4 | No master value exists. The secrets bootstrap has no entry for it. | Nothing can be wrapped, so the flag must stay off. | Section 3, a person step. |
| 5 | No escrow exists. | A loss cannot be recovered. | Section 4, with both custodians. |
| 6 | The production overlay (`docker-compose.production.yml`) has no entry for these variables. | There is no production path. | The owner settles the production secret path first. |
| 7 | `QUARRY_ORG_CORE_SERVICE_TOKEN` must be mirrored into Ingestion's ledger. The tree cannot show whether that has happened. | The edge refuses to boot with `org_core` when the token is missing. | Copy the same value. Never mint it again. |
| 8 | The restore drill has not run. | Recovery is not shown to work. | Section 8. |

The `quarry-edge` credential holds `org:keys:wrap:any`, `org:keys:unwrap:any`, `org:keys:destroy:any` and `org:retention:read:any`. The destroy scope is used only by the erasure saga's shred hop (`QUARRY_EDGE__ERASE_SHRED_KEYS`, default off). No operator can shred an organisation today.

## 2. Rules that always apply

- One id, one value, for its whole life. Never regenerate, rename or reuse an id that has a canary or a KEK row.
- A master value never goes into the repository, a ticket, chat, email, a CI variable, a container image, a log, a shell history or a database backup set. A backup set that holds both an org-core dump and a master value defeats the shred (section 11).
- Do not copy a value out of a running container. Use the escrow.
- Keep master values away from re-mint helpers. In `scripts/bootstrap_runtime_environment.sh`, `ensure_secret` mints a new value when the stored one is a placeholder. `ensure_base64_32_secret` mints one when the stored value does not decode to 32 bytes. The JSON form never decodes to 32 bytes, so a re-run would rotate the master and orphan every `oc:` profile.
- `scripts/run-control-plane.sh` appends a name only when it is missing, so a manual entry survives a re-run. Its list of names does not include these variables.
- Do not run `docker compose config` into a shared place. It prints interpolated values.
- Restart org-core after any change to the master variables. The process reads them at boot, and it does not repeat the canary check once that check has passed.

## 3. Minting the master value (once)

The design is that the secrets bootstrap mints this value once into the ledger and never re-mints it. Until the bootstrap has that entry, the procedure below is the manual equivalent.

Preconditions, run as the owner on the `org_core` database:

```sql
SELECT count(*) FROM org_key_master_canaries;  -- must be 0
SELECT count(*) FROM org_key_encryption_keys;  -- must be 0
```

Any non-zero count means this is not a first mint. Stop.

1. The owner and both custodians are present. Use an offline machine.
2. Generate 32 random bytes as base64 with `openssl rand -base64 32`. Write the entries straight into the Control Plane ledger, `apps/Control Plane/.env.generated-secrets` (gitignored, mode 0600). Single-quote the JSON so that Compose keeps it as written:

   ```
   ORG_CORE_KEY_MASTER_KEYS='{"v1":"<base64 of 32 bytes>"}'
   ORG_CORE_KEY_MASTER_ACTIVE=v1
   ```

   On the dev runner, `.env.generated-secrets` is passed to Compose last, so it wins. Do not put the value in `org-core/.env`. The compose entry `ORG_CORE_KEY_MASTER_KEYS: ${ORG_CORE_KEY_MASTER_KEYS:-}` takes its value from interpolation, and an empty interpolation overrides `env_file:`.
3. Escrow the value (section 4) before the flag is ever set.
4. Write the minting record (section 12). Store the SHA-256 of the JSON string, computed offline, and never the value: `printf '%s' '<json>' | sha256sum`.
5. Leave `ORG_CORE_KEYS_ENABLED` at 0 until section 9 is complete.

## 4. Two-person escrow

Each master id is split in two. Both halves are needed to recover it, and neither half alone says anything about the value.

- Offline, with both custodians present, decode the base64 value to its 32 raw bytes. Share A is 32 fresh random bytes. Share B is the raw value XOR share A, byte by byte.
- Custodian A holds share A. Custodian B holds share B. Each share goes in its own tamper-evident, dated envelope. Each envelope is initialled across the seal, and each is kept in its own locked location.
- Opening an envelope needs its custodian and one witness, and one entry in the escrow log: date, id, reason, the roles present, and the seal number.
- Recombine only for the restore drill (section 8), a loss event (section 5) or a re-split. Recombine on an offline machine.
- A lost or compromised share means the escrow is not working for that id. Re-split from the live value while it still exists: both custodians present, new shares, new envelopes, and the old envelopes destroyed under the log.
- Each new master id gets its own split before any switch to it (section 6).
- Custodian names stay in the owner's records. They are not in this repository. Who holds and backs up the master key is an open owner question.

## 5. Loss

A loss is any master id whose value no longer exists anywhere: not in the live ledger and not in either escrow share.

- Every KEK wrapped under that id cannot be opened, so every DEK under those KEKs is unrecoverable. Every `oc:`-sealed profile of every organisation that uses those KEKs stays unreadable. The rows are not deleted or changed, so a recovered value restores access.
- Each id's canary is stored, so a missing or wrong value fails the canary and every key route except the shred answers 503 `keys_unavailable`. This is the fail-closed rule. It takes the key service down for every organisation, not only for the organisations wrapped under the lost id.

Response:

1. Stop. Do not regenerate a value, remove an id, run SQL against canaries or KEK rows, or restart in a loop.
2. Check the live ledger value, then the escrow shares, with both custodians present. A recovered value restores the service. Confirm it with section 7 and an unwrap as in section 8, steps 4 and 5. Log the incident.
3. If no copy survives, record a loss event and tell the owner and counsel. The profiles wrapped under those KEKs stay unreadable. Restoring the service for the other organisations needs an owner decision on removing the lost id's canary and shredding its KEKs. That is a recorded decision, not an ad hoc query.

## 6. Adding a master version

This is add-only. The add step needs the escrow for the new id (section 4). The rotation step also needs section 1 rows 1 to 3 to be clear.

1. Pick the next id, for example `v2`. Ids match `^[a-z0-9][a-z0-9._-]{0,31}$`. Never reuse an old id.
2. Mint the new value (section 3, steps 2 to 4, for this id) and escrow it (section 4). Keep every older id in the JSON.
3. Set `ORG_CORE_KEY_MASTER_ACTIVE=v2`. Restart org-core. The canary for `v2` is written on the first key call. `ORG_CORE_KEY_MASTER_ACTIVE` is required. The design's default (the highest id) was not built.
4. New KEK versions are wrapped under `v2`. That covers new organisations on first use, and organisations that are rotated. Existing KEKs stay under the id they were wrapped with, so `v1` must stay configured for as long as any KEK row or canary names it.
5. To move an organisation onto `v2`, call `POST /internal/orgs/<org>/keys/rotate`. This needs the admin scope, which section 1 row 3 blocks. The new KEK is wrapped under `v2`. The old one becomes `retired` and still unwraps.
6. Quarry moves each profile onto the current KEK when it reseals the row, which makes a new DEK under the current KEK. No operator call is needed for this. It depends on section 1 row 2. No Quarry path in this tree calls the org-core rewrap route.
7. Check the result:

   ```sql
   SELECT kek_version, master_key_id, state
   FROM org_key_encryption_keys
   WHERE org_id = '<org>'
   ORDER BY kek_version;
   ```

   The newest `active` row should name `v2`.

Retirement is not possible in this build. A stored canary whose id is no longer configured makes the service unavailable, and rewrapping existing KEKs under a new master is not built. Keep `v1` configured and escrowed.

## 7. Diagnosing 503 `keys_unavailable`

Every org-core key answer of 503 logs one line: `org keys: <op> org=<org> unavailable: <error text>`. The response body does not say why. The error text names a master id and never a value.

| Error text after `unavailable:` | Meaning | Action |
|---|---|---|
| `orgkeys: keys unavailable: master key <id> does not open its canary` | The configured value for that id is not the one that sealed its canary. It may be a typo, a wrong copy or a changed value. | Restore the value from the ledger or the escrow. Change nothing else. |
| `... master key <id> has a stored canary but is not configured` | A canary exists for an id that is missing from `ORG_CORE_KEY_MASTER_KEYS`. | Put the id back with its value. Never delete a canary to clear this. |
| `... active master key is not configured` | `ORG_CORE_KEY_MASTER_ACTIVE` names an absent id, so no new KEK can be made. | Fix `ORG_CORE_KEY_MASTER_ACTIVE` and restart. |
| `... master key <id> is not configured` | A stored KEK names an id that is absent. | Put the id back with its value. |
| `... kek <n> does not open under master key <id>` | One KEK row does not open under the configured value of its id. | The value for that id is wrong. Restore it from the escrow. |
| `... read canaries: ...`, `... store canary: ...` or another database error | The owner pool or the store is failing. | Check the database. The request is retryable and nothing has changed. |
| `org keys: no key service configured for org=<org>` (no `unavailable:` text) | The binary does not wire the service. | Do not enable the flag on this image (section 1, row 1). |

Queries for the Control Plane side:

```sql
SELECT master_key_id, created_at FROM org_key_master_canaries ORDER BY master_key_id;
SELECT master_key_id, state, count(*) FROM org_key_encryption_keys GROUP BY 1, 2 ORDER BY 1, 2;
```

On the Quarry side, the edge logs `org-core key call failed op=<generate|unwrap|rewrap> status=<n> key_error=<class>`. A failed restore logs `profile_load_failed reason=<class>`.

| Status | `key_error` | Meaning |
|---|---|---|
| 0 (transport error or timeout) | `unavailable` | org-core is down or the path is broken. Each call can take about 6 s (3 s timeout, one retry). |
| 503 or another 5xx | `unavailable` | Read the org-core log for the same request, then the table above. |
| 401, 408 or 429 | `unavailable` | For 401, the tokens have drifted. Compare `QUARRY_ORG_CORE_SERVICE_TOKEN` in both ledgers without printing it. |
| 404 | `denied` | No such KEK version, or the flag is off and the route is absent. Check `ORG_CORE_KEYS_ENABLED` and the KEK table. A point-in-time restore causes this too (section 11). |
| 403 | `denied` | The credential row lacks the scope. |
| 422 | `denied` | Wrong organisation, profile or context, a tampered wrap, or a KEK version that was reused after a restore. |
| 410 | `destroyed` | The organisation's keys were shredded. The profile is gone by design. |
| 400 and other 4xx | `invalid` | The request is malformed. Read the org-core log. |

A restore reports `reason=key_unavailable` (retry later; the stored profile is not overwritten), `reason=key_destroyed` (treated as no profile), or `reason=corrupt`. The `corrupt` reason also covers `denied` and `invalid` key errors. When a key call is the cause, look at the key error and not at the snapshot.

## 8. Restore drill

Run the drill before any flip, after any change to the master variables, and on a cadence the owner sets while `oc:` rows exist. It shows that the escrowed value opens the stored canaries and one stored DEK, and that one profile decrypts.

Inputs: both escrow shares, with both custodians present. A dump of the `quarry_edge` database and a dump of the `org_core` database. An isolated scratch environment with no route to production org-core, no production Redis, and no production credential other than the mirrored service token.

Dump order: take `quarry_edge` first, then `org_core`. A profile row can name only a KEK that existed before that row was written. So an org-core dump taken after the quarry dump always holds every KEK the quarry dump names. The reverse order can orphan profiles.

1. Restore both dumps into the scratch stack. Note the time of each dump.
2. Rebuild the value offline from both shares. Write it to a scratch-only file (mode 0600). Never write it to the ledger.
3. Negative control. Start scratch org-core with one character of one id's value changed in the scratch copy, and call unwrap as in step 5. Expect 503. The log line names the changed id and ends in `does not open its canary`. Check that the row counts and the `rev` values are the same before and after.
4. Start scratch org-core with the correct value. Pick one stored profile:

   ```sql
   SELECT org_id, profile_id, key_id, saved_at, rev
   FROM quarry_profiles
   WHERE key_id LIKE 'oc:%'
   ORDER BY saved_at DESC
   LIMIT 1;
   ```

   Read its wrapped DEK from the envelope, which is JSON in a byte column:

   ```sql
   SELECT convert_from(envelope, 'UTF8')::jsonb ->> 'wrapped_dek'
   FROM quarry_profiles
   WHERE org_id = '<org>' AND profile_id = '<profile_id>';
   ```

5. Unwrap it through scratch org-core. The answer holds the plaintext DEK, so print only the status. Use unwrap, not data-keys, because data-keys can create a KEK.

   ```bash
   curl -s -o /dev/null -w "%{http_code}\n" -X POST \
     "http://<scratch org-core>/internal/orgs/<org>/keys/data-keys/unwrap" \
     -H "X-Service-Id: quarry-edge" \
     -H "X-Service-Token: <mirrored token>" \
     -H "Content-Type: application/json" \
     -d '{"purpose":"quarry.profile","context":{"profile_id":"<profile_id>"},"key_id":"oc:<n>","wrapped_dek_b64":"<wrapped DEK>"}'
   ```

   Expect `200`. The auth is by the `X-Service-Id` and `X-Service-Token` headers, not a bearer token.
6. Decrypt the snapshot through the scratch edge. Call `POST /v1/profiles/<profile_id>/restore_probe` with a user token for the profile's organisation and body `{"url":"https://drill.invalid/"}`. Expect 200, and `cookies_count` equal to the count recorded from production before the dump. The probe returns counts only. If no user token can be issued in the scratch stack, the drill is incomplete. Say so in the drill log.
7. Record the drill (section 12). Destroy the scratch stack and its volumes, and delete the scratch value file.

## 9. Enabling

Enable org-core first, then Quarry. Start on a non-production stack.

Gates. All of them must hold, and each one is recorded in the custody log:

1. Section 1 rows 1, 2, 6 and 7 are clear. Row 3 is clear if rotation is part of the plan.
2. Sections 3 and 4 are done for every id in use.
3. Section 8 has passed on the current data.
4. The owner has ratified the custodians and the limits in section 11.

org-core:

a. Set `ORG_CORE_KEYS_ENABLED=1` in the ledger, and restart org-core.
b. From the host, call data-keys for a test organisation with the `quarry-edge` credential and a body of `{"purpose":"quarry.profile","context":{"profile_id":"<test profile id>"}}`. Expect 201. The call creates that organisation's first KEK. With another principal's credential, expect 403. A 503 that says `no key service configured` means section 1 row 1 is not clear. Set the flag back to 0. That is safe only while no `oc:` row exists.

Quarry, staging first:

c. Set `QUARRY_EDGE__ORG_CORE_KEYS_URL`, `QUARRY_EDGE__ORG_CORE_KEYS_TOKEN` (from the mirror) and `QUARRY_EDGE__PROFILE_KEY_PROVIDER=org_core`.
d. Keep the mode at `off` or `compat` until section 1 row 2 is clear. Writes in `compat` stay plaintext, so no `oc:` row appears before mode `on`. Then follow the WS17.E1 rollout in RUNBOOKS.md: `compat`, then `on` once every pod runs the new code, then `REQUIRED` after the backfill.
e. Evidence: a persistent run writes an `oc:<n>` row. Check it with `SELECT key_id FROM quarry_profiles WHERE profile_id = '<id>';`.

Production: the production overlay has no entry for these variables (section 1, row 6). The owner settles that first. Then this section is repeated on production, with the owner present.

## 10. Rollback and switches

- `ORG_CORE_KEYS_ENABLED` on org-core is an outage switch, not a rollback. At 0 the key routes are not registered. quarry-edge gets 404, which it reads as `denied`, and every `oc:` profile is unreadable until the flag is back at 1. Set it to 0 only with the owner's approval.
- `QUARRY_EDGE__PROFILE_KEY_PROVIDER` can go back from `org_core` to `env` only while no `oc:` row exists:

  ```sql
  SELECT count(*) FROM quarry_profiles WHERE key_id LIKE 'oc:%';  -- must be 0
  ```

  After the first `oc:` row, the provider stays `org_core`.
- `QUARRY_PROFILE_MASTER_KEYS` (Quarry's env master) stays configured while any `env:` row exists: `SELECT count(*) FROM quarry_profiles WHERE key_id LIKE 'env:%';`.
- Rolling the Quarry mode back from `on` to `compat` follows RUNBOOKS.md (WS17.E1).
- The down file for migration 030 destroys every organisation's key rows. Run it only when `SELECT count(*) FROM org_key_encryption_keys WHERE state <> 'destroyed';` returns 0.
- Master values are add-only (section 6). Never remove an id that has a canary or a KEK row.

## 11. Limits to take to counsel

These are facts about the design. Counsel decides what they mean.

1. What a shred removes. `DELETE /internal/orgs/<org>/keys` sets every KEK's wrapped value to NULL, clears its master id and marks it destroyed. Later key calls for that organisation answer 410. A shred does not check the canary, so it still works during a key outage. A hard delete of the organisation removes its rows through the foreign-key cascade. A soft delete, which has a 30-day grace period, does not shred.
2. Backups. A backup of org-core taken before a shred still holds the wrapped KEK. The master value is held outside the database. A shred is therefore effective once every backup that holds the KEK has expired or been destroyed. A backup set that also holds a master value defeats this.
3. Restores. Restoring an older org-core database brings back KEKs that were shredded after its time. Re-run the shred for every organisation destroyed after the restore point. The destroy events are audited under the content-free subject `verevon.audit.v2.control.org-core.org_key_custody`.
4. A point-in-time restore of org-core to a time before a KEK existed orphans every profile that names that KEK, when quarry_edge keeps those rows. Their key ids name rows that no longer exist, so they answer 404 and stay unreadable. A later KEK can reuse the same version number, but it cannot open the old DEKs, which gives 422. The fail mode is closed.
5. Process memory. A quarry-edge replica can keep an unwrapped DEK for up to 300 seconds. The erasure route evicts an organisation's DEKs only in the replica that answers the call. Other replicas keep them until the TTL ends.
6. Shared master. One master value protects every organisation. Shredding one organisation's KEKs does not change the master.
7. Loss. Losing a master value makes the profiles wrapped under its KEKs unreadable, for every organisation that uses them, and takes the key service down for all of them (section 5).
8. Availability. While org-core cannot answer, restores and saves of `oc:` profiles fail with a retryable class. Saves do not overwrite the stored profile. Content is not changed.
9. Logs and audit. No log line or audit payload carries a key or a wrapped key. Successful unwraps are counted in the process, not audited. Denied unwraps are audited.
10. Evidence. Until the drill in section 8 has run and is logged, the recovery position is a design claim, not a demonstrated fact.

## 12. Custody records (outside the repository)

Keep these in the owner's records, never in this repository:

- Minting record: date, id, the SHA-256 of the JSON value, custodians by role, and where each share is kept.
- Escrow log: every opening and closing, with date, id, reason, roles present and seal number.
- Change log: every change to `ORG_CORE_KEYS_ENABLED`, `ORG_CORE_KEY_MASTER_KEYS`, `ORG_CORE_KEY_MASTER_ACTIVE`, `QUARRY_EDGE__PROFILE_KEY_PROVIDER` and the Quarry mode, with the reason.
- Drill log: date, dump times, scratch image versions, the results of steps 3, 5 and 6, and the commit of this file that was used.
- Loss log, if there is one (section 5).

## References

- Build plan slice WS17.E2 in `docs/plans/quarry-browser-build-plan-2026-10-14.md`, and decision X-8 in the same file.
- `apps/Control Plane/org-core/internal/orgkeys/` (the code), and `apps/Control Plane/org-core/migrations/030_org_key_encryption_keys.up.sql`.
- `apps/Control Plane/CONTROL_PLANE_OWNERSHIP.md`, section "Organisation Key Custody".
- `apps/Ingestion Plane/Quarry-v2/docs/RUNBOOKS.md`, the WS17.E1 section and the profile routes section.
