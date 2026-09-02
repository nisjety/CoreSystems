/**
 * The live-run rail receives its Work content once, not once per read.
 *
 * `ChatPage` used to pass `workContent={workCanvasActive() ? contextualPanel() : undefined}`.
 * Solid compiles an inline prop expression to a getter, and `ChatLiveRunPanel`
 * reads `props.workContent` eight times (class, aria-label, title, onClick, two
 * <Show>s and the render slot). Each read re-invoked `contextualPanel()` and
 * instantiated a fresh Work panel tree. Only one reached the DOM; the other
 * seven lived on as orphans whose plan resources kept fetching -- measured live
 * as seven identical `/plans` requests per refresh, and the multiplier behind
 * the plan panel's original ~165-request storm.
 *
 * The fix is a `createMemo` in the parent so every read gets the same element.
 * This guard reads the source because the two components are only wired
 * together inside the full chat page, which has no cheap unit harness.
 *
 * Enforced as a test rather than an ESLint rule because `pnpm lint` cannot run
 * in this repo today (`typescript-eslint` does not support the pinned
 * TypeScript 7). Reading source text follows the precedent in
 * `mid-run-input.test.ts` and `chat-route-ownership.test.ts`.
 */

import { describe, expect, it } from 'vitest'

async function read(relative: string): Promise<string> {
  const { readFileSync } = await import('node:fs')
  const { resolve } = await import('node:path')
  return readFileSync(resolve(process.cwd(), relative), 'utf8')
}

describe('live-run rail work content identity', () => {
  it('does not pass contextualPanel() to the rail as an inline prop expression', async () => {
    const page = await read('src/features/chat/components/ChatPage.tsx')
    // Any `workContent={... contextualPanel() ...}` would re-instantiate per read.
    // The canvas's own `navigation={workspaceNavigation()}` is fine -- it reads
    // the prop once -- so only the conditional form handed to the rail is banned.
    expect(page).not.toMatch(/workContent=\{[^}]*contextualPanel\(\)/)
    expect(page).not.toMatch(/navigation=\{[^}]*workCanvasActive\(\)/)
  })

  it('memoizes the Work content and navigation handed to the rail', async () => {
    const page = await read('src/features/chat/components/ChatPage.tsx')
    expect(page).toMatch(/const liveRailWorkContent = createMemo\(\(\) => \(workCanvasActive\(\) \? contextualPanel\(\) : undefined\)\)/)
    expect(page).toMatch(/const liveRailNavigation = createMemo\(\(\) => \(workCanvasActive\(\) \? workspaceNavigation\(\) : undefined\)\)/)
    expect(page).toContain('workContent={liveRailWorkContent()}')
    expect(page).toContain('navigation={liveRailNavigation()}')
  })
})
