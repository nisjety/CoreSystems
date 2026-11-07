import { describe, expect, it } from 'vitest'
import { COMPUTER_VIEWERS, pickComputerViewer } from './viewer-registry'

describe('pickComputerViewer', () => {
  it('draws a command and a process in the terminal viewer', () => {
    expect(pickComputerViewer('terminal')?.kind).toBe('terminal')
    expect(pickComputerViewer('process')?.kind).toBe('terminal')
  })

  it('draws a workspace file in the file viewer', () => {
    expect(pickComputerViewer('file')?.kind).toBe('file')
  })

  /**
   * A browser step keeps its raw output in its card: the live page is drawn by the run panel's browser view, and
   * every other intent keeps the output it always had.
   */
  it('draws no viewer in a tool card for a browser step or any other intent', () => {
    for (const intent of ['search', 'read', 'diff', 'generic', 'browser'] as const) {
      expect(pickComputerViewer(intent)).toBeUndefined()
    }
  })

  it('lists each intent under at most one viewer', () => {
    const claimed = COMPUTER_VIEWERS.flatMap((entry) => entry.intents)
    expect(new Set(claimed).size).toBe(claimed.length)
  })
})
