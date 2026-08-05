# Data Retention Schedule & Erasure Procedure

> **DRAFT — INTERNAL TEMPLATE, NOT LEGAL ADVICE.** Not reviewed by Norwegian
> privacy counsel. TTL values other than the confirmed audit-events period are
> **proposed defaults** for counsel and engineering to finalise; they are not yet
> all enforced by automated retention sweeps. See [`README.md`](./README.md).

---

## Principles

- **Storage limitation (Art. 5(1)(e)):** keep Personal Data only as long as needed
  for the purpose, then delete or anonymise.
- **ZDR ≠ no retention.** Zero Data Retention applies at the **model layer** (Azure
  OpenAI does not retain or train on prompts/completions). It does **not** relieve
  Verevon of retaining and then deleting its **own** run-history, conversations, and
  audit records on schedule.
- **Retention exceptions** are allowed only for documented legal, security,
  billing, or audit obligations, retaining the minimum necessary and using
  pseudonymisation where possible.
- **Audit ≠ shadow store.** Audit events record *what happened* (identifiers,
  hashes, policy decisions), never the erased content itself.

## Retention table

| Data type | Class | Default retention (TTL) | Disposition | Enforcement status |
|-----------|-------|--------------------------|-------------|--------------------|
| **Model prompts/completions (at provider)** | `zdr_ephemeral` / per-request | **Not retained** by provider (ZDR) | N/A | Confirmed (ZDR config) |
| **Agent run-history** | `customer_private`, `personal` | **`[proposed: 365 days]`** — confirm with customer/counsel | Hard-delete; or pseudonymise if under exception | **Proposed** — sweep TBC |
| **Conversations / messages** | `customer_private`, `personal` | **`[proposed: 365 days]`** or contract term | Hard-delete or anonymise | **Proposed** — sweep TBC |
| **Uploaded files / documents** | `customer_private`, `personal` | Until customer deletion or contract end | Delete object + derived chunks/vectors | Command-driven; sweep TBC |
| **Embeddings / vectors** | derived | **Tied to source** | Deleted when source document deleted | Command-driven; sweep TBC |
| **Web-retrieval / crawl artefacts** | per classification | **`[proposed: 90 days]`** | Delete | **Proposed** — sweep TBC |
| **Audit & security events** | minimised metadata | **365 days** (legal-hold exceptions) | Delete after TTL unless on hold | **Confirmed period;** sweep TBC |
| **Telemetry / operational metrics** | non-personal/pseudonymous | **`[proposed: 30–90 days]`** | Delete/aggregate | **Proposed** |
| **Identity / accounts** | `personal` | Until account deletion | `gdpr_hard_delete_user` / `gdpr_anonymize_user` | **Implemented (auth-core)** |
| **Credentials / connector secrets** | `credential_or_secret` | Until revoked/deleted | Vault deletion | Implemented (vault) |
| **Billing records** | `personal` (limited) | Statutory — typically **5 years** (*bokføringsloven*) — **confirm** | Retain then delete | **TBC — verify** |
| **Backups** | mirrors above | **`[proposed: 30–35 days]` rolling**, then overwritten | Erasure reconciled on next cycle (documented exception window) | **Proposed** — verify cycle |

> **Backups note:** immediate erasure from rolling backups is generally not
> feasible; the accepted approach is to delete from live systems immediately and
> let the erasure flow through backups within the documented backup-rotation
> window. State this window to data subjects when responding to erasure requests.

## Erasure / right-to-be-forgotten procedure (Art. 17)

Deletion is **command-driven and verifiable** (per the GDPR summary's deletion flow):

1. **Authority & scope.** Control Plane verifies the request's authority and scope
   (subject, org, source, retention exception, deadline).
2. **Command emission.** Control Plane emits a deletion/anonymisation command with
   request ID and scope to all planes.
3. **Application Plane.** Deletes/pseudonymises derived conversations, messages,
   jobs, notifications, support projections.
4. **Data Plane.** Deletes documents, chunks, vectors, graph nodes, wiki pages,
   search records, source traces, objects, and caches.
5. **Ingestion Plane.** Deletes import jobs, crawl artefacts, provider sessions,
   connector state, and in-scope source credentials.
6. **Model Plane.** Deletes sessions, messages, runs, memory, provider caches,
   execution artefacts, sandbox snapshots, and browser traces.
7. **Audit.** Records completion, failures, retries, and any retained exceptions
   **without** preserving erased payloads.

### Identity-layer erasure primitives (implemented in `auth-core`)
- **`gdpr_hard_delete_user(user_id)`** — deletes the user and related rows:
  `session`, `account` (OAuth links), `two_factor`, `passkey`, `apikey`, `member`
  (all org memberships), `team_member`, `oauth_consent`, and the `user` record;
  returns a JSON manifest of deleted IDs for audit.
- **`gdpr_anonymize_user(user_id)`** — softer option: anonymises the user record
  (name → "Deleted User", email → `deleted_<id>@anonymized.local`, clears phone/
  image, bans), and removes sessions, OAuth accounts, 2FA, passkeys, and API keys.

### DSAR / erasure intake
- A customer-facing **DSAR intake endpoint is being built**; until live, requests
  are handled via the account contact and executed through the procedure above.
  **(Confirm production status before publishing externally.)**

### Verification & gaps to close (from the GDPR readiness review)
- Retention **sweeps** must run for documents, vectors, objects, conversations,
  sessions, jobs, connector state, and audit events — **status: to be implemented/
  tested.**
- Application-Plane (Convex) derived copies need **mandatory hard-delete or
  documented pseudonymisation** — anonymisation must not remain optional for GDPR
  flows.
- Audit-core needs **retry/dead-letter** behaviour for security/deletion events
  and explicit erasure-exception semantics.
- ZDR/erasure tests must cover provider rejection, cache invalidation, logs, audit
  events, Data-Plane indexes, Model-Plane sessions, and Quarry captures.

---

*This retention schedule and erasure procedure are drafts pending Norwegian privacy
counsel review; proposed TTLs and TBC items require confirmation, and several
automated sweeps are not yet implemented.*
