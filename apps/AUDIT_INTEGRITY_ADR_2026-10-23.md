# ADR-0013: Audit integrity: per-org hash chain, signed attestations, compliance read, SIEM witness

**Date**: 2026-10-23  
**Status**: proposed number (DEC-39: the integrator assigns ADR numbers across the three sessions; 0013 is the plan's placeholder). The encoding in section 1 is accepted for implementation as the plan default (WS06 S1, DEC-35 leaves S1 and S2 free). The trust anchor, read role and SIEM decisions in sections 4 to 8 take effect only when the owner ratifies DEC-35; WS06 S3 to S8 do not merge before that.  
**Deciders**: owner (DEC-35); Control Plane (audit-core, org-core); Frontend Plane (gateway routes, admin view with the Space session)  
**Scope**: the audit-core `audit_events` table: how a row is made tamper-evident, how retention stays chain-safe, who may read the log as a compliance record, and how a copy leaves the platform to a customer's SIEM. Model Plane has no code change: its audit events enter through the existing outbox and NATS path and are chained like any other row.  
**Supersedes / extends**: extends the audit-core storage contract (one insert chokepoint, `insertAudit`, identity by `(source_bus, producer, event_id)`). Resolves F-t3-004, tamper-evidence half; phase 0 (publisher fix and replay) stays WS01.

## Context

audit-core stores one append-only row per audit event. Nothing today says a stored row is the row that was written. `Purge` hard-deletes by `ingested_at` and leaves no trace of what went. There is no way for an organisation to read its own log as a compliance record (`GET /v1/audit` reaches no role check), and no copy of the log exists outside the database that holds it. A person with write access to Postgres can edit, delete or reorder rows and nobody, including the platform operator, can tell.

A hash chain alone does not fix that. Whoever can rewrite a row can recompute every hash after it. A chain is evidence only when something outside the database's control pins its head: a signature made with a key the database does not hold, and a copy of the head held somewhere else.

## Decision

### 1. Canonical encoding, format v1 (this slice)

The row hash is computed over the stored row, read back, never over the event as received:

```
row_hash = SHA256( "VAC1\x00" || prev_hash
                || F(org_id) || U64(chain_seq)
                || I64(occurred_at_micros) || I64(ingested_at_micros)
                || F(user_id) || F(actor_role) || F(plane) || F(event) || F(subject)
                || F(resource_id) || F(outcome) || F(event_id) || F(request_id)
                || F(host(ip_address)) || F(user_agent)
                || F(source_producer) || F(source_bus)
                || F(payload_hash) || F(details_canonical) )
```

- `F(x) = uvarint(len(x)) || x`. A NULL column is the empty string. The length prefix is what keeps `("ab","c")` and `("a","bc")` from hashing alike.
- `U64` and `I64` are 8 bytes, big endian. Times are Unix microseconds, which is Postgres `timestamptz` precision, so a value hashes the same in any time zone.
- `host(ip_address)` is the address without a netmask, as Postgres prints it.
- `details_canonical` is the `details` JSONB text as read back, rewritten with object keys sorted by UTF-8 bytes, no insignificant whitespace, every number kept as its original token (never parsed, so `1.0`, `1e3` and `1000` stay distinct), and one fixed string escape: `"` and `\` backslash-escaped, `\b \f \n \r \t` short escapes, other bytes below 0x20 as lowercase `\u00xx`, everything else raw UTF-8 (no HTML escaping, U+2028 and U+2029 raw). The escape is written out in code rather than taken from `encoding/json`, so it cannot move with a Go release. Canonicalising a canonical document returns it unchanged. Postgres normalises numbers it stores (for example `1e21`); reading the hashed text back from the row, not from the incoming event, is what makes that irrelevant.
- The first row of an org has `prev_hash = SHA256("VAC1/genesis/" + org_id)`, so a row moved to another org's chain cannot verify. `chain_seq` starts at 1 and is gap-free per org.
- Not hashed: `id`, `source_subject`, `source_stream_sequence`. They are storage coordinates, not the audited fact.

Signed payloads are exact byte strings, each with its own magic so a signature over one kind can never be replayed as the other:

- Head attestation: `"VAH1\n" + org + "\n" + seq + "\n" + hex(row_hash) + "\n" + signed_at`.
- Retention checkpoint: `"VAK1\n" + org + "\n" + through_seq + "\n" + hex(through_hash) + "\n" + pruned_total + "\n" + created_at`.
- Times in a signed payload are UTC with fixed microsecond width, `2006-01-02T15:04:05.000000Z`. An org id containing a newline is refused, because a newline-delimited payload would be ambiguous.

A change to any byte of this layout is a new format version with a new magic, recorded in `chain_version`; v1 is never edited. The reference implementation is `apps/Control Plane/audit-core/internal/chain/chain.go`. The golden vectors in `internal/chain/testdata/vectors.json` were produced by an independent Python implementation written from this description, not from the Go code, and are the single source for the inserter, the verifier, the compliance export and the offline `audit-verify` CLI.

`vectors.json` sha256 (LF line endings): `a4d3e9a8a3beec560015d0d7b6eb5dd1c9312585d4210eb7c8daff14fe7e73cf`. `TestVectorsFileIsPinned` fails if the file and this value drift apart.

### 2. One serialised insert per org (WS06 S2)

Each chained insert takes the org's head row (`audit_chain_heads`, `SELECT ... FOR UPDATE`), writes the row, computes its hash from the stored values and advances the head in one transaction. Replays, which today hit `ON CONFLICT DO NOTHING`, do not advance the head. `AUDIT_CHAIN_ENABLED` defaults off.

### 3. No backfill

Rows written before activation stay unchained and are reported as `legacy_unchained`. The verifier says `verified_with_legacy`, never a bare "ok". Chaining history we did not witness would only launder it.

### 4. Trust anchor: a signing key outside the database (WS06 S3, DEC-35 D1)

The head is attested with Ed25519, on an interval, with a key held in the service's environment and generated by `run-control-plane.sh`. The public key is served so a verifier needs nothing from the database to check a signature. A key ring supports rotation. KMS is deferred; no signing at all is rejected. In production the service refuses to boot with the chain on and no key.

### 5. Chain-safe retention (WS06 S3)

`Purge` is the only delete path. Per org it verifies the prefix it is about to delete, deletes whole contiguous prefixes by `chain_seq` (never by timestamp, because `ingested_at` is the transaction start and is not commit-ordered), and writes a signed checkpoint recording how much was removed. On a failed verification it deletes nothing and records an incident. Pruning has its own switch and a dry-run mode.

### 6. Verifier and offline CLI (WS06 S4)

A verifier recomputes every hash, checks contiguity, the head, the latest checkpoint and the attestation signatures, and reports `verified`, `verified_with_legacy`, `broken`, `partial` or `key_unavailable` with the first bad sequence. An offline CLI uses only `internal/chain` and exported pages.

### 7. Compliance read, owner and admin only (WS06 S5, S6, DEC-35 D3)

The gateway and audit-core both check the role. A member cannot read the log as a compliance record. An agent never reads or administers audit.

### 8. External witness: SIEM export (WS06 S7, S8, DEC-35 D4, D5)

A separate `siem-exporter` binary in the audit-core module streams rows in `chain_seq` order, with `prev_hash`, `row_hash` and the newest attestation, to a customer-configured destination. Destination secrets are sealed in org-core and write-only. The export is the independent copy that makes a rewritten chain detectable.

### 9. ZDR

Nothing here stores or copies content. The hash covers columns audit-core already holds, which are content-free by the audit contract; ZDR-derived envelopes are suppressed upstream and stay so. Exporter logs never carry a URL query, a header or a body. Secrets live in memory only.

## Alternatives considered

### Hash chain with no signature

Rejected. Anyone who can write the database recomputes the chain. It would read as protection while giving none.

### Backfill existing rows into the chain

Rejected (DEC-35). The platform did not witness those rows being written; a chain over them proves nothing about their past and would be read as if it did.

### Hash the event as received, not the stored row

Rejected. Postgres normalises JSONB, inet and timestamptz, so the same event would hash differently depending on where it was computed. Hashing the read-back row makes every recomputation agree.

### Delegate string escaping and number handling to `encoding/json`

Rejected for the escape (a Go release could change bytes already signed) and for numbers (float64 round-trips change `1.0` and large integers). Numbers are kept as tokens and the escape is explicit.

### Per-org lock via advisory lock instead of a head row

Not chosen for v1. The head row carries the state the verifier needs (head hash, activation time, broken marker) and gives the lock for free.

## Consequences

### Positive

- One encoding, one set of golden vectors, derived twice independently, shared by every writer and verifier.
- Tampering with a stored field, a `details` value, a row's position or its org is detectable by anyone holding the vectors-compatible verifier and the public key.
- Retention can no longer remove evidence silently.

### Negative

- A hot org serialises on its head row. Volume is low today (4,992 rows in four weeks), and a lock-wait metric is part of S2.
- `Purge` becomes a chain-aware, per-org operation and is the most dangerous code in the stream.
- Legacy rows stay outside the chain for good.

### Risks and release gates

- **False sense of security.** A database owner can rewrite the chain. They cannot forge a signature without the key or change a copy already held by a SIEM. The honest-limits text (WS06 S9) states that completeness is not proven, that legacy rows are unchained, and that the SIEM witness exists only where a destination is configured. No claim broadens before evidence.
- **Format drift.** A second implementation (verifier, CLI, a customer's script) that escapes or orders differently produces a different hash. The vectors are the contract; any implementation must pass them.
- **Number and string edge cases.** The escape rule and the keep-the-token rule are specified in section 1 and exercised by the vectors, a random-document idempotence test and a fuzz target.
- **ADR number.** 0013 is a placeholder until the integrator allocates numbers (DEC-39).
