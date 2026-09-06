/**
 * The contextual rail keeps one width, whoever owns it.
 *
 * The right zone is a single four-tab panel (VEREVON_CHAT_DESIGN.md 3.1), but
 * two components render it: `ChatWorkspaceCanvas` for Sources/Output/Trace, and
 * `ChatLiveRunPanel` when a live run hosts the Work tab. They had separate
 * widths, and the mismatch was visible: `.verevon-chat-page--canvas
 * .verevon-chat-run-panel` shrinks the run panel to make room for the canvas
 * when the two genuinely sit side by side, but that same rule also fired when
 * the run panel WAS the workspace and no canvas existed. Measured at a 974px
 * viewport, the rail went 234px on Work and 396px on Sources, so every tab
 * switch moved the panel sideways and Work -- carrying the densest content of
 * the four tabs (plan, steps, context window) -- was also the narrowest.
 *
 * Both rails now read one custom property, so a future width change cannot
 * update one and forget the other.
 *
 * Enforced as a test rather than an ESLint rule because `pnpm lint` cannot run
 * in this repo today (`typescript-eslint` does not support the pinned
 * TypeScript 7). Reading source text follows the precedent in
 * `mid-run-input.test.ts` and `chat-route-ownership.test.ts`.
 */

import { describe, expect, it } from 'vitest'

/** The single source of truth for how wide the contextual rail is. */
const RAIL_VAR = 'var(--verevon-workspace-rail'

async function read(relative: string): Promise<string> {
  const { readFileSync } = await import('node:fs')
  const { resolve } = await import('node:path')
  return readFileSync(resolve(process.cwd(), relative), 'utf8')
}

/** Body of the first rule whose selector block matches, comments included. */
function ruleBody(css: string, selector: string): string {
  const start = css.indexOf(selector + ' {')
  expect(start, `selector not found: ${selector}`).toBeGreaterThan(-1)
  const end = css.indexOf('}', start)
  return css.slice(start, end)
}

describe('workspace rail geometry', () => {
  it('sizes the canvas from the shared rail variable', async () => {
    const css = await read('src/styles/global.css')
    expect(ruleBody(css, '.verevon-chat-workspace-canvas')).toContain(RAIL_VAR)
  })

  it('sizes the run panel from the same variable when it hosts Work', async () => {
    const css = await read('src/styles/global.css')
    const hosting = ruleBody(css, '.verevon-chat-page--canvas .verevon-chat-run-panel--workspace')
    expect(hosting).toContain(RAIL_VAR)
  })

  it('keeps the narrow basis only for the genuine side-by-side case', async () => {
    const css = await read('src/styles/global.css')
    // The unmodified `--canvas` run-panel rule may stay narrow: there, a canvas
    // really is present and the run panel has to give up room for it.
    const beside = ruleBody(css, '.verevon-chat-page--canvas .verevon-chat-run-panel')
    expect(beside).not.toContain(RAIL_VAR)
  })

  it('overrides the rail width in one place per breakpoint', async () => {
    const css = await read('src/styles/global.css')
    // A breakpoint that re-sizes the canvas directly would desync the run
    // panel again, which is exactly the bug this file exists to prevent.
    const canvasBasisOverrides = css.match(/\.verevon-chat-workspace-canvas \{\s*flex-basis:/g) ?? []
    expect(canvasBasisOverrides).toHaveLength(0)
  })

  it('marks the run panel as the workspace whenever it hosts work content', async () => {
    const panel = await read('src/features/chat/components/ChatLiveRunPanel.tsx')
    expect(panel).toContain("'verevon-chat-run-panel--workspace': Boolean(props.workContent)")
  })

  it('names the rail for what it is hosting', async () => {
    // "Live agentkjøring" is wrong when the panel is showing the Work tab.
    // Both names go through `i18n.tr` since acceptance criterion 9; the rule
    // being guarded is that the name still SWITCHES on `workContent`.
    const panel = await read('src/features/chat/components/ChatLiveRunPanel.tsx')
    expect(panel).toContain('aria-label={props.workContent')
    expect(panel).toContain("i18n.tr('Arbeidsflate', 'Workspace')")
    expect(panel).toContain("i18n.tr('Live agentkjøring', 'Live agent run')")
  })
})
