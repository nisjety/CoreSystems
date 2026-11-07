// @vitest-environment jsdom

/**
 * EP-1 give-back through the chat controller (C6 a): the live panel's Give back reaches the takeover
 * approval and the continue, and a refused continue is shown to the person without a second send.
 */

import { createRoot } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { ApiError } from '@/shared/api/api-error'
import type { Approval } from '@/shared/api/orchestration-client'

const { mockListApprovals, mockDecideApproval, mockContinueRun } = vi.hoisted(() => ({
  mockListApprovals: vi.fn(),
  mockDecideApproval: vi.fn(),
  mockContinueRun: vi.fn(),
}))

vi.mock('@/shared/api/orchestration-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/orchestration-client')>()
  return {
    ...actual,
    listApprovals: mockListApprovals,
    decideApproval: mockDecideApproval,
    continueRun: mockContinueRun,
  }
})

vi.mock('@/shared/api/chat-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-client')>()
  return {
    ...actual,
    streamChat: vi.fn(() => new Promise<void>(() => {})),
    listModels: vi.fn().mockResolvedValue([]),
    listChatThreads: vi.fn().mockResolvedValue([]),
    saveChatThreadSnapshot: vi.fn().mockResolvedValue(null),
    getChatThreadTranscript: vi.fn().mockResolvedValue(null),
    getThreadMessages: vi.fn().mockResolvedValue([]),
    cancelInvocation: vi.fn().mockResolvedValue(undefined),
    queueInvocationInput: vi.fn().mockResolvedValue({ outcome: 'queued', pending: 1, persisted: true }),
  }
})

vi.mock('@/shared/api/run-console-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/run-console-client')>()
  return { ...actual, getRunProofBundle: vi.fn().mockResolvedValue(null) }
})

import { useChatController } from './use-chat-controller'

const takeover: Approval = {
  id: 'apr-takeover',
  kind: 'APPROVAL_KIND_TOOL_CALL',
  status: 'PENDING',
  cause: {
    code: 'browser_human_assist',
    scopeOptions: ['once'],
    stepUpRequired: false,
    assistKind: 'user_takeover',
    assistOrigin: 'person',
  },
}

function withController<T>(fn: (controller: ReturnType<typeof useChatController>) => Promise<T>): Promise<T> {
  return new Promise((resolvePromise, rejectPromise) => {
    createRoot((dispose) => {
      const controller = useChatController()
      fn(controller).then(
        (value) => {
          dispose()
          resolvePromise(value)
        },
        (error) => {
          dispose()
          rejectPromise(error)
        },
      )
    })
  })
}

afterEach(() => {
  mockListApprovals.mockReset()
  mockDecideApproval.mockReset()
  mockContinueRun.mockReset()
})

describe('the chat controller hands the browser back', () => {
  it('decides the open takeover with the note, then continues the run', async () => {
    mockListApprovals.mockResolvedValue([takeover])
    mockDecideApproval.mockResolvedValue(null)
    mockContinueRun.mockResolvedValue(undefined)

    await withController((controller) => controller.giveBackBrowserRun('run_1', 'use the second result'))

    expect(mockListApprovals).toHaveBeenCalledWith('run_1')
    expect(mockDecideApproval).toHaveBeenCalledWith('apr-takeover', 'approve', 'use the second result')
    expect(mockContinueRun).toHaveBeenCalledWith(
      'run_1',
      'apr-takeover',
      expect.stringMatching(/^claim-/),
      expect.any(Function),
    )
  })

  it('tells the person a refused continue did not run, and sends it once', async () => {
    mockListApprovals.mockResolvedValue([takeover])
    mockDecideApproval.mockResolvedValue(null)
    mockContinueRun.mockRejectedValue(new ApiError('not granted', 409, 'approval_not_granted'))

    const notice = await withController(async (controller) => {
      await controller.giveBackBrowserRun('run_1', '')
      return controller.feedbackNotice()
    })

    expect(notice).toBe('Kjøringen kunne ikke fortsette etter at nettleseren ble gitt tilbake.')
    expect(mockContinueRun).toHaveBeenCalledTimes(1)
  })

  it('does nothing when the run has no open takeover', async () => {
    mockListApprovals.mockResolvedValue([])

    await withController((controller) => controller.giveBackBrowserRun('run_1', 'note'))

    expect(mockDecideApproval).not.toHaveBeenCalled()
    expect(mockContinueRun).not.toHaveBeenCalled()
  })
})
