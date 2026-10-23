# `@tab` page-context mention (E3.3-c) runbook

Status: built dark. The token is parsed and resolved by a pure module. Nothing
in the composer or the gateway calls it yet. The composer render (picker and
chip) waits for open point D9 (where the card renders when no dock is open),
and the gateway re-resolution waits for its route.

## Where it lives

| Piece | File |
|---|---|
| Parser, formatter, resolver, target sanitiser (pure) | `src/features/browser/page-context/tab-mention.ts` |
| Unit tests (Vitest) | `src/features/browser/page-context/tab-mention.test.ts` |
| Tab type it reads | `BrowserTab` in `src/shared/api/browser-client.ts` (type only) |

## Token forms

| Draft text | Meaning |
|---|---|
| `@tab` | The single active tab. |
| `@tab:<tabId>` | The tab with that id. Ids are `[A-Za-z0-9_-]`, 1 to 64 characters. |

The keyword is case-insensitive. A token needs a boundary before the `@`
(start of text, whitespace, or opening punctuation) and must end at text end,
whitespace, or closing punctuation. `@tabs`, `@table`, `me@tab.no` and
`@tab:` with no id are not tokens.

## Resolution rules (fail closed)

`resolveTabMentions(mentions, tabs, { zeroRetention })` returns one result per
mention, in draft order:

| Condition | Result reason |
|---|---|
| `zeroRetention: true` | `zero-retention` (every mention, even with tabs present) |
| Tab list empty or missing | `no-tabs` |
| Bare `@tab` and no active tab | `no-active-tab` |
| Bare `@tab` and more than one active tab, or an id that appears twice | `ambiguous-tab` |
| Explicit id not in the list | `unknown-tab` |

Tabs whose id is not a valid token id are ignored, so they can never match.
A resolved mention yields a `TabContextRef` with `grant: 'none'`. A mention
never opens, focuses, or acts on a tab, and it never reads page content. The
`label` is the tab title, else the origin and path, else the tab id. The
`target` is origin and path only (query, fragment and credentials dropped).

## Not in this slice (boundaries)

- The gateway must re-resolve every `@tab` against the live Quarry tab list
  before any model call. The client result is a hint, not authority.
- Composer wiring: `detectTrigger` in `DashboardComposer.tsx` still skips `@`
  on purpose. The picker, the chip, and the submit payload wait for D9.
- Region and selection attachments (cropping the streamed frame) and the
  read-only hit-test route are separate slices of E3.3-c and E3.3-e.

## Verify

```
cd "apps/Frontend Plane/verevonv3"
node node_modules/vitest/vitest.mjs run src/features/browser/page-context/tab-mention.test.ts
node node_modules/eslint/bin/eslint.js src/features/browser/page-context/
```
