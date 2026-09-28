import { describe, expect, it } from 'vitest'

import {
  activityFromProcess,
  activityFromThread,
  activitySentence,
  buildSpaceActivity,
  buildSpaceRecord,
  buildSpaceWork,
  orderActivity,
  type SpaceActivityItem,
  type ThreadLikeActivitySource,
} from './activity-grammar'

function thread(overrides: Partial<ThreadLikeActivitySource> = {}): ThreadLikeActivitySource {
  return { thread_id: 'thr_1', ...overrides }
}

describe('activity grammar', () => {
  describe('resolving the object — never show a raw id', () => {
    it('prefers the title', () => {
      const [conversation] = activityFromThread(thread({ title: 'Aquatiq-saken', preview: 'noe annet' }))
      expect(conversation?.object).toBe('Aquatiq-saken')
    })

    it('falls back to the preview when there is no title', () => {
      const [conversation] = activityFromThread(thread({ preview: 'Kan du sjekke fakturaen?' }))
      expect(conversation?.object).toBe('Kan du sjekke fakturaen?')
    })

    it('truncates a long preview instead of letting it run', () => {
      const [conversation] = activityFromThread(thread({ preview: 'a'.repeat(200) }))
      expect(conversation?.object).toHaveLength(81) // 80 + ellipsis
      expect(conversation?.object.endsWith('…')).toBe(true)
    })

    it('never falls through to the thread id', () => {
      const [conversation] = activityFromThread(thread({ thread_id: 'thr_abc123' }))
      expect(conversation?.object).toBe('Uten tittel')
      expect(conversation?.object).not.toContain('thr_abc123')
    })

    it('treats whitespace-only title and preview as absent', () => {
      const [conversation] = activityFromThread(thread({ title: '   ', preview: '\n' }))
      expect(conversation?.object).toBe('Uten tittel')
    })
  })

  describe('run status — mapped from the vocabulary the services actually emit', () => {
    it('surfaces an approval request as the most consequential thing in the feed', () => {
      const items = activityFromThread(thread({ latest_run_status: 'awaiting_approval' }))
      const run = items.find((item) => item.renderClass === 'run')
      expect(run?.salience).toBe('critical')
      expect(run?.outcome.tone).toBe('pending')
      expect(run?.outcome.live).toBe(true)
    })

    it.each(['failed', 'timed_out'])('treats %s as critical and terminal', (status) => {
      const run = activityFromThread(thread({ latest_run_status: status })).find(
        (item) => item.renderClass === 'run',
      )
      expect(run?.salience).toBe('critical')
      expect(run?.outcome.tone).toBe('failure')
      expect(run?.outcome.live).toBe(false)
    })

    it.each(['completed', 'succeeded', 'approved'])('lets %s recede', (status) => {
      const run = activityFromThread(thread({ latest_run_status: status })).find(
        (item) => item.renderClass === 'run',
      )
      expect(run?.salience).toBe('muted')
      expect(run?.outcome.tone).toBe('success')
    })

    it.each(['running', 'queued', 'pending'])('marks %s as still moving, so the row mutates in place', (status) => {
      const run = activityFromThread(thread({ latest_run_status: status })).find(
        (item) => item.renderClass === 'run',
      )
      expect(run?.outcome.live).toBe(true)
    })

    it('gives a live run a stable id so refreshes update the row instead of duplicating it', () => {
      const first = activityFromThread(thread({ latest_run_id: 'run_9', latest_run_status: 'queued' }))
      const later = activityFromThread(thread({ latest_run_id: 'run_9', latest_run_status: 'running' }))
      const idOf = (items: SpaceActivityItem[]) => items.find((i) => i.renderClass === 'run')?.id
      expect(idOf(first)).toBe(idOf(later))
    })
  })

  describe('honesty over guessing', () => {
    it('renders an unrecognized status verbatim rather than inventing a reading', () => {
      const run = activityFromThread(thread({ latest_run_status: 'quantum_superposition' })).find(
        (item) => item.renderClass === 'unknown',
      )
      expect(run).toBeDefined()
      expect(run?.outcome.label).toBe('quantum_superposition')
      expect(run?.outcome.tone).toBe('neutral')
    })

    it('never drops an item it does not understand — never go dark', () => {
      const items = activityFromThread(thread({ latest_run_status: 'something_new' }))
      expect(items).toHaveLength(2)
      expect(items.some((item) => item.renderClass === 'unknown')).toBe(true)
    })

    it('emits only the conversation when there is no run at all', () => {
      const items = activityFromThread(thread({ title: 'Bare en samtale' }))
      expect(items).toHaveLength(1)
      expect(items[0]?.renderClass).toBe('conversation')
    })

    it('treats a whitespace-only status as no run rather than an unknown one', () => {
      const items = activityFromThread(thread({ latest_run_status: '  ' }))
      expect(items).toHaveLength(1)
    })
  })

  describe('ordering — failures rise, reads recede', () => {
    it('puts a day-old failure above a minute-old completed run', () => {
      const ordered = buildSpaceActivity([
        thread({
          thread_id: 'fresh',
          title: 'Fersk',
          latest_run_status: 'completed',
          latest_run_updated_at: '2026-08-13T12:00:00Z',
        }),
        thread({
          thread_id: 'stale',
          title: 'Gammel',
          latest_run_status: 'failed',
          latest_run_updated_at: '2026-08-12T12:00:00Z',
        }),
      ])
      expect(ordered[0]?.object).toBe('Gammel')
      expect(ordered[0]?.outcome.tone).toBe('failure')
    })

    it('orders by recency within the same salience band', () => {
      const ordered = orderActivity([
        { id: 'a', renderClass: 'run', verb: 'Kjøring', object: 'Eldre', outcome: { label: 'feilet', tone: 'failure', live: false }, salience: 'critical', at: '2026-08-12T00:00:00Z' },
        { id: 'b', renderClass: 'run', verb: 'Kjøring', object: 'Nyere', outcome: { label: 'feilet', tone: 'failure', live: false }, salience: 'critical', at: '2026-08-13T00:00:00Z' },
      ])
      expect(ordered.map((item) => item.object)).toEqual(['Nyere', 'Eldre'])
    })

    it('sorts an item with no timestamp last within its band, not first', () => {
      const ordered = orderActivity([
        { id: 'undated', renderClass: 'run', verb: 'Kjøring', object: 'Udatert', outcome: { label: 'feilet', tone: 'failure', live: false }, salience: 'critical' },
        { id: 'dated', renderClass: 'run', verb: 'Kjøring', object: 'Datert', outcome: { label: 'feilet', tone: 'failure', live: false }, salience: 'critical', at: '2026-08-01T00:00:00Z' },
      ])
      expect(ordered[0]?.object).toBe('Datert')
    })

    it('does not mutate the input array', () => {
      const input: SpaceActivityItem[] = [
        { id: 'a', renderClass: 'conversation', verb: 'Samtale', object: 'A', outcome: { label: 'åpen', tone: 'neutral', live: false }, salience: 'muted' },
        { id: 'b', renderClass: 'conversation', verb: 'Samtale', object: 'B', outcome: { label: 'åpen', tone: 'neutral', live: false }, salience: 'critical' },
      ]
      orderActivity(input)
      expect(input.map((item) => item.id)).toEqual(['a', 'b'])
    })
  })

  describe('where a row sends you', () => {
    // The Activity tab is where a supervisor sees that something needs them.
    // The decision itself lives on the post, in the Chat tab, so a row that
    // dropped the reader on whichever tab the hash happened to hold would show
    // them a duty and hide the control for it.
    it('lands on the chat tab, where the decision surface is', () => {
      const items = activityFromThread(
        thread({ space_id: 'space-room', latest_run_status: 'awaiting_approval' }),
      )
      for (const item of items) {
        expect(item.href).toContain('/spaces/space-room')
        expect(item.href?.endsWith('#chat')).toBe(true)
      }
    })

    // Unchanged by the hash: an item whose owning Space is unknown still has
    // nowhere honest to point.
    it('still refuses to link when the owning Space is unknown', () => {
      const items = activityFromThread(thread({ space_id: '', latest_run_status: 'failed' }))
      expect(items.every((item) => item.href === undefined)).toBe(true)
    })
  })

  describe('the sentence', () => {
    it('reads as verb, object, outcome', () => {
      const run = activityFromThread(
        thread({ title: 'Fakturakontroll', latest_run_status: 'awaiting_approval' }),
      ).find((item) => item.renderClass === 'run')
      expect(activitySentence(run!)).toBe('Kjøring: Fakturakontroll → venter på godkjenning')
    })
  })
})

describe('run cost detail — a zero is not a figure', () => {
  const run = (extra: Record<string, unknown>) => ({
    run_id: 'run_1',
    thread_id: 't1',
    space_id: 's1',
    goal: 'Tell lageret',
    status: 'completed',
    ...extra,
  })

  it('never prints a zero token figure: session-core reports 0 for every run', () => {
    const [item] = buildSpaceRecord({ runs: [run({ input_tokens: 0, output_tokens: 0, steps_completed: 3 })] })
    expect(item?.detail ?? []).toEqual(['3 steg'])
  })

  it('prints tokens only when the listing actually carries some', () => {
    const [item] = buildSpaceRecord({ runs: [run({ input_tokens: 80, output_tokens: 40, steps_completed: 0 })] })
    expect(item?.detail).toEqual(['120 tokens'])
  })

  it('has no detail at all when nothing is known', () => {
    const [item] = buildSpaceRecord({ runs: [run({})] })
    expect(item?.detail).toBeUndefined()
  })
})

describe('activityFromProcess', () => {
  it('reads a live process by when it will stop, not when it started', () => {
    const item = activityFromProcess({
      process_id: 'p1',
      state: 'RUNNING',
      command: 'python3 build.py',
      started_at: '2026-09-13T10:00:00Z',
      expires_at: '2026-09-13T10:30:00Z',
    })
    expect(item?.renderClass).toBe('process')
    expect(item?.object).toBe('python3 build.py')
    expect(item?.outcome.live).toBe(true)
    // The deadline, not the start: "what needs me" is answered by when this
    // stops, and a start time hours ago is not actionable.
    expect(item?.at).toBe('2026-09-13T10:30:00Z')
  })

  it('splits EXITED on its code rather than calling every finish a success', () => {
    const clean = activityFromProcess({ process_id: 'p1', state: 'EXITED', exit_code: 0 })
    expect(clean?.outcome.tone).toBe('success')
    const failed = activityFromProcess({ process_id: 'p2', state: 'EXITED', exit_code: 2 })
    expect(failed?.outcome.tone).toBe('failure')
    expect(failed?.outcome.label).toContain('2')
    expect(failed?.salience).toBe('attention')
  })

  it('ranks LOST above a plain failure, because the outcome is unknown', () => {
    const lost = activityFromProcess({ process_id: 'p1', state: 'LOST' })
    const failed = activityFromProcess({ process_id: 'p2', state: 'EXITED', exit_code: 1 })
    expect(lost?.salience).toBe('critical')
    expect(failed?.salience).toBe('attention')
  })

  it('names an unrecognized state as it came instead of flattening it', () => {
    const item = activityFromProcess({ process_id: 'p1', state: 'QUARANTINED' })
    expect(item?.outcome.label).toBe('quarantined')
    expect(item?.outcome.tone).toBe('neutral')
  })

  it('drops a row with no id, and survives a row with no command', () => {
    expect(activityFromProcess({ state: 'RUNNING' })).toBeNull()
    expect(activityFromProcess({ process_id: 'p1', state: 'RUNNING' })?.object).toBe(
      'Bakgrunnsprosess',
    )
  })

  it('is optional in buildSpaceWork, so an older gateway still renders', () => {
    const withoutProcesses = buildSpaceWork([], [])
    expect(withoutProcesses).toEqual([])
    const withProcesses = buildSpaceWork([], [], [{ process_id: 'p1', state: 'RUNNING' }])
    expect(withProcesses).toHaveLength(1)
    expect(withProcesses[0]?.renderClass).toBe('process')
  })
})
