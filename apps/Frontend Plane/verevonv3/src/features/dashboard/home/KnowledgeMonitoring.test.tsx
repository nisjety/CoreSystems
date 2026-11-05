// @vitest-environment jsdom
import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'

const { listMonitors } = vi.hoisted(() => ({ listMonitors: vi.fn() }))
vi.mock('@/shared/api/monitoring-client', () => ({ listMonitors }))
vi.mock('@/features/ingestions/components/IngestionsMonitoringView', () => ({
  IngestionsMonitoringView: (props: { orgId: string; monitors: unknown[] | null; onMonitorsChanged: () => Promise<void>; showWebhooks?: boolean }) => (
    <button type="button" data-webhooks={String(props.showWebhooks)} onClick={() => void props.onMonitorsChanged()}>{props.orgId}: {props.monitors?.length ?? 'loading'}</button>
  ),
}))
import { KnowledgeMonitoring } from './KnowledgeMonitoring'

describe('KnowledgeMonitoring', () => {
  afterEach(() => vi.resetAllMocks())
  it('loads the real monitoring list and refreshes it after a change', async () => {
    listMonitors.mockResolvedValueOnce([{ id: 'one' }]).mockResolvedValueOnce([{ id: 'one' }, { id: 'two' }])
    render(() => <KnowledgeMonitoring orgId="org-1" />)
    await waitFor(() => expect(screen.getByRole('button').textContent).toContain('org-1: 1'))
    expect(screen.getByRole('button').getAttribute('data-webhooks')).toBe('false')
    fireEvent.click(screen.getByRole('button'))
    await waitFor(() => expect(screen.getByRole('button').textContent).toContain('org-1: 2'))
  })
  it('shows an error rather than inventing an empty monitoring list', async () => {
    listMonitors.mockRejectedValue(new Error('unavailable'))
    render(() => <KnowledgeMonitoring orgId="org-1" />)
    await waitFor(() => expect(screen.getByRole('alert')).toBeTruthy())
    expect(screen.getByRole('button').textContent).toContain('loading')
  })
})
