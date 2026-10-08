# 001 — Make the active Insights chips readable (define `--verevon-primary-foreground`)

- **Status**: DONE - applied 2026-09-29 in the working tree (uncommitted); deviations in plans/README.md
- **Commit**: 83927eda
- **Severity**: HIGH
- **Category**: States / colour, focus-visible
- **Estimated scope**: 1 file (`src/styles/global.css`), ~20 lines

## Problem

On /insights the active section tab ("Oversikt"), the active range button ("30d") and the active scope button ("Organisasjon") render as a solid black pill with no visible label. Computed in the browser: `background rgb(23,23,23)`, `color rgb(26,26,26)` (≈1.05:1).

All three active rules use a token that is never defined anywhere in the stylesheet (`grep -c "verevon-primary-foreground:" src/styles/global.css` → 0; 4 uses), so `color` falls back to the inherited `--foreground: #1a1a1a`:

```css
/* src/styles/global.css:40361 — current */
.verevon-insights-tab.is-active {
  border-color: var(--verevon-primary);
  background: var(--verevon-primary);
  color: var(--verevon-primary-foreground);
}

/* src/styles/global.css:40434 — current */
.verevon-insights-scope button.is-active {
  border-color: var(--verevon-primary);
  background: var(--verevon-primary);
  color: var(--verevon-primary-foreground);
}

/* src/styles/global.css:40481 — current (same shape, range buttons) */
.verevon-insights-range button.is-active { … color: var(--verevon-primary-foreground); }
```

The same chips also hide keyboard focus, because hover and focus share one rule that removes the outline:

```css
/* src/styles/global.css:40427 — current */
.verevon-insights-scope button:hover,
.verevon-insights-scope button:focus-visible {
  border-color: var(--verevon-border);
  background: var(--verevon-surface-soft);
  outline: none;
}
/* src/styles/global.css:40474 — same for .verevon-insights-range button */
```

## Target

```css
/* :root token block, directly after `--verevon-primary: #171717;` (global.css:54) */
--verevon-primary-foreground: #ffffff;

/* hover and focus split; focus gets a visible ring */
@media (hover: hover) and (pointer: fine) {
  .verevon-insights-scope button:not(.is-active):hover,
  .verevon-insights-range button:not(.is-active):hover {
    border-color: var(--verevon-border);
    background: var(--verevon-surface-soft);
  }
}

.verevon-insights-scope button:focus-visible,
.verevon-insights-range button:focus-visible,
.verevon-insights-tab:focus-visible {
  outline: 2px solid var(--focus-ring, var(--verevon-accent-ink));
  outline-offset: 2px;
}
```

`--focus-ring` is introduced by plan 002; the fallback to `--verevon-accent-ink` (`#b8501f`, defined at `global.css:358`) works on its own.

## Repo conventions to follow

- Colour tokens live in the `:root` block at the top of `src/styles/global.css` (lines 15–60); brand tokens are prefixed `--verevon-`. Exemplar: `--verevon-primary: #171717;` at line 54.
- Component rules are plain BEM-ish classes in the same file; the Insights block starts near line 40040.
- Hover media gating exemplar: `global.css:6975`.

## Steps

1. In `src/styles/global.css`, after line 54 (`--verevon-primary: #171717;`), insert `  --verevon-primary-foreground: #ffffff;`.
2. Replace the combined hover/focus rule at 40427–40432 (`.verevon-insights-scope button:hover, …:focus-visible { … outline: none; }`) with the two rules from **Target** (hover inside the media query using `:not(.is-active)`, focus-visible with the ring). Do the same for the range rule at 40474–40479; you can merge both into the shared selectors shown in Target.
3. Add `.verevon-insights-tab:focus-visible` to the focus-visible rule (the tabs currently have no focus style at all; their base rule is at 40346).
4. Search the file for any other `var(--verevon-primary-foreground)` use (there are 4) and confirm they now resolve to white on the primary background.

## Boundaries

- Do NOT touch `InsightsPage.tsx` or any TSX.
- Do NOT change the chips' size, radius or weight in this plan (that is a separate LOW finding).
- Do NOT add new dependencies.
- If line numbers do not match the excerpts above (drift since 83927eda), search for the selectors and STOP if the rule shapes differ.

## Verification

- **Mechanical**: `grep -c "verevon-primary-foreground:" src/styles/global.css` → `1`. `grep -n "outline: none" src/styles/global.css | grep -c "4042[7-9]\|4043[0-2]\|4047[4-9]"` → `0`. `pnpm lint` passes (CSS is not linted, but the command must still exit 0).
- **Feel check**: open http://localhost:5173/insights. "Oversikt", "30d" and "Organisasjon" show white text and icons on the black pill. Press Tab from the page heading: each chip shows a 2px orange ring offset 2px; hovering a non-active chip tints it, hovering the active chip changes nothing.
- **Done when**: every active chip label is legible at ≥ 4.5:1 and every chip shows a focus ring on Tab.
