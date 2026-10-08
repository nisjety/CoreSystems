# 010 — Render the shell in Geist (fix `--font-sans` and font loading)

- **Status**: DONE - applied 2026-09-29 in the working tree (uncommitted); deviations in plans/README.md
- **Commit**: 83927eda
- **Severity**: HIGH
- **Category**: Typography
- **Estimated scope**: 2 files (`src/styles/global.css`, `index.html`), ~12 lines

## Problem

Only Geist is loaded, but the root font stack asks for Inter, and the `--font-sans` token begins with an undefined variable, which makes the whole declaration invalid at computed-value time. Result: the body renders in Inter (if installed) or the system font, while the chat composer (which hardcodes Geist at `global.css:30144`) renders in Geist — two typefaces on one screen. Measured on /insights: body `font-family` computes to the Inter/system stack, while the chat composer textarea computes Geist.

```css
/* src/styles/global.css:1 — current */
@import url('https://fonts.googleapis.com/css2?family=Geist:wght@100..900&family=Geist+Mono:wght@100..900&display=swap');

/* global.css:3 — current */
:root {
  …
  font-family:
    Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;

/* global.css:366 — current (`--font-inter` is never defined anywhere) */
--font-sans:
  var(--font-inter), var(--font-geist-sans), "Inter", "Geist",
  ui-sans-serif, system-ui, sans-serif;
--font-geist-sans: "Geist";

/* global.css:413 — current (same token, second definition) */
--font-sans:
  var(--font-inter), var(--font-geist-sans), "Inter", "Geist",
  "Geist Fallback", ui-sans-serif, system-ui, sans-serif;
--font-mono: "Geist Mono", "Geist Mono Fallback", ui-monospace, monospace;
```

Hardcoded Inter-first stacks also appear at `global.css:1891–1895` (`.verevon-sidebar-type`), `30144`, `32129`, `32158`, `32197`.

## Target

```html
<!-- index.html, inside <head> after the favicon link (line 5) -->
<link rel="preconnect" href="https://fonts.googleapis.com" />
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin />
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Geist:wght@100..900&family=Geist+Mono:wght@100..900&display=swap" />
```

```css
/* global.css — remove the @import at line 1 */

:root {
  font-family: var(--font-sans);
  …
  --font-sans: "Geist", "Geist Fallback", ui-sans-serif, system-ui, -apple-system, "Segoe UI", sans-serif;
  --font-mono: "Geist Mono", "Geist Mono Fallback", ui-monospace, monospace;
}
```

Every rule that hardcodes `var(--font-geist-sans), "Geist", Inter, …` becomes `font-family: var(--font-sans);` (or is deleted when the parent already inherits it).

## Repo conventions to follow

- Tokens are defined in `:root` in `global.css`; `--font-mono` at 416 is the exemplar.
- `index.html` is the Vite entry; keep the existing `<meta>` order.

## Steps

1. `index.html`: add the three `<link>` tags after line 5.
2. `global.css:1`: delete the `@import` line.
3. `global.css:3–7`: change the root `font-family` to `var(--font-sans)`.
4. `global.css:366–369`: replace the `--font-sans` value with the **Target** value; delete `--font-geist-sans: "Geist";` at 369 only after step 6 removes its uses.
5. `global.css:413–415`: replace the second `--font-sans` with the same value (or delete the duplicate so the token is defined once).
6. Replace the hardcoded stacks at 1891–1895, 30144, 32129, 32158, 32197 with `font-family: var(--font-sans);`. Then `grep -n "var(--font-geist-sans)\|var(--font-inter)" src/styles/global.css` → no results.

## Boundaries

- Do NOT change font sizes, weights or tracking.
- Do NOT self-host the font in this plan (fine as a follow-up).
- If the excerpts at 366 or 413 differ, STOP and report.

## Verification

- **Mechanical**: `grep -c "font-inter" src/styles/global.css` → `0`. `grep -c "@import" src/styles/global.css` → `0`. `pnpm lint && pnpm build` exit 0.
- **Feel check**: open http://localhost:5173/insights and run in the console: `getComputedStyle(document.body).fontFamily` starts with `Geist`; `document.fonts.check('16px Geist')` is `true` after load. The heading "Verevon Innsikt", the sidebar labels and the chat composer all render in the same face (compare the double-storey "a" and the straight-tailed "y"). In the Network panel the Geist CSS is requested from the `<link>` before `global.css` finishes parsing.
- **Done when**: no element on the four audited routes computes to Inter or Segoe UI.
