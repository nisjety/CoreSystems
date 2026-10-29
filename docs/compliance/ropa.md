# Records of Processing Activities (ROPA) — Article 30

> **DRAFT — INTERNAL TEMPLATE, NOT LEGAL ADVICE.** Not reviewed by Norwegian
> privacy counsel. Lawful bases shown are **illustrative** placeholders for the
> controller/counsel to confirm. See [`README.md`](./README.md).

---

## Scope and roles

Verevon maintains this register as a **processor** under **GDPR Art. 30(2)** (records
of categories of processing carried out on behalf of controllers). Each customer
(controller) maintains its own Art. 30(1) record; Verevon's record supports it.

- **Processor:** Verevon `[legal entity, org. no., address]`
- **Processor representative / privacy contact:** `[name, email]`
- **Subprocessors:** see [Subprocessor List](./subprocessor-list.md)
- **General security measures:** see §"Security measures" below and the [DPA](./dpa-template.md)

---

## Processing activities register

### PA-1 — AI model inference (agent reasoning)
- **Purpose** (`purpose_id`): model execution / delivering the agent service.
- **Data subjects:** customer's employees; individuals named in processed content.
- **Data categories:** prompts, completions; `customer_private`, `personal`
  (`sensitive_personal` denied by default unless DPIA-authorised).
- **Recipients / subprocessors:** Microsoft Azure OpenAI (Sweden Central).
- **Transfers:** EEA-resident; ZDR; CLOUD Act residual disclosed ([TIA](./transfer-assessment.md)).
- **Retention:** not retained by model provider (ZDR); Verevon retains only as part
  of conversation/run-history ([Retention Schedule](./retention-schedule.md)).
- **Lawful basis (illustrative):** Art. 6(1)(b)/(f).

### PA-2 — Embeddings & retrieval (vector index over customer corpus)
- **Purpose:** retrieval / search over the customer's own content.
- **Data subjects / categories:** as in source documents; `customer_private`, `personal`.
- **Subprocessors:** Microsoft Azure OpenAI (embeddings) + Verevon-hosted vector store (EEA).
- **Transfers:** EEA-resident.
- **Retention:** embeddings tied to source document; deleted when source is deleted.
- **Lawful basis:** Art. 6(1)(b)/(f).

### PA-3 — Document & file storage
- **Purpose:** import / retrieval / agent operation over uploaded files.
- **Data categories:** file contents; `customer_private`, `personal`.
- **Subprocessors:** Azure object storage (EEA).
- **Retention:** until customer deletion or contract end ([Retention Schedule](./retention-schedule.md)).
- **Lawful basis:** Art. 6(1)(b)/(f).

### PA-4 — Conversations & messages
- **Purpose:** support / interaction history.
- **Data categories:** message content; `customer_private`, `personal`.
- **Subprocessors:** Verevon-hosted (EEA).
- **Retention:** per [Retention Schedule](./retention-schedule.md).
- **Lawful basis:** Art. 6(1)(b)/(f).

### PA-5 — Autonomous agent run-history (with Approve/Reject gate)
- **Purpose:** observability, human oversight, and accountability (Art. 5(2),
  Art. 22 oversight) for autonomous agent runs.
- **Data categories:** run steps, tool calls, human approval/rejection decisions,
  references to processed content; `customer_private`, `personal`.
- **Subprocessors:** Verevon-hosted (EEA).
- **Retention:** per [Retention Schedule](./retention-schedule.md) (run-history).
- **Lawful basis:** Art. 6(1)(b)/(f); supports controller's Art. 22 safeguards.

### PA-6 — Web retrieval / crawl (Quarry egress broker)
- **Purpose:** retrieving external web content directed by the agent.
- **Data categories:** crawled/retrieved content, target URLs/metadata.
- **Subprocessors:** Verevon-owned egress first; external browser/proxy/unblocker
  vendor(s) only when policy permits (default `allow_third_party_processing=false`).
- **Transfers:** region-bound egress preferred; external vendor regions **TBC**.
- **Retention:** as run artefacts ([Retention Schedule](./retention-schedule.md)).
- **Lawful basis:** Art. 6(1)(f) (subject to robots/terms and abuse controls).

### PA-7 — Identity, authentication & accounts (auth-core)
- **Purpose:** user authentication, sessions, OAuth, 2FA, organisation membership.
- **Data categories:** name, email, phone number, OAuth account links, passkeys,
  API keys, sessions, OAuth consents; `personal`, `credential_or_secret`.
- **Subprocessors:** Verevon-hosted (EEA); Twilio (SMS/2FA — region **TBC**).
- **Retention:** until account deletion; hard-delete via `gdpr_hard_delete_user`,
  anonymisation via `gdpr_anonymize_user` ([Retention Schedule](./retention-schedule.md)).
- **Lawful basis:** Art. 6(1)(b) (account) / 6(1)(c) (security obligations).

### PA-8 — Billing
- **Purpose:** invoicing and payment for the service.
- **Data categories:** billing identifiers, usage; `personal` (limited).
- **Subprocessors:** billing processor **TBC — not confirmed in codebase** (e.g. Stripe/Lago if used).
- **Retention:** statutory accounting retention (typically **5 years** under
  Norwegian *bokføringsloven* — **confirm with counsel**).
- **Lawful basis:** Art. 6(1)(b)/(c).

### PA-9 — Transactional notifications (email/SMS)
- **Purpose:** service notifications and verification messages.
- **Data categories:** email address, phone number, minimised message content; `personal`.
- **Subprocessors:** Resend (email — region **TBC**), Twilio (SMS — region **TBC**).
- **Retention:** minimal; per [Retention Schedule](./retention-schedule.md).
- **Lawful basis:** Art. 6(1)(b)/(f).

### PA-10 — Audit & security event logging (audit-core)
- **Purpose:** security, integrity, accountability, and deletion verification.
- **Data categories:** identifiers, policy references, source-trace IDs, hashes,
  status, processor decisions — **not** full content/secrets.
- **Subprocessors:** Verevon-hosted (EEA).
- **Retention:** **365 days** (subject to legal-hold/security exceptions).
- **Lawful basis:** Art. 6(1)(c)/(f).

### PA-11 — Connector / integration state & credentials
- **Purpose:** syncing customer-authorised external sources.
- **Data categories:** OAuth tokens, connector sessions, cookies, proxy
  credentials — all `credential_or_secret` (vault-only).
- **Subprocessors:** the connected source's provider (customer-directed).
- **Retention:** until revoked/deleted; deleted in scope on erasure.
- **Lawful basis:** Art. 6(1)(b).

### PA-12 — Telemetry / operational metrics
- **Purpose:** service health, performance, abuse prevention.
- **Data categories:** operational metrics; minimised, ideally non-personal /
  pseudonymous.
- **Subprocessors:** Verevon-hosted (EEA).
- **Retention:** short ([Retention Schedule](./retention-schedule.md)).
- **Lawful basis:** Art. 6(1)(f).

---

## International transfers (summary)
Default processing in the **EU/EEA (Azure OpenAI Sweden Central)**; Norway East for
Tier 2. Any third-country transfer relies on **SCCs (+ DPF where applicable) plus
supplementary measures** per the [Transfer Impact Assessment](./transfer-assessment.md).
**TTS in East US 2 (US)** is a disclosed exception, excluded for protected classes
pending remediation. The **US CLOUD Act residual is disclosed**, not eliminated.

## Security measures (summary)
EU/EEA residency; encryption in transit/at rest; multi-tenant per-organisation
isolation; privacy classification; vault-only secrets; default-deny third-party
processing; Zero Data Retention at the model layer; minimised audit logging with
retry/dead-letter for security/deletion events; command-driven verifiable deletion.
Full description in the [DPA](./dpa-template.md), Annex 2.

---

*This ROPA is a draft pending Norwegian privacy counsel review; all lawful bases and
TBC items require confirmation.*
