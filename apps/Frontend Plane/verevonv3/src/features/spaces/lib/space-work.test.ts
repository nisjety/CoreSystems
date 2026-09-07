import { describe, expect, it } from 'vitest'

import {
  activityFromRun,
  activityFromSchedule,
  buildSpaceWork,
  type RunLikeActivitySource,
  type ScheduleLikeActivitySource,
} from './activity-grammar'

describe('activityFromRun', () => {
  it('names the run by its goal, never by its id', () => {
    const item = activityFromRun({ id: 'run_1', goal: 'Send fakturaoppsummering', status: 'running' })
    expect(item?.object).toBe('Send fakturaoppsummering')
    expect(item?.object).not.toContain('run_1')
  })

  it('says so when a run has no stated goal, rather than showing an id', () => {
    const item = activityFromRun({ id: 'run_1', status: 'running' })
    expect(item?.object).toBe('Uten beskrevet mål')
  })

  // The whole point of sharing the grammar: a waiting run reads identically in
  // Work and in Activity, so the two tabs cannot describe one run differently.
  it('uses the same status vocabulary as a thread-derived run row', () => {
    const waiting = activityFromRun({ id: 'run_1', goal: 'X', status: 'awaiting_approval' })
    expect(waiting?.outcome.label).toBe('venter på godkjenning')
    expect(waiting?.salience).toBe('critical')
    expect(waiting?.outcome.live).toBe(true)
  })

  // "Honesty over guessing": an unrecognized status is named as it arrived and
  // degrades to the generic class rather than being dressed up.
  it('renders an unknown status verbatim as the generic class', () => {
    const item = activityFromRun({ id: 'run_1', goal: 'X', status: 'quantum_tunnelling' })
    expect(item?.renderClass).toBe('unknown')
    expect(item?.outcome.label).toBe('quantum_tunnelling')
  })

  it('links to the run’s conversation only when it can name both Space and thread', () => {
    const linked = activityFromRun({ id: 'r', goal: 'X', status: 'running', space_id: 's', thread_id: 't' })
    expect(linked?.href).toBe('/spaces/s?thread_id=t#chat')
    // No Space, or no thread, means nowhere honest to point.
    expect(activityFromRun({ id: 'r', goal: 'X', status: 'running', thread_id: 't' })?.href).toBeUndefined()
    expect(activityFromRun({ id: 'r', goal: 'X', status: 'running', space_id: 's' })?.href).toBeUndefined()
  })

  // Session Core sends run timestamps as epoch SECONDS; a raw pass-through
  // would date every row to 1970 and silently break the ordering.
  it('reads epoch-second timestamps as times, not as 1970', () => {
    const item = activityFromRun({ id: 'r', goal: 'X', status: 'completed', updated_at: 1_757_000_000 })
    expect(item?.at?.startsWith('2025-')).toBe(true)
  })

  it('drops a run with no id, since nothing could identify or link it', () => {
    expect(activityFromRun({ goal: 'X', status: 'running' } as RunLikeActivitySource)).toBeNull()
  })
})

describe('activityFromSchedule', () => {
  it('reads as a future intention rather than a past outcome', () => {
    const item = activityFromSchedule({ id: 'c1', name: 'Daglig oppsummering', schedule_expr: '0 8 * * *' })
    expect(item?.verb).toBe('Planlagt')
    expect(item?.outcome.label).toContain('0 8 * * *')
    // Nothing is moving, so the row must not pulse as though it were.
    expect(item?.outcome.live).toBe(false)
  })

  // A schedule somebody switched off is part of "what is set up here". Hiding
  // it would make the room look emptier than it is.
  it('shows a disabled schedule, and calls it disabled', () => {
    const item = activityFromSchedule({ id: 'c1', name: 'Ukesrapport', enabled: false })
    expect(item?.outcome.label).toBe('slått av')
    // And it outranks a healthy one, because it is the one a person may need.
    expect(item?.salience).toBe('normal')
    expect(activityFromSchedule({ id: 'c2', name: 'Aktiv', enabled: true })?.salience).toBe('muted')
  })

  it('falls back to the description, then to an honest placeholder', () => {
    expect(activityFromSchedule({ id: 'c1', description: 'Sjekker lager' })?.object).toBe('Sjekker lager')
    expect(activityFromSchedule({ id: 'c1' })?.object).toBe('Uten navn')
  })
})

describe('buildSpaceWork', () => {
  // Same consequence-first rule as the Activity feed: what needs a person comes
  // first, regardless of which row is newer.
  it('orders by consequence, so a failed run outranks a healthy schedule', () => {
    const items = buildSpaceWork(
      [{ id: 'r1', goal: 'Feilet jobb', status: 'failed', updated_at: 1_700_000_000 }],
      [{ id: 'c1', name: 'Aktiv plan', enabled: true, schedule_expr: '0 8 * * *' }],
    )
    expect(items[0]?.object).toBe('Feilet jobb')
    expect(items).toHaveLength(2)
  })

  it('drops nothing it can identify, and nothing it cannot', () => {
    const items = buildSpaceWork(
      [{ id: 'r1', goal: 'A', status: 'running' }, {} as RunLikeActivitySource],
      [{ id: 'c1', name: 'B' }, {} as ScheduleLikeActivitySource],
    )
    expect(items.map((item) => item.object).sort()).toEqual(['A', 'B'])
  })

  it('is empty for an empty room, without inventing a row', () => {
    expect(buildSpaceWork([], [])).toEqual([])
  })
})
