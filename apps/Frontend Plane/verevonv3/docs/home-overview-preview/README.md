# Home overview preview

2026-09-28. A reversible Home redesign based on the user's three references.

Later refinements: [Home sidebar overviews and plain Web/Extract containers](sidebar-overviews.md).

**Standard since 2026-09-29.** The Dashboard overview, and every other
appearance refinement, is the default in production and in development.
Turning **Dashboard overview** off, or choosing **Original**, still shows the
legacy Home for comparison. See `src/features/chat/lib/chat-appearance-preview.ts`.

The same day the heading gained the Verevon mark: 48 px, centred on the page
in the gap between the greeting and the Chat / Nett / Hent ut tabs, behind the
text and never taking pointer events. It fades to a watermark at 640 px and
below, where the description reaches the centre. Above 640 px the chat dock's
bottom edge lines up with the fixed Tilbakemelding button: both sit
`--feedback-widget-bottom` (24.5 px) above the window's bottom edge. Where the two would
meet, the dock narrows to stay 12 px clear of it, so the button never covers
the send controls. The shell also closes around the content: `.core-main`
leaves a 5 px strip of the shell colour on the right and bottom (and on the
left on phones), and `.dashboard-main-panel` is rounded on all four corners.

## Design decisions

- Image 1 supplies the main direction: warm neutral surfaces, quiet type,
  generous spacing, subtle panel separation and a restrained weather accent.
- Image 2 supplies hierarchy: a larger primary panel with smaller supporting
  panels, rather than equally weighted cards under an oversized composer.
- Image 3 supplies the two-row chat dock: title, minimize and more controls
  above a compact input with attachment, voice and send controls.

This is a workspace for someone returning to an unfinished conversation,
finding evidence or bringing in new knowledge. Its vocabulary is conversations,
sources, documents, agents, runs and shared spaces. Recent work leads; weather,
traffic and news provide secondary context. Knowledge, Agents and Spaces remain
direct navigation targets. No decorative metrics, invented trends or counts
derived from the browser's capped history are shown.

The palette uses paper white, soft grey, graphite, muted warm grey and a small
peach weather glow. Dark mode uses charcoal and the same visual hierarchy.
Existing Geist/Inter typography keeps it part of Verevon: 36 px greeting,
18 px panel heading, 13 px body, 11 px metadata, 64 px weather reading. Panels
use 20 px padding and 16 px gutters. Surface tones establish depth; the floating
chat dock alone receives a soft shadow. The dashboard does not copy the
references' deliberately low-contrast or tiny text.

## Try and compare

On `/dashboard`, open **… → Appearance → Dashboard overview**. Its choice is
independent of all previous chat refinements, saved only in this browser, and
ignored in production. Disable it to restore the original Home layout. The
appearance comparison and tab switches preserve the mounted chat composer,
including its draft and attachments while the preview is enabled.

- Chat shows the overview with a compact, sticky composer that reserves space
  in document flow. **…** expands model, context and tool controls. **−** hides
  the input without destroying its state; **+** restores it.
- Search and Crawl keep their existing implementation and show the board
  beneath their input until actual results need the space.
- A card's **Ask Verevon** action prepares a draft and expands/focuses the chat
  composer. It does not send a message.
- Recent conversations come from the existing workspace-scoped Chat API.
  Loading, empty and retry states are explicit. A scope change cancels the old
  response's ability to update the panel. Pinned is a view filter.
- Weather, traffic and news reuse the existing live feed components. Provider
  unavailability remains visible; this preview adds no new backend operations.
- Small screens use one column. The dock can be minimized to uncover more of
  the board. All controls retain keyboard operation and focus styling.

## Verification

- 36 tests pass across recent work, tab/draft/attachment preservation, Enter
  submission, pasted attachments and voice mode.
- ESLint on the changed TypeScript files passes without warnings.
- Vite production bundling passes. Full typecheck still reports the unrelated
  `src/features/chat/components/ScheduleDialog.tsx:215` `readOnly` error.
- Browser checks cover Chat/Search/Crawl, draft preservation across tabs and
  original/preview comparison, minimizing/expanding the composer, light/dark
  themes, and the 390 px mobile layout and model picker. No messages, searches
  or crawls were submitted. The final desktop view has no horizontal overflow.
- The final screenshot is `dashboard-preview.png` in the backup directory below.

## Revert the code

The later copy refinement labels the tabs **Web** and **Extract** and changes
the web input to **Search on the web or ask…**, including its accessible label
and Norwegian translation. This text applies to both Home layouts. Its isolated
`copy-refinement.patch` can be reversed separately; reverse it before the original
implementation when removing both passes (use the commands below with that
filename first).

`implementation.patch` contains only this pass, compared to before-edit copies,
including the narrow changes to the shared composer. Existing worktree changes
are excluded. Check first from `C:/dev/CoresSystem`:

```powershell
git apply -p1 --reverse --check --directory="apps/Frontend Plane/verevonv3" "apps/Frontend Plane/verevonv3/docs/home-overview-preview/implementation.patch"
git apply -p1 --reverse --directory="apps/Frontend Plane/verevonv3" "apps/Frontend Plane/verevonv3/docs/home-overview-preview/implementation.patch"
```

Reverse this pass before earlier chat appearance patches when removing the
entire experiment. If a check conflicts with later work, reconcile the small
hunks instead of replacing whole files. Source backups and screenshots are in
`C:/Users/ImaFernandesDaCosta/AppData/Local/Temp/verevon-home-overview-20260928`.
