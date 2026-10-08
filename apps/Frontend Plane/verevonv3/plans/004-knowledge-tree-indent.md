# 004 — Make the Knowledge sidebar tree readable below depth 2

- **Status**: DONE - applied 2026-09-29 in the working tree (uncommitted); deviations in plans/README.md
- **Commit**: 83927eda
- **Severity**: HIGH
- **Category**: Typography / truncation, hit areas
- **Estimated scope**: 2 files (`src/styles/global.css`, `src/features/core/components/sidebar/CoreSidebarKnowledgePanel.tsx`), ~40 lines

## Problem

The collections tree in the 288px sidebar indents 30px + 12px at level 1 and another 24px + 12px per level, so depth-3 rows start 102px in and their labels are ~12px wide: rows read "W", "W", "P." with a "1" pill each. Labels have no `title`, so the full text is unreachable. The tree is always fully expanded.

```css
/* src/styles/global.css:3180 — current */
.core-sidebar-subtree {
  position: relative;
  margin: 6px 0 8px 34px;
}

.core-sidebar-subtree {
  margin-left: 30px;
}

.core-sidebar-subtree--nested {
  margin-left: 24px;
}

.core-sidebar-subnav__line { position: absolute; top: 4px; bottom: 4px; left: 0; width: 1px; background: var(--verevon-border); }

.core-sidebar-subtree__child {
  padding-left: 12px;
}
```

Rows are built at `CoreSidebarKnowledgePanel.tsx:486–511` (buttons with `aria-pressed`, an icon, the label, and a count pill).

## Target

```css
/* replace the two .core-sidebar-subtree blocks */
.core-sidebar-subtree {
  position: relative;
  margin: 2px 0 2px 15px;   /* guide line sits 15px in */
}

.core-sidebar-subtree--nested {
  margin-left: 12px;
}

.core-sidebar-subtree__child {
  padding-left: 4px;
}

/* hide the leaf icon and the count pill at depth ≥ 2 to give the label room */
.core-sidebar-subtree--nested .core-sidebar-subtree__child .core-sidebar-panel-link__icon,
.core-sidebar-subtree--nested .core-sidebar-subtree__child [data-count="1"] {
  display: none;
}
```

With the above, a depth-3 label gets about 112px (never below 96px). Each row element gets `title={folder.label}` so the full name is available on hover and to assistive tech. Subtrees below depth 0 start collapsed.

## Repo conventions to follow

- Sidebar CSS lives in `global.css` 2050–3400; the tree uses `.core-sidebar-subtree`, `--nested`, `__child`, and rows are `.core-sidebar-panel-link` (`global.css:2244`).
- The knowledge panel already derives `active` state for tags from the search query (`CoreSidebarKnowledgePanel.tsx:245`); follow that pattern rather than effects.
- Icons use `size-[N]` utility classes on lucide components (exemplar: `CoreSidebar.tsx:1141`).

## Steps

1. In `src/styles/global.css`, replace the rules at 3180–3204 with the **Target** CSS (keep `.core-sidebar-subnav__line` unchanged).
2. In `CoreSidebarKnowledgePanel.tsx` at the row `<button>` (line ~490), add `title={folder.label}` and render the count pill with `data-count={folder.count}` so the CSS above can hide "1" pills on leaves.
3. Add a collapsed state: a `createSignal<Set<string>>` of expanded folder ids initialised to the depth-0 ids; render a chevron button (`aria-expanded`, `aria-label={i18n.tr('Vis undermapper', 'Show subfolders')}`) before the label for any folder with children; render the `<For>` of children only when expanded. Keep it instant (no height animation).
4. Do not virtualise; the builder caps groups at 6 leaves (`knowledge-live-client.ts:326, 342, 401`).

## Boundaries

- Do NOT change row height (36px) or the `.core-sidebar-panel-link` base rule.
- Do NOT add tree ARIA roles/arrow keys here (separate MEDIUM finding); keep `aria-pressed` for now.
- Do NOT add dependencies.
- If the CSS at 3180 does not match the excerpt, STOP and report.

## Verification

- **Mechanical**: `pnpm typecheck` and `pnpm lint` exit 0. `pnpm vitest run src/features/core/components/sidebar` passes (update snapshot-free tests if they assert the old markup).
- **Feel check**: open http://localhost:5173/knowledge. Depth-0 collections show with chevrons; expanding "General Knowledge" → "Integrations" → "Microsoft 365" shows leaf labels like "Welcome to our new c…" with at least 6–8 readable characters and a tooltip with the full name. No leaf shows a "1" pill. The guide line still connects rows.
- **Done when**: no visible tree row is reduced to a single letter at the default sidebar width.
