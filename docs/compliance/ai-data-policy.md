# Velion AI Data Policy

> **DRAFT — INTERNAL TEMPLATE, NOT LEGAL ADVICE.** This document has not been
> reviewed by Norwegian privacy counsel and must not be used in any customer-facing
> context until that review is complete. Capabilities described as planned or
> "being built" are not yet in production. See [`README.md`](./README.md).

*Customer-facing tone. Plain language. No claim made here may overstate what is
implemented — where a control is planned, this is stated.*

---

## In one sentence

Velion is an AI agent platform. We process your prompts, files, and the content
your agents work with **inside the EU/EEA on Microsoft Azure (Sweden Central)**,
under a **Zero Data Retention** arrangement with the model provider, and we **never
allow your data to be used to train AI models**. We keep a record of what your
agents do (for your audit and oversight), and we delete data on the schedule and
on request as described below.

---

## 1. Where your data is processed

| Stage | Where | Notes |
|-------|-------|-------|
| AI model inference (prompts → completions) | **Azure OpenAI, Sweden Central (EU/EEA)** | Default for all customers — *Residency Tier 1: EEA Standard*. |
| Embeddings generation | **Azure OpenAI, Sweden Central (EU/EEA)** | `text-embedding-3-large` via Azure. |
| Norway-residency customers | **Norway East** where models allow | *Residency Tier 2.* Sweden Central is used as a fallback **only with your documented approval**. |
| Text-to-speech (TTS) | **East US 2 (United States)** — *known exception* | Disclosed gap under active remediation. TTS must not be used for confidential or special-category content until relocated to the EEA. |
| Application data, conversations, run-history, audit log | EU/EEA Velion-controlled stores (PostgreSQL, object storage, vector store) | Multi-tenant with strict per-organization isolation. |

**Honest limitation.** EU residency reduces, but does not eliminate, exposure to
non-EU government access. Because Microsoft is a US-headquartered company, data it
hosts can in principle be reached under the US **CLOUD Act** even when stored in
Sweden. We disclose this as a residual risk and apply supplementary protections
(encryption, Zero Data Retention, EU residency, strict access controls) — see our
[Transfer Impact Assessment](./transfer-assessment.md). We do not claim immunity
from foreign-government access.

## 2. What happens to each kind of data

| Data | What we do with it | Used to train AI models? | Retention (default) |
|------|--------------------|--------------------------|---------------------|
| **Prompts** (what you/your agents send the model) | Sent to Azure OpenAI in Sweden Central to generate a response. | **No.** | Not retained by the model provider (ZDR). Retained by Velion only as part of conversation / run-history per §3. |
| **Completions** (the model's responses) | Returned to you and stored as part of the conversation / agent run. | **No.** | As conversation / run-history (§3). |
| **Files you upload** | Stored in EU/EEA object storage; chunked and embedded for retrieval. | **No.** | Until you delete them or your contract ends; then erased per the [Retention Schedule](./retention-schedule.md). |
| **Embeddings** (numeric vectors derived from your content) | Stored in our EU/EEA vector store to power retrieval/search. | **No.** | Tied to the source document — deleted when the source is deleted. |
| **Retrieved web content** (when an agent browses/crawls) | Fetched through Velion's own egress broker (Quarry); content is captured for the run and minimised. Third-party proxy/browser vendors are used only when policy permits, never by default. | **No.** | Stored as run artefacts per the [Retention Schedule](./retention-schedule.md). |
| **Agent run-history** (the durable, step-by-step record of what an agent did, including the human Approve/Reject decisions) | Stored so you can review, audit, and supervise autonomous actions. | **No.** | Per the [Retention Schedule](./retention-schedule.md). |
| **Logs and audit events** | Identifiers, hashes, status, and policy decisions — **not** full content, secrets, or document bodies. | **No.** | Audit events: **365 days** (subject to legal-hold exceptions). |
| **Credentials / secrets** (OAuth tokens, API keys, connector cookies) | Stored only in a credential vault; classified `credential_or_secret`; never sent to third-party AI providers. | **No.** | Until revoked or deleted. |

### No training on your data
Under Velion's Zero Data Retention configuration, **Microsoft / Azure OpenAI does
not use your prompts or completions to train or improve its models, and does not
retain them beyond serving the request.** Velion itself does not train or
fine-tune models on customer content for any other customer or for general model
improvement. (Where a customer explicitly commissions a fine-tune of their *own*
data for their *own* use, that is a separate, opt-in, contractually scoped
activity — it is never the default and never spans tenants.)

## 3. How we classify your data

Every record that can contain personal data or your confidential content carries a
privacy classification that governs how it may be stored, indexed, and shared:

- `public_non_personal` — public data, no identifiable person.
- `customer_private` — your confidential content; stays inside approved boundaries.
- `personal` — personal data under GDPR.
- `sensitive_personal` — special-category / high-risk personal data; denied by
  default unless an approved DPIA and explicit processing policy exist.
- `credential_or_secret` — tokens, keys, passwords, cookies; vault-only, never
  persisted in the clear, never sent to third parties.
- `zdr_ephemeral` — processed transiently and **never** stored, cached, embedded,
  logged, or sent to a non-ZDR provider.

Third-party processing (any AI model vendor, browser/proxy vendor, or external
SaaS) is **denied by default** for `customer_private`, `personal`,
`sensitive_personal`, `credential_or_secret`, and `zdr_ephemeral` data. It is
allowed only when your organisation's policy permits it for a specific purpose,
the processor is on our approved [subprocessor list](./subprocessor-list.md), and
the request is explicitly authorised.

## 4. Autonomous agents and human oversight

Velion agents can run multi-step, multi-tool tasks. To satisfy your obligations
around automated decision-making (GDPR Art. 22), Velion provides:

- A **human Approve/Reject gate** on agent actions, so a person can review and
  authorise consequential steps before they execute.
- A **durable run-history** of every step and decision, for audit and oversight
  (supporting GDPR Art. 5(2) accountability and Art. 30 records).

Velion is a tool you operate; **you (the controller) remain responsible** for the
decisions your agents take and for ensuring meaningful human involvement where the
law requires it.

## 5. Your rights and deletion

- You can request deletion of a user's or organisation's data. Deletion is
  command-driven: Control Plane verifies authority and scope, then deletion /
  anonymisation propagates to derived copies (conversations, documents, vectors,
  run-history, sessions, connector state). Audit records that deletion happened
  **without** retaining the erased content.
- A **Data Subject Access Request (DSAR) intake endpoint is being built**; until
  then, DSARs are handled through your account contact. *(Confirm production
  status before relying on this externally.)*
- Some records are retained under documented legal, security, billing, or audit
  obligations — see the [Retention Schedule](./retention-schedule.md).

## 6. Security in brief

EU/EEA hosting; encryption in transit and at rest; per-organisation (multi-tenant)
isolation; least-privilege access; audit logging that records *what happened*
without becoming a shadow copy of your content; and a default-deny posture for
sending your data to any third party. Full processor-side security measures are in
the [DPA](./dpa-template.md).

---

*Questions about this policy should go to your Velion account contact. This is a
draft pending Norwegian privacy counsel review and is not a contractual commitment
until incorporated into your agreement.*
