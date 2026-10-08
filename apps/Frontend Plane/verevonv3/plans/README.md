# UI/UX plans — /chat, /support, /insights, /knowledge

Source: `docs/UI_UX_AUDIT_2026-09-29.md`. Each plan is self-contained (exact
files, current code, target code, steps, boundaries, verification). All were
written at commit `83927eda`.

**Status 2026-09-29: plans 001–010 are applied in the working tree and are NOT
committed.** The clean record of exactly what each plan changed is in
`plans/applied/NNN.diff` (per-plan, before vs after). The working tree also
holds unrelated uncommitted work from other sessions, so `git diff` alone is
not a clean record; `plans/applied/combined-includes-other-sessions.diff` shows
the combined diff of the ten main target files and includes that other work.

| # | Plan | Severity | Status | Repair pass |
| --- | --- | --- | --- | --- |
| 001 | [Insights active chips readable](001-insights-active-chip-foreground.md) | HIGH | DONE | no |
| 002 | [Visible focus ring token](002-focus-ring-token.md) | HIGH | DONE | no |
| 003 | [Remove stale Knowledge folder-card override](003-knowledge-folder-card-override.md) | HIGH | DONE | yes |
| 004 | [Knowledge tree indent and collapse](004-knowledge-tree-indent.md) | HIGH | DONE | no |
| 005 | [Keyed lists: Support tickets and Chat transcript](005-keyed-lists-support-and-chat.md) | HIGH | DONE | no |
| 006 | [Delete chat launch wash and dock entrance](006-chat-launch-wash-and-dock-motion.md) | HIGH | DONE | no |
| 007 | [Chat message actions visible on touch](007-chat-actions-hover-gate.md) | HIGH | DONE | no |
| 008 | [Support "Utkast" surface scrolls](008-support-drafts-scroll.md) | HIGH | DONE | no |
| 009 | [Instant CTRL+K and sidebar; motion tokens](009-shell-instant-motion-and-tokens.md) | HIGH | DONE | no |
| 010 | [Render the shell in Geist](010-font-sans-geist.md) | HIGH | DONE | yes |

Every plan was applied by one executor and then reviewed by two independent
reviewers (one checking the diff against the plan, one hunting regressions).
Plans 003 and 010 each had one blocking review finding, fixed in a repair pass
and re-reviewed clean.

## Verification that ran

| Check | Result |
| --- | --- |
| `pnpm typecheck` | exit 0 (baseline was also clean) |
| eslint on the 8 changed TS/TSX files | 0 errors, 51 warnings, all pre-existing (compared against the pre-change copies) |
| `vitest run` over styles, inbox, chat, core, knowledge, insights, support, shared/ui | 102 files, 1065 tests passed. The baseline had 1 failing test (`CoreShell.test.tsx`, the "Søk" button) which now passes; no plan touches that code |
| `vite build` to a scratch folder | succeeds, no CSS warnings (only the usual chunk-size notice); the real `dist/` was not touched |
| `global.css` parsed with postcss and lightningcss | 0 warnings; braces balance; `transition: all` count 0; no new undefined custom properties |
| NUL-byte and truncation scan of all changed files | clean (see the incident note below) |
| Live, signed in (browser pane, details below) | every plan checked on the running app; two adjustments made as a result |

### Live verification (signed in, 2026-09-29)

The dev server on :5199 hot-reloads whenever another session edits shared
files (`session-store.ts`, the dashboard files, and so on), and each hot update
remounts components. That invalidates any remount test, so the behaviour tests
ran on a static production build of the same tree served by `vite preview` on a
scratch port with the same session and no hot reload. CSS-only checks ran on
the dev server.

| Plan | What was measured | Result |
| --- | --- | --- |
| 001 | Active Insights chips (Oversikt, 30d, Organisasjon) | white text on `#171717`; labels visible in the screenshot |
| 002 | Focus ring on Tab (chips, sidebar link) | orange `#b8501f` ring visible; it rendered 1 device pixel thick at first, see adjustment 1 |
| 003 | Folder cards on /knowledge | flex column 265×239, radius 14, art 263×128, "183 Indexed" and "2 Indexed" unclipped |
| 004 | Sidebar tree, fully expanded | labels readable with full-name tooltips; hierarchy was inverted at first, see adjustment 2 |
| 005 | Support: focused row across a focus-triggered refresh and a 22 s window with 3 list refetches. Chat: Copy click | Support: same button node, still focused, 0 removals. Chat: same button and article survive, label flips to "Copied", focus stays |
| 006 | Chat page at idle | no launch-wash element, composer dock `animation-name: none`, no running animations |
| 007 | Latest assistant turn | carries `data-latest`; its 5 action buttons are at opacity 1 without hovering; the hide rule sits inside `(hover: hover) and (pointer: fine)` |
| 008 | /support?surface=drafts | `flex: 1 1 auto`, `overflow-y: auto`, `min-height: 0`; with 2,739px of injected filler in a 1,036px pane it scrolls, the page does not, the tab bar stays |
| 009 | CTRL+K, sidebar, stylesheet | panel has no animation class, full opacity with nothing running, input focused within 50 ms; sidebar transitions only `background-color`, main only `padding-top`; 0 `transition: all` across 7,420 CSS rules |
| 010 | Fonts | 133 rendered text elements use Geist, the rest are KaTeX math faces; 0 Inter declarations in the stylesheet |

**Adjustments made because of what the live check showed:**

1. **Focus ring width (plan 002).** The shell runs at `zoom: 0.9`, so the 2px outline was 1.8 device pixels and Chrome floors outline widths to whole device pixels, which drew a 1px ring. Added `--focus-ring-width` (`2px`, and `calc(2.25px / 0.9)` inside the zoomed shell, the same pattern as `--verevon-touch-target`) and used it in the four ring rules. Verified: 2.22px, which is 2 device pixels.
2. **Tree hierarchy (plan 004).** Hiding icons from depth 2 down, and reserving the chevron column only when a sibling had children, made a child's text start to the left of its parent's (measured 133, then 123, then 118). Icons now stay at every depth (only the "1" count pill is hidden on deep leaves) and every child row reserves the chevron column (`childrenReserveToggle` removed from `CoreSidebarKnowledgePanel.tsx`). Label x is now 121, 133, 146, 158 and stable. The cost is width: the deepest labels are 86 to 98px, so long names show about a dozen characters and rely on the tooltip; "Microsoft 365" with its "183" pill truncates at depth 2.

**Still not verified:** reduced-motion behaviour (the pane cannot emulate the
media query), a real touch device (the hover gate was confirmed by reading the
CSSOM, not by tapping), dark mode, and the first send in a new chat (nothing
was sent, to avoid spending model usage on your account).

**Concurrent edits.** Another session is editing this tree while these plans
run (`session-store.ts`, `home-local-content.ts`, `chat-retention.ts` and the
dashboard files were being changed every 15 to 30 seconds). Its changes are
mixed into the working tree and into any dev-server test; they are not part of
these plans.

## Deviations from the plans as written

- **003** — kept a two-declaration rule (`cursor: default; outline: none`) on `.knowledge-page-surface--docs .knowledge-folder-card`, because an older unscoped rule at about `global.css:19630` sets `cursor: pointer` and a 1px outline on what is now a static `<article>`. The plan's literal grep for one rule now returns two lines. Also, in the repair pass, the six `__stat-primary` / `__stat-secondary` rules were re-scoped under `.knowledge-page-surface--docs` (plus `padding-bottom: 0`): with the stale block gone, older unscoped `.knowledge-folder-card__stats span` rules (specificity 0,1,1) still beat the intended stat styling (a 24px secondary stat with 8px padding, measured by the reviewer on the running app). That was an existing bug the stale block had masked, so the audit's attribution of the clipped "183 Inde" stat to the grid override alone was only partly right. A follow-up could delete the dead legacy `__stats span/p/strong` rules instead of out-scoping them.
- **004** — real class names differ from the plan's guesses (`.core-sidebar-dedicated-icon`, `.core-sidebar-count-pill[data-count="1"]`). Margins are 10px/10px instead of 15px/12px because the guide line has to sit under the new 20px chevron column. A shared `.core-sidebar-subnav, .core-sidebar-subtree` rule was split so the Inbox and Ticketing sidebars keep their old margin. Rows are wrapped in a flex row holding the chevron button and the row button (a button cannot contain a button). While the sidebar filter has text, every branch is forced open. The chevron label is "Show subfolders: <name>".
- **005** — the article `keyed` key is `article.id`, which is a 31-bit hash of the message id rather than a guaranteed-unique id; collisions are negligible and the keyed list tolerates duplicates.
- **006** — the optional step 6 (merging the three `.verevon-chat-composer-dock` blocks at about 22893, 28086 and 46803) was deliberately skipped; none of the three has an `animation` now.
- **007** — the latest-turn flag marks the newest **assistant** turn (a `latestAssistantTurnId` memo), not the last index, so a trailing user turn cannot leave the real latest reply hidden.
- **009** — `.verevon-search-expanded-shell` uses `transition: opacity var(--dur-slow) var(--ease-out)`: it has one rule and a static class, its width and height are 100%, so the old `all 500ms` never animated anything visible. Only two files use the bare `.transition` class, not three; `WorkflowCanvas.tsx` node buttons now transition at 120ms instead of the ineffective 150ms and no longer animate left/top (the nodes are not draggable, so nothing visible changes).
- **010** — beyond the five stacks listed, the executor and repair pass converted every remaining hardcoded Inter-first stack in `global.css` (0 remain), plus `.verevon-popover`, `--dashboard-home-font-display` and `--chat-font-display/-body`. `--font-sans` is defined once; `--font-mono` stays in the second `:root` block.

## Follow-ups found during execution (not done)

1. **Global focus rule specificity (plan 002).** The new `:where(...)` rule has zero specificity on purpose, so about 15 component rules that set `outline: none` on a button, link or textarea now win over it (for example `.verevon-dashboard-textarea`, `.verevon-chat-edit-box textarea`, the inbox textarea rules, `.knowledge-page-surface--docs .knowledge-folder-card`). Their old fallback ring was the near-invisible 18% one. Give each its own `:focus-visible` treatment or `:focus-within` on its shell.
2. **Knowledge tree collapse state (plan 004).** A folder selected from outside the tree, or roots that arrive after the first toggle, can end up under a collapsed parent (`isFolderExpanded` uses `expandedFolderIds()?.has(id) ?? depth === 0`, so once a Set exists the depth-0 fallback no longer applies). Expand the ancestors of the active folder, and keep the depth-0 default for ids not yet seen.
3. **Touch access to user-message actions (plan 007).** Assistant actions are fixed; `.verevon-chat-message-actions--user` (Edit, Copy) is still `opacity: 0` with no hover gate.
4. **Fonts outside `global.css` (plan 010).** `src/features/onboarding/components/steps/WebsiteStep.css` (six stacks) and the canvas/WebGL labels in `connectGraphScene.ts` and `connectGraphScene.webgl.ts` still hardcode Inter. `EmailBody.tsx` should probably stay as is (email sandbox).
5. **Dead CSS.** Dark-mode selectors near `global.css:42228` and `42242` still reference the removed `.knowledge-folder-card__copy`; plan 001's `var(--focus-ring, var(--verevon-accent-ink))` fallback is now redundant but harmless.
6. **Undefined custom properties.** Seven names are used without a definition anywhere and were not introduced by these plans: `--verevon-danger`, `--verevon-text-primary`, `--verevon-border-subtle`, `--verevon-e-primary`, `--verevon-e-strong`, `--verevon-surface-hover`, `--verevon-text-secondary`. (The plans fixed three others: `--font-inter`, `--dashboard-rail-width`, `--verevon-primary-foreground`.)
7. **MEDIUM and LOW rows of the audit** that no plan covers: the root dark-mode token block, the route-pending progress bar, Insights grid and number formatting, Support sender label and row hit area, Knowledge skeleton and loading, and the gated animation opportunities in section 7 of the audit.

## Incident note

The previous Claude session ended abruptly while plan 009 was being applied.
`global.css` and `CoreNavbar.tsx` came back zero-filled (right size, all NUL
bytes). Both were restored from pre-edit snapshots after proving the snapshot
chain intact (the diff between the pre-008 and pre-009 snapshots matched plan
008's recorded diff exactly, and all 128 lines added by plans 001–008 were
present); plan 009 was then re-run from a clean state. The final integration
pass scanned all 86 modified or untracked files under this app for NUL bytes
and found none.
