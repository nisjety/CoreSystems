# UI state checklist

Use this list before a chat or agents surface merges. A change that touches a surface ticks every state the surface can show. A box is ticked only by the test named beside it, or by a browser check made for this change and recorded in the pull request. Where no test exists the line says so, and the check is by hand.

Run the browser checks on a dev server started from the files this change edits, for example `pnpm dev --port 5174 --strictPort` in a worktree, so the page shows the change and not an older copy.

## States

- [ ] **Loading.** The surface says what it is waiting for, in the reader's language. No spinner without a label.
  Automated: `chat-locale-smoke.test.tsx` (run receipt, English).
- [ ] **Empty.** The surface says why it is empty and what fills it. It does not show a blank box.
  Automated: `ChatArtifactPanel.test.tsx` ("shows the honest empty state"); `chat-locale-smoke.test.tsx` (artifacts, English).
- [ ] **Error.** The surface says what failed and what the reader can do next. The server's English text is never shown in place of the reader's language.
  Automated: `ChatLiveRunPanel.test.tsx` (stream error, English); `chat-locale-smoke.test.tsx` (receipt failure, English); `src/shared/i18n/errors.zdr-refusal.test.ts`.
- [ ] **Partial or stale.** The surface says which part is missing or out of date, for example the receipt's note on sections it does not cover, or a live run that was interrupted.
  Automated: `chat-locale-smoke.test.tsx` (the receipt's unavailable note is pinned, see Known gaps); `ChatLiveRunPanel.test.tsx` (interrupted state).
- [ ] **ZDR refused.** The refusal names the feature or action and never says where the request went. A code this SPA does not know gets the generic text, never the server's English.
  Automated: `src/shared/api/zdr-refusals.test.ts` (every golden code in both languages, with its action); `src/features/chat/components/use-chat-controller.zdr-refusal.test.ts`; `src/features/chat/components/chat-normalizers.zdr-refusal.test.ts`.
- [ ] **Offline.** The reader sees that the connection is gone and what happens to an unsent message.
  Automated: none. By hand: take the browser offline in DevTools, send a message, and check the copy in both languages.
- [ ] **Narrow viewport.** No horizontal page scroll at 320 px. The full-screen sheet takes over at 760 px and below.
  Automated: none for layout. The 44 px target rule is covered by `src/features/chat/lib/chat-accessibility.test.ts` (item 15). By hand: the layout at 320 px and 760 px.
- [ ] **Reduced motion.** Motion stops under `prefers-reduced-motion: reduce`.
  Automated: none. By hand: emulate reduced motion in DevTools and watch the live panel and the artifact panel.
- [ ] **Keyboard.** Every control is reachable and named. Escape closes a contextual panel, and focus lands on something real afterwards.
  Automated: `src/features/chat/lib/chat-accessibility.test.ts` (item 14). By hand: the tab order of any new control.

## Language and names

- [ ] **Both languages.** Every string on the surface goes through `i18n.tr(no, en)`, including the fallback copy a conditional or an empty value shows.
  Automated: the UI copy gate `spa-ui-gates` (`eslint.ui-gates.config.js`) for bare copy in chat and agents, and for every new `src` file; `chat-locale-smoke.test.tsx` for the approval block, the plan-grant control, the artifact panel, the live run and the run receipt, under `en`, with the `no` wording checked as well.
- [ ] **Screen-reader names.** `aria-label`, `aria-description`, `title`, `placeholder` and `alt` follow the locale, as the visible text does.
  Automated: `readableCopy` in `src/test/locale-leak.ts` reads these attributes, and `chat-locale-smoke.test.tsx` checks them.
- [ ] **Data is not copy.** Text the server sends (an approval's detail, a run's goal, an error message) is shown as sent. It never fills a label, and a fixture keeps user content free of the words the detector lists.
  Automated: none beyond the fixtures in `chat-locale-smoke.test.tsx`.

## Known gaps

These are Norwegian under `en` today. Each is pinned by its exact text in `chat-locale-smoke.test.tsx`. A pinned string that is translated fails the smoke test until its pin is removed in the same change.

- **Run receipt** (`ChatPanels.tsx`, `RunProofPanel`): the eyebrow, the refresh button, the metric labels, the effect-class labels, the outcome and verification row labels, and the unavailable note.
- **Fallback copy of a sparse receipt** (`ChatPanels.tsx`): the goal fallback, the status fallback and the effect-class label for an unknown class.
- **File-type labels** (`chat-artifacts.ts`, `friendlyMimeLabel`): labels such as `Excel-regneark (XLSX)`, `Markdown-dokument`, `HTML-dokument` and `Tekstfil` on the file meta line.
