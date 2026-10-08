# 003 — Remove the stale compact-card rules that crush the Knowledge folder cards

- **Status**: DONE - applied 2026-09-29 in the working tree (uncommitted); deviations in plans/README.md
- **Commit**: 83927eda
- **Severity**: HIGH
- **Category**: Surfaces / layout bug (cascade)
- **Estimated scope**: 1 file (`src/styles/global.css`), delete ~92 lines

## Problem

On /knowledge ("Mapper" section) the folder cards render their artwork as a 29px sliver and clip their stat to "183 Inde" / "2 Indexe". Two rule blocks target the same selector; the later one (a leftover compact design whose sub-elements `__icon` and `__copy` no longer exist in the markup) wins and forces a three-column grid onto a flex-column card.

```css
/* src/styles/global.css:29476 — the intended card (keep) */
.knowledge-page-surface--docs .knowledge-folder-card {
  display: flex;
  min-height: 266px;
  flex-direction: column;
  overflow: hidden;
  border: 1px solid #deded9;
  border-radius: 14px;
  background: #ffffff;
  color: #22221f;
  box-shadow: 0 5px 14px rgb(24 24 21 / 4%);
  transition: border-color 160ms ease, box-shadow 160ms ease, transform 160ms ease;
}

/* src/styles/global.css:41901 — the stale override (delete) */
.knowledge-page-surface--docs .knowledge-folder-card {
  display: grid;
  min-height: 96px;
  grid-template-columns: 32px minmax(0, 1fr) auto;
  grid-template-rows: 1fr auto;
  gap: 7px 9px;
  …
  border-radius: 10px;
  padding: 12px;
}
```

The stale block continues through `41992` with `.knowledge-folder-card__icon` (+ `--blue/--gray/--green/--warm`), `.knowledge-page-surface--docs .knowledge-folder-card__copy`, `…__copy h3`, `…__copy small`, `…__stats { position: static; grid-column: 2; grid-row: 2 }`, `…__stats strong`, `…__stats small`, and `…__arrow`. The markup (`KnowledgePage.tsx:1468–1517`) renders `__visual`, `__body`, `__heading`, `__stats` with `__stat-primary/__stat-secondary`, `__linked-meter` and `__arrow`; it never renders `__icon` or `__copy`.

## Target

Only the original card rules (29476–29600 region) apply. The card is a 266px-min flex column with the gradient `__visual` art on top and the body below; the stat reads "183 Indekserte" in full.

## Repo conventions to follow

- One rule block per selector. The file already has duplicates (`.verevon-chat-bubble` ×5, `.verevon-chat-composer-dock` ×3); this plan removes one of them, it does not add scoping hacks.
- Do not rename classes in TSX to work around CSS.

## Steps

1. In `src/styles/global.css`, confirm line 41901 starts with `.knowledge-page-surface--docs .knowledge-folder-card {` and that line 41995 starts `.knowledge-pulse-grid {`.
2. Confirm the sub-elements are dead: `grep -c "knowledge-folder-card__icon\|knowledge-folder-card__copy" src/features/knowledge/components/KnowledgePage.tsx` → `0`.
3. Delete lines 41901 through the closing brace of the `.knowledge-page-surface--docs .knowledge-folder-card__arrow { … }` rule (ends at 41992), leaving `.knowledge-pulse-grid` as the next rule. Keep one blank line before it.
4. Search for any remaining `.knowledge-page-surface--docs .knowledge-folder-card` rule after line 41000; there should be none.

## Boundaries

- Do NOT edit the original block at 29476–29600.
- Do NOT touch the tree indent (plan 004) or the 9s sheen animation (separate finding).
- Do NOT edit TSX.
- If the block at 41901 does not match the excerpt, STOP and report.

## Verification

- **Mechanical**: `grep -n "^\.knowledge-page-surface--docs \.knowledge-folder-card {" src/styles/global.css` → exactly one line (near 29476). `pnpm lint` exits 0.
- **Feel check**: open http://localhost:5173/knowledge, wait for the "Mapper" cards. Each card is a tall tile with the gradient folder art on top (≈142px), the title and full stat text below ("183 Indekserte dokumenter" wraps or fits, never clips), and the segmented meter at the bottom. Resize the window from 1280px to 900px: cards reflow but never clip text.
- **Done when**: no folder card shows a truncated stat and the art occupies the full card width.
