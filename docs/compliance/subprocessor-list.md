# Verevon Subprocessor List

> **DRAFT — INTERNAL TEMPLATE, NOT LEGAL ADVICE.** Not reviewed by Norwegian
> privacy counsel. Regions and transfer bases marked **TBC — verify** are
> unconfirmed and must be validated before any external publication. A published
> subprocessor list is a contractual artefact (GDPR Art. 28(2)/(4)); customers must
> be given notice of changes. See [`README.md`](./README.md).

---

## How this list is governed

Verevon's architecture **denies third-party processing by default** for
`customer_private`, `personal`, `sensitive_personal`, `credential_or_secret`, and
`zdr_ephemeral` data. A subprocessor may only process such data when (a) the
customer's org policy allows it for a specific purpose, (b) the subprocessor holds
an approved **processor registry** record (DPA, residency, sub-subprocessors,
retention, incident contact), and (c) the request is explicitly authorised
(`allow_third_party_processing=true` + a valid `processor_id`).

**Transfer-basis legend:** *EEA-resident* = processing/storage within the EU/EEA;
*SCCs* = EU Standard Contractual Clauses (2021/914); *DPF* = EU-US Data Privacy
Framework certification (under legal challenge — not relied on alone);
*+ supplementary* = supplementary measures per the [TIA](./transfer-assessment.md).

## Subprocessors

| Subprocessor | Purpose | Data categories | Region (processing/storage) | Transfer basis |
|--------------|---------|-----------------|------------------------------|----------------|
| **Microsoft Azure — Azure OpenAI** | AI model inference (prompts/completions) and embeddings generation | `customer_private`, `personal`, prompts, completions, embeddings | **Sweden Central (EU/EEA)** — Tier 1 default; Norway East for Tier 2 | EEA-resident + ZDR + SCCs/DPF + supplementary; **CLOUD Act residual disclosed** |
| **Microsoft Azure — Hosting / infrastructure** | Compute, storage, networking for Verevon services (app data, run-history, audit, vector store, object storage) | All classes processed by the platform (per classification policy) | **EU/EEA** | EEA-resident + SCCs/DPF + supplementary; **CLOUD Act residual disclosed** |
| **Microsoft Azure — Text-to-Speech (TTS)** | Speech synthesis | Text submitted for synthesis | **East US 2 (United States)** — *known exception, remediation in progress* | US processing — **excluded for protected classes until relocated to EEA** |
| **Resend** | Transactional email delivery (notifications) | Recipient email address, message content (minimised) | **TBC — verify** | **TBC — verify (likely SCCs/DPF + supplementary)** |
| **Twilio** | SMS delivery and 2FA / one-time passcodes | Phone number, OTP, delivery metadata | **TBC — verify** | **TBC — verify (likely SCCs/DPF + supplementary)** |
| **Browser-automation / residential-proxy / unblocker vendor(s) (behind Quarry egress broker)** | Web fetch/crawl/browse for agent retrieval, only when Verevon-owned egress is insufficient and policy permits | Crawled/retrieved content, target URLs/metadata | **TBC — verify per vendor** | Default-deny; per-processor approval; **TBC — verify** |
| **Additional model vendors (via Model Plane routing)** | Alternative LLM inference where routed | prompts, completions | **TBC — confirm ZDR + EEA per vendor** | **TBC — verify; only the Azure OpenAI Sweden Central path is confirmed today** |

## Notes

- **Microsoft Azure is the principal subprocessor** and the only one whose region
  (Sweden Central / Norway East / East-US-2-for-TTS) is confirmed from
  configuration. All other regions are **TBC — verify** against the vendor DPA.
- Billing processors (e.g. Stripe / Lago, if used) are **allowed for the billing
  purpose only** and must carry their own processor record and retention policy —
  add them here once confirmed in production. *(Not confirmed in the codebase; do
  not list externally until verified.)*
- Sub-subprocessors of each subprocessor are governed by that subprocessor's DPA
  and should be enumerated in the processor registry record.

## Change management

- New or replacement subprocessors require: an approved processor registry record,
  a TIA update, and **advance notice to customers** with a right to object, per the
  DPA's subprocessor clause.
- This table is the single source of truth; the [DPA](./dpa-template.md) references
  it rather than duplicating it.

---

*This subprocessor list is a draft pending Norwegian privacy counsel review and
engineering confirmation of all TBC items.*
