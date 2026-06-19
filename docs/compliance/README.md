# Velion Compliance Documentation Pack

> **DRAFT — INTERNAL TEMPLATES, NOT LEGAL ADVICE.**
> Every document in this directory is an internal working draft prepared by the
> Velion team. **None of these documents has been reviewed by Norwegian privacy
> counsel and none may be used in any customer-facing, contractual, or regulator-
> facing context until that review is complete and recorded in the status table
> below.** These templates are a starting point for counsel, not a substitute for
> counsel. They describe Velion's intended and (where stated) implemented data
> handling; where a control is planned but not yet verified in production it is
> marked accordingly. Do not represent any control as "in place" externally
> without engineering confirmation.

---

## Purpose

This pack collects the GDPR / Norwegian-market privacy and security artefacts
Velion needs as a data processor offering an autonomous AI agent platform to
B2B customers (controllers). It is grounded in Velion's real architecture
(Azure OpenAI Sweden Central, Zero Data Retention at the model layer,
multi-tenant org isolation, durable agent run-history with a human Approve/Reject
gate, and the `audit-core` event log). It is **not** aspirational marketing — any
capability not yet built is flagged as planned or TBC.

## Regulatory frame

- **GDPR** (Regulation (EU) 2016/679), as incorporated into Norwegian law via the
  **Personal Data Act** (*personopplysningsloven*, LOV-2018-06-15-38) and the EEA
  Agreement.
- Supervisory authority: **Datatilsynet** (the Norwegian Data Protection Authority).
- Cross-border transfer law: **Schrems II** (C-311/18), EU SCCs (2021/914), and
  the EU-US **Data Privacy Framework** (under ongoing legal challenge — "Schrems III").

## Documents in this pack

| # | File | What it is | Audience |
|---|------|-----------|----------|
| 1 | [`ai-data-policy.md`](./ai-data-policy.md) | Plain-language statement of what happens to prompts, completions, files, embeddings, logs, retrieved web content, and agent run-history. | Customer-facing |
| 2 | [`dpia-template.md`](./dpia-template.md) | Data Protection Impact Assessment template (Datatilsynet's 4 elements) + a filled instance for the autonomous-agent use case. | Internal / counsel / Datatilsynet |
| 3 | [`transfer-assessment.md`](./transfer-assessment.md) | Schrems II Transfer Impact Assessment, incl. CLOUD Act residual and supplementary measures. | Internal / counsel |
| 4 | [`subprocessor-list.md`](./subprocessor-list.md) | Register of subprocessors: purpose, data categories, region, transfer basis. | Customer-facing |
| 5 | [`dpa-template.md`](./dpa-template.md) | Data Processing Agreement template (Velion as processor), GDPR Art. 28 structure. | Contractual |
| 6 | [`ropa.md`](./ropa.md) | Records of Processing Activities (Art. 30) register. | Internal / Datatilsynet |
| 7 | [`retention-schedule.md`](./retention-schedule.md) | Per-data-type retention TTLs + erasure / right-to-be-forgotten procedure. | Internal / counsel |

## Shared facts (kept consistent across the pack)

These facts are asserted identically in every document. If any changes, update it
everywhere:

- **Primary processing region:** Azure OpenAI **Sweden Central** (EU/EEA). This is
  **Residency Tier 1 — EEA Standard** (default for all customers).
- **Residency Tier 2 — Norway Residency:** Norway East where the required models
  are available; Sweden Central is used as a fallback **only on documented customer
  approval**.
- **Zero Data Retention (ZDR):** Microsoft / Azure OpenAI does **not** use customer
  prompts or completions to train or improve its models, and does not retain them
  beyond the request, under Velion's ZDR-enrolled configuration.
- **Known exception (disclosed):** Text-to-speech (TTS) currently runs in
  **East US 2** (United States). This is a known gap under active remediation;
  TTS must not be used for `sensitive_personal` or `customer_private` content
  until it is relocated into the EEA.
- **Honesty constraint:** **EU residency is not data sovereignty.** A
  US-headquartered subprocessor — including Microsoft Azure — remains reachable
  under the US CLOUD Act regardless of where data is physically stored. This is
  treated as a **disclosed residual risk** in the transfer assessment, never as
  "we are immune."
- **ZDR does not discharge Velion's own obligations.** ZDR at the model layer means
  the *model vendor* does not retain content; Velion still retains agent
  run-history, conversation records, and audit events under its own retention
  schedule, and remains responsible for their lawful processing and deletion.
- **First Design Partner:** Aquatiq (food-safety / hygiene B2B).

## Status table

| Document | Drafted | Counsel-reviewed | Signed / approved |
|----------|---------|------------------|-------------------|
| AI Data Policy | ✅ 2026-06-18 | ⬜ Pending | ⬜ |
| DPIA template + instance | ✅ 2026-06-18 | ⬜ Pending | ⬜ |
| Transfer Impact Assessment | ✅ 2026-06-18 | ⬜ Pending | ⬜ |
| Subprocessor List | ✅ 2026-06-18 | ⬜ Pending | ⬜ |
| DPA template | ✅ 2026-06-18 | ⬜ Pending | ⬜ |
| ROPA (Art. 30) | ✅ 2026-06-18 | ⬜ Pending | ⬜ |
| Retention Schedule | ✅ 2026-06-18 | ⬜ Pending | ⬜ |

Legend: ✅ done · ⬜ not yet · Counsel = Norwegian privacy counsel review.

## Open items flagged for verification (TBC)

These are factual claims in the pack that engineering / legal must confirm before
external use:

- Exact Resend processing/storage region and current DPA + SCC status.
- Exact Twilio processing region for SMS/2FA and current DPA + SCC status.
- Identity, region, and contract terms of the Quarry browser-automation / residential-proxy / unblocker vendor(s).
- Whether all model vendors reached through the Model Plane are ZDR-enrolled and EEA-regioned (currently only the Azure OpenAI Sweden Central path is confirmed).
- Confirmation that the cross-plane deletion command flow (Control → Application/Data/Ingestion/Model) is fully implemented and tested end-to-end (per the GDPR summary it is partly a target state).
- The customer-facing DSAR intake endpoint is **being built**; confirm its production status before publishing the AI Data Policy.

---

*Maintainer: Velion engineering. Source of truth for architecture facts:*
*`apps/GDPR_SUMMARY.md`, `apps/Control Plane/auth-core/migrations/gdpr_hard_delete.sql`,*
*and the per-plane responsibility notes in the CoreSystem monorepo.*
