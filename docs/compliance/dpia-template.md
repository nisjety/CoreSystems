# Data Protection Impact Assessment (DPIA) — Template + Filled Instance

> **DRAFT — INTERNAL TEMPLATE, NOT LEGAL ADVICE.** Not reviewed by Norwegian
> privacy counsel. A DPIA is the controller's legal responsibility (GDPR Art. 35);
> Velion (processor) provides this template and the technical facts to assist.
> Must not be relied on externally until counsel-reviewed. See [`README.md`](./README.md).

---

## How to use this template

GDPR Art. 35 and Datatilsynet require a DPIA for processing **likely to result in
a high risk** to individuals — in particular: systematic and extensive automated
decision-making/profiling with significant effects, large-scale processing of
special categories, large-scale systematic monitoring, and **use of new
technologies**. An AI agent that acts autonomously over customer and company data
hits several of these triggers, so a DPIA is expected.

Datatilsynet structures a DPIA around **four required elements** (mirroring Art.
35(7)). Fill each section. A DPIA is a living document — revisit it when the
processing changes.

> **Consultation trigger:** if, after applying the safeguards, the residual risk
> is still *high*, the controller must consult Datatilsynet under Art. 36 **before**
> starting the processing.

---

## Part A — Template (controller fills in)

### A.0 Administrative
- DPIA owner / controller:
- Processor(s) involved: Velion (AI agent platform) + its subprocessors (see [list](./subprocessor-list.md)).
- DPO / privacy contact:
- Date / version:
- Review date:

### A.1 — Element (a): Systematic description of the processing and its purposes
- What processing operations take place (collection, storage, AI inference, agent automation, retrieval, web crawling, deletion)?
- Purpose(s) of each operation (`purpose_id` in Velion: support, billing, import, retrieval, model execution, etc.).
- Categories of data subjects (employees, customers, contacts, external subjects).
- Categories of personal data, mapped to Velion classes (`personal`, `sensitive_personal`, `customer_private`, …).
- Recipients and subprocessors; international transfers and their basis.
- Lawful basis (Art. 6) per purpose; Art. 9 condition if special categories.
- Retention periods (reference the [Retention Schedule](./retention-schedule.md)).
- Data flow diagram / description.

### A.2 — Element (b): Assessment of necessity and proportionality
- Is the processing necessary to achieve the purpose, or could less data / a less intrusive method achieve it?
- Data minimisation: what is *not* collected; what is redacted/classified before persistence.
- Accuracy, storage limitation, purpose limitation.
- How data-subject rights are supported (access, rectification, erasure, objection, restriction; rights re: automated decisions under Art. 22).
- Information provided to data subjects (transparency, Art. 13/14).

### A.3 — Element (c): Assessment of risks to the rights and freedoms of data subjects
For each risk: source → event → impact on individuals → **likelihood** (low/med/high)
× **severity** (low/med/high) = **inherent risk**.
- Illegitimate access (confidentiality).
- Unwanted modification (integrity).
- Loss / unavailability.
- Risks specific to AI agents: incorrect autonomous action, opaque/automated
  decisions, over-collection during crawl/retrieval, model "memorisation" /
  leakage across tenants, third-country access.

### A.4 — Element (d): Measures envisaged to address the risks
For each risk in A.3, list the safeguard, who owns it, and the **residual risk**
after the safeguard. Reference the security controls in the [DPA](./dpa-template.md)
and the supplementary measures in the [Transfer Impact Assessment](./transfer-assessment.md).

### A.5 — Conclusion
- Residual risk acceptable? (yes / no)
- Art. 36 prior consultation with Datatilsynet required? (yes / no)
- Sign-off (controller / DPO):

---

## Part B — Filled instance

### Use case: *Autonomous AI agent processing customer + company data with human-approval gates*

> Example instance for Velion's flagship use case, first deployed with Design
> Partner **Aquatiq** (food-safety / hygiene B2B). Figures and basis are
> **illustrative** for the controller to validate, not legal conclusions.

#### Why a DPIA is required (high-risk triggers)
- **New technology:** an autonomous LLM-driven agent that plans and executes
  multi-step tool actions.
- **Automated decision-making / profiling potential:** the agent can take actions
  with effects, even though a human Approve/Reject gate is interposed.
- **Systematic processing** of company and customer data, including potentially
  personal data within documents, conversations, and crawled content.

#### (a) Systematic description
- **Operations:** ingest customer documents and queries → classify
  (`privacy_classification`) → embed and index for retrieval → run an agent loop
  (LLM inference + tools) → propose actions → **human Approve/Reject** → execute
  approved actions → persist a durable run-history → emit audit events.
- **Purposes:** delivering the contracted agent service (e.g. food-safety/hygiene
  knowledge tasks, support automation, retrieval over the customer's own corpus).
- **Data subjects:** the customer's employees and, incidentally, individuals named
  in documents/conversations the agent processes.
- **Data categories:** mostly `customer_private` and `personal`;
  `sensitive_personal` **denied by default** unless this DPIA and an explicit
  policy authorise it; `credential_or_secret` for connector tokens (vault-only).
- **Processing location:** Azure OpenAI **Sweden Central (EU/EEA)**, ZDR. Norway
  East available as Tier 2. **TTS in East US 2 is excluded** from this use case
  until remediated.
- **Recipients / transfers:** Microsoft Azure (hosting + model inference, EEA);
  other subprocessors only per the [list](./subprocessor-list.md). CLOUD Act
  residual disclosed in the [TIA](./transfer-assessment.md).
- **Lawful basis (illustrative):** Art. 6(1)(b) performance of the customer
  contract and/or 6(1)(f) legitimate interest of the customer-controller in
  operating its business; **no special-category processing** unless an Art. 9
  condition is documented.
- **Retention:** per the [Retention Schedule](./retention-schedule.md) (audit
  events 365 days; run-history and conversations per schedule; embeddings tied to
  source).

#### (b) Necessity and proportionality
- The agent only processes data the customer supplies or directs it to retrieve;
  Velion's design **classifies and can redact/reject** secrets and high-risk
  personal data before persistence.
- **Minimisation:** audit/log layer stores identifiers, hashes, and policy
  decisions — **not** full content, prompts with private content, or document
  bodies.
- **Less-intrusive alternative considered:** non-autonomous (suggest-only) mode;
  the human Approve/Reject gate is the proportionality control that keeps a person
  in the loop for consequential actions.
- **Rights supported:** access/erasure via command-driven deletion across derived
  copies; the run-history gives data subjects/controllers visibility into
  automated processing (transparency); DSAR intake **being built**.

#### (c) Risk assessment (illustrative)

| # | Risk | Likelihood | Severity | Inherent |
|---|------|-----------|----------|----------|
| R1 | Agent takes an incorrect/harmful autonomous action | Med | High | **High** |
| R2 | Personal data leaks across tenants (multi-tenant) | Low | High | Med-High |
| R3 | Over-collection during web crawl/retrieval | Med | Med | Med |
| R4 | Third-country (US) government access via Azure (CLOUD Act) | Low | High | Med-High |
| R5 | Sensitive content sent to a non-ZDR / non-EEA provider | Low | High | Med-High |
| R6 | Secrets/credentials persisted or exposed in logs | Low | High | Med |
| R7 | Opaque automated decision affecting an individual (Art. 22) | Med | Med-High | **High** |

#### (d) Measures / safeguards and residual risk

| # | Safeguard | Residual |
|---|-----------|----------|
| R1 | **Human Approve/Reject gate** before consequential actions; durable run-history for review; ability to halt a run. | Low-Med |
| R2 | Per-organisation isolation; org-scoped policy lookup and deletion scope; no cross-tenant retrieval. | Low |
| R3 | Velion-owned egress broker (Quarry) with per-domain limits, classification before persistence, default `allow_third_party_processing=false`. | Low-Med |
| R4 | EU residency + encryption + ZDR + access controls as supplementary measures; CLOUD Act **disclosed** as residual (see [TIA](./transfer-assessment.md)). | **Disclosed residual** |
| R5 | Default-deny third-party processing for protected classes; provider calls gated by classification + residency + ZDR + processor approval. | Low |
| R6 | `credential_or_secret` class is vault-only; audit policy forbids logging tokens, cookies, prompts with private content, document bodies. | Low |
| R7 | Human-in-the-loop gate + run-history transparency; controller informs data subjects; safeguards per Art. 22(3). | Med (controller-owned) |

#### Conclusion (illustrative)
- After safeguards, the **CLOUD Act residual (R4)** remains the principal residual
  and is **disclosed, not eliminated**. Whether overall residual risk is
  acceptable, and whether **Art. 36 prior consultation with Datatilsynet** is
  needed, is a **decision for the controller and its counsel** — this template does
  not make that determination.
- Special-category processing remains **out of scope** unless separately assessed.

---

*This DPIA template and instance are drafts pending Norwegian privacy counsel
review.*
