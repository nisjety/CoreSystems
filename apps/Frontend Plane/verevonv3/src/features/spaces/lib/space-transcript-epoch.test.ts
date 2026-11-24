import { flush } from 'solid-js'
import { describe, expect, it } from 'vitest'

import { bumpThreadTranscriptEpochs, threadTranscriptEpoch } from './space-transcript-epoch'
import { threadRevision } from './space-timeline-store'

describe('space transcript epoch (DEPTH-WS12-S4-1)', () => {
  it('starts at 0 and bumps each named thread once per signal', () => {
    expect(threadTranscriptEpoch('epoch-a')).toBe(0)
    bumpThreadTranscriptEpochs(['epoch-a', 'epoch-a', 'epoch-b', ''])
    // Solid 2 applies a signal write at the next flush.
    flush()
    expect(threadTranscriptEpoch('epoch-a')).toBe(1)
    expect(threadTranscriptEpoch('epoch-b')).toBe(1)
    expect(threadTranscriptEpoch('')).toBe(0)
    bumpThreadTranscriptEpochs([])
    flush()
    expect(threadTranscriptEpoch('epoch-a')).toBe(1)
  })

  it('moves the revision a transcript is keyed on, for the touched thread only', () => {
    const thread = { thread_id: 'epoch-c', updated_at: '2026-11-24T08:00:00Z', last_message_seq: 4 }
    const other = { thread_id: 'epoch-d', updated_at: '2026-11-24T08:00:00Z', last_message_seq: 4 }
    const before = threadRevision(thread as never)
    const otherBefore = threadRevision(other as never)
    bumpThreadTranscriptEpochs(['epoch-c'])
    flush()
    expect(threadRevision(thread as never)).not.toBe(before)
    expect(threadRevision(other as never)).toBe(otherBefore)
  })
})
