# 002 — Give the app a visible focus ring

- **Status**: DONE - applied 2026-09-29 in the working tree (uncommitted); deviations in plans/README.md
- **Commit**: 83927eda
- **Severity**: HIGH
- **Category**: Accessibility / focus-visible
- **Estimated scope**: 1 file (`src/styles/global.css`), ~15 lines changed, plus a follow-up list

## Problem

The only global focus indicator is an 18% black outline. On the `#f4f4f3` rail and `#ffffff` panels that is roughly 1.5:1 contrast; on dark surfaces (`#34363d`) it is invisible. Every rail link, navbar button, breadcrumb, sidebar link and most page controls inherit it.

```css
/* src/styles/global.css:467 — current */
button:focus-visible,
a:focus-visible,
textarea:focus-visible {
  outline: 3px solid rgb(0 0 0 / 18%);
  outline-offset: 2px;
}
```

`.button` (the shared Button component) uses coral at 34% (≈1.4:1) at `global.css:38532–38537`. 54 rules in the file set `outline: none` / `outline: 0`.

## Target

```css
/* :root token block (add after global.css:358 `--verevon-accent-ink: #b8501f;`) */
--focus-ring: var(--verevon-accent-ink); /* #b8501f, ≈4.5:1 on white */

/* dark value — add next to the other .dark overrides near the top of the file (e.g. after line 586) */
.dark { --focus-ring: #f0936a; }

/* replace global.css:467–471 */
:where(button, a, summary, input, select, textarea, [tabindex]):focus-visible {
  outline: 2px solid var(--focus-ring);
  outline-offset: 2px;
  box-shadow: none;
}
```

`:where()` keeps specificity at zero so component rules that intentionally style focus (for example an inset ring inside a clipped scroller: `outline-offset: -2px`) still win.

## Repo conventions to follow

- Tokens live in `:root` in `src/styles/global.css` (15–460); dark overrides are `.dark …` rules (692 of them; exemplar `.dark .core-sidebar-panel-link` at 2312).
- `--verevon-accent-ink` already exists at line 358 and is re-tinted for dark surfaces at 4557 (`.dark .verevon-run-console { --verevon-accent-ink: #f0936a; }`); reuse its values.

## Steps

1. Add `--focus-ring: var(--verevon-accent-ink);` to the `:root` block directly after line 358.
2. Add a `.dark { --focus-ring: #f0936a; }` rule near the first `.dark` override (line 586).
3. Replace the rule at 467–471 with the `:where(…)` rule from **Target**.
4. Replace the `.button` focus ring at 38532–38537 with `outline: 2px solid var(--focus-ring); outline-offset: 2px; box-shadow: none;` (keep the selector).
5. Fix the three `outline: none` rules the audit named, and leave the other 51 for a follow-up (list them in the PR description with `grep -n "outline: none\|outline: 0" src/styles/global.css`):
   - `global.css:40431` and `40478` (Insights chips) — handled by plan 001; skip if 001 has landed.
   - `global.css:47917–47923` (`.verevon-remote-support` canvas, which has `tabindex=0` and forwards keystrokes to the customer): replace `outline: none` with `box-shadow: inset 0 0 0 2px var(--focus-ring);` on `:focus-visible`.

## Boundaries

- Do NOT remove `outline: none` from rules you have not read; some sit on elements that draw their own ring.
- Do NOT change hover or active styles in this plan.
- Do NOT add new dependencies.
- If the rule at 467 no longer matches the excerpt, STOP and report.

## Verification

- **Mechanical**: `grep -c "\-\-focus-ring:" src/styles/global.css` → `2`. `grep -n "rgb(0 0 0 / 18%)" src/styles/global.css` → no focus rule left. `pnpm lint` exits 0.
- **Feel check**: on /chat press Tab repeatedly from the address bar: the rail links, navbar buttons, search trigger, thread buttons and composer controls each show a 2px orange ring with 2px offset. Switch the theme to dark (navbar "…" → Mørkt) and repeat: the ring is the lighter `#f0936a` and visible on every surface. Click with the mouse: no ring appears (focus-visible only).
- **Done when**: every focusable element on the four audited routes shows a ring at ≥ 3:1 against its surface in both themes.
