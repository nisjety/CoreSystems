# T1 / Category 6: Apps & extensibility (cross-source comparison)

Input: 103 reference records (teams 23, slack 14, grokbot 23, buzz 18, qm 25) and 17 Verevon baseline records. Result: 37 capability clusters; 100 records assigned and 3 excluded as implementation trivia (Grok Bot RPC inventory, Teams Game Bar hook, Teams Copilot origin list). Clusters file: `06-apps-extensibility.clusters.json`.

## Shared baseline (table stakes, 3 of 5 sources)
- **Public API, SDKs and rate-limit contract** (C-001: Teams, Slack, buzz). Verevon: **partial**. It has a typed internal action registry and AG-UI, but no public token API, SDK or rate-limit tiers.
- **Platform slash commands** (C-013: Teams, Slack, buzz). Verevon: **partial**. `/` opens skills only.
- **MCP server registry with shared or per-user credentials** (C-020: Grok Bot, buzz, qm). Verevon: **partial**. Model Gateway has MCP with OAuth 2.1 and DCR, but nothing is scoped to a Space.
- **Workflow automation** (C-024: Teams, Slack, buzz). Verevon: **partial**. Schedules exist, but you can only create them in Settings.

Also present in both human-chat incumbents (Teams and Slack): an app store with manifests (C-007) and interactive cards (C-009).

## Best of breed
| Capability | Best source | Why (mechanism in short) |
|---|---|---|
| Agent self-API (C-002) | qm (+ buzz CLI) | Discoverable `/v1/apis` catalogue. Signed capability tokens are bound to session, run and execution, and each call re-checks authority. Buzz: a JSON-in/JSON-out CLI for every action. |
| Agent into Slack/Teams (C-005) | Grok Bot | One Slack app per bot, created by API, with a typed install-outcome enum (pending admin approval, insufficient scopes, conflict). Per-tenant Teams install. |
| Scoped consent (C-008) | Teams | About 106 plain-language permissions, each tied to one container (chat, team or user). The install dialog says that channel agents will ask before using an app's MCP tools. |
| Cards / blocks (C-009) | Slack Block Kit (Teams Adaptive Cards) | A validated block schema with action_id/value round-trips and a required plain-text fallback. |
| Forms and secret capture (C-010) | qm secret drop (+ Grok Bot widgets) | A one-time form whose value goes to the keychain, with take-on-redeem. The transcript records only that a secret was supplied. |
| Governed skills (C-015) | qm | Lifecycle draft → reviewed → published → archived; signed manifests; scope promotion; git packs pinned to a ref. |
| Plugin bundles (C-016) | Grok Bot | Reads both Cursor-style and Claude-style marketplace manifests, pins sources to a git sha, and has a variables schema (writeOnly marks secrets). |
| MCP runtime reliability (C-021) | Grok Bot | Reconnect with backoff, health probes, 2 min idle / 60 min total call limits, 24 h stale-while-revalidate discovery, typed errors with recovery hints, OAuth through a loopback relay. |
| Connected apps (C-022) | qm | Person-bound consent links, a callback that checks identity, per-call checks on owner, status and version, and no retry when an outcome is uncertain. |
| Webhooks that wake agents (C-023) | qm | Signature verifiers for each provider, delivery-id dedup, path filters, payload wrapped as untrusted data, runs with the owner's authority. |
| Workflows (C-024) | Slack (open alternative: buzz YAML) | Broad triggers and steps, run history, workflows drafted by AI from plain language. |
| Crons, loops, watches (C-025) | qm | Destinations, runAs, a fire log, loops with ledger, ship gate and governor, and watches that turn process output into new turns. |
| Shareable agent templates (C-018) | Grok Bot | Public or team visibility, versions you can activate, an export policy, import from a chat link. |

## Differentiators worth carrying
These are agent-centric capabilities that the human-chat incumbents lack:
- the agent self-API (C-002)
- forms and secret drop (C-010)
- governed skills (C-015)
- ecosystem-compatible plugin manifests (C-016)
- versioned agent templates (C-018)
- per-agent email (C-006)
- webhook-woken agents (C-023)
- agents published into Slack and Teams (C-005)

## Verevon's biggest gaps
Ordered by tier, then significance.

1. **PRIORITY: Agent self-API (C-002, sig 5).** The 14 `spaces.*` actions exist, but none is eligible for the model to call. Fix: let agents call them with run-bound grants from Control Plane, and send risky actions to the in-room approval card. Effort M, Model Plane.
2. **PRIORITY: Space-scoped MCP and connectors (C-020, sig 4).** Add per-room enablement, a tool allowlist per binding, and a choice between per-user and shared credentials (plan item S3.6). The backend already exists in Model Gateway. Effort M.
3. **PRIORITY: Webhooks that wake room agents (C-023, sig 4).** Ingestion Plane verifies and dedups each delivery, then sends a wake envelope down the same capability-core trigger path that schedules use. Control Plane decides each fire. Effort M.
4. **PRIORITY: Agent forms and secret capture (C-010, sig 4).** Add a `request_input` pause/resume tool. Secret fields go straight to the S3.4 credential broker, and only an opaque handle enters the run. This matters for ZDR. Effort M.
5. **RECOMMENDED:**
   - Room-created schedules and watches (C-025)
   - Scoped consent cards (C-008)
   - Connected-app consent links per Space (C-022)
   - Agents in Slack/Teams (C-005: XL, blocked on S4.4 and Channel Plane)
   - Slash commands from the action registry (C-013: S)
   - Cards (C-009)
   - Link unfurl and message actions (C-027)
   - Governed Space skills (C-015)
   - Plugin manifest compatibility (C-016)
   - Templates (C-018)
   - Per-agent email (C-006: the pipeline currently returns 503)
   - MCP health and timeouts (C-021)
   - Public API and tokens (C-001)
6. **SKIP:**
   - BYO local agent harnesses (C-019): they break ZDR and Model Plane authority.
   - Office/device integrations and native-shell extension APIs (C-030, C-031): there are no meetings and no desktop shell.
   - Vendor CRM channels (C-032).
   - Agent-added custom emoji (C-033): qm does it through the person's web session.

## Verevon strengths (keep)
- **`/` skill picker (C-014, shipped).** It is ahead of qm: skills are sent as structured `skill_ids` resolved on the server, with org/personal scope badges.
- **Plane isolation (C-036).** The plane boundaries match or exceed qm's rule that surface plugins never import core.
- **Authority model.** Binding policy is enforced when an agent is invoked, and Activity ties grants to receipts. This is stricter than Teams resource-specific consent (RSC), and it is the base for C-008 and C-020.
- **MCP connect.** One auto-detecting route with OAuth 2.1 and DCR. Arguments are checked against the tool schema before the call (`argument_repair.rs`).
- **Schedule governance.** A create decision bound to a template digest, plus a separate Control decision on every fire.

## Caveats
- Three clusters rest only on Slack DOC records: C-003, C-012 and C-032. Their confidence is capped at medium. C-001 (public API) and C-004 (third-party bots) lean mainly on Teams and Slack DOC records.
- Absence-only records do not count toward prevalence: buzz-product-079 (no app store) and buzz-product-080 (scoped tokens that are not enforced).
- Seven clusters are marked **unknown** because no baseline record covers them: C-011, C-017, C-019, C-032, C-034, C-035, C-037. A later step must check them in code.
- Some gap text includes spot-checks from this pass (SRC; not baseline records) that a later step should confirm:
  - integration-corev2 OAuth provider catalogue
  - the org plugin-pack Settings section
  - the bridge-core outbound webhook adapter
  - AG-UI client tools (up to 32)
  - Model Gateway argument repair and its 6 s / 10 s MCP timeouts
- Prevalence counts how many sources have a record. Teams and Slack were analysed as installed desktop clients, partly from strings and docs, so "absent" for a reference may only mean "not observed".
