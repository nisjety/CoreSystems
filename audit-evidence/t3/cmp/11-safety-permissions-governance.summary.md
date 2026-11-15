# T3 / Category 11: Safety, permissions & governance (cross-source comparison)

There are 367 reference records from 12 sources: 364 are assigned to 61 clusters and 3 are excluded as trivia. They are compared against 37 code-verified Verevon baseline records. Full detail is in `11-safety-permissions-governance.clusters.json`. Provenance in this category is mostly SRC. The four web benchmarks are DOC only, so the web-only clusters below (Lockdown, Elevated-risk labels, Compliance API) carry DOC confidence.

## Shared baseline (table-stakes, 7 or more of 12 sources)
- **Named permission modes** (ask / accept-edits / plan / auto / full) (C-001). Verevon: partial. Posture is resolved on the server and can only be tightened, but users get no named mode selector.
- **Model-based auto-review ("approve for me")** (C-006). Verevon: absent. Risk is decided by a substring match on tool names.
- **Agent-browser site governance** (C-012). Verevon: partial. The broker grant carries `AllowedDomains`, but the chat path never mints grants, so agent browser runs fail closed.
- **Prompt-injection defences** (C-022). Verevon: partial but strong. It has trust-class provenance and a fail-closed scan before truncation. The scan is a lowercase substring match with no Unicode/homoglyph normalisation, and the semantic pass runs only in shadow mode.
- **Credential storage** (C-027). Verevon: unknown.
- **Admin policy registry and locks** (C-035). Verevon: partial.
- **Identity, RBAC and spend limits** (C-048). Verevon: shipped, in the Control Plane.

## Best of breed
| Capability | Best source | Why |
|---|---|---|
| Mode vocabulary | Codex | Typed modes, with a granular approval config for each effect class |
| Auto-review | Codex Guardian v2 (+ Claude auto mode) | Scores the trajectory asynchronously against a threshold and gates later actions; fails closed |
| Classifier policy text | Claude (unique) | Intent-clearing "must name" rules; the classifier never sees tool results; forced-card escalations |
| Approval decisions | Codex JSON-RPC + Claude can_use_tool | Decision scopes once / session / amend; a one-click "always" is reduced to session scope |
| Approval cards | Claude anti-decoy + Codex templates | Recipients shown first; duplicate "to" fields flagged; plain-language template for each tool |
| Unattended runs | Claude scheduled tasks + Hermes headless modes | Stored rules with expiry; some tools can never be delegated; deny by default |
| Browser governance | Claude (+ Perplexity Comet) | Consent per site; admin allow/block lists; DNS-pinned private origins |
| Model-blind secrets | Manus | `{{secret:x}}` substituted by the executor, bound to the live frame origin |
| Memory write governance | Hermes (unique depth) | Staged, entry-pinned writes; review fork limited to an allowlist; read-before-write |
| Policy registry | Claude | Schema-as-source with a fail-closed value per key; config can only tighten |
| Connector tool policy | ChatGPT tiers + Claude never-allow set | Graduated read / low-risk / all; write tools default to "ask" |
| MCP gate | Claude (unique) | Single-use consent receipts bound to the SHA-256 of the exact arguments |
| Side-effect taxonomy | Manus | Five declared levels plus MCP hints drive policy |
| Sandbox transparency | DeepSeek | Reports full or partial enforcement; tells a denial apart from a runner failure |
| Privacy modes | Cursor | Matrix of purpose by data classification; a RedactedString type |

## Differentiators worth noting
- Information-flow and data classification (Manus, Antigravity), C-009.
- Lockdown mode (ChatGPT), C-003.
- Credential-injecting egress proxies (Antigravity, Hermes), C-030.
- Denied-action equivalence (Antigravity), C-008.
- Step-up passkey before approvals (Codex), C-049.
- Explicit trust model: "the OS is the only boundary" (Hermes), C-057.

## Verevon's biggest gaps (priority tier)
1. **C-012 Browser grants and site consent (sig 5).** Mint real broker grants per run from Control/Space decisions. Add a per-site consent table and admin allow/block lists.
2. **C-014 Approval decision scopes (sig 5).** Approvals offer only Approve/Reject today. Add "allow for this run" and amendable rules. Editable "don't ask again" is explicitly not built.
3. **C-044 ZDR is unusable (sig 5).** The design is the best seen, but no provider is attested, so every ZDR turn is refused and Temporary chat is off. The fix is a small operator step.
4. **C-006 Auto-review classifier (sig 4).** This is the lever for Do-mode and scheduled agents. Build it with Claude's policy discipline (C-007).
5. **C-061 Declared side-effect taxonomy (sig 4).** Replace the substring classifier. The frontend action registry already has risk and reversibility metadata.
6. **C-038 Per-connector, per-tool allow / ask / block (sig 4).**
7. **C-017 Unattended run approval contract (sig 4).** Scheduled steps are no-tool calls today.
8. **C-047 Audit trail delivery (sig 4).** The outbox fix is built but not deployed; 4,992 events were never delivered.

Recommended next tier:
- Mode selector (C-001) and elevated-risk UX (C-002).
- Approval-card templates and anti-decoy display (C-015).
- Memory write scanning and staging (C-021).
- Injection normaliser, with the semantic pass moved to enforcing (C-022).
- Hooks wired into Do mode (C-042).
- Egress tiers (C-031) and sandbox denial transparency (C-034).
- Admin policy registry (C-035).
- Retention windows and legal hold (C-045).
- Never-auto tool class (C-018).
- Approval notifications (C-016).
- Moderation UX (C-023).
- Signed releases (C-056).

## Verevon strengths (equal or ahead; tier n/a, keep)
- **Service authority and tenancy (C-054):** fail-closed JWKS on every service, `authorize_rpc` on every RPC, per-audience delegated bearers. This is ahead of all references. Remaining gaps: NATS has no mTLS, and verified worker delegation is missing.
- **ZDR design (C-044):** a signed claim that cannot be downgraded, typed localised refusals, retention receipts.
- **Per-call escalation with justification (C-013).**
- **Kernel sandbox (C-033)** and **server-side file confinement (C-019).**
- **SSRF guards (C-032).**
- **Redaction (C-028).**
- **Opaque-origin artifacts (C-052).**
- **Approvals:** durable, restart-safe, with encrypted exact-effect descriptors and postcondition re-checks (stronger than T3 Code's in-process approvals).
- **Capability-core as a central decision point:** Allow requires signed evidence. Claude's external policy endpoint is still unreleased.
- **Dreaming memory extraction:** closed slots and an `extractor:llm` provenance tag.

## Skipped as not fitting Verevon
- Desktop computer-use guardrails (C-024; parked by the owner).
- GPO/MDM config delivery (C-036).
- Device pairing (C-055; future Channel Plane).
- Config-change LLM review (C-010).
- Prompt confidentiality (C-011).
- Quit guards (C-059; runs are server-side).

## Caveats
- **What "shipped" means.** It means present in source and wired into a reachable path, not deployed or proven live. Status docs say the source is not a production release.
- **"Unknown" clusters still need a code check:** C-011, C-016, C-020, C-027, C-029, C-034, C-046, C-049, C-053, C-058, C-060.
- **Verified in code during this pass:**
  - `browser-broker` `AllowedDomains`
  - `MpNetworkPolicy::AllowDomains`
  - the `dream_extractor` untrusted-content design
  - `scan_injection` being a substring match
- **Web-only sources (DOC).** ChatGPT, Claude.ai, Manus web and Perplexity are documented behaviour, not observed code.
- **Antigravity's permission-engine record (architecture-066)** is DOC-backed for its grammar.
- **Prevalence undercounts web sources.** The web benchmarks expose fewer internals, so some clusters are likely more widespread than shown.
- **Effort estimates are rough** and assume the existing Model Plane seams (capability-core policy, session-core approvals) are reused.
