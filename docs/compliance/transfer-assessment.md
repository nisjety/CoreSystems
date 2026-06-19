# Transfer Impact Assessment (TIA) — Schrems II

> **DRAFT — INTERNAL TEMPLATE, NOT LEGAL ADVICE.** Not reviewed by Norwegian
> privacy counsel. Must not be relied on externally until counsel-reviewed.
> See [`README.md`](./README.md).

---

## 1. Why this assessment exists

Following **Schrems II** (CJEU C-311/18), a transfer mechanism such as Standard
Contractual Clauses (**SCCs**, EU 2021/914) **may be valid but is not always
sufficient on its own**. The exporter must assess, case by case, whether the law
and practice of the destination country provide a level of protection
**essentially equivalent** to the EU/EEA — and, where it does not, apply
**supplementary measures**. This TIA does that for Velion's processing.

Crucially, the relevant question is **actual access risk**, not merely where data
is physically stored. Even when production runs in **Sweden Central** or **Norway
East**, third-country access can arise through:

1. **Remote support / administration** by a subprocessor's personnel located
   outside the EEA.
2. **Subprocessors** that are themselves established in, or owned by entities in,
   a third country.
3. **Parent-company legal reach** — a US-headquartered provider (including
   Microsoft) is subject to US law (FISA 702, the **CLOUD Act**) regardless of the
   data centre's location.

## 2. Transfers in scope

| Flow | Data | Destination of *storage* | Possible third-country *access* | Mechanism |
|------|------|--------------------------|----------------------------------|-----------|
| Model inference & embeddings | prompts, completions, embeddings | **Sweden Central (EEA)** | US (Microsoft parent under CLOUD Act); remote support TBC | SCCs + supplementary measures; DPF (Microsoft) — see §5 |
| Application/run-history/audit hosting | conversations, run-history, audit | **EEA** (Velion-controlled) | Cloud provider parent reach | SCCs + supplementary; EU-resident infra |
| **TTS (known exception)** | text to synthesise | **East US 2 (US)** | Direct US processing | **Gap — remediation in progress; exclude protected classes until fixed** |
| Transactional email (Resend) | recipient email, message content | **TBC — verify** | TBC | SCCs / DPF — **TBC, verify** |
| SMS / 2FA (Twilio) | phone number, OTP | **TBC — verify** | TBC | SCCs / DPF — **TBC, verify** |
| Browser/proxy/unblocker (Quarry vendors) | crawled/retrieved content, target metadata | **TBC — verify per vendor** | TBC | Default-deny; per-processor approval; **TBC, verify** |

## 3. Assessment of the destination legal regime (US — principal residual)

- US surveillance law (**FISA 702**, **EO 12333**, **CLOUD Act**) can compel US
  providers to disclose data, including data held by EEA subsidiaries.
- The **EU-US Data Privacy Framework (DPF)** (2023 adequacy decision) provides a
  transfer route for DPF-certified US recipients, **but it is under active legal
  challenge ("Schrems III")** and may be invalidated, as Privacy Shield and Safe
  Harbor were before it.
- **Therefore:** Velion does **not** rely on the DPF alone and does **not** claim
  immunity from US access. The CLOUD Act exposure is treated as a **disclosed
  residual risk** even where data is stored in Sweden/Norway, because Microsoft's
  US parentage keeps that access legally conceivable.

## 4. Supplementary measures applied

Per EDPB Recommendations 01/2020, Velion applies a combination of technical,
organisational, and contractual measures:

**Technical**
- **EU/EEA residency** for processing and storage (Sweden Central default; Norway
  East for Tier 2). Reduces exposure surface (does not eliminate parent-company reach).
- **Encryption** in transit (TLS) and at rest.
- **Zero Data Retention** at the model layer — Azure OpenAI does not retain or
  train on prompts/completions, so there is materially less data at rest at the
  model provider to be compelled.
- **`zdr_ephemeral` class** — content that is never stored, cached, embedded, or
  logged, removing it from any at-rest disclosure target.
- **Data minimisation in logs/audit** — identifiers, hashes, and policy decisions
  only; no document bodies, prompts with private content, tokens, or screenshots.
- **Pseudonymisation / classification before persistence** where policy requires.

**Organisational**
- **Default-deny third-party processing** for `customer_private`, `personal`,
  `sensitive_personal`, `credential_or_secret`, `zdr_ephemeral`.
- Strict **least-privilege access controls** and **per-organisation isolation**.
- **Processor registry** — external calls are gated on an approved-processor
  record carrying DPA, residency, subprocessors, retention, and incident contact.
- Government-access **transparency / challenge commitments** to be secured
  contractually from subprocessors (**TBC — verify in each DPA**).

**Contractual**
- SCCs (and DPF where the recipient is certified) with each non-EEA-resident
  subprocessor; commitment to notify and to legally challenge disproportionate
  access requests where lawful.

## 5. Per-subprocessor conclusion

| Subprocessor | Effective protection level | Conclusion |
|--------------|----------------------------|------------|
| Microsoft Azure (hosting + Azure OpenAI, Sweden Central) | EEA residency + ZDR + encryption + access controls; **CLOUD Act residual disclosed** | Proceed with supplementary measures; residual disclosed to customers. |
| Microsoft Azure — **TTS (East US 2)** | US processing, no EEA residency | **Do not use for protected classes**; remediate (relocate to EEA). |
| Resend (email) | **TBC — verify region, DPA, SCC/DPF** | Cannot conclude until verified. |
| Twilio (SMS/2FA) | **TBC — verify region, DPA, SCC/DPF** | Cannot conclude until verified. |
| Quarry browser/proxy/unblocker vendor(s) | **TBC — identify vendor(s), region, contract** | Default-deny; per-processor approval before use. |
| Other model vendors via Model Plane | **TBC — confirm ZDR + EEA per vendor** | Only the Azure OpenAI Sweden Central path is confirmed today. |

## 6. Overall conclusion (draft)

- The Sweden Central / ZDR / EEA-residency design, plus the supplementary measures
  above, supports a defensible transfer position for the **core model-inference and
  hosting flows**, with the **US CLOUD Act exposure expressly disclosed as a
  residual** rather than claimed away.
- **TTS (East US 2)** is a live gap; protected-class content is excluded pending
  remediation.
- Email, SMS, and browser/proxy subprocessor transfers are **not yet concludable**
  and are marked **TBC — verify**.
- Final sufficiency of measures, and any Art. 36 implications, are for Norwegian
  privacy counsel and the controller to confirm.

---

*This Transfer Impact Assessment is a draft pending Norwegian privacy counsel review.*
