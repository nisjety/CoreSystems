# T1 / Category 10: Governance, security and compliance (cross-source comparison)

Inputs: 163 reference records (buzz 29, grokbot 34, qm 41, slack 31, teams 28) and 20 Verevon baseline records. Output: 48 clusters; 154 records assigned and 9 excluded as engineering-process trivia. Coverage was checked with node (154 + 9 = 163). Clusters file: `10-governance-security-compliance.clusters.json`.

## Shared baseline (table-stakes and common)
- **Server-side tenant binding** (4/5). The server decides which tenant a request belongs to; the client can never pick it. Verevon already ships this (SRC).
- **Enterprise SSO, SCIM and identity federation** (4/5). Verevon has this only partly: SSO is behind a flag and there is no SCIM.
- **Secret and PII redaction in logs, telemetry and agent output** (4/5). Not in the baseline. A preliminary read found `execution-core/src/scrub.rs`.
- **Telemetry that waits for consent** (4/5). Not in the baseline.
- **Retention policies** (3/5, the standard at Slack and Teams). Verevon has none; it only has ZDR, which is all-or-nothing.
- **Desktop-client items** (3/5): admin-locked settings, secrets at rest, shell hardening and signed updates. Verevon is a web app with no Electron or Tauri found, so only the org-level admin-lock part applies.

## Best of breed
| Capability | Best source | Why |
|---|---|---|
| Tenant binding | buzz (SRC q5) | The tenant is bound from the host before any handler runs. A typed `AdmittedTx` and DB write fences enforce it twice. |
| Formal isolation | buzz (SRC q5) | Isolation is specified in TLA+/Tamarin, the specs are mutation-tested, and runtime traces are replayed against the model. |
| Audit export | slack (DOC q4) + qm (SRC q3) | Slack has the Audit Logs API to SIEM. QM shows idempotent `recordOnce` and audited admin reads. |
| Moderation loop | buzz (SRC q4); teams for end-user UX | Private reports feed a queue. Actions are signed, leave tombstones and notify the people involved. Bans also apply to agents the banned person owns. |
| Retention | slack (DOC q5) | Rules at org, workspace, channel and DM level, with an org lock and owner approval. |
| eDiscovery, hold, barriers | teams (DOC q5) | Purview covers eDiscovery, legal hold, communication compliance and information barriers. |
| Residency and sovereign clouds | teams (SRC q5) | Nine cloud configs; telemetry is routed by region. |
| Authority provenance | grokbot (SRC q5) | Every agent request says whether a standing grant or an approval id allowed it, and that id can be retired. |
| Command policy | grokbot (SRC q4) | Shell commands are parsed into a syntax tree before policy checks; refusals carry typed reasons from one permission service. |
| Sandbox | grokbot (SRC q4) | Declarative read-only/read-write policies, cgroups and symlink-safe confinement. |
| Egress in shared rooms | qm (SRC q3) | A room's allowlist is the intersection of every member's lists; denies are combined from all members. |
| Prompt-injection screening | qm (SRC q3-4) | Content is labelled by provenance, screened in off/observe/enforce modes, and a new policy can be tested on past screenings before it is turned on. |
| Agent information-flow control | buzz (SRC q3, draft only) | Each agent instance is bound to one audience; memory carries audience labels and membership epochs. |
| CSP and Trusted Types | teams (SRC q5) | Trusted Types, nonces, a report-only shadow policy and an SRI manifest. |
| Desktop IPC gate | slack (SRC q5) | IPC calls are checked for frame, process, origin and schema (relevant only if Verevon builds a desktop client). |

## Differentiators (2+ sources, not most)
- Admin break-glass for agent computers (grokbot, qm).
- Human-only authority walls (qm, grokbot).
- Room egress intersection (qm, grokbot).
- Information-flow control for agents (buzz, grokbot).
- Separate rate limits for agents and humans (buzz, qm).
- Agent and model allowlisting (teams, qm).
- Rules on what AI may see (slack, teams).
- Supply-chain controls (buzz, qm).

Single-source items worth taking:
- QM's security posture levels.
- buzz's bans that also stop the banned person's agents.
- GrokBot blocking persistent features when an org uses zero data retention.

## Verevon's biggest gaps (tier "priority")
1. **Moderation** (absent, sig 4). There is no report, queue, tombstone or block for rooms. Build it as an Application Plane projection with a signed Control decision.
2. **Retention** (absent, sig 4). Reuse the per-plane receipts from the deletion saga, with legal hold taking precedence.
3. **Audit export and admin audit view** (partial, sig 4). Have audit-core expose org-scoped events, with decision ids, to a SIEM.
4. **SSO GA and SCIM** (partial, sig 4). Deprovisioning a person must also suspend that person's agents.
5. **ZDR enforcement in every plane** (partial, sig 5). Session Core ignores the ZDR claim, Novu still receives content, and the typed refusal UI is uncommitted.
6. **Information-flow control for personal agents across rooms** (partial, sig 5). Agent memory needs audience labels, plus a declassify step the owner signs.
7. **Sandbox enforcement for agent computers** (unknown, sig 4). Code exists in `execution-core/src/sandbox.rs`, but it falls back to a passthrough unless bubblewrap works on the host. It must fail closed in production.

Recommended next:
- Ban cascade to agents.
- Per-Space posting rules.
- Posture levels.
- Command-policy syntax-tree parsing.
- Room egress intersection.
- Treat room members' messages as external content for injection screening.
- Sharing opt-in.
- Shared-Space deletion.
- Agent and model allowlist.
- Admin-locked settings.
- Value-based secret masking.
- Agent-specific rate limits.
- SSRF checks.
- Stricter CSP: Trusted Types, narrower connect-src, report-only policy.
- Fix the swallowed 403 on shared reads.

Skip: customer-managed keys, attestations (organisational work), desktop-only hardening and updates, recording and biometrics, age gate.

## Verevon strengths (tier n/a)
- **Signed authority** (C-020). Ed25519-signed, short-lived Control decisions are bound to the subject, org, Space, audience revision, privacy policy, payload digest and idempotency key, and effect policies deny by default. This is stronger than GrokBot's grant/approval fields and QM's walls.
- **Rules on what AI may see** (C-016). Audience snapshots that cannot change, plus the rule that "Space access never widens an owner-plane ACL", are more verifiable than Slack's or Teams' policy statements.
- **Tenant binding** (C-001). The org comes from the verified session and is re-checked on every call, and the gateway strips 12 forged authority fields. This is at par with buzz and ahead of QM, which serves one org per deployment.
- **Likely strengths, still to verify:**
  - Browser-action risk gates (`browser_risk.rs`, consistent with "Model proposes, Quarry executes").
  - Deterministic injection defence plus a sampled shadow classifier (`moderation.rs`, `semantic_screening.rs`).
  - Fail-closed capability policy (`capability_policy.rs`).

## Caveats
- Slack and Teams compliance items (retention, eDiscovery, EKM, residency, attestations, AI boundaries) are mostly DOC provenance. buzz and QM "absent" notes are self-reported (INF/DOC). Absence records were not counted towards prevalence.
- 32 clusters are marked `unknown`; most of them have no baseline record.
  - In several of them I added pointers from my own quick greps (labelled "preliminary"): scrub.rs, moderation.rs, sandbox.rs, capability_policy.rs, browser_risk.rs, the nginx CSP, gateway rate_limit.rs, Quarry browser_egress_proxy.rs, cost-core, and an integration-corev2 scim_tokens migration that is probably outbound.
  - These need a code-verification pass before their status changes.
  - For the CSP cluster my read suggests "partial", but the status stays "unknown" because no baseline record covers it.
- The desktop-client clusters (shell, updates, secrets at rest, trusted UI) assume Verevon stays web-only. This is inferred from verevonv3/package.json and the absence of a tauri.conf.json.
- Prevalence counts only the five analysed sources. Slack/Teams moderation and posting controls exist in the real products but were not recorded in this category, so some counts understate the incumbents.
