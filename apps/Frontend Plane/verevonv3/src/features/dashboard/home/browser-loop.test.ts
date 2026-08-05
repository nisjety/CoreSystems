import { afterEach, describe, expect, it, vi } from 'vitest'
import { createBrowserLoopController } from './browser-loop'
import * as browserRunClient from '@/shared/api/browser-run-client'

describe('createBrowserLoopController — client-side loop (mode 1, unchanged)', () => {
  it('gates on pause and resumes when requestResume is called', async () => {
    const controller = createBrowserLoopController()
    expect(controller.begin('find flights')).toBe(true)
    controller.requestPause()

    const gatePromise = controller.gate()
    expect(controller.state().status).toBe('paused')

    controller.requestResume()
    expect(await gatePromise).toBe('continue')
  })

  it('gates to stopped when requestStop is called', async () => {
    const controller = createBrowserLoopController()
    controller.begin('find flights')
    controller.requestStop()

    expect(await controller.gate()).toBe('stopped')
    expect(controller.state().status).toBe('stopped')
  })

  it('does not call the network control endpoint without attachRun', () => {
    const spy = vi.spyOn(browserRunClient, 'controlBrowserAiRun')
    const controller = createBrowserLoopController()
    controller.begin('find flights')
    controller.requestPause()
    expect(spy).not.toHaveBeenCalled()
    spy.mockRestore()
  })
})

describe('createBrowserLoopController — durable server-side run (Phase 2, mode 2)', () => {
  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('requestPause/Resume/Stop call controlBrowserAiRun once a run is attached', () => {
    const spy = vi.spyOn(browserRunClient, 'controlBrowserAiRun').mockResolvedValue(undefined)
    const controller = createBrowserLoopController()
    controller.begin('find flights')
    controller.attachRun({ orgId: 'org_1', runId: 'run_1' })

    controller.requestPause()
    controller.requestResume()
    controller.requestStop()

    expect(spy).toHaveBeenNthCalledWith(1, 'org_1', 'run_1', 'pause')
    expect(spy).toHaveBeenNthCalledWith(2, 'org_1', 'run_1', 'resume')
    expect(spy).toHaveBeenNthCalledWith(3, 'org_1', 'run_1', 'stop')
  })

  it('onActionDispatched/onObservationReceived mirror markActing/markStepDone', () => {
    const controller = createBrowserLoopController()
    controller.begin('find flights')
    controller.attachRun({ orgId: 'org_1', runId: 'run_1' })

    controller.onActionDispatched()
    expect(controller.state().status).toBe('acting')

    controller.onObservationReceived()
    expect(controller.state().step).toBe(1)
  })

  it('onRunPaused/onRunResumed restore the pre-pause status', () => {
    const controller = createBrowserLoopController()
    controller.begin('find flights')
    controller.attachRun({ orgId: 'org_1', runId: 'run_1' })
    controller.onActionDispatched() // status: acting

    controller.onRunPaused()
    expect(controller.state().status).toBe('paused')

    controller.onRunResumed()
    expect(controller.state().status).toBe('acting')
  })

  it('finish clears the attached run so a later requestPause is a local no-op', () => {
    const spy = vi.spyOn(browserRunClient, 'controlBrowserAiRun').mockResolvedValue(undefined)
    const controller = createBrowserLoopController()
    controller.begin('find flights')
    controller.attachRun({ orgId: 'org_1', runId: 'run_1' })
    controller.finish('done')

    controller.requestPause()
    expect(spy).not.toHaveBeenCalled()
  })
})

describe('createBrowserLoopController — HITL approval gate (Phase 5)', () => {
  it('onApprovalRequired sets awaiting_approval while the loop is running', () => {
    const controller = createBrowserLoopController()
    controller.begin('find flights')
    controller.attachRun({ orgId: 'org_1', runId: 'run_1' })
    controller.onActionDispatched() // status: acting

    controller.onApprovalRequired()
    expect(controller.state().status).toBe('awaiting_approval')
  })

  it('is a no-op when the loop is not running', () => {
    const controller = createBrowserLoopController()
    controller.onApprovalRequired()
    expect(controller.state().status).toBe('idle')
  })

  it('onApprovalDecided(granted) returns to acting from awaiting_approval', () => {
    const controller = createBrowserLoopController()
    controller.begin('find flights')
    controller.attachRun({ orgId: 'org_1', runId: 'run_1' })
    controller.onApprovalRequired()

    controller.onApprovalDecided('granted')
    expect(controller.state().status).toBe('acting')
  })

  it('onApprovalDecided(denied) ends the loop and records a reason', () => {
    const spy = vi.spyOn(browserRunClient, 'controlBrowserAiRun').mockResolvedValue(undefined)
    const controller = createBrowserLoopController()
    controller.begin('find flights')
    controller.attachRun({ orgId: 'org_1', runId: 'run_1' })
    controller.onApprovalRequired()

    controller.onApprovalDecided('denied')
    expect(controller.state().status).toBe('stopped')
    expect(controller.state().error).toBe('Handlingen ble avslått.')

    // The run is treated as finished — a later local control call is a no-op.
    controller.requestPause()
    expect(spy).not.toHaveBeenCalled()
    spy.mockRestore()
  })

  it('onApprovalDecided(timed_out) ends the loop with a timeout-specific reason', () => {
    const controller = createBrowserLoopController()
    controller.begin('find flights')
    controller.attachRun({ orgId: 'org_1', runId: 'run_1' })
    controller.onApprovalRequired()

    controller.onApprovalDecided('timed_out')
    expect(controller.state().status).toBe('stopped')
    expect(controller.state().error).toBe('Handlingen fikk ikke godkjenning innen tidsfristen.')
  })
})
