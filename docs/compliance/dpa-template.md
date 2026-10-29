# Data Processing Agreement (DPA) — Template

> **DRAFT — INTERNAL TEMPLATE, NOT LEGAL ADVICE.** Not reviewed by Norwegian
> privacy counsel. This is a drafting starting point, **not** an executable
> contract. Bracketed `[…]` fields must be completed; all terms are subject to
> counsel review and negotiation. See [`README.md`](./README.md).

**Data Processing Agreement** pursuant to Article 28 GDPR, forming part of the
agreement between:

- **Controller:** `[Customer legal name, org. no., address]` ("Customer")
- **Processor:** Verevon `[legal entity, org. no., address]` ("Verevon")

each a "Party". Where Norwegian law applies, references to the GDPR include the
Norwegian **Personal Data Act** (*personopplysningsloven*).

---

## 1. Definitions
Terms used here (Controller, Processor, Personal Data, Processing, Data Subject,
Personal Data Breach, Subprocessor, Supervisory Authority) have the meanings given
in the GDPR. The Supervisory Authority is **Datatilsynet** unless otherwise agreed.

## 2. Roles and scope
2.1 The Customer is the **Controller**; Verevon is the **Processor**, processing
Personal Data only on the Customer's behalf to provide the Verevon AI agent
platform.
2.2 Subject-matter, duration, nature, and purpose of processing, the types of
Personal Data, and the categories of Data Subjects are set out in **Annex 1**
(consistent with Verevon's [ROPA](./ropa.md)).

## 3. Processor obligations (Art. 28(3))
Verevon shall:
- **(a) Documented instructions.** Process Personal Data only on the Customer's
  documented instructions (including this DPA and platform configuration),
  including for international transfers, unless required by EU/EEA or Norwegian law
  (in which case Verevon informs the Customer unless legally prohibited). Verevon
  informs the Customer if an instruction appears to infringe data-protection law.
- **(b) Confidentiality.** Ensure personnel authorised to process Personal Data
  are bound by confidentiality.
- **(c) Security.** Implement the technical and organisational measures in
  **Annex 2** (Art. 32).
- **(d) Subprocessors.** Engage Subprocessors only per **clause 4**.
- **(e) Data-subject rights.** Assist the Customer per **clause 6**.
- **(f) Assistance.** Assist the Customer with security, breach notification,
  DPIAs, and prior consultation (Arts. 32–36) per **clause 7** and **clause 8**.
- **(g) Deletion/return.** On termination, delete or return Personal Data per
  **clause 9**.
- **(h) Audit.** Make available information needed to demonstrate compliance and
  allow audits per **clause 10**.

## 4. Subprocessors (Art. 28(2),(4))
4.1 The Customer grants **general authorisation** to engage the Subprocessors
listed in Verevon's [Subprocessor List](./subprocessor-list.md), as in force from
time to time.
4.2 Verevon gives the Customer **prior notice** of any intended addition or
replacement of a Subprocessor, allowing the Customer to **object** on reasonable
data-protection grounds within `[30]` days.
4.3 Verevon imposes on each Subprocessor, by written contract, data-protection
obligations **no less protective** than those in this DPA, and remains fully
liable to the Customer for the Subprocessor's performance.
4.4 No Personal Data is sent to any Subprocessor for `customer_private`,
`personal`, `sensitive_personal`, `credential_or_secret`, or `zdr_ephemeral` data
except where the Customer's policy permits it for a specific purpose and the
Subprocessor holds an approved processor record (default-deny posture).

## 5. International transfers
5.1 Verevon processes Personal Data in the **EU/EEA (Azure OpenAI Sweden Central)**
by default (Residency Tier 1). Norway East is available as Tier 2; Sweden Central
fallback is used only on the Customer's documented approval.
5.2 Where any transfer to a third country occurs (directly or via a Subprocessor),
it is made under an Art. 46 mechanism (**SCCs**, and DPF where applicable) together
with the **supplementary measures** in the [Transfer Impact Assessment](./transfer-assessment.md).
5.3 The Parties acknowledge the **disclosed residual risk** that US-headquartered
Subprocessors (including Microsoft) may be subject to US law (CLOUD Act) regardless
of storage location; Verevon does not warrant immunity from lawful foreign-government
access.
5.4 **TTS** is processed in **East US 2 (US)** and **must not** be used for
`customer_private` or `sensitive_personal` content until relocated to the EEA.

## 6. Assistance with data-subject rights (Art. 28(3)(e))
Taking into account the nature of the processing, Verevon assists the Customer by
appropriate technical and organisational measures, insofar as possible, in
fulfilling requests to exercise rights of access, rectification, erasure,
restriction, portability, and objection — including rights relating to **automated
decision-making (Art. 22)**, supported by Verevon's human Approve/Reject gate and
durable run-history. Verevon forwards any request received directly from a Data
Subject to the Customer without undue delay and does not respond itself unless
instructed.

## 7. Security measures (Art. 32) — summary; full text in Annex 2
- EU/EEA residency; encryption in transit and at rest.
- **Multi-tenant isolation** — strict per-organisation scoping of data, policy
  lookup, and deletion.
- **Least-privilege access controls**; authenticated/authorised access only.
- **Privacy classification** of records (`public_non_personal` …
  `credential_or_secret`, `zdr_ephemeral`) governing storage, indexing, and sharing.
- **`credential_or_secret`** handled in a vault only; never persisted in the clear
  or sent to third-party AI providers.
- **Default-deny third-party processing** for protected data classes.
- **Zero Data Retention** at the model layer (no model-provider retention or
  training on Customer content).
- **Audit logging** of processing and policy decisions that records *what happened*
  without storing document bodies, prompts with private content, tokens, cookies,
  or screenshots; with retry/dead-letter handling for security/deletion events.
- Resilience, backup, and restoration; regular testing of measures.

## 8. Personal Data Breach (Arts. 33–34)
8.1 Verevon notifies the Customer **without undue delay** and in any event within
`[24–48]` hours after becoming aware of a Personal Data Breach affecting the
Customer's Personal Data.
8.2 The notification includes, to the extent known: nature of the breach,
categories and approximate number of Data Subjects and records, likely
consequences, and measures taken/proposed.
8.3 Verevon assists the Customer in meeting its own Art. 33 (Datatilsynet, 72-hour)
and Art. 34 (Data Subject) obligations. Verevon does **not** notify Datatilsynet on
the Customer's behalf unless expressly instructed.

## 9. Deletion and return on termination (Art. 28(3)(g))
9.1 On termination, and at the Customer's choice, Verevon **deletes or returns** all
Personal Data and deletes existing copies, unless EU/EEA or Norwegian law requires
retention.
9.2 Deletion is **command-driven and verifiable**: Control Plane verifies
authority/scope and propagates deletion/anonymisation to derived copies
(conversations, documents, vectors, run-history, sessions, connector state, caches,
browser/sandbox artefacts). Audit records that deletion occurred **without**
retaining erased content.
9.3 Retention exceptions are limited to documented legal, security, billing, or
audit obligations, retaining the minimum necessary and using pseudonymisation where
possible (see [Retention Schedule](./retention-schedule.md)).

## 10. Audit rights (Art. 28(3)(h))
10.1 Verevon makes available information reasonably necessary to demonstrate
compliance with Art. 28 and this DPA.
10.2 The Customer (or an independent auditor it mandates) may audit on `[30]` days'
notice, no more than `[once per year]` except where a breach or Supervisory
Authority requires more, subject to confidentiality and minimal disruption.
Available certifications/reports (e.g. SOC 2 / ISO 27001, **if and when held —
TBC**) may be provided to satisfy audit requests.

## 11. Liability, term, governing law
11.1 Liability as set out in the main agreement.
11.2 This DPA runs for the term of the main agreement and survives as to clause 9.
11.3 Governed by **Norwegian law**; venue `[…]`; the GDPR/EEA framework applies.

---

### Annex 1 — Description of processing
*(Complete from the [ROPA](./ropa.md): subject-matter, duration, nature, purpose,
data categories, data-subject categories.)*

### Annex 2 — Technical and organisational security measures
*(Expand clause 7 into the full TOM description; reference the security controls and
the [Transfer Impact Assessment](./transfer-assessment.md) supplementary measures.)*

### Annex 3 — Subprocessors
*(Incorporates the [Subprocessor List](./subprocessor-list.md) by reference.)*

---

*This DPA template is a draft pending Norwegian privacy counsel review and is not an
executable contract.*
