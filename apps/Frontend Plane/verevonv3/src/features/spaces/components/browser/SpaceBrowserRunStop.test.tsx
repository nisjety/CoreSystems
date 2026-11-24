import { cleanup, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { ApiError } from '@/shared/api/api-error'

// WS20 / D21h: Stop on a live room browser run. A refusal a retry cannot change closes the control; a failure leaves it
// open for a retry. The stop calls are stubbed at the module boundary. No I18nProvider, so the copy is Norwegian.

const browserRuns = vi.hoisted(() => ({ stopSpaceBrowserRunAsStarter: vi.fn() }))
const runStop = vi.hoisted(() => ({ stopSpaceRun: vi.fn() }))
vi.mock('@/shared/api/spaces-client/browser-runs', () => browserRuns)
vi.mock('@/shared/api/spaces-client/run-stop', () => runStop)

const { SpaceBrowserRunStop } = await import('./SpaceBrowserRunStop')

const button = () => screen.getByRole('button', { name: 'Stopp kjøringen' }) as HTMLButtonElement

beforeEach(() => {
  browserRuns.stopSpaceBrowserRunAsStarter.mockReset()
  runStop.stopSpaceRun.mockReset()
})

afterEach(() => cleanup())

describe('SpaceBrowserRunStop: refusals close Stop (D21h, WS09-S8)', () => {
  it('closes Stop after the starter route says the viewer did not start the run', async () => {
    browserRuns.stopSpaceBrowserRunAsStarter.mockRejectedValue(new ApiError('no', 403, 'browser_run_not_yours'))
    render(() => <SpaceBrowserRunStop spaceRef="room-1" runId="run-1" asStarter />)
    button().click()
    expect((await screen.findByRole('alert')).textContent).toContain('Bare den som startet kjøringen')
    expect(button().disabled).toBe(true)
    button().click()
    expect(browserRuns.stopSpaceBrowserRunAsStarter).toHaveBeenCalledTimes(1)
  })

  it('closes Stop after the organisation refuses a manager stop', async () => {
    runStop.stopSpaceRun.mockRejectedValue(new ApiError('no', 403, 'space_run_control_not_authorized'))
    render(() => <SpaceBrowserRunStop spaceRef="room-1" runId="run-1" />)
    button().click()
    await screen.findByRole('alert')
    expect(button().disabled).toBe(true)
  })

  it('closes Stop after a retention refusal, a waived credential included', async () => {
    runStop.stopSpaceRun.mockRejectedValue(new ApiError('no', 403, 'zdr_waiver_route_mismatch'))
    render(() => <SpaceBrowserRunStop spaceRef="room-1" runId="run-1" />)
    button().click()
    expect((await screen.findByRole('alert')).textContent).toContain('nulllagring')
    expect(button().disabled).toBe(true)
  })

  it('leaves Stop open after a failure, and a retry names the same attempt', async () => {
    runStop.stopSpaceRun
      .mockRejectedValueOnce(new ApiError('down', 503, 'run_control_unavailable'))
      .mockResolvedValueOnce({ runId: 'run-1', status: 'cancelled', cancelled: true, receiptId: 'r-1' })
    render(() => <SpaceBrowserRunStop spaceRef="room-1" runId="run-1" />)
    button().click()
    expect((await screen.findByRole('alert')).textContent).toBe('Kunne ikke stoppe kjøringen. Prøv igjen.')
    expect(button().disabled).toBe(false)
    button().click()
    expect(await screen.findByText('Stopp sendt …')).toBeTruthy()
    await waitFor(() => expect(runStop.stopSpaceRun).toHaveBeenCalledTimes(2))
    expect(runStop.stopSpaceRun.mock.calls[1]?.[2]).toBe(runStop.stopSpaceRun.mock.calls[0]?.[2])
    expect(button().disabled).toBe(true)
  })
})
