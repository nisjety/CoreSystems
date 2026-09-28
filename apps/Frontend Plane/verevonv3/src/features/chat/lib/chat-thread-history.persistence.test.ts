// @vitest-environment jsdom

/**
 * What a reload keeps of a turn, through the real local transcript store.
 *
 * `ChatThreadTranscriptTurn` promised that stopReason, memoryRecallCount,
 * recalledMemories and verification survive a reload, but the store's own
 * normaliser dropped all four on the way in. The subscription cost wording and
 * the "Modelltid"/"Systemtid" rows need costBasis and the timing totals, which
 * were not stored at all.
 */

import { afterEach, describe, expect, it } from 'vitest'
import type { RecalledMemory } from '@/shared/api/chat-client'
import { transcriptTurnToChatTurn, turnsToTranscript } from '@/features/chat/components/chat-normalizers'
import type { ChatTurn } from '@/features/chat/components/chat-types'
import {
  CHAT_THREAD_TRANSCRIPTS_KEY,
  readChatThreadTranscript,
  upsertChatThreadTranscript,
  type ChatThreadTranscriptTurn,
} from './chat-thread-history'

const THREAD = 'thread-persist'

const memory: RecalledMemory = {
  memoryId: 'mem-1',
  role: 'recall',
  origin: 'stated',
  label: 'USER',
  preview: 'Foretrekker metriske enheter',
}

const liveTurn = (overrides: Partial<ChatTurn> = {}): ChatTurn => ({
  id: 'asst-1',
  role: 'assistant',
  content: 'Svaret.',
  createdAt: '2026-09-24T08:00:00.000Z',
  streaming: false,
  tools: [],
  attachments: [],
  modelUsed: 'gpt-5.6-terra',
  latencyMs: 43_440,
  costBasis: 'subscription',
  timing: {
    serverMs: 43_440,
    providerMs: 43_220,
    orchestrationMs: 220,
    phases: [{ phase: 'answer_stream', startMs: 0, endMs: 43_000 }],
  },
  stopReason: 'stream_incomplete',
  memoryRecallCount: 1,
  recalledMemories: [memory],
  verification: { verdict: 'contradicts', kbCitations: 2, webCitations: 1, webAllowed: true },
  ...overrides,
})

/** Store the turn exactly as the controller's snapshot does, then read it back. */
function reload(turn: ChatTurn): ChatTurn | undefined {
  upsertChatThreadTranscript({ threadId: THREAD, turns: turnsToTranscript([turn]), updatedAt: turn.createdAt })
  const stored = readChatThreadTranscript(THREAD)?.turns.at(0)
  return stored ? transcriptTurnToChatTurn(stored) : undefined
}

afterEach(() => {
  window.localStorage.clear()
})

describe('local transcript persistence', () => {
  it('keeps what the transcript type promises, plus cost basis and timing totals', () => {
    const restored = reload(liveTurn())
    expect(restored).toMatchObject({
      costBasis: 'subscription',
      timing: { serverMs: 43_440, providerMs: 43_220, orchestrationMs: 220 },
      stopReason: 'stream_incomplete',
      memoryRecallCount: 1,
      recalledMemories: [{ ...memory, preview: '' }],
      verification: { verdict: 'contradicts', kbCitations: 2, webCitations: 1, webAllowed: true },
    })
    // Only the totals: the span list is a live diagnostic.
    expect(restored?.timing).not.toHaveProperty('phases')
    // A subscription turn stores no cost of its own, so it can never reappear as "$0".
    expect(restored?.costUsd).toBeUndefined()
  })

  it('stores which memories a turn recalled, never their text', () => {
    // Glem/Rediger change the memory, not this turn, so stored text would bring
    // a forgotten memory, or the text a correction replaced, back on reload.
    upsertChatThreadTranscript({
      threadId: THREAD,
      // Straight into the store too, not only through `turnsToTranscript`.
      turns: [{ ...turnsToTranscript([liveTurn()])[0]!, recalledMemories: [memory] }],
      updatedAt: '2026-09-24T08:00:00.000Z',
    })
    expect(window.localStorage.getItem(CHAT_THREAD_TRANSCRIPTS_KEY)).not.toContain(memory.preview)
    const restored = transcriptTurnToChatTurn(readChatThreadTranscript(THREAD)!.turns[0]!)
    expect(restored.recalledMemories).toEqual([{ ...memory, preview: '' }])
  })

  it('never stores answer text the server did not save', () => {
    reload(liveTurn({ status: 'error', content: 'Svaret ble ikke lagret.', unsavedContent: 'Hele svaret.' }))
    expect(window.localStorage.getItem(CHAT_THREAD_TRANSCRIPTS_KEY)).not.toContain('Hele svaret.')
  })

  it('normalises what it cannot vouch for on the way in', () => {
    const restored = reload(liveTurn({
      memoryRecallCount: 0,
      verification: { verdict: 'maybe' } as unknown as ChatTurn['verification'],
      timing: { serverMs: 10, providerMs: Number.NaN, orchestrationMs: 1 },
      costBasis: 'free' as unknown as ChatTurn['costBasis'],
      stopReason: '  ',
    }))
    expect(restored?.memoryRecallCount).toBeUndefined()
    expect(restored?.verification).toBeUndefined()
    expect(restored?.timing).toBeUndefined()
    expect(restored?.costBasis).toBeUndefined()
    expect(restored?.stopReason).toBeUndefined()
  })

  it('still reads a transcript stored before these fields existed', () => {
    const legacy: ChatThreadTranscriptTurn = {
      id: 'asst-old',
      role: 'assistant',
      content: 'Et eldre svar.',
      createdAt: '2026-08-01T08:00:00.000Z',
      costUsd: 0.0031,
    }
    window.localStorage.setItem(CHAT_THREAD_TRANSCRIPTS_KEY, JSON.stringify([
      { threadId: THREAD, updatedAt: legacy.createdAt, turns: [legacy] },
    ]))
    const restored = transcriptTurnToChatTurn(readChatThreadTranscript(THREAD)!.turns[0]!)
    expect(restored).toMatchObject({ content: 'Et eldre svar.', costUsd: 0.0031 })
    expect(restored.costBasis).toBeUndefined()
    expect(restored.timing).toBeUndefined()
  })
})
