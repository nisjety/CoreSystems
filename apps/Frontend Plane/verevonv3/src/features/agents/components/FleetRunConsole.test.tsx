import { cleanup, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { I18nProvider } from '@/shared/i18n'
import { localeStorageKey } from '@/shared/i18n/locales'
import { FleetRunConsole } from './FleetRunConsole'

// The fleet is the children of the parent run, read from its thread's lineage (EP-2). Each child's own status is
// read from its run, and the parent's status decides whether the fleet can still be stopped.
const parentRunId = 'run_parent'
const children = [
  { child_run_id: 'run_11111111', goal_summary: 'Summarise the first source', status: 'attached' },
  { child_run_id: 'run_22222222', goal_summary: 'Summarise the second source', status: 'attached' },
  { child_run_id: 'run_33333333', goal_summary: 'Summarise the third source', status: 'attached' },
]
const runStatuses: Record<string, string> = {
  [parentRunId]: 'running',
  run_11111111: 'running',
  run_22222222: 'completed',
  run_33333333: 'queued',
}

function stubFleetFetch() {
  const fetchSpy = vi.fn(async (url: string) => {
    if (url.includes('/lineage')) {
      const edges = children.map((child) => ({ parent_run_id: parentRunId, ...child }))
      return { ok: true, json: async () => ({ lineage: { edges } }) } as Response
    }
    const runId = /\/agents\/runs\/([^/?]+)/.exec(url)?.[1] ?? ''
    const status = runStatuses[runId]
    if (status === undefined) {
      return { ok: false, status: 404, json: async () => ({ error: { code: 'run_not_found', message: 'not found' } }) } as Response
    }
    return { ok: true, json: async () => ({ run: { run_id: runId, status } }) } as Response
  })
  vi.stubGlobal('fetch', fetchSpy as unknown as typeof fetch)
}

describe('FleetRunConsole', () => {
  afterEach(() => {
    cleanup()
    window.localStorage.removeItem(localeStorageKey)
  })

  it('renders one row per child run of the lineage, with its goal, its link and its own run status', async () => {
    stubFleetFetch()

    const { container } = render(() => <FleetRunConsole threadId="thread_abc" runId={parentRunId} />)

    expect(await screen.findByText('Flåte run_parent')).toBeTruthy()
    expect(await screen.findByText('3 underkjøringer')).toBeTruthy()
    await waitFor(() => expect(container.querySelectorAll('[data-run-id]')).toHaveLength(3))
    expect(screen.getByText('Summarise the second source')).toBeTruthy()
    // The row shows the edge's own status and the child run's status, read from the run itself.
    const second = container.querySelector('[data-run-id="run_22222222"]')
    expect(second?.textContent).toContain('Koblet')
    expect(second?.textContent).toContain('Ferdig')
  })

  it('labels the fleet and its stop control in Norwegian under no', async () => {
    window.localStorage.setItem(localeStorageKey, 'no')
    stubFleetFetch()

    render(() => (
      <I18nProvider>
        <FleetRunConsole threadId="thread_abc" runId={parentRunId} />
      </I18nProvider>
    ))

    expect(await screen.findByText('Flåte run_parent')).toBeTruthy()
    expect(await screen.findByText('3 underkjøringer')).toBeTruthy()
    expect(await screen.findByRole('button', { name: 'Stopp alle' })).toBeTruthy()
  })

  it('labels the fleet and its stop control in English under en', async () => {
    window.localStorage.setItem(localeStorageKey, 'en')
    stubFleetFetch()

    render(() => (
      <I18nProvider>
        <FleetRunConsole threadId="thread_abc" runId={parentRunId} />
      </I18nProvider>
    ))

    expect(await screen.findByText('Fleet run_parent')).toBeTruthy()
    expect(await screen.findByText('3 child runs')).toBeTruthy()
    expect((await screen.findAllByText('Link: Attached')).length).toBe(3)
    expect(await screen.findByRole('button', { name: 'Stop all' })).toBeTruthy()
  })
})
