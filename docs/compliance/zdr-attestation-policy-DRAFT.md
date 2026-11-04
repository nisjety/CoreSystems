# Zero Data Retention: written attestation policy and provider evidence checklist

> **DRAFT, NOT APPROVED.** Written 2026-11-03 for register rows D1-327 and D1-330
> (`docs/plans/model-plane-depth-register.md`). Nothing in this document is in force.
> It marks no model as ZDR-capable and changes no configuration. It takes effect only
> when the owner, or a reviewer the owner names, signs section 9.
> Until then every ZDR turn is refused before any model call, as the live proofs of
> 2026-10-28 and 2026-11-03 showed (`docs/compliance/evidence/live-proof-wave1-zdr-2026-11-03.txt`).

## What this document is, and what it is not

**It is** Verevon's internal written policy for task Z-40, which covers four things:

- what evidence must exist before one provider surface may be attested as zero data retention;
- what the platform guarantees for a ZDR turn;
- how the attestation is granted and revoked;
- who reviews and signs it.

D1-327 asks for this policy and for an owner-named reviewer.

**It is not the provider contract.** The zero-retention commitment itself has to come
in writing from the provider: Microsoft for Azure OpenAI and Azure AI Foundry, Anthropic
for its own API. This document does not state what any provider has committed to.
Where the repository records an assumption about a provider's offering, it is labelled
as an assumption and repeated as an open question in section 8.

## 1. Terms

- **ZDR turn.** An interactive chat turn whose prompt and answer must leave nothing
  durable behind, in Verevon or at the provider (`apps/Model Plane/docs/ZDR.md`, section 1). Two things
  make a turn ZDR, and they only ever add up:
  - the per-turn Temporary chat toggle;
  - the org-wide switch. Auth Core signs it into the token as the org's stored intent AND
    its plan entitlement (plans `pro` and `enterprise`).

  The scope is interactive content only.
- **Provider surface.** One cloud resource behind one endpoint that inference-core
  sends chat requests to. One attestation covers exactly one surface
  (`inference-core/src/provider/zdr.rs`).
- **Attested.** inference-core has a complete, in-force set of the six attestation
  fields for that surface (section 5), and so advertises `supports_zdr`. The model
  catalogue (`/api/v1/models`) then lists the `zdr` feature on the models it serves.

## 2. Which providers and models could be ZDR-capable

Read from the code and configuration on main 0b970c5d9 (2026-11-03). **No surface is
attested today, and the live catalogue lists no `zdr` chat model.**

| Surface | How inference-core reaches it | Can it carry an attestation today? | Notes |
|---|---|---|---|
| Azure OpenAI (checked-in config points at `core-ai-rg.cognitiveservices.azure.com`; region Sweden Central per `docs/compliance/subprocessor-list.md`) | `OpenAiProvider` | Yes: `AZURE_OPENAI_ZDR_*` | Decision DEC-33 names it the first surface to attest. Which chat deployments it serves in production is open (Q2). |
| Azure AI Foundry Claude (`cloude-ai-resource.services.ai.azure.com`) | `AnthropicProvider::new_azure` | Yes: `AZURE_ANTHROPIC_ZDR_*`, attached only to this route | DEC-33: second, after the Azure OpenAI one. Whether a retention exception exists for Claude on Foundry is open (Q3). |
| Anthropic first-party API (`api.anthropic.com`) | `AnthropicProvider` direct route | **No.** The direct route never advertises ZDR (`direct_anthropic_defaults_to_no_zdr`) | The developer stack serves `claude-sonnet-5-5`, `claude-opus-5-5` and `claude-haiku-5-5` through this route today. Making it attestable needs a decision and a code change (Q4). |
| ChatGPT account (`openai-codex-subscription`) | `CodexSubscriptionProvider` | **Never.** `supports_zdr: false` is fixed in code | A ZDR turn on this connection is refused `zdr_connection_not_supported`. The per-connection waiver (ZDR.md section 5) is designed and not built. |
| Other Azure modalities (speech, translation, document intelligence, language, image, realtime, video) | various | Out of scope | A ZDR request for them is refused. Text-to-speech runs in East US 2 (subprocessor list, known exception). |
| Data Plane embeddings and rerank (Cohere on Azure AI Foundry), contextual retrieval, graph extraction | not through inference-core | Out of scope | Not covered by org ZDR today (`docs/compliance/evidence/honest-limits.md` L12, L33). See Q9. |

## 3. Evidence required before a surface may be attested

The reviewer checks every item below against original documents, never against a
summary, and records where each document is kept (Q6). An item with no document
means the surface is not attested.

**For every surface:**

| # | Evidence | What it must show | Becomes |
|---|---|---|---|
| E1 | The provider's written commitment (contract, addendum, or approval letter) | Prompts and completions for the named resource are not retained after processing. Any exception (abuse monitoring, legal hold, operational logs) is named, with its duration. | the basis of the claim |
| E2 | The exact resource covered | Resource name or id, subscription, and endpoint host. These must equal the endpoint inference-core is configured with; the code only warns on a mismatch, so the reviewer compares by hand. | `<prefix>_ZDR_RESOURCE_ID` |
| E3 | The provider's approval or reference number | The arrangement in E1 was granted for this resource | `<prefix>_ZDR_APPROVAL_REF` |
| E4 | Effective date and term | From when it applies, and when it must be renewed | `<prefix>_ZDR_EFFECTIVE_DATE`; `<prefix>_ZDR_REVIEW_BY` no later than the renewal date (and see Q7) |
| E5 | Training use | The provider's written statement on whether customer prompts and completions are used to train models | recorded with E1 |
| E6 | Region and deployment type | Where the resource processes data, and its deployment type (for example `DataZoneStandard` or `GlobalStandard`, `AZURE_OPENAI_DEPLOYMENT_TYPE`). This must match the subprocessor list. | residency record |
| E7 | Human review and abuse monitoring | Whether provider staff or systems can read content for abuse monitoring, and that this is switched off for the resource if E1 depends on it | recorded with E1 |
| E8 | The provider's own subprocessors for this resource | Who else touches the content | subprocessor list |
| E9 | Change and termination notice | How Verevon learns that the arrangement changed or ended, and how fast | revocation trigger (section 6) |

**Per surface:**

- **Azure OpenAI.** The repository's working assumption (ZDR.md section 4, step 0) is
  that the relevant arrangement is the "no data retention / no human review"
  configuration (abuse monitoring off), granted per Azure subscription on approval.
  That is an assumption written in this repository, not a verified commitment. The
  reviewer checks it against Microsoft's written approval for the subscription that
  holds the resource (Q2).
- **Azure AI Foundry Claude.** It is open whether a comparable retention exception
  exists for Claude models on Foundry, and whose terms govern it: Microsoft's,
  Anthropic's, or both (Q3).
- **Anthropic first-party API.** ZDR.md step 2 assumes "a ZDR agreement on the account".
  Even with such an agreement the code cannot attest this route today (section 2, Q4).

**Never acceptable as evidence:** a region alone; the legacy boolean
`<prefix>_ZDR_CONFIRMED=true` (it fails boot); a test fixture or a fixture-marked
resource id (D1-327: the check cannot tell a fixture from a real attestation, so a
fixture value must never reach a signed policy); a provider's general marketing page
instead of a document that names our resource.

## 4. What the platform guarantees for a ZDR turn

This is the guarantee as built and as proven on the developer stack. It is not
production evidence.

### 4.1 Refused before any model call, with a typed code

- **No attested provider for the turn:** `zdr_unavailable` (inference-core skips every
  provider surface without an in-force attestation). Proven live 2026-10-28 and 2026-11-03.
- **A ChatGPT-account connection:** `zdr_connection_not_supported`.
- **A feature the persistence-free path cannot honour:** `zdr_feature_not_supported`
  with the feature named, checked in this fixed order (`model-gateway/src/zdr_features.rs`):
  1. attachments, image generation, deep research;
  2. agentic and Do runs, plan mode, message versions;
  3. web search (browsing or a web-only filter), connector reads;
  4. client tools, skills, pinned messages, spend cap, deployed agents.

  Since D1-815 (8689b1c43) the composer sends no web search for a ZDR org, so an
  ordinary turn is not refused for it.
- **Other refusals:**
  - `zdr_durable_mutation_forbidden` for durable gateway mutations;
  - `zdr_draft_persistence_forbidden` and `zdr_ai_proposal_forbidden` in the inbox;
  - `zdr_recurrence_forbidden` in support;
  - scheduled and unattended runs, browser runs, speech and other modalities.
- **Turning org ZDR on:** refused `409 zdr_no_attested_provider` while no chat model is
  attested (`503 zdr_gate_unavailable` if the catalogue cannot be read). Turning it off
  is never refused.

### 4.2 Never stored by Verevon

A ZDR turn creates no thread, message or run row; nothing goes into the response cache;
no feedback row, no memory extraction, no provider-side prompt caching; and no audit or
bus envelope carries its content.

Live proof, 2026-10-28 and 2026-11-03: these counters did not change across refused ZDR
turns:

- threads, messages, runs;
- feedback ratings, the session audit outbox, ZDR rows in that outbox;
- the `mp:gw:cache:*` keyspace.

The pass 2 proof (section 5, step 7) must show the same for an *answered* turn.

### 4.3 Logs

The log lines seen for a ZDR turn in the proofs were content-free: request id, route,
timings, "provider skipped". **No full log audit has been done.** Before approval, the
services on the ZDR path must be checked at their production log levels, debug
included: BFF, model-gateway, inference-core, session-core and Auth Core. The check
must show that no prompt or answer text is logged (Q8).

### 4.4 What ZDR does not cover (disclosed limits)

- connector ingestion, contextual retrieval, graph extraction, embeddings and rerank
  (honest-limits L12, L33);
- the browser's local copy of earlier chats after an org turns ZDR on (task Z-09);
- the cost ledger: ZDR spend is not ledgered (DEF-Z-06b, Q10);
- the model provider itself, until a surface is attested under this policy.

### 4.5 How it fails

- **No attested surface:** the turn is refused.
- **An attestation that has lapsed (review-by passed) or is not yet effective:** the
  surface stops granting ZDR. inference-core logs an ERROR and exports the review-by
  gauge, but does not stop booting (DEC-33, WS01-S5).
- **An incomplete or malformed attestation, a digest mismatch, or the legacy boolean:**
  inference-core refuses to boot.

## 5. How a surface is marked ZDR-capable (grant procedure)

None of these steps has been done.

1. **Collect the evidence** E1–E9 for one surface, and store the documents (Q6).
2. **Review:** the reviewer checks the evidence against section 3 and signs the checklist
   in section 9. The operator and the reviewer must be different people (proposal).
3. **Fill the six fields** for that surface in the runtime public policy, which is part
   of the signed release artifact. Use the real values only.

   | Field | Value |
   |---|---|
   | `<prefix>_ZDR_RESOURCE_ID` | E2 |
   | `<prefix>_ZDR_APPROVAL_REF` | E3 |
   | `<prefix>_ZDR_EFFECTIVE_DATE` | E4 |
   | `<prefix>_ZDR_REVIEW_BY` | E4 |
   | `<prefix>_ZDR_REVIEWER` | the signing reviewer's name |
   | `<prefix>_ZDR_DIGEST` | sha256 over the five fields above, in that order, one per line |

   Leave `<prefix>_ZDR_CONFIRMED` false.
4. **Check the attestation:** `scripts/release-artifact.sh check-zdr-attestation <runtime-public-policy.env>`
   must pass (fields complete, digest recomputed, review-by not passed).
5. **Set the gates file:** `ZDR_RETENTION_PATH=zdr-provider-route-attested`, with the
   SHA-256 of the signed evidence record as its evidence digest.
6. **Build, verify and deploy a signed candidate** (WS01-S12; open rows D1-302, D1-307).
7. **Pass 2 live proof** on the ZDR-TEST org (`apps/Model Plane/docs/runbooks/zdr-attestation-and-live-proof.md`,
   `ZDR_LIVE_TURN_EXPECT=answer`):
   - `/api/v1/models` lists `zdr` on the covered model;
   - a turn streams an answer;
   - every counter of 4.2 is unchanged;
   - the org switch can now be turned on.
8. **Record the attestation:**
   - ZDR.md, "Where the attestation record goes": resource, approval reference, dates,
     reviewer and digest, never a document's content;
   - `MODEL_PLANE_RELEASE_EVIDENCE.md`;
   - the decision ledger.
9. **Public wording** (subprocessor list, DPA, trust claims) changes only after step 7
   passes, and only by counsel or the owner.

## 6. How the marking is revoked

**Scheduled.**

- The review-by date passes, and the surface stops granting ZDR on its own. Alerts
  give warning, in the Prometheus UI only, since no pager is wired:
  - `ZdrAttestationReviewDue`, 14 to 3 days before;
  - `ZdrAttestationReviewImminent`, under 3 days;
  - `ZdrAttestationLapsed`, after the date.
- Renewal repeats section 5 with new dates and a new digest.

**Immediate.** Revoke at once when any of these happens:

- the provider gives notice of a change or an end to the arrangement (E9);
- any piece of evidence turns out wrong;
- the resource or endpoint changes;
- a security incident touches the surface;
- the reviewer withdraws the sign-off.

Steps:

1. Remove the six fields for the surface from the runtime public policy, or set
   review-by in the past, and deploy that. In production this means a new signed
   candidate. On the developer stack it means recreating inference-core with the
   changed configuration.
2. Confirm `/api/v1/models` no longer lists `zdr` for the surface, and that a ZDR turn
   is refused `zdr_unavailable`.
3. Organisations with ZDR on keep it on. Their chat is refused, so it fails closed, and
   the settings page shows the lapse banner. Nothing switches them to non-ZDR.
4. Record the revocation (date, reason, who) next to the attestation record.

A faster emergency path than a signed redeploy (the Z-14 kill switch) is designed and
not built (Q12).

## 7. Roles

| Role | Responsibility |
|---|---|
| Owner | Accountable. Approves this policy, names the reviewer, decides the open questions. |
| Reviewer | Named by the owner and independent of the operator. Checks E1–E9 against the original documents, signs section 9, and appears in `<prefix>_ZDR_REVIEWER`. Signs every renewal and every revocation. |
| Operator | Collects the documents, fills the fields, runs the check, builds the candidate, runs the pass 2 proof. Never signs their own attestation. |
| inference-core owner | Keeps the lapse and fail-closed behaviour of section 4.5 (DEC-33). |
| Counsel or compliance | Owns customer-facing wording: the subprocessor list, the DPA, trust claims rows 36–41. |

## 8. Open questions (for the owner)

1. **Which surface first?** DEC-33 says Azure OpenAI, then Azure AI Foundry Claude. But
   the developer stack and the live-test rule use Claude through the direct Anthropic
   API, which cannot be attested (section 2). Which surface should the first contract
   cover?
2. **Azure OpenAI.** Does Microsoft grant the no-retention, no-human-review arrangement
   for the subscription that holds the configured resource, and in what written form?
   Which chat deployments does that resource serve in production?
3. **Claude on Azure AI Foundry.** Is a retention exception available, and under whose
   terms?
4. **Direct Anthropic API.** Should it get an attestation slot? That needs a decision
   and a code change. It also needs Anthropic's written ZDR terms for the account.
5. **Reviewer.** Who is it (D1-327)?
6. **Document storage.** Where are the contract documents kept? Not in this repository;
   the evidence record holds references and digests only.
7. **Review-by ceiling.** What is the longest allowed review-by period? Proposal: no
   more than 12 months, and never after the contract's renewal date.
8. **Log audit.** Who runs the audit of 4.3, and is it required before approval?
   Proposal: yes.
9. **Background work.** Should embeddings, contextual retrieval and graph extraction
   follow org ZDR (honest-limits L12, L33) before ZDR is sold?
10. **Metering.** Is a cost ledger for ZDR turns (DEF-Z-06b) required for a paid add-on?
11. **Subprocessor list.** It lists "EEA-resident + ZDR" as a transfer basis for Azure
    OpenAI (`docs/compliance/subprocessor-list.md`), while no surface is attested.
    Should counsel correct the wording until step 7 of section 5 has passed?
12. **Emergency revocation.** Is a faster path needed than a signed redeploy (the Z-14
    kill switch, not built)?
13. **Plan lapse.** Should the plan-lapse rule (OD-LAPSE: the stored intent stays on
    while the plan has lapsed) be settled before ZDR is offered?

## 9. Sign-off (empty until approved)

| Item | Name | Date | Signature or reference |
|---|---|---|---|
| This policy approved by the owner | | | |
| Reviewer named by the owner | | | |
| Surface attested (one row per surface) | | | |
| Evidence E1–E9 checked (per surface) | | | |
| Log audit of 4.3 done | | | |
| Pass 2 live proof passed (per surface) | | | |

## Appendix A. Related open rows (proposed one-line fixes)

- **D1-814 (harness).** Add `http://localhost:5199` and `http://127.0.0.1:5199` to the
  developer auth-core's `BETTER_AUTH_TRUSTED_ORIGINS` (Control Plane env), so the
  Playwright `e2e` setup can sign in from its own base URL.
- **D1-816 (doc-code checker).** In `docs/compliance/evidence/tools/doc-codes.json`:
  - repoint `zdr_feature_not_supported` and `zdr_durable_mutation_forbidden` to
    `apps/Model Plane/rust/services/model-gateway/src/zdr_posture.rs`, where the
    literals now live;
  - register `zdr_turn_registers_no_run_control_listener` as a term;
  - move `zdr_connection_not_supported` to built;
  - repoint the three scanned documents from `apps/` to `docs/` (moved in 56019468a):
    `master-ownership-matrix.md`, `CODEBASE_INFORMATION_SYSTEM.md`, `GDPR_SUMMARY.md`.

  The checker fails 15 times on main today; all 15 come from these causes.

## Appendix B. Sources

- `apps/Model Plane/docs/ZDR.md` (sections 1–4, 5.9)
- `apps/Model Plane/rust/services/inference-core/src/provider/zdr.rs`, `config.rs`, `provider/anthropic.rs`, `provider/fallback.rs`, `provider/codex_subscription.rs`
- `apps/Model Plane/rust/services/model-gateway/src/zdr_features.rs`, `apps/Model Plane/rust/crates/mp-contracts/src/zdr_refusal.rs`
- `apps/Model Plane/scripts/release-artifact.sh` (check-zdr-attestation, ZDR_RETENTION_PATH), `apps/Model Plane/scripts/zdr-attestation-check.sh`
- `apps/Model Plane/docs/runbooks/zdr-attestation-and-live-proof.md`, `apps/Control Plane/observability/rules/zdr-attestation.rules.yml`
- `docs/plans/model-plane-chat-build-plan-2026-10-14.md` (DEC-33; WS01-S7, S12, S13)
- `docs/compliance/subprocessor-list.md`, `docs/compliance/evidence/honest-limits.md`
- `docs/compliance/evidence/live-proof-wave1-zdr-2026-10-28.txt`, `docs/compliance/evidence/live-proof-wave1-zdr-2026-11-03.txt`
