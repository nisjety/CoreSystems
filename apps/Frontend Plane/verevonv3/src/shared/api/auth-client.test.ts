import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  acceptOrganizationInvitation,
  getAuthSession,
  resetPassword,
  sendEmailVerificationOtp,
  sendPasswordReset,
  sendPhoneVerificationOtp,
  signUp,
  verifyEmailVerificationOtp,
  verifyPhoneVerificationOtp,
} from './auth-client'

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}

describe('auth client email verification OTP', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('sends signup captcha as a header and keeps it out of the JSON body', async () => {
    const fetchMock = vi.fn(async () => jsonResponse({
      user: { id: 'user_1', email: 'ima@example.com', name: 'Ima', emailVerified: false },
    }))
    vi.stubGlobal('fetch', fetchMock)

    await signUp({
      email: 'ima@example.com',
      password: 'correct horse',
      name: 'Ima',
      phoneNumber: '+4712345678',
      captchaToken: 'turnstile-token',
    })

    const [path, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit]
    const headers = new Headers(init.headers)
    expect(path).toBe('/api/v1/auth/sign-up')
    expect(headers.get('x-captcha-response')).toBe('turnstile-token')
    expect(JSON.parse(String(init.body))).toEqual({
      email: 'ima@example.com',
      password: 'correct horse',
      name: 'Ima',
      phoneNumber: '+4712345678',
    })
  })

  it('requests a new email-verification OTP through the gateway', async () => {
    const fetchMock = vi.fn(async () => jsonResponse({ success: true }))
    vi.stubGlobal('fetch', fetchMock)

    await sendEmailVerificationOtp('ima@example.com')

    const [path, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit]
    expect(path).toBe('/api/v1/auth/email-verification/otp/send')
    expect(init.method).toBe('POST')
    expect(init.credentials).toBe('include')
    expect(JSON.parse(String(init.body))).toEqual({ email: 'ima@example.com' })
  })

  it('verifies the email OTP and rejects failed upstream responses', async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(jsonResponse({ success: true, user: { id: 'user_1', email: 'ima@example.com', name: 'Ima', emailVerified: true } }))
      .mockResolvedValueOnce(jsonResponse({ success: false, error: 'Invalid or expired OTP' }))
      .mockResolvedValueOnce(jsonResponse({ success: false, error: 'Invalid OTP' }, 401))
    vi.stubGlobal('fetch', fetchMock)

    await expect(verifyEmailVerificationOtp({ email: 'ima@example.com', otp: '313117' })).resolves.toMatchObject({
      user: { email: 'ima@example.com', emailVerified: true },
    })

    const [path, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit]
    expect(path).toBe('/api/v1/auth/email-verification/otp/verify')
    expect(JSON.parse(String(init.body))).toEqual({ email: 'ima@example.com', otp: '313117' })

    await expect(verifyEmailVerificationOtp({ email: 'ima@example.com', otp: '000000' })).rejects.toThrow('Invalid or expired OTP')
    await expect(verifyEmailVerificationOtp({ email: 'ima@example.com', otp: '111111' })).rejects.toThrow(
      'Invalid or expired verification code',
    )
  })

  it('sends and verifies phone OTPs through the gateway', async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(jsonResponse({ success: true }))
      .mockResolvedValueOnce(jsonResponse({
        success: true,
        user: { id: 'user_1', email: 'ima@example.com', name: 'Ima', emailVerified: false },
      }))
    vi.stubGlobal('fetch', fetchMock)

    await sendPhoneVerificationOtp('+4712345678')
    await expect(verifyPhoneVerificationOtp({ phoneNumber: '+4712345678', otp: '313117' })).resolves.toMatchObject({
      user: { email: 'ima@example.com' },
    })

    const [sendPath, sendInit] = fetchMock.mock.calls[0] as unknown as [string, RequestInit]
    expect(sendPath).toBe('/api/v1/auth/phone-verification/otp/send')
    expect(JSON.parse(String(sendInit.body))).toEqual({ phoneNumber: '+4712345678' })

    const [verifyPath, verifyInit] = fetchMock.mock.calls[1] as unknown as [string, RequestInit]
    expect(verifyPath).toBe('/api/v1/auth/phone-verification/otp/verify')
    expect(JSON.parse(String(verifyInit.body))).toEqual({ phoneNumber: '+4712345678', otp: '313117' })
  })

  it('requests and completes password reset through the gateway', async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(jsonResponse({ success: true }))
      .mockResolvedValueOnce(jsonResponse({ success: true }))
    vi.stubGlobal('fetch', fetchMock)

    await sendPasswordReset('ima@example.com', 'http://localhost:5173/reset-password')
    await resetPassword('reset-token', 'NewPassword!2026')

    const [sendPath, sendInit] = fetchMock.mock.calls[0] as unknown as [string, RequestInit]
    expect(sendPath).toBe('/api/v1/auth/password/send-reset')
    expect(JSON.parse(String(sendInit.body))).toEqual({
      email: 'ima@example.com',
      redirectTo: 'http://localhost:5173/reset-password',
    })

    const [resetPath, resetInit] = fetchMock.mock.calls[1] as unknown as [string, RequestInit]
    expect(resetPath).toBe('/api/v1/auth/password/reset')
    expect(JSON.parse(String(resetInit.body))).toEqual({
      token: 'reset-token',
      newPassword: 'NewPassword!2026',
    })
  })

  it('accepts an organization invitation through the canonical gateway route', async () => {
    const fetchMock = vi.fn(async () => jsonResponse({
      invitation: { id: 'invite_123-abc', organizationId: 'org_1', status: 'accepted' },
      member: { id: 'member_1', organizationId: 'org_1', role: 'member' },
    }))
    vi.stubGlobal('fetch', fetchMock)

    await expect(acceptOrganizationInvitation('invite_123-abc')).resolves.toMatchObject({
      invitation: { organizationId: 'org_1', status: 'accepted' },
    })

    const [path, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit]
    expect(path).toBe('/api/v1/orgs/invitations/invite_123-abc/accept')
    expect(init.method).toBe('POST')
    expect(init.credentials).toBe('include')
  })

  it('can bypass Better Auth session cookie cache after an authority transition', async () => {
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      void input
      return jsonResponse({ user: { id: 'user_1' } })
    })
    vi.stubGlobal('fetch', fetchMock)

    await getAuthSession({ disableCookieCache: true })

    expect(fetchMock.mock.calls[0]?.[0]).toBe(
      '/api/v1/auth/session?disableCookieCache=true',
    )
  })
})
