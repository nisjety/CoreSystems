# Audit chain runbook

This runbook covers the Control Plane audit chain: the per-org hash chain on `audit_events`, the signed head attestations and retention checkpoints, the chain verifier, the owner and admin compliance API, and the SIEM exporter. It is the operator companion to build plan WS06 (`docs/plans/model-plane-chat-build-plan-2026-10-14.md`). Owner: Control Plane (`apps/Control Plane/audit-core`, `apps/Control Plane/org-core`).

**Status.** The code is built and off by default (WS06 S0 to S8): every change of behaviour sits behind a switch that defaults to off. This file describes what that code does and what an operator does with it. It does not say that the chain, pruning or export is switched on in any environment. Dated evidence for each switch change is kept separately (section 9). The facts here were checked against the source tree on 2026-10-24.

## 1. What the chain proves, and what it does not

With the chain on, the rows it holds are consistent with each other and with the signed heads and checkpoints that still exist.

- **Per row.** A new `audit_events` row for an org that has the chain on receives `chain_seq`, `prev_hash` and `row_hash`. The row hash is SHA-256 over a fixed, length-prefixed encoding of the stored fields (format v1, `internal/chain`). Inserts for one org are serialised by a head lock, so concurrent writers do not leave gaps in `chain_seq`.
- **Per head.** Every `AUDIT_CHAIN_ATTEST_INTERVAL` (default 60 seconds), each org head that is not marked broken is signed with Ed25519, and the signature is stored in `audit_chain_attestations`.
- **Per prune.** A retention purge deletes chained rows only from a prefix it has just verified, and writes a signed checkpoint in the same transaction.
- **Content.** The chain covers the audit envelope and its `details`. It does not cover conversation content. The size and key checks on `details` only count (`audit_core_details_guard_total`); they do not reject a row. "Content-free" is a producer contract.

What it does not prove:

- **A database owner acting alone.** The signing key is held by the audit-core process, not in the database. A database owner can rewrite rows, recompute their hashes and delete the attestations that cover them. The rewritten chain then recomputes cleanly, and the verifier does not report it as broken. Detection needs a signed head or checkpoint that covers the rewritten rows and is held outside the database: a SIEM copy, or a customer export taken earlier. That copy is the external witness.
- **Completeness.** The chain covers the rows audit-core stored. An event that was never published, or that was dead-lettered, is not in the chain, and nothing in the chain shows it is missing.
- **Rows from before activation.** Rows written before an org's chain was switched on are `legacy_unchained`. They are never rewritten and are outside the chain. Reports count them (`legacy_unchained_rows`) and say `verified_with_legacy` when the chained part verifies.
- **The guard trigger.** Migration 007 adds a trigger on `audit_events` that refuses ordinary UPDATE and DELETE. It stops mistakes. A role that owns the table can drop it or set `audit.maintenance`, so it is not a control against an attacker.

## 2. Switches

All switches default to off. audit-core and the SIEM exporter accept only the literal `1` to turn a switch on. org-core also accepts `true`. The verifier follows the chain switch and turns off only for the literal `0`.

| Variable | Read by | Default | Effect |
|---|---|---|---|
| `AUDIT_CHAIN_ENABLED` | audit-core | `0` | Only `1` turns the chain on. Off, new rows are not chained. |
| `AUDIT_CHAIN_KEY_ID` | audit-core | empty | Id of the active signing key: letters, digits, `.`, `_` and `-`, at most 64 characters. |
| `AUDIT_CHAIN_PRIVATE_KEY_BASE64` | audit-core | empty | Active Ed25519 seed (32 bytes) or expanded key (64 bytes), base64url without padding or standard base64. |
| `AUDIT_CHAIN_PUBLIC_KEY_BASE64` | audit-core | empty | Public key of the active pair. Boot fails when it does not match the private key. |
| `AUDIT_CHAIN_VERIFY_KEYS` | audit-core | empty | Retired public keys as comma-separated `key_id:public_key` pairs. |
| `AUDIT_CHAIN_ATTEST_INTERVAL` | audit-core | `60s` | How often healthy heads are signed. An invalid value falls back to the default. |
| `AUDIT_CHAIN_PRUNE_ENABLED` | audit-core | `0` | Only `1` lets the purge delete verified chained prefixes, with a signed checkpoint per batch. |
| `AUDIT_CHAIN_PRUNE_DRYRUN` | audit-core | `0` | Only `1` makes the purge verify and log its plan. It wins over `AUDIT_CHAIN_PRUNE_ENABLED`. |
| `AUDIT_CHAIN_VERIFY_ENABLED` | audit-core | empty | Empty follows `AUDIT_CHAIN_ENABLED`. `0` turns the verifier off. Any other value keeps it on only while the chain is on. |
| `AUDIT_CHAIN_VERIFY_INTERVAL` | audit-core | `24h` | Spacing of verifier passes. Every seventh pass is a full pass from the trust base. |
| `AUDIT_RETENTION_DAYS` | audit-core | `365` | Retention window for `audit_events` and `usage_events`. |
| `AUDIT_SIEM_EXPORT_ENABLED` | siem-exporter | `0` | Only `1` makes the exporter deliver. Otherwise it idles. |
| `AUDIT_SIEM_ALLOW_PRIVATE_DESTINATIONS` | siem-exporter | `0` | Development only. Boot refuses `1` when `ENVIRONMENT=production`. |
| `ORG_SIEM_DESTINATIONS_ENABLED` | org-core | `0` | `1` or `true` mounts the destination routes. Otherwise they answer 404. |
| `ORG_CORE_SIEM_SEAL_KEY` | org-core | empty | 32-byte AES-256-GCM key that seals destination secrets. |
| `ORG_CORE_SIEM_SEAL_KEY_ID` | org-core | `v1` | Id stored with each sealed secret. |

Boot rules. With `AUDIT_CHAIN_ENABLED=1` and `ENVIRONMENT=production`, audit-core refuses to start without a signing key. `AUDIT_CHAIN_PRUNE_ENABLED=1` refuses to start without a signing key in any environment. When the chain or pruning is on, a key that does not decode, or a public key that does not match its private key, stops boot. With the chain on and no key outside production, the chain runs, heads are not attested, and chained rows cannot be pruned; audit-core logs a warning.

`scripts/run-control-plane.sh` generates the audit-chain pair and the seal key when they are missing. It never overwrites an existing value.

## 3. Verifying a chain

### 3.1 The verifier inside audit-core

When the chain is on and `AUDIT_CHAIN_VERIFY_ENABLED` is not `0`, audit-core runs a verifier pass at start and then every `AUDIT_CHAIN_VERIFY_INTERVAL`. Each pass checks every org that has a chain head, resumes from the stored cursor, and writes one row per org to `audit_chain_verifications` (`last_status`, `last_reason`, `first_broken_seq`, `verified_through_seq`, `legacy_unchained_rows`, `last_run_at`, `last_full_at`).

Metrics and alerts (`apps/Control Plane/observability/rules/audit-core.rules.yml`):

- `audit_core_chain_verify_failures_total` increases once for each broken org in each pass. Alert `ControlAuditChainVerifyFailed`.
- `audit_core_chain_broken_total` increases when the insert path finds a head that no longer matches its stored tip row. Alert `ControlAuditChainBroken`.
- `audit_core_chain_last_verify_timestamp_seconds` is the time of the last complete pass. Alert `ControlAuditChainVerifyStale` fires after 36 hours. The series exists only while the verifier runs, so with the verifier off that alert cannot fire. Keep the verifier on while the chain is on.

### 3.2 On demand, through the gateway

An owner or admin of the organisation can call:

`POST /api/v1/audit/compliance/verify`, with an optional JSON body that takes `from_seq`, `from_hash` and `max_rows`. Unknown fields are refused. `max_rows` defaults to 250,000 and cannot exceed 1,000,000. A run capped by `max_rows` reports `through_seq` and `through_hash`; send them back as `from_seq` and `from_hash` to continue. The response is `{"data": report, "error": null}`.

Each call writes a self-audit row (`compliance_audit_verify`). If that row cannot be written, the call fails with 500 and returns no report. Members get 403.

### 3.3 Offline, from an export

Use this when the database cannot be read, or to check an export kept for an auditor. Do not filter the export. A filtered set has gaps, and the verifier reports a gap as a break.

1. Get the keys: `GET /api/v1/audit/compliance/keys`. Each entry has `key_id`, `algorithm`, `public_key` (base64url) and `status`. A key the server does not hold comes back without `public_key`, and the anchors that name it cannot be checked.
2. Get the manifest: `GET /api/v1/audit/compliance/manifest`, with optional `since` and `until` (RFC 3339). Save the object under `data` as `manifest.json`. The CLI reads a bare manifest object. A saved envelope gives no checkpoint and no attestation, so the result is weaker than the chain really is.
3. Get every page: `GET /api/v1/audit/compliance/events?limit=1000`, then repeat with `cursor` set to `meta.next_cursor` until it is null. Save each response unchanged as `page-001.json`, `page-002.json` and so on. The CLI reads the envelope.
4. Run the CLI. It is `/usr/local/bin/audit-verify` in the audit-core image, or a build of `cmd/audit-verify`:

```
audit-verify -manifest manifest.json -key-id <active key_id> -public-key <active public_key> \
  -verify-keys "<retired key_id>:<retired public_key>" page-*.json
```

Flags come before the page files. `-verify-keys` takes retired keys as comma-separated pairs. Add `-json` for the machine-readable report.

5. Read `status`, `reason` and `first_broken_seq`. Exit codes: 0 for `verified` or `verified_with_legacy`; 2 for `broken`; 3 for `partial`, `key_unavailable`, or input the tool cannot read; 1 for bad usage. Rows at or below a signed checkpoint are dropped, and the CLI says how many. The checkpoint covers them.

### 3.4 Reading a result

| Status | Meaning | What to do |
|---|---|---|
| `verified` | The checked range recomputes, its head is the last row, and a valid signed attestation anchors it. It does not show completeness. | Record the range and the head. |
| `verified_with_legacy` | As `verified`, and the org also has rows from before activation. Those rows are not covered. | Record `legacy_unchained_rows`. |
| `broken` | A defect was found. `first_broken_seq` is the first failing row and `reason` names the check. | Follow section 4. |
| `partial` | Nothing wrong in the rows read, but the run did not cover the whole chain or has no anchor. Reasons include `row_limit_reached`, `unanchored_start`, `no_attestation`, `no_head_claim` and `no_chain`. | Run again from the cursor, or supply the missing manifest or pages. |
| `key_unavailable` | A signature names a key the verifier does not hold. The chain may be fine. | Supply the retired key (`AUDIT_CHAIN_VERIFY_KEYS` or `-verify-keys`). See section 5. |

The closed list of reasons is in `internal/verifier/verifier.go`. Reasons that can mean loss or alteration include `row_hash_mismatch`, `prev_hash_mismatch`, `sequence_gap`, `tail_missing`, `truncated_below_attestation`, `attestation_hash_mismatch` and `checkpoint_link_mismatch`.

## 4. A chain is reported broken

Signals: alert `ControlAuditChainVerifyFailed` or `ControlAuditChainBroken`, a `broken` status from section 3, or an incident of kind `chain_marked_broken`.

Two different states are involved. Keep them apart.

- `audit_chain_verifications.last_status = 'broken'` is a verifier finding. It changes nothing else. The verifier keeps its cursor on the broken range, so the next pass finds the same defect until the chain changes.
- `audit_chain_heads.state = 'broken'` is set once, by the insert path, when the stored row at the head sequence no longer has the head's hash. From then on the org receives no new signed heads, and the purge refuses the org and writes a `chain_marked_broken` incident. New rows are still appended. No code path sets the state back to `ok`.

Steps:

1. **Stop changing things.** Do not run the purge in any mode for this org. Do not run a revert migration. Do not edit `audit_events`, the `audit_chain_*` tables or the verifier state.
2. **Capture the state.** Keep the verifier report (section 3.2 or 3.3 with `-json`), the audit-core error logs for the org from the time of the first failing check, and any SIEM copy of the rows around `first_broken_seq`. Run these read-only queries and keep the output:

```sql
SELECT org_id, last_status, last_reason, first_broken_seq, last_run_at
  FROM audit_chain_verifications WHERE last_status <> 'verified';
SELECT org_id, head_seq, state, updated_at FROM audit_chain_heads WHERE state <> 'ok';
SELECT org_id, kind, first_bad_seq, detail, created_at
  FROM audit_chain_incidents ORDER BY created_at DESC LIMIT 50;
```

Back up the affected org's rows, for example with a `COPY` of `audit_events` for that org, before anything else changes.

3. **Classify by reason.**
   - `row_hash_mismatch`, `prev_hash_mismatch`, `resume_anchor_mismatch`: a stored row differs from the value it was hashed to. Compare that `chain_seq` with the SIEM copy, and find out which role changed it from the database logs, if they are kept.
   - `sequence_gap`, `prefix_missing`, `tail_missing`, `truncated_below_attestation`, `head_behind_chain`: rows are absent. Compare with the newest checkpoint, the newest attestation and the SIEM copy. Check whether a purge ran: a purge leaves a checkpoint.
   - `attestation_signature_invalid`, `checkpoint_signature_invalid`, `signing_key_unavailable`, or the incident kind `checkpoint_key_unknown`: usually a key-configuration problem, such as the wrong key, or a retired key missing from `AUDIT_CHAIN_VERIFY_KEYS`. Check section 5 before treating it as tampering.
   - `unsupported_chain_version`, `unhashable_row`, `org_mismatch`, `duplicate_sequence`: a format or code problem. Escalate with the first failing row.
4. **Escalate** to the Control Plane owner with the captured evidence and the classification. Repair is the owner's decision. The repository has no repair procedure (section 10).
5. **Record every manual step**: who did it, when, why, and the evidence it relied on. Record it in the dated evidence file. Never record a private key.

## 5. Key rotation

Facts from the code:

- One active key signs. Retired keys are kept only to verify. Each checkpoint and attestation names the key that signed it.
- A key id is bound to one public key. Boot refuses a verify key whose id is already bound to a different public key, so a rotation needs a new key id.
- A retired public key must stay in `AUDIT_CHAIN_VERIFY_KEYS` for as long as any retained checkpoint or attestation names it.
- `scripts/run-control-plane.sh` mints the pair only when a value is missing, and it never overwrites a value. A rotation is therefore a deliberate change in the secret store, not a script run. There is no rotation command.

Procedure:

1. Generate a new Ed25519 pair the way `scripts/run-control-plane.sh` does: a 32-byte seed and its 32-byte public key, base64url without padding. Choose a key id that has never been used.
2. Record the current key id and public key, from `GET /api/v1/audit/compliance/keys`, in the rotation evidence file.
3. In the audit-core environment, in the secret store behind the deployment, set `AUDIT_CHAIN_KEY_ID`, `AUDIT_CHAIN_PRIVATE_KEY_BASE64` and `AUDIT_CHAIN_PUBLIC_KEY_BASE64` to the new pair. Add `<old key_id>:<old public key>` to `AUDIT_CHAIN_VERIFY_KEYS` and keep the entries already there.
4. Restart audit-core. The key ring loads at boot. Heads are signed with the new key from the next attestation pass.
5. Check that `GET /api/v1/audit/compliance/keys` shows the new key as active, and that a verify run returns `verified` with the retired key still in `AUDIT_CHAIN_VERIFY_KEYS`. The keys endpoint lists a retired key only while the newest checkpoint or attestation is signed by it.
6. Keep the retired key until nothing retained names it. Remove it from `AUDIT_CHAIN_VERIFY_KEYS` in a later change.

If the private key is lost, new heads cannot be signed (`signing_key_unavailable`), and a production boot with the chain on is refused. Keep the private key in the secret store with a restorable backup. Never store it in the database, in a repository or in a ticket.

## 6. Retention purge: dry run and enabling

The purge runs at start and then every 24 hours in each audit-core replica. For unchained rows it is the existing rule: delete rows whose `ingested_at` is older than `AUDIT_RETENTION_DAYS`. For chained rows it stays off until `AUDIT_CHAIN_PRUNE_ENABLED=1`.

**Dry run.**

1. Set `AUDIT_CHAIN_PRUNE_DRYRUN=1`. Leave `AUDIT_CHAIN_PRUNE_ENABLED` at `0`.
2. Restart audit-core, or wait for the next daily sweep.
3. Read the logs. For each org, in batches of up to 5,000 rows:
   - `audit chain purge dry run: prefix verified, nothing deleted` (with `org_id`, `from_seq`, `through_seq` and `rows`) means that prefix would be deleted.
   - `audit chain purge blocked: the prefix does not verify, nothing was deleted` means the org is not purged, and an incident row with the kind and `first_bad_seq` is written. This happens in a dry run too. Section 4 applies.
4. A dry run deletes no chained row. It does delete unchained rows older than `AUDIT_RETENTION_DAYS`, and `usage_events` older than that, exactly as it did before the chain existed.

**Enabling pruning.**

1. Enable it only after clean dry-run plans for every org that will be pruned, and with a signing key in place. Pruning refuses to start without a private key.
2. Set `AUDIT_CHAIN_PRUNE_ENABLED=1` and `AUDIT_CHAIN_PRUNE_DRYRUN=0`, then restart.
3. Each batch runs in one transaction. It takes the org's advisory lock, verifies the prefix again, deletes it, and inserts a signed checkpoint with `reason = retention`. A replica that cannot take the lock skips that org until the next sweep.
4. The cut is made by `chain_seq`, not by time. Rows are removed up to the row just before the first chained row that is still inside the window. `ingested_at` is not in commit order, so a time cut could skip a row.
5. Capture the checkpoint rows and the verifier report before and after the first sweep (section 9).

No code path writes a checkpoint with `reason = 'erasure'`, although the table allows it. Erasure through the chain is not built.

## 7. Rollback

Roll back in this order. Every step except the last can be undone.

1. **Stop new chaining.** Set `AUDIT_CHAIN_ENABLED=0` and restart audit-core. New rows are unchained. Rows written while the chain is off, after the org was activated, are chained by the boot catch-up when the switch is turned on again (`chain_mode = 'catchup'`). Nothing is lost.
2. **Stop pruning.** Set `AUDIT_CHAIN_PRUNE_ENABLED=0` and restart. Rows already pruned stay pruned, and their checkpoints stay. The rest of the chain verifies from the newest checkpoint.
3. **Stop export.** Set `AUDIT_SIEM_EXPORT_ENABLED=0` or stop the `audit-siem-exporter` service. Cursors stay in `audit_export_cursors`, and a re-enabled destination resumes from its cursor. An owner or admin can also disable a destination with PATCH.
4. **Hide the destination routes.** Set `ORG_SIEM_DESTINATIONS_ENABLED=0` on org-core. Stored destinations stay in the table.
5. **Compliance API.** It has no switch. audit-core accepts it only from the gateway principal, with the compliance scope and an owner or admin role. Roll back the gateway image, and roll the audit-core image and its credential registry together through `scripts/run-control-plane.sh`.
6. **Do not roll audit-core back below the WS06-S2 build while migration 007 is applied.** Migration 007 adds the guard trigger, and the S2 build's purge sets `audit.maintenance` before it deletes. A build from before S2 deletes without that setting, the guard trigger refuses the delete, and the retention sweep fails, for `usage_events` as well. If an older image cannot be avoided, get the Control Plane owner's sign-off first and watch the sweep logs.
7. **The revert migrations are not a rollback.** The files in `apps/Control Plane/audit-core/internal/store/migrations/revert/` are run by hand, and they destroy chain state:
   - `007` drops the chain columns and `audit_chain_heads`. Every chained row becomes an ordinary row, and the chain is lost.
   - `008` drops `audit_chain_checkpoints`, `audit_chain_attestations` and `audit_chain_incidents`. A pruned chain can no longer be verified from its trust base.
   - `009` drops `audit_chain_verifications`. Only verifier state is lost.
   - `010` drops `audit_export_cursors`. Only exporter state is lost.

   Run one only with the owner's written decision, and only after the switches named in its header are off.

## 8. SIEM export

**Setting up a destination.** An owner or admin creates one through the gateway with `POST /api/v1/audit/siem-destinations`. The body has `kind` (`ndjson_webhook` or `splunk_hec`), `url`, `secret`, and optionally `enabled` and `event_filter` (lists of `events` and `planes`). An org can have at most three destinations. `GET` on the same path lists them. `PATCH /api/v1/audit/siem-destinations/{destination_id}` takes `expected_revision` and the fields to change; the kind cannot change. `DELETE` removes a destination. The routes are gated by the operator switch `ORG_SIEM_DESTINATIONS_ENABLED` and the owner or admin role. No plan check exists yet (section 10).

- URLs must be https, with no user name, no fragment and no private, loopback or link-local address literal.
- Secrets are 16 to 512 bytes. They are write-only: responses show `secret_set` and never the value. org-core seals them with AES-256-GCM under `ORG_CORE_SIEM_SEAL_KEY`.

**What the receiver gets.** Three record types: `audit_event`, `audit_chain_head` and `audit_export_gap`.

- `audit_event` carries the stored fields of each row: `org_id`, `chain_seq`, `occurred_at`, `ingested_at`, `user_id`, `actor_role`, `plane`, `event`, `subject`, `resource_id`, `outcome`, `event_id`, `request_id`, `ip_address` (the host part only), `user_agent`, `source_producer`, `source_bus`, `payload_hash`, `details`, `chain_version`, `prev_hash` and `row_hash`. User identifiers, the IP host and the user agent are personal data about the customer's users. The destination is the customer's own system. Counsel decides how the egress register classifies it.
- `audit_chain_head` appears in each batch. It gives the org's head sequence with its newest signed attestation.
- `audit_export_gap` names a `from_seq` to `to_seq` range that is absent from the export, and the reason: missing from the chain table, excluded by the destination's filter, or too large for one body.
- Deduplicate `audit_event` on `(org_id, chain_seq)`. Delivery is at least once.

**How it is delivered.**

- The exporter polls org-core every 30 seconds for destinations. For each enabled destination it reads rows in chain order, up to 500 per batch and 1 MiB per body, and advances the cursor only after a 2xx response.
- A failing destination backs off: 5 seconds, doubling, up to 15 minutes.
- `ndjson_webhook` sends `X-Verevon-Timestamp` (Unix seconds) and `X-Verevon-Signature: sha256=` followed by the hex HMAC-SHA256 of `timestamp + "." + body`, keyed with the destination secret. Verify the signature before trusting a body, and refuse timestamps you consider stale.
- `splunk_hec` sends `Authorization: Splunk <secret>` and one event per line.
- The egress guard accepts https only. At dial time it refuses private, loopback, link-local (including 169.254.169.254), carrier-grade NAT, IPv6 unique-local and IPv4-mapped addresses. It follows no redirects and allows 10 seconds for each request.
- The exporter has no ingest credential and no signing key. It keeps destination secrets in process memory while it polls.

**Watching it.**

- `GET /api/v1/audit/compliance/export-status` (owner or admin) shows, per destination, the cursor, the delivered count, the consecutive failures, the last error and the chain head. It never shows a URL or a secret.
- Alerts: `ControlAuditSIEMExportLag` (the oldest undelivered row is over 15 minutes old, for 5 minutes) and `ControlAuditSIEMExportFailing` (most batches in the last 15 minutes failed). Without the `siem` compose profile, neither series exists.
- A destination is a witness only for the rows it has received. A disabled, failing or unconfigured destination proves nothing about rows it did not receive. Say so in any customer statement.

## 9. Evidence to keep

Keep dated evidence in `docs/compliance/evidence/`, not in the database. Never keep a private key. Key ids and public keys are fine.

- **Before a switch changes:** the switch values (not secrets), the image digest, the verifier report, and the `audit_chain_heads` rows.
- **Chain on:** a run of new rows with contiguous `chain_seq`, each `prev_hash` equal to the previous `row_hash`, the head equal to the last row, and no gap across a restart.
- **Pruning:** the dry-run log lines, then the checkpoint rows and the verifier report before and after the first sweep.
- **Key rotation:** the old and new key ids and public keys, the verifier report after the restart, and the `AUDIT_CHAIN_VERIFY_KEYS` value (public half only).
- **Export:** a receiver capture for one batch, the signature check, the gap marker from a deliberate stop and restart, and the export status before and after.

## 10. Known gaps

These are open. This runbook does not hide them.

- No repair procedure exists for a head marked `broken`, and no code path sets it back to `ok`.
- No key-rotation command exists. Section 5 is a manual procedure.
- The enterprise entitlement that DEC-35 proposes for SIEM export (`feature.siem_export`) is not built. Until it is, the operator switch is the only gate on destinations, and the commercial ruling on that gate is open.
- `audit-verify` reads a bare manifest object, but the compliance API returns the manifest inside `{"data": ...}`. Section 3.3 is the workaround.
- The manifest carries a signature (`key_id` and `signature`) when a key is held. No code verifies it. `audit-verify` relies on the signed checkpoint and attestation and on the head claim.
- The comment on `AUDIT_CHAIN_PRUNE_DRYRUN` in `apps/Control Plane/audit-core/.env.example` reads as if a dry run deletes nothing at all. It deletes no chained row, but unchained rows and `usage_events` older than the retention window still age out (section 6).
- If only one half of the audit-chain key pair is in the secret ledger, the script mints a new pair and keeps the existing half. When the chain or pruning is on, audit-core then refuses to boot on the mismatch. Repair the ledger deliberately.
