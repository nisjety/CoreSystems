/**
 * The chat page's agent-mode picker (F-f06-17, WS25-S6).
 *
 * The picker shows only when the gateway's composer capabilities say agent modes are on for this caller (its own
 * AGENT_MODES switch, off for a zero-retention caller). The org's mode cap is read only then, through the one composer
 * capabilities read the page already makes, so a failed read or a switch that is off reads as no picker and no cap read.
 * The page has no cheap render harness (see ChatPage.fork-wiring.test.ts), so the wiring is pinned as source text. The
 * picker's own offer and its cap are covered by DashboardComposer.agent-mode.test.tsx.
 */

import { describe, expect, it } from 'vitest'

async function read(relative: string): Promise<string> {
  const { readFileSync } = await import('node:fs')
  const { resolve } = await import('node:path')
  return readFileSync(resolve(process.cwd(), relative), 'utf8')
}

function occurrences(source: string, needle: string): number {
  return source.split(needle).length - 1
}

describe('chat page agent-mode picker', () => {
  it('reads the org mode cap only when the composer capabilities answer agentModes true', async () => {
    const page = await read('src/features/chat/components/ChatPage.tsx')
    expect(page).toMatch(
      /const \[agentModeCap\] = createResource\(\s*\(\) => composerCapabilities\(\),\s*async \(capabilities\) => \(capabilities\.agentModes === true \? loadEffectiveAgentModeCap\(\) : undefined\),?\s*\)/,
    )
  })

  it('makes one composer capabilities read per mount, and the org cap read happens only inside that gate', async () => {
    const page = await read('src/features/chat/components/ChatPage.tsx')
    expect(occurrences(page, 'getComposerCapabilities()')).toBe(1)
    expect(occurrences(page, 'loadEffectiveAgentModeCap(')).toBe(1)
  })

  it('feeds the picker from that gated resource and from nothing else', async () => {
    const page = await read('src/features/chat/components/ChatPage.tsx')
    expect(occurrences(page, 'agentModeCap={agentModeCap()}')).toBe(1)
    expect(page).not.toMatch(/agentModeCap=\{(?!agentModeCap\(\)\})/)
  })
})
