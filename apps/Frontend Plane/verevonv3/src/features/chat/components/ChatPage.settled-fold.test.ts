/**
 * Settled-turn folding (EP-14 S1), page wiring. The full page has no cheap unit harness (see
 * ChatPage.voice-first-turn.test.tsx), so this pins the page's source to the wiring the fold depends on: one set
 * of fold choices per view, the switch and the density rule in front of the plan, and the turns a run is still on
 * or parked kept out of it.
 */

import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { describe, expect, it } from 'vitest'

const page = readFileSync(resolve(process.cwd(), 'src/features/chat/components/ChatPage.tsx'), 'utf8')

describe('ChatPage settled-turn fold wiring (EP-14 S1)', () => {
  it('builds one set of fold choices per page view and gives every row that set', () => {
    expect(page.match(/const settledFoldChoices = createSettledFoldChoices\(\)/g)).toHaveLength(1)
    expect(page).toContain('settledFoldChoices={settledFoldChoices}')
  })

  it('folds only behind the switch and the density rule', () => {
    expect(page).toContain('settledFoldEnabled && transcriptDensity.rules().foldSettled')
  })

  it('keeps the turn a run is still on, and the turn of a parked run, out of the plan', () => {
    expect(page).toContain(
      'activeTurnId: isStreaming() || liveRunId() || pendingQuestion() ? latestAssistantTurnId() : null',
    )
    expect(page).toContain('parkedTurnId: parkedTurnId(),')
  })
})
