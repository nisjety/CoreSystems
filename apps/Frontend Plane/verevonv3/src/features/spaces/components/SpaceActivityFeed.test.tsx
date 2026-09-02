import { render, screen } from '@solidjs/testing-library'
import { describe, expect, it } from 'vitest'

import { SpaceActivityFeed } from './SpaceActivityFeed'
import type { ThreadLikeActivitySource } from '@/features/spaces/lib/activity-grammar'

const threads: ThreadLikeActivitySource[] = [
  {
    thread_id: 'thr_done',
    title: 'Ferdig sak',
    latest_run_id: 'run_done',
    latest_run_status: 'completed',
    latest_run_updated_at: '2026-08-13T12:00:00Z',
  },
  {
    thread_id: 'thr_block',
    title: 'Fakturakontroll',
    latest_run_id: 'run_block',
    latest_run_status: 'awaiting_approval',
    latest_run_updated_at: '2026-08-12T09:00:00Z',
  },
]

describe('SpaceActivityFeed', () => {
  it('renders each item as a verb, object and outcome sentence', () => {
    render(() => <SpaceActivityFeed threads={threads} />)
    expect(screen.getAllByText('Kjøring').length).toBeGreaterThan(0)
    expect(screen.getByText('venter på godkjenning')).toBeTruthy()
    expect(screen.getAllByText('Fakturakontroll').length).toBeGreaterThan(0)
  })

  it('puts the item needing a human first, above a newer completed run', () => {
    const { container } = render(() => <SpaceActivityFeed threads={threads} />)
    const rows = container.querySelectorAll('.verevon-activity-row')
    expect(rows[0]?.textContent).toContain('venter på godkjenning')
    expect(rows[0]?.className).toContain('verevon-activity-row--critical')
  })

  it('marks a still-moving row as live so it can mutate in place', () => {
    const { container } = render(() => (
      <SpaceActivityFeed threads={[{ thread_id: 't', title: 'Pågår', latest_run_status: 'running' }]} />
    ))
    expect(container.querySelector('.verevon-activity-row--live')).toBeTruthy()
    expect(container.querySelector('[aria-live="polite"]')?.textContent).toBe('kjører')
  })

  it('shows an unrecognized status verbatim rather than hiding the row', () => {
    render(() => (
      <SpaceActivityFeed
        threads={[{ thread_id: 't', title: 'Ukjent', latest_run_status: 'brand_new_state' }]}
      />
    ))
    expect(screen.getByText('brand_new_state')).toBeTruthy()
  })

  it('never renders a raw thread id as the object', () => {
    const { container } = render(() => <SpaceActivityFeed threads={[{ thread_id: 'thr_secret_id' }]} />)
    const objects = container.querySelectorAll('.verevon-activity-object')
    for (const node of objects) {
      expect(node.textContent).not.toContain('thr_secret_id')
    }
    expect(screen.getByText('Uten tittel')).toBeTruthy()
  })

  it('links an item to its conversation in the owning Space', () => {
    const { container } = render(() => (
      <SpaceActivityFeed threads={[{ thread_id: 'thr_1', title: 'A', space_id: 'space_1' }]} />
    ))
    const link = container.querySelector('.verevon-activity-link')
    expect(link?.getAttribute('href')).toBe('/spaces/space_1?thread_id=thr_1')
  })

  /**
   * The old fallback linked an owner-less item into `/chat`, which contradicted
   * the very comment above it ("a foreign Space thread cannot be adopted by
   * Chat through a new activity link") and would have handed Chat a thread it
   * does not own. Unknown owner now means no link.
   */
  it('renders no link when the owning Space is unknown', () => {
    const { container } = render(() => <SpaceActivityFeed threads={[{ thread_id: 'thr_1', title: 'A' }]} />)
    const row = container.querySelector('.verevon-activity-link')
    // Still rendered — the row must stay readable — but not as a link.
    expect(row).toBeTruthy()
    expect(row?.tagName.toLowerCase()).toBe('span')
    expect(row?.getAttribute('href')).toBeNull()
    expect(container.querySelector('a.verevon-activity-link')).toBeNull()
  })

  it('says so plainly when the space has no activity', () => {
    render(() => <SpaceActivityFeed threads={[]} />)
    expect(screen.getByText('Ingen aktivitet i dette rommet ennå.')).toBeTruthy()
  })
})
