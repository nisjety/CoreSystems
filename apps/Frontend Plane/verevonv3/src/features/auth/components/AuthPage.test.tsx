// @vitest-environment jsdom

import { createRouter, memoryHistory } from '@solidjs/router'
import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import AuthPage from './AuthPage'

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}

function renderAuthPage() {
  // AuthPage reads window.location.search/pathname directly (not via a router
  // hook) for its returnTo/reset-token handling, so the real browser URL must
  // still be set here in addition to the router's own memoryHistory below.
  window.history.pushState(null, '', '/login')
  const TestRouter = createRouter({
    routes: [
      { path: '/login', component: AuthPage },
      { path: '/reset-password', component: AuthPage },
      { path: '/onboarding', component: () => <div>Onboarding</div> },
      { path: '/dashboard', component: () => <div>Dashboard</div> },
    ],
    history: memoryHistory('/login'),
    explicitLinks: true,
  })
  return render(() => <TestRouter>{(props) => <>{props.children}</>}</TestRouter>)
}

function deferredResponse() {
  let resolve!: (response: Response) => void
  let reject!: (error: unknown) => void
  const promise = new Promise<Response>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise
    reject = rejectPromise
  })
  return { promise, resolve, reject }
}

describe('AuthPage email verification OTP flow', () => {
  afterEach(() => {
    cleanup()
    vi.unstubAllGlobals()
  })

  it('shows a one-time-code form after signup and verifies through the OTP endpoint', async () => {
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path === '/api/v1/auth/sign-up') {
        return jsonResponse({
          user: { id: 'user_1', email: 'ima@example.com', name: 'Ima', emailVerified: false },
        })
      }
      if (path === '/api/v1/auth/email-verification/otp/verify') {
        return jsonResponse({
          success: true,
          user: { id: 'user_1', email: 'ima@example.com', name: 'Ima', emailVerified: true },
        })
      }
      if (path === '/api/v1/auth/sign-in') {
        return jsonResponse({
          user: { id: 'user_1', email: 'ima@example.com', name: 'Ima', emailVerified: true },
        })
      }
      if (path === '/api/v1/auth/session') {
        return jsonResponse({
          user: { id: 'user_1', email: 'ima@example.com', name: 'Ima', emailVerified: true },
        })
      }
      if (path === '/api/v1/session/current') {
        return jsonResponse({
          user: { id: 'user_1', email: 'ima@example.com', name: 'Ima', emailVerified: true },
          org: null,
          permissions: [],
          onboardingStatus: 'CREATED',
          status: 'authenticated',
        })
      }
      throw new Error(`Unexpected request: ${path} ${init?.method ?? 'GET'}`)
    })
    vi.stubGlobal('fetch', fetchMock)

    renderAuthPage()

    fireEvent.click(screen.getByRole('button', { name: 'Registrer' }))
    flush()
    fireEvent.input(screen.getByLabelText('Fullt navn *'), { target: { value: 'Ima' } })
    flush()
    fireEvent.input(screen.getByLabelText('E-postadresse *'), { target: { value: 'ima@example.com' } })
    flush()
    fireEvent.input(screen.getByLabelText('Telefonnummer *'), { target: { value: '+47 123 45 678' } })
    flush()
    fireEvent.input(screen.getByLabelText('Passord *'), { target: { value: 'correct horse' } })
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Opprett konto' }))

    expect(await screen.findByText(/Velg hvordan/)).toBeTruthy()
    expect(screen.getByRole('button', { name: 'E-post' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'SMS' })).toBeTruthy()
    expect(screen.queryByText(/bekreftelseslenke/)).toBeNull()

    const signUpCall = fetchMock.mock.calls.find(([path]) => path === '/api/v1/auth/sign-up')
    expect(signUpCall).toBeTruthy()
    expect(JSON.parse(String(signUpCall?.[1]?.body))).toEqual({
      email: 'ima@example.com',
      password: 'correct horse',
      name: 'Ima',
      phoneNumber: '+4712345678',
    })

    fireEvent.input(screen.getByLabelText('Engangskode'), { target: { value: '313117' } })
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Bekreft kode' }))

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        '/api/v1/auth/email-verification/otp/verify',
        expect.objectContaining({ method: 'POST' }),
      )
    })

    const verifyCall = fetchMock.mock.calls.find(([path]) => path === '/api/v1/auth/email-verification/otp/verify')
    expect(verifyCall).toBeTruthy()
    expect(JSON.parse(String(verifyCall?.[1]?.body))).toEqual({ email: 'ima@example.com', otp: '313117' })

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        '/api/v1/auth/sign-in',
        expect.objectContaining({ method: 'POST' }),
      )
    })

    const signInCall = fetchMock.mock.calls.find(([path]) => path === '/api/v1/auth/sign-in')
    expect(signInCall).toBeTruthy()
    expect(JSON.parse(String(signInCall?.[1]?.body))).toEqual({
      email: 'ima@example.com',
      password: 'correct horse',
    })
  })

  it('shows verified email fallback as a success notice when post-verification sign-in fails', async () => {
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path === '/api/v1/auth/sign-up') {
        return jsonResponse({
          user: { id: 'user_1', email: 'ima@example.com', name: 'Ima', emailVerified: false },
        })
      }
      if (path === '/api/v1/auth/email-verification/otp/verify') {
        return jsonResponse({
          success: true,
          user: { id: 'user_1', email: 'ima@example.com', name: 'Ima', emailVerified: true },
        })
      }
      if (path === '/api/v1/auth/sign-in') {
        return jsonResponse({
          error: { code: 'AUTH_CORE_UNAVAILABLE', message: 'Auth service is temporarily unavailable' },
        }, 502)
      }
      throw new Error(`Unexpected request: ${path} ${init?.method ?? 'GET'}`)
    })
    vi.stubGlobal('fetch', fetchMock)

    renderAuthPage()

    fireEvent.click(screen.getByRole('button', { name: 'Registrer' }))
    flush()
    fireEvent.input(screen.getByLabelText('Fullt navn *'), { target: { value: 'Ima' } })
    flush()
    fireEvent.input(screen.getByLabelText('E-postadresse *'), { target: { value: 'ima@example.com' } })
    flush()
    fireEvent.input(screen.getByLabelText('Telefonnummer *'), { target: { value: '+47 123 45 678' } })
    flush()
    fireEvent.input(screen.getByLabelText('Passord *'), { target: { value: 'correct horse' } })
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Opprett konto' }))

    fireEvent.input(await screen.findByLabelText('Engangskode'), { target: { value: '313117' } })
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Bekreft kode' }))

    const status = await screen.findByRole('status')
    expect(status.textContent).toContain('E-posten er bekreftet. Logg inn for å fortsette.')
    expect(status.classList.contains('auth-status-notice--success')).toBe(true)
    expect(screen.queryByRole('alert')).toBeNull()
  })

  it('combines the selected country code with the local phone number on signup', async () => {
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path === '/api/v1/auth/sign-up') {
        return jsonResponse({
          user: { id: 'user_1', email: 'ima@example.com', name: 'Ima', emailVerified: false },
        })
      }
      throw new Error(`Unexpected request: ${path} ${init?.method ?? 'GET'}`)
    })
    vi.stubGlobal('fetch', fetchMock)

    renderAuthPage()

    fireEvent.click(screen.getByRole('button', { name: 'Registrer' }))
    flush()
    fireEvent.input(screen.getByLabelText('Fullt navn *'), { target: { value: 'Ima' } })
    flush()
    fireEvent.input(screen.getByLabelText('E-postadresse *'), { target: { value: 'ima@example.com' } })
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Landskode' }))
    flush()
    fireEvent.click(screen.getByRole('menuitemradio', { name: /USA.*\+1/ }))
    flush()
    fireEvent.input(screen.getByLabelText('Telefonnummer *'), { target: { value: '555 010 1234' } })
    flush()
    fireEvent.input(screen.getByLabelText('Passord *'), { target: { value: 'correct horse' } })
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Opprett konto' }))

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        '/api/v1/auth/sign-up',
        expect.objectContaining({ method: 'POST' }),
      )
    })

    const signUpCall = fetchMock.mock.calls.find(([path]) => path === '/api/v1/auth/sign-up')
    expect(JSON.parse(String(signUpCall?.[1]?.body))).toEqual({
      email: 'ima@example.com',
      password: 'correct horse',
      name: 'Ima',
      phoneNumber: '+15550101234',
    })
  })

  it('shows a disabled loading state while sign-in is in progress', async () => {
    const signIn = deferredResponse()
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path === '/api/v1/auth/sign-in') return signIn.promise
      if (path === '/api/v1/auth/session') {
        return jsonResponse({
          user: { id: 'user_1', email: 'ima@example.com', name: 'Ima', emailVerified: true },
        })
      }
      if (path === '/api/v1/session/current') {
        return jsonResponse({
          user: { id: 'user_1', email: 'ima@example.com', name: 'Ima', emailVerified: true },
          org: null,
          permissions: [],
          onboardingStatus: 'CREATED',
          status: 'authenticated',
        })
      }
      throw new Error(`Unexpected request: ${path} ${init?.method ?? 'GET'}`)
    })
    vi.stubGlobal('fetch', fetchMock)

    renderAuthPage()

    fireEvent.input(screen.getByLabelText('E-postadresse *'), { target: { value: 'ima@example.com' } })
    flush()
    fireEvent.input(screen.getByLabelText('Passord *'), { target: { value: 'correct horse' } })
    flush()
    const submitButton = screen
      .getAllByRole('button', { name: 'Logg inn' })
      .find((button) => (button as HTMLButtonElement).type === 'submit')
    expect(submitButton).toBeTruthy()
    fireEvent.click(submitButton as HTMLButtonElement)

    const loadingButton = await screen.findByRole('button', { name: /^Logger inn/ })
    expect((loadingButton as HTMLButtonElement).disabled).toBe(true)

    signIn.resolve(jsonResponse({
      user: { id: 'user_1', email: 'ima@example.com', name: 'Ima', emailVerified: true },
    }))

    await screen.findByText('Onboarding')
  })

  it('sends a fresh verification code when an existing account signs in before verifying email', async () => {
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path === '/api/v1/auth/sign-in') {
        return jsonResponse({ code: 'EMAIL_NOT_VERIFIED', message: 'Email not verified' }, 403)
      }
      if (path === '/api/v1/auth/email-verification/otp/send') {
        return jsonResponse({ success: true })
      }
      throw new Error(`Unexpected request: ${path} ${init?.method ?? 'GET'}`)
    })
    vi.stubGlobal('fetch', fetchMock)

    renderAuthPage()

    fireEvent.input(screen.getByLabelText('E-postadresse *'), { target: { value: 'ima@example.com' } })
    flush()
    fireEvent.input(screen.getByLabelText('Passord *'), { target: { value: 'correct horse' } })
    flush()
    const submitButton = screen
      .getAllByRole('button', { name: 'Logg inn' })
      .find((button) => (button as HTMLButtonElement).type === 'submit')
    fireEvent.click(submitButton as HTMLButtonElement)

    expect(await screen.findByText(/Velg hvordan/)).toBeTruthy()

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        '/api/v1/auth/email-verification/otp/send',
        expect.objectContaining({ method: 'POST' }),
      )
    })

    const sendCall = fetchMock.mock.calls.find(([path]) => path === '/api/v1/auth/email-verification/otp/send')
    expect(JSON.parse(String(sendCall?.[1]?.body))).toEqual({ email: 'ima@example.com' })
    expect(await screen.findByRole('button', { name: 'Ny kode sendt' })).toBeTruthy()
  })

  it('requests a password reset link from the sign-in form', async () => {
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      void init
      if (path === '/api/v1/auth/password/send-reset') {
        return jsonResponse({ success: true })
      }
      throw new Error(`Unexpected request: ${path}`)
    })
    vi.stubGlobal('fetch', fetchMock)

    renderAuthPage()

    fireEvent.input(screen.getByLabelText('E-postadresse *'), { target: { value: 'ima@example.com' } })
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Glemt passord?' }))

    await screen.findByRole('status')

    const resetCall = fetchMock.mock.calls.find(([path]) => path === '/api/v1/auth/password/send-reset')
    expect(resetCall).toBeTruthy()
    expect(JSON.parse(String(resetCall?.[1]?.body))).toEqual({
      email: 'ima@example.com',
      redirectTo: 'http://localhost:3000/reset-password',
    })
  })

  it('resets a password when opened with a reset token', async () => {
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      void init
      if (path === '/api/v1/auth/password/reset') {
        return jsonResponse({ success: true })
      }
      throw new Error(`Unexpected request: ${path}`)
    })
    vi.stubGlobal('fetch', fetchMock)
    // Same dual-source-of-truth as renderAuthPage() above: AuthPage strips the
    // token from the real address bar itself via a direct
    // window.history.replaceState call (not through the router), so the real
    // pushState here must stay alongside the router's own memoryHistory.
    window.history.pushState(null, '', '/reset-password?token=reset-token')

    const TestRouter = createRouter({
      routes: [
        { path: '/login', component: AuthPage },
        { path: '/reset-password', component: AuthPage },
      ],
      history: memoryHistory('/reset-password?token=reset-token'),
      explicitLinks: true,
    })

    render(() => <TestRouter>{(props) => <>{props.children}</>}</TestRouter>)

    await waitFor(() => expect(window.location.search).toBe(''))

    fireEvent.input(screen.getByLabelText('Nytt passord'), { target: { value: 'NewPassword!2026' } })
    flush()
    fireEvent.input(screen.getByLabelText('Bekreft nytt passord'), { target: { value: 'NewPassword!2026' } })
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Oppdater passord' }))

    await screen.findByRole('status')

    const resetCall = fetchMock.mock.calls.find(([path]) => path === '/api/v1/auth/password/reset')
    expect(resetCall).toBeTruthy()
    expect(JSON.parse(String(resetCall?.[1]?.body))).toEqual({
      token: 'reset-token',
      newPassword: 'NewPassword!2026',
    })
  })

  it('lets a newly registered user choose SMS verification', async () => {
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path === '/api/v1/auth/sign-up') {
        return jsonResponse({
          user: { id: 'user_1', email: 'ima@example.com', name: 'Ima', emailVerified: false },
        })
      }
      if (path === '/api/v1/auth/phone-verification/otp/send') {
        return jsonResponse({ success: true })
      }
      if (path === '/api/v1/auth/phone-verification/otp/verify') {
        return jsonResponse({
          success: true,
          user: { id: 'user_1', email: 'ima@example.com', name: 'Ima', emailVerified: false },
        })
      }
      if (path === '/api/v1/auth/session') {
        return jsonResponse({
          user: { id: 'user_1', email: 'ima@example.com', name: 'Ima', emailVerified: false },
        })
      }
      if (path === '/api/v1/session/current') {
        return jsonResponse({
          user: { id: 'user_1', email: 'ima@example.com', name: 'Ima', emailVerified: false },
          org: null,
          permissions: [],
          onboardingStatus: 'CREATED',
          status: 'authenticated',
        })
      }
      throw new Error(`Unexpected request: ${path} ${init?.method ?? 'GET'}`)
    })
    vi.stubGlobal('fetch', fetchMock)

    renderAuthPage()

    fireEvent.click(screen.getByRole('button', { name: 'Registrer' }))
    flush()
    fireEvent.input(screen.getByLabelText('Fullt navn *'), { target: { value: 'Ima' } })
    flush()
    fireEvent.input(screen.getByLabelText('E-postadresse *'), { target: { value: 'ima@example.com' } })
    flush()
    fireEvent.input(screen.getByLabelText('Telefonnummer *'), { target: { value: '+47 123 45 678' } })
    flush()
    fireEvent.input(screen.getByLabelText('Passord *'), { target: { value: 'correct horse' } })
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Opprett konto' }))

    fireEvent.click(await screen.findByRole('button', { name: 'SMS' }))
    flush()

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        '/api/v1/auth/phone-verification/otp/send',
        expect.objectContaining({ method: 'POST' }),
      )
    })

    fireEvent.input(screen.getByLabelText('SMS-kode'), { target: { value: '313117' } })
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Bekreft kode' }))

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        '/api/v1/auth/phone-verification/otp/verify',
        expect.objectContaining({ method: 'POST' }),
      )
    })

    const sendCall = fetchMock.mock.calls.find(([path]) => path === '/api/v1/auth/phone-verification/otp/send')
    expect(JSON.parse(String(sendCall?.[1]?.body))).toEqual({ phoneNumber: '+4712345678' })

    const verifyCall = fetchMock.mock.calls.find(([path]) => path === '/api/v1/auth/phone-verification/otp/verify')
    expect(JSON.parse(String(verifyCall?.[1]?.body))).toEqual({ phoneNumber: '+4712345678', otp: '313117' })
  })
})
