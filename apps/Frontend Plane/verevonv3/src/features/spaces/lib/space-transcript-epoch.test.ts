import { flush } from 'solid-js'
import { describe, expect, it } from 'vitest'

import { bumpThreadTranscriptEpochs, listedTextChanges, threadTranscriptEpoch } from './space-transcript-epoch'
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

  it('poll fallback: names only the threads whose listed title or preview changed', () => {
    const seen = new Map<string, string>()
    expect(listedTextChanges(seen, [{ thread_id: 'p1', title: 'Q', preview: 'A' }, { thread_id: 'p2', title: 'R' }])).toEqual([])
    // An edit of p1's last line folds into its preview; p2 is unchanged; p3 is new, not a change.
    expect(
      listedTextChanges(seen, [
        { thread_id: 'p1', title: 'Q', preview: 'A edited' },
        { thread_id: 'p2', title: 'R' },
        { thread_id: 'p3', title: 'S' },
      ]),
    ).toEqual(['p1'])
    // A thread that left the listing is forgotten, so its return is not a change either.
    expect(listedTextChanges(seen, [{ thread_id: 'p3', title: 'S' }])).toEqual([])
    expect(listedTextChanges(seen, [{ thread_id: 'p1', title: 'other' }, { thread_id: 'p3', title: 'S' }])).toEqual([])
  })
})
