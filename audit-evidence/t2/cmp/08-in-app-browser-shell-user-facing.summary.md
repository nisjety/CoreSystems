# T2 / Category 8: In-app browser shell (user-facing), cross-source comparison

Inputs: 112 reference records (Aside 58, Codex in-app browser 52, Firecrawl 2). All are SRC-tagged and come from analysis of shipped bundles; 16 are medium confidence. There are also 34 Verevon baseline records. Output: 51 clusters, with 110 records assigned and 2 excluded (Aside's default-browser nudges and Windows Mica styling). Firecrawl has no UI, so it only shows up in the live-view and replay clusters. For it, "table-stakes" means present in both browser products (Codex and Aside).

## Shared baseline (table-stakes)
- **Tab strip, navigation, page utilities.** Covers tab create/close/duplicate/reopen, back/forward/hard reload, and find/zoom/print/mute/DevTools (C-002, C-011, C-013).
- **Profiles and persistent logins** (C-015). Verevon ships this and is ahead.
- **Chat beside the page** (C-008). Aside uses a side panel. Codex uses a docked browser with a floating composer.
- **Agent-control indicator, live view, take-over** (C-017). Verevon ships this.
- **Screenshots** (C-031). **History/bookmarks** (C-035). **Clear data/privacy** (C-039). **One settings hub** with a per-site agent-permission matrix (C-041).

## Best of breed
| Capability | Best source (records) | Why |
|---|---|---|
| Hardened page hosting | Codex (iab-028/029) | Real Chromium. The main process rewrites every guest attach (no popups, no Node, fixed partition) |
| Address-bar suggestions | Codex (iab-036) | Token-based start/accept/deleteMatch service that can be re-implemented server-side |
| Task-scoped tab lifecycle | Codex (iab-031/034/035) | Tabs keyed by thread. History snapshots. A 32-page live budget with protection rules, then cold restore |
| Live view + takeover contract | Firecrawl (api-096) | Separate read-only and interactive live-view URLs per session; Codex has the best overlay UI |
| Approval contract / UX | Codex (auto-064) / Aside (aca-076) | riskLevel, display params, session/always persistence; keyboard card with "do this instead" redirect |
| Page context to chat | Codex (iab-059) / Aside (bp-017) | @tab mention checked fail-closed against tab id + URL; rich selection/region element payload |
| On-page annotation | Codex (iab-060/061) | Comment payload carries role, name, selector, path, rect, nearby text and frame; quick vs batch flows |
| Page-originated content gate | Codex (iab-064) | Page metadata or suggested prompts are shown to the user and labelled; the model gets only what the user submits |
| Secret redaction at capture | Aside (bp-041) | Input-type, autocomplete, label and secret-pattern rules plus a Luhn check, applied before storage |
| Recording/replay | Firecrawl (api-097) | Server-side HLS replay, refused under ZDR |
| Attention notifications | Aside (aca-059, bp-097) | The UI reports which sessions are visible, so it can skip notifying about a session the user is looking at |

## Differentiators worth noting
- Codex: co-browsing in hidden agent tabs (C-019), a visible agent cursor that the click waits for (C-018), and a site annotation API (C-027).
- Codex also has a visual CSS-diff design editor (C-028), admin-managed and synced agent policy (C-042), and localhost port-forwarding (C-043).
- Aside: AI tab tidy (C-005), Remix per-site scripts (C-050), ambient activity context (C-047), a full password vault (C-038), and skill/MCP install for external agents (C-051).

## Verevon's biggest gaps (priority tier)
1. **C-001: production session start is blocked.** Prod-mode Quarry requires a BrowserBroker grant. Nothing issues durable grants, and grants are held in memory only. Fix it in Model Plane BrowserBroker and the gateway's bearer forwarding. Effort M.
2. **C-011: the user cannot browse freely.** Each session is locked to the first host, 12 steps and 120 s. The fix is a separate human-session policy:
   - an idle timeout instead of the step/time budget
   - navigations the user starts extend the grant allowlist
   - navigations the agent starts across domains stay behind the existing HITL gate
   - Quarry egress/SSRF policy is unchanged.
3. **C-019: no co-browsing.** The AI run uses its own hidden lease. Bind agent actions to the user's session so the Model proposes and Quarry executes in an agent-flagged tab. Effort L.
4. **C-008: no chat-beside-browser surface.** The browser lives only in the Crawl composer, and the chat panel is read-only. Make BrowserChrome a dockable chat panel.
5. **C-024: no way to point the agent at the page.** Add a Quarry hit-test that returns an AX-ref element descriptor and crop, plus @tab mentions with a staleness check that fails closed. This also unblocks annotation (C-026, which today is a fake anchor while the capability is advertised) and context-menu actions (C-025).

Recommended next: omnibox through Verevon search (C-009), approval persistence and redirect (C-022), run-scoped tab persistence (C-014), agent cursor (C-018), downloads into quarantined artifacts (C-034), capture-time secret redaction in Quarry (C-048), browser settings hub (C-041), Control Plane org browser policy (C-042), and attention notifications in Application Plane (C-023).

Skipped as poor fits: vertical tabs, tab groups, tab search, and the browser-side chat list (desktop-browser chrome). Also skipped:
- extensions, Remix scripts and the design editor: they conflict with the no-evaluate sanitizer and with Quarry being the executor
- password vault and cookie import: credential custody (takeover login into a scoped profile covers the need)
- ambient activity capture: conflicts with ZDR.

## Verevon strengths (keep)
- **Remote isolation.** The page never runs in the SPA or on the user's machine, and the contract is "frames out, typed input in" (C-001 engine side).
- **Take-over authority sits in Quarry.** Agent actions get 409 during takeover, and human input automatically switches to takeover (C-017).
- **Profiles are better governed:** scope, restore probe and ZDR (C-015).
- **ZDR goes further than the references' incognito modes** (C-049). It is propagated end to end: no persistent profiles, artifacts withheld, vision skipped, history purged.
- **The timeline/evidence drawer with receipt replay** goes beyond a plain transcript (C-029). The DevTools drawer has network and vision views (C-013). Browser risk categories are backed by a deterministic check that fails closed (C-022).

## Caveats
- 18 clusters have Verevon status `unknown` because no baseline record covers them (for example C-012, C-023, C-024, C-041, C-042, C-048). Where I did a light read-only grep it is tagged INF; a later step must check these in code. The C-048 grep found only URL redaction in Quarry.
- Reference records are SRC from static bundle/binary analysis, not runtime tests. Behaviour claims flagged as INF in the source records, such as Codex quick vs batch annotate, stay inferences.
- Some prevalence classes are adjusted beyond the record count. C-001, C-002 and C-011 have only Codex records, but Aside is a Chromium fork (INF), so they are treated as common or table-stakes. Each such cluster says so in its description.
- The baseline's AI-path breakages (missing inference/browser bearers, LLM planner off by default) belong to category 9. They are only referenced here where they block shell features.
