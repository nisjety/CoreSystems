# Reversible chat appearance preview

Updated 2026-09-28 after the twelve annotated browser comments. This is an
opt-in development preview. Production continues to use the existing UI.

**2026-09-29:** All eight switches are now the standard appearance: always on
in production and on by default in development. The Appearance panel only
turns single refinements off for comparison. **Original** shows the legacy UI
and **Reset all** returns to the standard; **Try all** was removed. Choices use
`verevon.chat-appearance-preview.v3` and store only the switches turned off.
v1 and v2 choices are ignored, because they were saved against the old all-off
default.

## Try it

Open `/chat`. With the refined navigation enabled, choose **… → Appearance**.
With it disabled, choose **Appearance** directly in the header (the sliders
button below the header on small screens).

- Each checkbox is independent and saved in this browser.
- **Original** temporarily compares the original without clearing your choices.
- **Your mix** restores the choices. Composer drafts survive this comparison.
- **Reset all** disables the experiment. **Try all** enables every switch.
- Menus close using Escape or an outside pointer click.

The browser's existing choices were preserved: welcome, history and navigation
on; bottom composer and flatter composer off. The requested organizer and
combined context button have been enabled separately. Seven switches are now
available:

| Switch | Effect |
| --- | --- |
| Simpler welcome | Left-aligned title and description, centered brand mark; prompt ideas available on demand |
| Bottom composer | Places the empty-state composer near the bottom, in normal document flow |
| Flatter composer | Joins the composer toolbar and input into a quieter surface |
| Compact history | Prioritizes titles; dates remain in accessible labels/tooltips and appear on hover/focus |
| Softer navigation | Back/forward first, then sidebar toggle; selected rail icons filled; visible search pill; orange profile ring; profile, notifications, calendar, messages, divider, More |
| Chat organizer | Search, grouping, sorting, project folders, drag-and-drop moves, pins, active/attention filters |
| Combined context button | Knowledge icon and label inside the same context button, independent of the flatter composer |

The navigation switch applies across the shell so the sidebar toggle and filled
selection stay consistent when changing routes. The other layout switches apply
to chat. On narrow screens the sidebar opens over the content with a close
backdrop; header search remains available as an icon.

## Organizer scope

Search covers the loaded chat history's titles, previews and assigned project
names. Existing history currently retains up to 40 entries. Group by project,
date or no grouping; sort by most recent, oldest or title. Pins retain the
existing chat pin behavior. Active and Needs attention filters use actual run
statuses; no model call or generated classification is involved.

The folder button creates a project. Its … action renames, pins/unpins or removes
the folder. A chat's … menu offers **Move to project**, including **No project**.
Removing a folder preserves its conversations. Existing chat rename/delete and
pin behavior stays in the existing handlers.

In **Group by → Project**, drag a chat onto a project folder (including a closed
folder) or **Recent** to change its assignment. Empty folders and Recent remain
drop destinations. A highlighted target and status message confirm the move.
Escape or dropping outside a destination cancels it. The existing **Move to
project** menu remains available for keyboard and touch users and in other
grouping modes. Pinned chats keep their pin when moved. This changes folder
membership; the selected sort order still controls the row order.

Projects and assignments are browser-only, scoped to account **and** workspace,
using `verevon.chat-organization.v1:<org>:<user>`. They do not sync across devices
or grant workspace access. Only project names, thread IDs and view choices are
stored by this feature. Local organization actions have Zod contracts in
`src/shared/actions/chat-organization.ts`. They never call a backend or modify
conversation content. Turning the organizer off preserves its saved folders.

Appearance choices use `verevon.chat-appearance-preview.v1`. Fresh browsers
start with everything off. Existing five-switch preferences migrate without
changing their selections. Production ignores the appearance preference.

## Verification — second pass

- 37 tests passed across chat organization, organizer interactions, CoreShell,
  CoreSidebar chat sessions, and ChatPage voice first-turn behavior.
- ESLint on the affected TypeScript files: no errors, five warnings.
- `pnpm exec vite build` passed; existing large-chunk advisories remain.
- The full `pnpm typecheck` is blocked by an unrelated current-worktree error:
  `src/features/chat/components/ScheduleDialog.tsx:215` uses `readOnly` where
  Solid expects `readonly`. This file was not edited in this pass.
- Live browser verified project creation, rename, pin, moving a chat, persistence
  after reload, and removing the temporary folder while retaining the chat.
- Search, title sorting, date grouping and the attention filter were exercised
  against actual history, then restored to the normal recent/project view.
- Verified desktop and 390 × 844 layouts, mobile sidebar and More menu,
  keyboard search activation, Original/Your mix draft preservation, and filled
  Home/Chat icons while navigating between routes.
- Before the final touch-ups, the search pill was 44 px high in the tested desktop view. Measured text
  contrast is 17.40:1 light / 13.51:1 dark; border-to-fill contrast is 3.34:1 /
  4.50:1. These exceed the relevant 4.5:1 text and 3:1 component thresholds in
  [WCAG 2.2](https://www.w3.org/WAI/WCAG22/quickref/). This is a focused check,
  not a full application accessibility audit.
- Restored light theme, removed the temporary project, preserved the user's
  existing composer choices, and returned the live browser to `/chat`.
- No chat/model request was sent during verification.

Screenshots and before-edit backups are in:
`C:/Users/ImaFernandesDaCosta/AppData/Local/Temp/verevon-chat-cleanup-pass2-20260928`.

## Final touch-ups

The next three browser comments remove the search border in both themes, soften
selected rail icon fills to grey, and center only the welcome logo. The heading
and description remain left-aligned. The search retains its keyboard focus
outline. These are CSS-only changes under the existing appearance switches.

`touchups.patch` isolates these three adjustments. Reverse it **before** the
second-pass patch when rolling back the complete preview:

```powershell
git apply -p1 --reverse --check --directory="apps/Frontend Plane/verevonv3" "apps/Frontend Plane/verevonv3/docs/chat-appearance-preview/touchups.patch"
git apply -p1 --reverse --directory="apps/Frontend Plane/verevonv3" "apps/Frontend Plane/verevonv3/docs/chat-appearance-preview/touchups.patch"
```

The reverse check passed. The earlier border-contrast measurements above are
historical; the final search pill intentionally has no resting border.

## Code rollback without touching existing work

### Drag-and-drop and compact search pass

The next three comments add chat moves by dragging, a round chat bubble with
three dots (retaining the grey selected fill), and a smaller borderless search
pill using the page font. The pill is at most 420 px wide and 36 px high, with
13 px text and the existing focus outline.

`drag-and-search.patch` isolates this pass against before-edit copies of its six
source files. It excludes other worktree changes. Reverse it **before**
`touchups.patch`, `refinements.patch`, and `implementation.patch` when removing
the entire preview. From the repository root:

```powershell
git apply -p1 --reverse --check --directory="apps/Frontend Plane/verevonv3" "apps/Frontend Plane/verevonv3/docs/chat-appearance-preview/drag-and-search.patch"
git apply -p1 --reverse --directory="apps/Frontend Plane/verevonv3" "apps/Frontend Plane/verevonv3/docs/chat-appearance-preview/drag-and-search.patch"
```

To keep only some of these changes, substitute `organizer-drag.patch`,
`chat-bubble.patch`, or `compact-search.patch` in both commands above. These
three patches affect separate files and can be reversed independently. Use
either the combined patch or the individual patches, not both.

The reverse check passed. Validation for this pass:

- 25 targeted tests passed, covering moves into closed/empty destinations,
  cancellation, ignored external/mismatched drops, persisted assignments,
  existing menu moves, organization actions, and sidebar chat sessions.
- ESLint: no errors; two existing CoreSidebar reactivity warnings.
- Vite build passed with existing chunk-size advisories. Full typecheck still
  reports the unrelated `ScheduleDialog.tsx:215` `readOnly` error noted above.
- A real browser pointer drag moved the user's chat from `test` into Recent
  and back to `test`; its original assignment and all 40 chats were preserved.
- Before-edit copies and the final screenshot are in
  `C:/Users/ImaFernandesDaCosta/AppData/Local/Temp/verevon-chat-drag-pass-20260928`.

### Earlier passes

`refinements.patch` contains only this second pass, against copies taken before
these edits. `implementation.patch` records the first pass. Existing unrelated
worktree changes are excluded. No commit, reset, clean or checkout was performed.

From **the repository root** (`C:/dev/CoresSystem`), check before applying:

```powershell
git apply -p1 --reverse --check --directory="apps/Frontend Plane/verevonv3" "apps/Frontend Plane/verevonv3/docs/chat-appearance-preview/refinements.patch"
git apply -p1 --reverse --directory="apps/Frontend Plane/verevonv3" "apps/Frontend Plane/verevonv3/docs/chat-appearance-preview/refinements.patch"
```

This returns to the first pass, including its independent switches. To remove
both passes, reverse `refinements.patch` first, then check/reverse
`implementation.patch` using the same commands. Both include newly added code
files, but leave the documentation and patches as a review record. Reverse
checks must succeed before applying; if later edits conflict, reconcile the
small hunks manually instead of restoring whole files.

The original first pass was validated with 28 tests, `pnpm build` including its
then-passing typecheck, browser comparison and mobile checks. Its backups remain
in the sibling `verevon-chat-cleanup-20260928` temporary folder.
