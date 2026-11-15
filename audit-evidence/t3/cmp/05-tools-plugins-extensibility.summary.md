# T3 / Category 5: Tools, plugins and extensibility (comparison summary)

Input: 304 reference records from 12 sources and 40 code-verified Verevon baseline records. Output: 49 clusters. 300 records are assigned to clusters and 4 are excluded as trivia (`.clusters.json`). Provenance tags: SRC = read in source, REA = reverse-engineered, DOC = documentation, INF = inference.

## Shared baseline (table stakes)

- **MCP client** (6 sources; app-side table stakes, since the web-only benchmarks cannot show runtime internals). The product connects to MCP servers, shows per-server and per-tool switches, and recovers when a server fails.
- **SKILL.md skill packages with progressive disclosure** (9 sources). Most sources have adopted the open Agent Skills format.
- **Plugin bundles** (7 sources). One manifest packages skills, MCP servers/apps, commands, subagents, hooks and templates.
- **Composer slash commands and mentions** (9 sources).
- **Public API/SDK and programmatic control** (10 sources).
- Common (4-6 sources): first-party connectors, user-added custom connectors, MCP Apps widgets, hooks, tool search, admin governance, skill libraries, custom agents, persistent instructions, product-as-tool, external channels.

## Best of breed

| Capability | Best source (records) | Why |
|---|---|---|
| MCP client runtime | Hermes (hermes-architecture-049/054) | Supervised per-server tasks; per-call recovery ladder (re-auth, session expiry, circuit breaker); tool-list swap that keeps the prompt cache (SRC q5) |
| MCP OAuth | Cursor (cursor-ai-052) | Refresh leases, offline_access, tolerance of transient errors (SRC q5) |
| Tool search | Hermes (hermes-architecture-046/047) | search/describe/call bridge; BM25 gated on the rarest query token (SRC q5) |
| Code mode | Codex (codexagent-plugins-035) | JavaScript in a V8 isolate calls the tools; DeepSeek PTC routes every sub-call back through the policy pipeline (SRC q5) |
| MCP Apps widgets | Codex (codexagent-plugins-048), ChatGPT (web-chatgpt-068) | Full implementation of the ext-apps standard; JSON-RPC over postMessage with CSP (SRC/DOC q5) |
| Hooks | Claude (claude-extensibility-077/089) | 33 events; handler types: command, HTTP, prompt and agent verifier (SRC q5) |
| Plugin package | Claude (claude-extensibility-056/137) | Typed userConfig, sha-pinned binaries, many extension points (SRC q5) |
| Supply-chain safety | Hermes (hermes-architecture-064) | About 570 SHA-pinned catalogue entries checked in CI, plus a kill list (SRC q5) |
| Admin governance | Claude (claude-extensibility-090/063) | Plugin-only lock for each surface; per-feature list of required egress hosts (SRC q5) |
| Per-tool approvals | Perplexity (web-perplexity-059) | Always allow / ask / never per tool, plus grants scoped to a thread (DOC q5) |
| Skill format | claude.ai (web-claudeai-047) | Open standard with 3-stage loading (DOC q5) |
| Executable skills | Claude (claude-extensibility-151) | Validated document skills for docx, pdf, pptx and xlsx (SRC q5) |
| Skill libraries | Manus (manus-product-089) | Personal, team, project and hub libraries; versions; project lock (SRC q5) |
| Product-as-tool | Claude (claude-product-092/040); T3 (t3code-062) | The app's own state is exposed as MCP servers; each setting declares whether the model may read or change it; per-session scoped token (SRC q5) |
| Secret brokering | Claude (claude-extensibility-057) | Vault-injected credentials, per-operation confirm rules, network allow-list (SRC q5) |
| In-chat connect | Hermes (hermes-architecture-048) | Typed state machine; the turn waits until every connection settles (SRC q4) |

## Differentiators worth noting

- Code mode (Codex, DeepSeek).
- Artifacts that call the model and connectors (Claude).
- Tunnels to private MCP servers (Manus).
- In-chat connection cards and suggested installs (Hermes, Codex, Claude).
- Credential proxy injection (Claude, Perplexity).
- Strict tool-output contracts (DeepSeek).
- Opening its own product to agents as tools (Claude, Codex, T3).
- Cross-vendor plugin compatibility through the agent-plugins.org manifest (Codex, Cursor, ChatGPT).

## Verevon's biggest gaps (priority tier, 6 of 49)

1. **MCP tools cannot be called** (C-t3-05-001, partial, sig 5). Registration, OAuth and staged disclosure exist. But plain chat denies `mcp__*` names, and in Do mode `trusted_capability_id` returns None, so the capability gate denies every MCP call (`model-gateway/src/sse.rs:2014`, `tool_loop.rs:3723`). Fix: map MCP tools to capability-core entries with a risk class taken from readOnlyHint.
2. **First-party connectors are not proven** (C-t3-05-004, partial, sig 5). Provider actions run only in Do mode, Outlook/Gmail/Drive are listings only, and nothing has run against a real mailbox.
3. **Per-tool approvals** (C-t3-05-012, partial, sig 4). There are no user-facing allow/ask/never settings and no thread-scoped grants. This is the prerequisite for writes through connectors and MCP.
4. **SKILL.md packages** (C-t3-05-027, partial, sig 4). Skills are text records only: no folder bundles, no scripts, no zip import.
5. **Instructions missing in Do runs** (C-t3-05-034, partial, sig 4, effort S). Org, Space and persona instructions apply only in Ask mode.
6. **Product-as-tool** (C-t3-05-037, partial, sig 4). Only 8 of the 168 typed actions can be executed by the model.

Recommended next, after the priority items:
- MCP Apps widgets
- tool search over all tools
- durable hooks in Do runs
- plugin manifest loader with supply-chain pins
- admin governance UI
- skill versions and Space libraries
- executable document skills
- "save as skill"
- custom agent presets with versions
- code mode
- secret proxy
- in-chat connect card
- user-authored slash templates (planned as I13)

Marked skip, with reasons in the JSON:
- local MCP/DXT (stdio is correctly quarantined)
- an in-process plugin substrate
- file-based config profiles
- OAuth apps and "Sign in with"
- platform APIs
- the t3.json project config

## Verevon strengths (tier n/a)

- A signed, fail-closed capability policy gate runs before every dispatch, which few references have (C-t3-05-011).
- One Capability registry with rollout state, scopes, audit log and health attestation (C-t3-05-008).
- MCP OAuth 2.1 with DCR, an encrypted vault and DNS revalidation (C-t3-05-002).
- A skill budget that degrades before dropping, plus reattach_skill (C-t3-05-028).
- Client-declared tools (C-t3-05-009).
- A compositional system prompt (C-t3-05-035).
- A composer menu that merges built-ins with backend skills and connectors (C-t3-05-032).
- OpenAPI with a generated SDK, gRPC and an AG-UI adapter (C-t3-05-046).

Keep all of these. Every new tool source (MCP, plugins, client tools) must map to a capability id so that it cannot bypass the gate.

## Caveats

- 12 clusters are `unknown` because no baseline record covers them. They are noted with what to check: MCP elicitation, MCP Apps, tunnels, supply chain, suggested installs and others. A light grep in this audit found no `resourceUri`, MCP elicitation handler or `agent-plugins.org` code (INF: absent).
- The grep did find MCP readOnlyHint parsing that treats annotations as untrusted (`model-gateway/src/mcp_jsonrpc.rs:56-109`).
- Baseline conflict: v-planned-decisions-071 says the MCP agent-loop bridge is shipped. The code-verified v-modelplane-086 shows the calls are denied, and this audit follows 086.
- Hooks, artifacts and channels are mapped from the narrative baselines, not from the category base.json, so their `baselineIds` lists are empty.
- The four web sources (ChatGPT, claude.ai, Manus web, Perplexity) are DOC-only. Clusters backed only by DOC records say so in their rationale: OAuth apps, platform APIs and parts of skill authoring. Prevalence undercounts runtime features that web products cannot reveal.
- The claude/web-claudeai and manus/web-manus pairs are the same vendors, so n can overstate independent adoption.
