# T1 / Category 11 — Client shell: cross-source comparison

181 reference records (buzz 20, grokbot 28, qm 16, slack 57, teams 60) were grouped into 41 clusters. 178 are assigned and 3 are excluded as trivia (Teams internal reactive core, PE manifest flags, Microsoft work/consumer convergence). Verevon baseline: 8 records (v-space-ui-103..109, v-space-backend-106), plus spot checks in `apps/Frontend Plane/verevonv3` at commit afb83928 (2026-10-10). Clusters file: `11-client-shell.clusters.json`.

## Shared baseline (table stakes, 4 or more of 5 sources)
- **Native desktop shell around the web UI** (all 5: Electron for Slack, grokbot and qm; WebView2 for Teams; Tauri for buzz). Verevon has none. SRC v-space-ui-109; `index.html` has no manifest.
- **OS protocol handler with single-instance link forwarding** (all 5). Verevon has HTTPS routes only.
- **Entity deep links and URL-synced view state** (4). Verevon is *partial*: Space and tab links exist; message and run permalinks were not found.
- **Narrow, typed native bridge** (4), **auto-update with channels** (4), **signed packaging** (4). Verevon has none of these because it has no shell.
- **Feature flags, remote config and preview rings** (4). Verevon is *partial*: plan entitlements only.
- **Keyboard shortcuts and command palette** (4). Verevon is *partial*: basic keys work, but there is no palette or quick switcher.
- **Theming with high contrast and branding** (4). Verevon is *partial*: system/light/dark is saved on the server; there is no forced-colors support and no org branding.
- **Accessibility** (4). Verevon has *shipped* this for the Space surface.

## Best of breed
| Capability | Best source | Why |
|---|---|---|
| Thin shell | Teams (teams-shell-and-native-001) | Native code owns the OS layer and the web code owns the UI. It handles a missing WebView and can fall back to the web version. buzz's Tauri shell (buzz-product-162) fits Verevon's Rust stack best. |
| Native bridge | Teams PIE (004/005) + Slack Joi schemas (slack-product-surface-146) | One versioned RPC envelope over capability modules. Every call is checked against a schema, and failures are reported. |
| Protocol links | Slack (shell 002-004) + buzz cold-start queue | One URL grammar on every OS. Links are held until the UI is ready. |
| Entity routing | Teams host command routing (020), qm `?seq=` (qm-product-124) | Toasts, links and the hub share one (entity, action) command path. Message links page back through history until the target is loaded. |
| Auto-update | Slack should-i-update (019/022) + grokbot (agent-product-104) | The server decides when to update and attributes update failures. grokbot never installs while an agent is working. |
| Version gating | grokbot (agent-runtime-079) | Each feature can require a minimum client version, and the server returns an update-required hint. |
| Command palette | grokbot (agent-product-103) | Every shell action is a registered command with an id and hotkey. Teams adds custom key assignment. |
| Quick composer | grokbot desktop mount panel (agent-product-106) | An always-on-top mini composer that sends to a chosen Bot. |
| Crash recovery | Teams error curtain (023) + Slack fatal/hang dialogs (035) + grokbot 30 s restore watchdog (170) | Shows a diagnosis-specific failure screen with restart, repair or open-on-web. |
| Error registry | grokbot SAND-E (agent-product-120) | 108 codes, each with domain, retryable flag and source anchor. |
| Localization | Teams 63 locales (033); grokbot RTL (agent-runtime-085) | Fallback map, and the app locale can differ from the OS locale. |
| Accessibility | Slack (product-surface-141); buzz tests (product-169) | Detects assistive tech and supports F6 region cycling. buzz's tests assert accessible names. |
| Desktop sign-in | qm PKCE handoff (architecture-113) | A single-use code bound to a proof key; credentials never enter the app. |
| Installable web | qm phone mode (architecture-159) | One build covers phone and desktop layouts and can be installed (but has no push). |
| Network compatibility | grokbot Network Debugger (agent-product-108) | Gives IT guidance so SSE and HTTP/2 survive proxies and TLS inspection. |

## Differentiators worth noting
- **Agent-aware shell behaviour.** grokbot blocks updates while an agent runs. buzz shows running agents in the tray. grokbot and Slack offer a floating quick composer or command center.
- **Pop-out windows** (Teams 014/015). Useful for watching an agent's run next to the Space.
- **Cross-workspace unread and multi-account** (Teams MTMA, Slack rail, buzz community rail).
- **Diagnostics kits** (Slack net log and log upload; Teams report-a-problem). Startup performance from a bundled local index (Teams).
- **Local agent runtimes** bundled in the desktop app (buzz sprig, grokbot local-exec). Both show platform gaps on Windows.

## Verevon's biggest gaps (proposed tiers)
1. **PRIORITY: installable PWA with Web Push and app badge** (C-t1-11-035, sig 4, M). There is no manifest, no service worker and no push, so agents that finish while the tab is closed cannot reach the user. Push payloads should carry ids only (for ZDR). The Application Plane fans out the notifications.
2. **PRIORITY: root error boundary and boot watchdog** (C-t1-11-024, sig 4, S). The code itself says there is no root ErrorBoundary (`src/shared/lib/create-resource-compat.ts:5`), so one render error blanks a Space that is open all day.
3. **PRIORITY: message and run permalinks with one entity router** (C-t1-11-005, sig 4, M). Links should be checked against the viewer's access before they open, without widening owner-plane ACLs.
4. **Recommended:**
   - Client version gating (no build id is exposed today; S).
   - Title and app badge from the existing navbar unread counts (S).
   - A command palette built on the typed ActionDescriptor registry, so humans and agents share one action vocabulary.
   - Finish nb/en localization (activity grammar is hard-coded Norwegian).
   - Org-scoped flags and preview rings in the Control Plane.
   - An ids-only "report this run" flow.
   - A Tauri shell later (L), together with PKCE desktop sign-in, a narrow bridge and the updater.
5. **Skip:**
   - Desktop-local agent execution: it conflicts with "Model proposes, Quarry executes" and with server-side audit and ZDR.
   - VDI, chaos drills, native downloads and spellcheck.
   - App zoom, the native menu, launch at login and settings deep links.

## Verevon strengths (keep)
- **Same-origin Rust BFF** (v-space-backend-106). The browser holds no upstream credential. This equals qm and is safer than shells that keep tokens locally, and any desktop shell should reuse it.
- **Space accessibility.** aria-live rows announce streaming agent activity, status marks do not rely on colour, focus moves through tabs with the arrow keys, and reduced motion is respected. This is at or above buzz and qm.
- **Error handling.** The gateway returns a `{code, message}` envelope, the client maps codes to Norwegian and English copy, and there is a ZDR refusal catalogue. This is close to grokbot's registry; it lacks retryable flags and a coverage test.
- **Bilingual UI.** Ahead of buzz and qm, which are English only.
- **Server-side agent computers.** They are OS-independent, so there are no Windows gaps like grokbot and buzz have.

## Caveats
- Several clusters are marked "unknown" and need a code check:
  - pop-outs;
  - report-a-problem;
  - SSE behaviour behind proxies;
  - performance telemetry;
  - how media and avatars are served (Range support and caching);
  - release notes.
- The preference-store and feature-flag status rests on limited greps (confidence low or medium).
- Teams update-staleness gates (teams-shell-and-native-022) are DOC only. buzz's tray/shortcut posture (buzz-architecture-102) is INF; its confidence was not upgraded.
- Absence of the PWA is confirmed in code (`index.html`) and also stated in a dated internal audit (COPILOT_APP_PARITY_2026-09-28, DOC).
- Prevalence counts native-shell records. qm and buzz are lighter shells, and Slack/Teams mobile apps are not represented in this category's records. Commodity native features (zoom, menu, auto-start) are therefore classed "common", not "differentiator".
