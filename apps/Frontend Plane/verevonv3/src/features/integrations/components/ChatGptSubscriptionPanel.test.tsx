// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ChatGptSubscriptionPanel } from './ChatGptSubscriptionPanel'
import { ApiError } from '@/shared/api/http'
import { getChatGptSubscriptionStatus, startChatGptSubscription } from '@/shared/api/chatgpt-subscription-client'

vi.mock('@/shared/i18n', () => ({ useI18n: () => ({ tr: (_no: string, en: string) => en }) }))
vi.mock('@/shared/api/chatgpt-subscription-client', () => ({
  listChatGptSubscriptions: vi.fn().mockResolvedValue([]),
  startChatGptSubscription: vi.fn(),
  getChatGptSubscriptionStatus: vi.fn(),
  disconnectChatGptSubscription: vi.fn(),
}))

const connection = { id: 'conn-1', providerKey: 'openai-codex-subscription' as const, status: 'pending' }
function deviceLogin(id = 'login-1') {
  return {
    connection,
    login: {
      loginId: id, connectionId: connection.id, verificationUrl: 'https://auth.openai.com/codex/device',
      userCode: id === 'login-1' ? 'TEST-OLD' : 'TEST-NEW', status: 'pending' as const,
      expiresAt: new Date(Date.now() + 600_000).toISOString(),
    },
  }
}
async function settle() {
  await Promise.resolve()
  await Promise.resolve()
  flush()
}
async function start() {
  render(() => <ChatGptSubscriptionPanel orgId="org-1" />)
  await settle()
  fireEvent.click(screen.getByRole('button', { name: 'Connect ChatGPT' }))
  await settle()
}

describe('ChatGPT subscription recovery', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    vi.mocked(startChatGptSubscription).mockReset().mockResolvedValue(deviceLogin())
    vi.mocked(getChatGptSubscriptionStatus).mockReset()
    vi.spyOn(window, 'open').mockReturnValue(null)
  })
  afterEach(() => {
    cleanup()
    vi.useRealTimers()
    vi.restoreAllMocks()
  })

  it('keeps the code, link, and expiry when polling returns only login status', async () => {
    const initial = deviceLogin()
    vi.mocked(startChatGptSubscription).mockResolvedValueOnce(initial)
    // Integration Core sends the code and URL only when starting the login.
    vi.mocked(getChatGptSubscriptionStatus).mockResolvedValue({
      connection,
      login: { loginId: 'login-1', connectionId: 'conn-1', status: 'pending' },
    })
    await start()
    await vi.advanceTimersByTimeAsync(6_000)
    await settle()
    expect(getChatGptSubscriptionStatus).toHaveBeenCalledTimes(3)
    expect(screen.getByText('TEST-OLD')).toBeTruthy()
    expect(screen.getByRole('link', { name: 'Open ChatGPT sign-in' }).getAttribute('href')).toBe(initial.login.verificationUrl)

    vi.setSystemTime(new Date(initial.login.expiresAt))
    await vi.advanceTimersByTimeAsync(2_500)
    await settle()
    expect(screen.queryByText('TEST-OLD')).toBeNull()
    expect(screen.getByText(/code expired/)).toBeTruthy()
    expect(getChatGptSubscriptionStatus).toHaveBeenCalledTimes(3)
  })

  it('stops polling an expired server login and lets the user start again', async () => {
    vi.mocked(getChatGptSubscriptionStatus).mockRejectedValue(new ApiError('Expired', 410, 'subscription_login_expired'))
    await start()
    expect(screen.getByRole('link', { name: 'Open ChatGPT security settings' }).getAttribute('href')).toBe('https://chatgpt.com/#settings/Security')
    await vi.advanceTimersByTimeAsync(1000)
    await settle()
    expect(screen.queryByText('TEST-OLD')).toBeNull()
    expect(screen.getByText(/code has expired/)).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Connect ChatGPT' })).toBeTruthy()
    await vi.advanceTimersByTimeAsync(20_000)
    expect(getChatGptSubscriptionStatus).toHaveBeenCalledTimes(1)
  })

  it('ignores a late error from an old code after a new code is requested', async () => {
    let rejectOld!: (error: Error) => void
    vi.mocked(getChatGptSubscriptionStatus).mockImplementationOnce(() => new Promise((_resolve, reject) => { rejectOld = reject }))
    await start()
    await vi.advanceTimersByTimeAsync(1000)
    vi.mocked(startChatGptSubscription).mockResolvedValueOnce(deviceLogin('login-2'))
    fireEvent.click(screen.getByRole('button', { name: 'Get a new code' }))
    await settle()
    rejectOld(new ApiError('Expired', 410, 'subscription_login_expired'))
    await settle()
    expect(screen.getByText('TEST-NEW')).toBeTruthy()
    expect(screen.queryByText(/code has expired/)).toBeNull()
    vi.mocked(getChatGptSubscriptionStatus).mockResolvedValue(deviceLogin('login-2'))
    await vi.advanceTimersByTimeAsync(1000)
    expect(getChatGptSubscriptionStatus).toHaveBeenLastCalledWith('org-1', 'conn-1', 'login-2')
  })

  it('retries temporary errors but does not recreate polling after unmount', async () => {
    vi.mocked(getChatGptSubscriptionStatus).mockRejectedValueOnce(new ApiError('Unavailable', 503, null))
    await start()
    await vi.advanceTimersByTimeAsync(1000)
    await settle()
    expect(screen.getByText('TEST-OLD')).toBeTruthy()
    let rejectPending!: (error: Error) => void
    vi.mocked(getChatGptSubscriptionStatus).mockImplementationOnce(() => new Promise((_resolve, reject) => { rejectPending = reject }))
    await vi.advanceTimersByTimeAsync(4000)
    expect(getChatGptSubscriptionStatus).toHaveBeenCalledTimes(2)
    cleanup()
    rejectPending(new Error('offline'))
    await settle()
    await vi.advanceTimersByTimeAsync(20_000)
    expect(getChatGptSubscriptionStatus).toHaveBeenCalledTimes(2)
    expect(vi.getTimerCount()).toBe(0)
  })
})
