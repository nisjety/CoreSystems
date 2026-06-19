import { describe, expect, it } from 'vitest'
import {
  aggregateToolActions,
  aggregateWorkspaceActivity,
  zeroDataRetention,
  type AuditEvent,
} from '@/shared/api/audit-client'

describe('zeroDataRetention', () => {
  it('reads the flag top-level or from details, defaulting to false', () => {
    expect(zeroDataRetention({ zdr: true })).toBe(true)
    expect(zeroDataRetention({ details: { zdr: true } })).toBe(true)
    expect(zeroDataRetention({ details: { zero_data_retention: 'true' } })).toBe(true)
    expect(zeroDataRetention({ details: { tool: 'x' } })).toBe(false)
    expect(zeroDataRetention({})).toBe(false)
  })
})

describe('aggregateWorkspaceActivity', () => {
  it('rolls up tool-action events per data category with ZDR coverage', () => {
    const events: AuditEvent[] = [
      { event: 'tool_action', details: { tool: 'search', data_category: 'customer', source: 'hubspot', zdr: true } },
      { event: 'tool_action', details: { tool: 'search', data_category: 'customer', source: 'hubspot' } },
      { event: 'tool_action', details: { tool: 'lookup', data_category: 'billing', source: 'stripe' } },
    ]
    const activity = aggregateWorkspaceActivity(events)
    expect(activity.totalEvents).toBe(3)
    expect(activity.zdrEvents).toBe(1)
    expect(activity.tools).toEqual(expect.arrayContaining(['search', 'lookup']))

    const customer = activity.categories.find((c) => c.category === 'customer')
    expect(customer).toEqual({ category: 'customer', count: 2, zdrCount: 1 })
    // Most-active category sorts first.
    expect(activity.categories[0]?.category).toBe('customer')
  })

  it('files events with no declared category under "Uncategorized"', () => {
    const activity = aggregateWorkspaceActivity([{ event: 'tool_action', details: { tool: 'x' } }])
    expect(activity.categories).toEqual([{ category: 'Uncategorized', count: 1, zdrCount: 0 }])
  })
})

describe('honesty invariant: no-source events never become a per-connection claim', () => {
  it('counts a source-less event in the workspace rollup but in no per-source bucket', () => {
    const events: AuditEvent[] = [
      // No `source`/`provider`/`resource` ⇒ unattributable to any connection.
      { event: 'tool_action', details: { tool: 'web_search', data_category: 'public' } },
      { event: 'tool_action', details: { tool: 'lookup', data_category: 'customer', source: 'hubspot' } },
    ]

    // Workspace rollup includes BOTH events.
    const activity = aggregateWorkspaceActivity(events)
    expect(activity.totalEvents).toBe(2)
    expect(activity.categories.find((c) => c.category === 'public')?.count).toBe(1)

    // Per-source aggregation includes ONLY the attributable event — the
    // source-less one must never create or inflate a per-connection bucket.
    const perSource = aggregateToolActions(events)
    expect(perSource.size).toBe(1)
    expect(perSource.has('hubspot')).toBe(true)
    const totalAttributed = [...perSource.values()].reduce((sum, s) => sum + s.count, 0)
    expect(totalAttributed).toBe(1)
  })
})
