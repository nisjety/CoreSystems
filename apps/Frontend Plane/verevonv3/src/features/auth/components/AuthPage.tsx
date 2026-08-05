import { useNavigate } from '@solidjs/router'
import {
  createEffect,
  createMemo,
  createSignal,
  onCleanup,
  onMount,
  Show,
} from 'solid-js'
import { AuthFormPanel } from '@/features/auth/components/sections/AuthFormPanel'
import { AuthVisualPanel } from '@/features/auth/components/sections/AuthVisualPanel'
import { AuthScreen } from '@/features/auth/components/shared/AuthScreen'
import { SOCIAL_PROVIDERS, getAuthCopy, type AuthMode, type Locale, type SocialProvider } from '@/features/auth/lib/model'
import { safeReturnTo } from '@/features/auth/lib/return-to'
import { gatewayBaseUrl } from '@/shared/api/config'
import {
  sendEmailVerificationOtp,
  sendPasswordReset,
  sendPhoneVerificationOtp,
  signIn,
  signUp,
  ssoInitiateUrl,
  resetPassword,
  verifyEmailVerificationOtp,
  verifyPhoneVerificationOtp,
  verifyTwoFactor,
} from '@/shared/api/auth-client'
import { ApiError } from '@/shared/api/http'
import { getSession, loadSession, setSessionUser } from '@/shared/session/session-store'

const EMAIL_RE = /^[^\s@]+@[^\s@]+\.[^\s@]+$/
const PHONE_RE = /^\+[1-9]\d{1,14}$/
type TwoFactorType = 'totp' | 'backup-code'
type RegistrationVerificationChannel = 'email' | 'phone'
type AuthNotice = { tone: 'success' | 'info'; message: string }
type PendingSignInCredentials = { email: string; password: string }

function normalizePhoneNumber(value: string): string {
  const trimmed = value.trim()
  if (!trimmed) return ''
  const withoutSpaces = trimmed.replace(/[\s().-]/g, '')
  return withoutSpaces.startsWith('+') ? `+${withoutSpaces.slice(1).replace(/\D/g, '')}` : withoutSpaces.replace(/\D/g, '')
}

function normalizeLocalPhoneNumber(value: string, dialCode: string): string {
  const trimmed = value.trim()
  const digits = trimmed.replace(/\D/g, '')
  const dialDigits = dialCode.replace(/\D/g, '')
  if (trimmed.startsWith('+') && dialDigits && digits.startsWith(dialDigits)) {
    return digits.slice(dialDigits.length)
  }
  return digits
}

function composePhoneNumber(dialCode: string, localNumber: string): string {
  return normalizePhoneNumber(`${dialCode}${normalizeLocalPhoneNumber(localNumber, dialCode)}`)
}

function supportedTwoFactorTypes(methods: readonly string[] | null | undefined): TwoFactorType[] {
  if (!methods?.length) return ['totp']
  const normalized = new Set(methods.map((method) => method.toLowerCase()))
  const types: TwoFactorType[] = []
  if (normalized.has('totp')) types.push('totp')
  if (normalized.has('backup-code') || normalized.has('backupcode')) types.push('backup-code')
  return types
}

export default function AuthPage() {
  const navigate = useNavigate()
  const [mode, setMode] = createSignal<AuthMode>('signin')
  const [locale, setLocale] = createSignal<Locale>('nb')
  const [email, setEmail] = createSignal('')
  const [password, setPassword] = createSignal('')
  const [name, setName] = createSignal('')
  const [phoneCountryCode, setPhoneCountryCode] = createSignal('+47')
  const [phoneNumber, setPhoneNumber] = createSignal('')
  const [captchaToken, setCaptchaToken] = createSignal('')
  const [showPassword, setShowPassword] = createSignal(false)
  const [viewportHeight, setViewportHeight] = createSignal<number | null>(null)
  const [contentHeight, setContentHeight] = createSignal<number | null>(null)
  const [pageVisible, setPageVisible] = createSignal(false)
  const [showConsent, setShowConsent] = createSignal(true)
  const [submitting, setSubmitting] = createSignal(false)
  const [resetSubmitting, setResetSubmitting] = createSignal(false)
  const [authError, setAuthError] = createSignal<string | null>(null)
  const [authNotice, setAuthNotice] = createSignal<AuthNotice | null>(null)
  const [passwordResetToken, setPasswordResetToken] = createSignal<string | null>(null)
  const [newPassword, setNewPassword] = createSignal('')
  const [newPasswordConfirm, setNewPasswordConfirm] = createSignal('')
  const [verifyEmailFor, setVerifyEmailFor] = createSignal<string | null>(null)
  const [verifyPhoneFor, setVerifyPhoneFor] = createSignal<string | null>(null)
  const [verificationChannel, setVerificationChannel] = createSignal<RegistrationVerificationChannel>('email')
  const [emailVerificationCode, setEmailVerificationCode] = createSignal('')
  const [phoneVerificationCode, setPhoneVerificationCode] = createSignal('')
  const [twoFactorMethods, setTwoFactorMethods] = createSignal<string[] | null>(null)
  const [twoFactorCode, setTwoFactorCode] = createSignal('')
  const [twoFactorType, setTwoFactorType] = createSignal<TwoFactorType>('totp')
  const [trustDevice, setTrustDevice] = createSignal(true)
  const [resendState, setResendState] = createSignal<'idle' | 'sending' | 'sent'>('idle')
  const [phoneResendState, setPhoneResendState] = createSignal<'idle' | 'sending' | 'sent'>('idle')
  const [ssoEmail, setSsoEmail] = createSignal('')
  const [ssoSubmitting, setSsoSubmitting] = createSignal(false)
  const [pendingSignInCredentials, setPendingSignInCredentials] = createSignal<PendingSignInCredentials | null>(null)
  let contentRef: HTMLDivElement | undefined

  const nb = () => locale() === 'nb'
  const returnTo = createMemo(() => {
    const params = new URLSearchParams(window.location.search)
    return safeReturnTo(params.get('returnTo'))
  })
  const postAuthDestination = () =>
    returnTo() ?? (getSession().onboardingStatus === 'COMPLETED' ? '/dashboard' : '/onboarding')
  const captchaSiteKey = createMemo(() => import.meta.env.VITE_TURNSTILE_SITE_KEY?.trim() ?? '')
  const clearAuthFeedback = () => {
    setAuthError(null)
    setAuthNotice(null)
  }
  const setAuthFailure = (message: string) => {
    setAuthNotice(null)
    setAuthError(message)
  }
  const setAuthStatus = (notice: AuthNotice) => {
    setAuthError(null)
    setAuthNotice(notice)
  }
  const setAuthMode = (nextMode: AuthMode) => {
    setMode(nextMode)
    setPendingSignInCredentials(null)
    clearAuthFeedback()
  }
  const handleEmailInput = (value: string) => {
    setEmail(value)
    clearAuthFeedback()
  }
  const handlePasswordInput = (value: string) => {
    setPassword(value)
    clearAuthFeedback()
  }
  const handleNameInput = (value: string) => {
    setName(value)
    clearAuthFeedback()
  }
  const handlePhoneInput = (value: string) => {
    setPhoneNumber(normalizeLocalPhoneNumber(value, phoneCountryCode()))
    clearAuthFeedback()
  }
  const handlePhoneCountryChange = (dialCode: string) => {
    setPhoneCountryCode(dialCode)
    setPhoneNumber((current) => normalizeLocalPhoneNumber(current, dialCode))
    clearAuthFeedback()
  }
  const handleSsoEmailInput = (value: string) => {
    setSsoEmail(value)
    clearAuthFeedback()
  }
  const validate = (): string | null => {
    if (!EMAIL_RE.test(email().trim())) return nb() ? 'Skriv inn en gyldig e-postadresse.' : 'Enter a valid email address.'
    if (!password()) return nb() ? 'Skriv inn passordet ditt.' : 'Enter your password.'
    if (mode() === 'signup' && password().length < 8) {
      return nb() ? 'Passordet må være minst 8 tegn.' : 'Password must be at least 8 characters.'
    }
    if (mode() === 'signup') {
      const cleanPhone = composePhoneNumber(phoneCountryCode(), phoneNumber())
      const cleanLocalPhone = normalizeLocalPhoneNumber(phoneNumber(), phoneCountryCode())
      if (!cleanLocalPhone) {
        return nb() ? 'Skriv inn telefonnummeret ditt.' : 'Enter your phone number.'
      }
      if (!PHONE_RE.test(cleanPhone)) {
        return nb()
          ? 'Skriv inn et gyldig telefonnummer for valgt land.'
          : 'Enter a valid phone number for the selected country.'
      }
      if (captchaSiteKey() && !captchaToken()) {
        return nb() ? 'Fullfør sikkerhetssjekken.' : 'Complete the security check.'
      }
    }
    return null
  }

  const copy = createMemo(() => getAuthCopy(locale(), mode()))
  const availableTwoFactorTypes = createMemo(() => supportedTwoFactorTypes(twoFactorMethods()))

  const cardScale = createMemo(() => {
    const height = viewportHeight()
    if (!height) return 1
    return Math.min(1, Math.max(0.52, (height - 18) / 1080))
  })

  const animatedContentHeight = createMemo(() => {
    const height = contentHeight()
    if (!height) return undefined
    return Math.max(0, height + 6)
  })

  onMount(() => {
    const params = new URLSearchParams(window.location.search)
    const resetToken = params.get('token')
    const resetError = params.get('error')
    if (resetToken) {
      setPasswordResetToken(resetToken)
      setMode('signin')
    } else if (window.location.pathname === '/reset-password' && resetError) {
      setMode('signin')
      setAuthFailure(
        nb()
          ? 'Tilbakestillingslenken er ugyldig eller utløpt. Be om en ny lenke.'
          : 'The reset link is invalid or expired. Request a new link.',
      )
    }
    if (window.location.pathname === '/reset-password' && (resetToken || resetError)) {
      // Reset tokens are bearer credentials. Keep the captured value only in
      // component memory and remove it from the address bar/history immediately.
      window.history.replaceState(null, '', '/reset-password')
    }

    const updateViewport = () => setViewportHeight(window.innerHeight)
    updateViewport()
    const frame = window.requestAnimationFrame(() => setPageVisible(true))
    window.addEventListener('resize', updateViewport)

    onCleanup(() => {
      window.cancelAnimationFrame(frame)
      window.removeEventListener('resize', updateViewport)
    })
  })

  createEffect(() => {
    mode()
    window.requestAnimationFrame(() => {
      if (contentRef) {
        setContentHeight(contentRef.scrollHeight)
      }
    })
  })

  const completeAuth = async () => {
    if (submitting()) return
    const validationError = validate()
    if (validationError) {
      setAuthFailure(validationError)
      return
    }
    setSubmitting(true)
    clearAuthFeedback()
    setTwoFactorMethods(null)
    const cleanEmail = email().trim()
    const currentPassword = password()
    try {
      if (mode() === 'signup') {
        const cleanPhone = composePhoneNumber(phoneCountryCode(), phoneNumber())
        await signUp({
          email: cleanEmail,
          password: currentPassword,
          name: name().trim() || undefined,
          phoneNumber: cleanPhone,
          captchaToken: captchaToken() || undefined,
        })
        // auth-core (Better Auth) requires a verified email before sign-in, and
        // sign-up does not mint a session cookie. Surface the verify state rather
        // than attempting a cookie-less session load that would bounce to /login.
        setResendState('idle')
        setPhoneResendState('idle')
        setEmailVerificationCode('')
        setPhoneVerificationCode('')
        setVerificationChannel('email')
        setVerifyEmailFor(cleanEmail)
        setVerifyPhoneFor(cleanPhone)
        setPendingSignInCredentials({ email: cleanEmail, password: currentPassword })
        setCaptchaToken('')
        return
      }
      const result = await signIn({ email: cleanEmail, password: currentPassword })
      if (result.twoFactorRedirect) {
        const methods = result.twoFactorMethods ?? []
        const supportedTypes = supportedTwoFactorTypes(methods)
        setTwoFactorMethods(methods)
        setTwoFactorType(supportedTypes[0] ?? 'totp')
        setTwoFactorCode('')
        clearAuthFeedback()
        return
      }
      setSessionUser(result.user)
      // Load the full session (org + onboarding status). Routing is onboarding-gated:
      // finished users land on /dashboard, everyone else resumes onboarding.
      await loadSession()
      navigate(postAuthDestination(), { replace: true })
    } catch (err) {
      if (err instanceof ApiError && err.code === 'EMAIL_NOT_VERIFIED') {
        setEmailVerificationCode('')
        setPhoneVerificationCode('')
        setVerificationChannel('email')
        setVerifyEmailFor(cleanEmail)
        setVerifyPhoneFor(null)
        setAuthNotice(null)
        await requestEmailVerificationCode(cleanEmail)
        return
      }
      if (err instanceof ApiError && err.status === 429) {
        setAuthFailure(
          nb()
            ? 'For mange innloggingsforsøk. Vent litt før du prøver igjen, eller tilbakestill passordet.'
            : 'Too many sign-in attempts. Wait a moment before trying again, or reset your password.',
        )
        return
      }
      setAuthFailure(err instanceof Error ? err.message : nb() ? 'Autentisering feilet' : 'Authentication failed')
    } finally {
      setSubmitting(false)
    }
  }

  const requestPasswordReset = async () => {
    if (resetSubmitting() || submitting()) return
    const target = email().trim()
    if (!EMAIL_RE.test(target)) {
      setAuthFailure(nb() ? 'Skriv inn e-postadressen din først.' : 'Enter your email address first.')
      return
    }

    setResetSubmitting(true)
    clearAuthFeedback()
    try {
      await sendPasswordReset(target, `${window.location.origin}/reset-password`)
      setAuthStatus({
        tone: 'info',
        message: nb()
          ? 'Hvis kontoen finnes, har vi sendt en lenke for å tilbakestille passordet.'
          : 'If the account exists, we sent a password reset link.',
      })
    } catch (err) {
      if (err instanceof ApiError && err.status === 429) {
        setAuthFailure(
          nb()
            ? 'For mange forsøk. Vent litt før du ber om ny lenke.'
            : 'Too many attempts. Wait a moment before requesting another link.',
        )
        return
      }
      setAuthFailure(err instanceof Error ? err.message : nb() ? 'Kunne ikke sende lenken.' : 'Could not send the link.')
    } finally {
      setResetSubmitting(false)
    }
  }

  const completePasswordReset = async (event: SubmitEvent) => {
    event.preventDefault()
    if (submitting()) return

    const token = passwordResetToken()
    if (!token) {
      setAuthFailure(
        nb()
          ? 'Tilbakestillingslenken mangler token. Be om en ny lenke.'
          : 'The reset link is missing a token. Request a new link.',
      )
      return
    }
    if (newPassword().length < 8) {
      setAuthFailure(nb() ? 'Passordet må være minst 8 tegn.' : 'Password must be at least 8 characters.')
      return
    }
    if (newPassword() !== newPasswordConfirm()) {
      setAuthFailure(nb() ? 'Passordene er ikke like.' : 'Passwords do not match.')
      return
    }

    setSubmitting(true)
    clearAuthFeedback()
    try {
      await resetPassword(token, newPassword())
      setPasswordResetToken(null)
      setNewPassword('')
      setNewPasswordConfirm('')
      setMode('signin')
      setAuthStatus({
        tone: 'success',
        message: nb()
          ? 'Passordet er oppdatert. Logg inn med det nye passordet.'
          : 'Your password has been updated. Sign in with the new password.',
      })
      navigate('/login', { replace: true })
    } catch (err) {
      setAuthFailure(err instanceof Error ? err.message : nb() ? 'Kunne ikke oppdatere passordet.' : 'Could not update password.')
    } finally {
      setSubmitting(false)
    }
  }

  const requestEmailVerificationCode = async (target: string) => {
    setResendState('sending')
    clearAuthFeedback()
    try {
      await sendEmailVerificationOtp(target)
      setResendState('sent')
    } catch {
      setResendState('idle')
      setAuthFailure(nb() ? 'Kunne ikke sende ny engangskode.' : 'Could not send a new verification code.')
    }
  }

  const resendVerification = async () => {
    const target = verifyEmailFor()
    if (!target || resendState() === 'sending') return
    await requestEmailVerificationCode(target)
  }

  const sendPhoneVerification = async () => {
    const target = verifyPhoneFor()
    if (!target || phoneResendState() === 'sending') return
    setPhoneResendState('sending')
    clearAuthFeedback()
    try {
      await sendPhoneVerificationOtp(target)
      setPhoneResendState('sent')
    } catch {
      setPhoneResendState('idle')
      setAuthFailure(nb() ? 'Kunne ikke sende SMS-kode.' : 'Could not send an SMS verification code.')
    }
  }

  const chooseVerificationChannel = (channel: RegistrationVerificationChannel) => {
    setVerificationChannel(channel)
    clearAuthFeedback()
    if (channel === 'phone' && phoneResendState() === 'idle') {
      void sendPhoneVerification()
    }
  }

  const resendSelectedVerification = () => {
    if (verificationChannel() === 'phone') {
      void sendPhoneVerification()
      return
    }
    void resendVerification()
  }

  const backToSignIn = () => {
    setPasswordResetToken(null)
    setNewPassword('')
    setNewPasswordConfirm('')
    setVerifyEmailFor(null)
    setVerifyPhoneFor(null)
    setVerificationChannel('email')
    setEmailVerificationCode('')
    setPhoneVerificationCode('')
    setTwoFactorMethods(null)
    setTwoFactorCode('')
    setTwoFactorType('totp')
    setResendState('idle')
    setPhoneResendState('idle')
    setPendingSignInCredentials(null)
    clearAuthFeedback()
    setMode('signin')
  }

  const completeEmailVerification = async () => {
    if (submitting()) return
    const target = verifyEmailFor()
    if (!target) return

    const code = emailVerificationCode().replace(/\D/g, '')
    if (code.length !== 6) {
      setAuthFailure(nb() ? 'Skriv inn den seks-sifrede koden.' : 'Enter the six-digit code.')
      return
    }

    setSubmitting(true)
    clearAuthFeedback()
    let emailVerified = false
    try {
      await verifyEmailVerificationOtp({ email: target, otp: code })
      emailVerified = true

      const credentials = pendingSignInCredentials()
      if (!credentials || credentials.email !== target || !credentials.password) {
        throw new Error('Missing pending sign-in credentials')
      }

      const result = await signIn({ email: target, password: credentials.password })
      if (result.twoFactorRedirect) {
        const methods = result.twoFactorMethods ?? []
        const supportedTypes = supportedTwoFactorTypes(methods)
        setVerifyEmailFor(null)
        setEmailVerificationCode('')
        setPendingSignInCredentials(null)
        setTwoFactorMethods(methods)
        setTwoFactorType(supportedTypes[0] ?? 'totp')
        setTwoFactorCode('')
        return
      }

      setPendingSignInCredentials(null)
      setSessionUser(result.user)
      await loadSession()
      navigate(postAuthDestination(), { replace: true })
    } catch (err) {
      if (emailVerified) {
        setVerifyEmailFor(null)
        setEmailVerificationCode('')
        setPendingSignInCredentials(null)
        setMode('signin')
        setAuthStatus({
          tone: 'success',
          message: nb()
            ? 'E-posten er bekreftet. Logg inn for å fortsette.'
            : 'Your email is verified. Sign in to continue.',
        })
        return
      }
      setAuthFailure(err instanceof Error ? err.message : nb() ? 'Ugyldig eller utløpt kode.' : 'Invalid or expired code.')
    } finally {
      setSubmitting(false)
    }
  }

  const completePhoneVerification = async () => {
    if (submitting()) return
    const target = verifyPhoneFor()
    if (!target) return

    const code = phoneVerificationCode().replace(/\D/g, '')
    if (code.length !== 6) {
      setAuthFailure(nb() ? 'Skriv inn den seks-sifrede SMS-koden.' : 'Enter the six-digit SMS code.')
      return
    }

    setSubmitting(true)
    clearAuthFeedback()
    try {
      const result = await verifyPhoneVerificationOtp({ phoneNumber: target, otp: code })
      if (result.user) setSessionUser(result.user)
      setPendingSignInCredentials(null)
      await loadSession()
      navigate(postAuthDestination(), { replace: true })
    } catch (err) {
      setAuthFailure(err instanceof Error ? err.message : nb() ? 'Ugyldig eller utløpt SMS-kode.' : 'Invalid or expired SMS code.')
    } finally {
      setSubmitting(false)
    }
  }

  const completeTwoFactor = async () => {
    if (submitting()) return
    const supportedTypes = availableTwoFactorTypes()
    if (!supportedTypes.length) {
      setAuthFailure(
        nb()
          ? 'Denne to-faktormetoden støttes ikke i Verevon ennå.'
          : 'This two-factor method is not supported in Verevon yet.',
      )
      return
    }

    const code = twoFactorCode().replace(/\s/g, '')
    if (!code) {
      setAuthFailure(nb() ? 'Skriv inn to-faktorkoden.' : 'Enter your two-factor code.')
      return
    }

    setSubmitting(true)
    clearAuthFeedback()
    try {
      await verifyTwoFactor({
        code,
        type: twoFactorType(),
        trustDevice: trustDevice(),
      })
      await loadSession()
      navigate(postAuthDestination(), { replace: true })
    } catch (err) {
      setAuthFailure(err instanceof Error ? err.message : nb() ? 'Ugyldig to-faktorkode.' : 'Invalid two-factor code.')
    } finally {
      setSubmitting(false)
    }
  }

  const handleSubmit = (event: SubmitEvent) => {
    event.preventDefault()
    void completeAuth()
  }

  const handleTwoFactorSubmit = (event: SubmitEvent) => {
    event.preventDefault()
    void completeTwoFactor()
  }

  const handleEmailVerificationSubmit = (event: SubmitEvent) => {
    event.preventDefault()
    if (verificationChannel() === 'phone') {
      void completePhoneVerification()
      return
    }
    void completeEmailVerification()
  }

  // Social sign-in is an OAuth redirect flow, not the email/password path. Send
  // the browser to the gateway's same-origin initiate endpoint, which hands off
  // to the provider. (Inactive providers are disabled and never reach here.)
  const handleSocialSignIn = (provider: SocialProvider) => {
    if (!provider.active || submitting()) return
    // callbackURL is where Better Auth lands the user after the provider round-trip;
    // keep it on the SPA origin so the session cookie stays first-party and the
    // onboarding-gated router decides /onboarding vs /dashboard.
    const callbackURL = `${window.location.origin}${returnTo() ?? '/onboarding'}`
    window.location.href = `${gatewayBaseUrl()}/api/v1/auth/oauth/${provider.id}?callbackURL=${encodeURIComponent(callbackURL)}`
  }

  // Enterprise SSO is a redirect flow like social sign-in: the gateway resolves
  // the IdP for the email's domain and 302s the browser to it. Same first-party
  // callback so the onboarding-gated router decides the landing route.
  const handleSsoSignIn = () => {
    if (ssoSubmitting()) return
    const target = ssoEmail().trim()
    if (!EMAIL_RE.test(target)) {
      setAuthFailure(nb() ? 'Skriv inn en gyldig e-postadresse.' : 'Enter a valid email address.')
      return
    }
    clearAuthFeedback()
    setSsoSubmitting(true)
    const callbackURL = `${window.location.origin}${returnTo() ?? '/onboarding'}`
    window.location.href = ssoInitiateUrl(target, callbackURL)
  }

  const twoFactorLabel = (type: TwoFactorType) => {
    if (type === 'backup-code') return nb() ? 'Gjenopprettingskode' : 'Backup code'
    return nb() ? 'Authenticator-kode' : 'Authenticator code'
  }

  const registrationVerificationCodeComplete = createMemo(() => {
    const code = verificationChannel() === 'phone' ? phoneVerificationCode() : emailVerificationCode()
    return code.replace(/\D/g, '').length === 6
  })

  const selectedResendState = createMemo(() => (
    verificationChannel() === 'phone' ? phoneResendState() : resendState()
  ))

  return (
    <AuthScreen
      cardScale={cardScale()}
      pageVisible={pageVisible()}
      left={
        <Show
          when={passwordResetToken()}
          fallback={
            <Show
              when={!verifyEmailFor()}
              fallback={
                <form class="auth-verify-notice" aria-live="polite" onSubmit={handleEmailVerificationSubmit}>
                  <h2 class="auth-verify-notice__title">{nb() ? 'Bekreft kontoen din' : 'Verify your account'}</h2>
                  <p class="auth-verify-notice__body">
                    {nb()
                      ? 'Velg hvordan du vil fullføre registreringen for '
                      : 'Choose how to finish registration for '}
                    <strong>{verifyEmailFor()}</strong>
                    .
                  </p>
                  <div class="auth-verify-notice__methods" role="tablist" aria-label={nb() ? 'Bekreftelsesmetode' : 'Verification method'}>
                    <button
                      type="button"
                      class={`auth-verify-notice__method ${verificationChannel() === 'email' ? 'auth-verify-notice__method--active' : ''}`}
                      onClick={() => chooseVerificationChannel('email')}
                    >
                      {nb() ? 'E-post' : 'Email'}
                    </button>
                    <button
                      type="button"
                      class={`auth-verify-notice__method ${verificationChannel() === 'phone' ? 'auth-verify-notice__method--active' : ''}`}
                      onClick={() => chooseVerificationChannel('phone')}
                    >
                      SMS
                    </button>
                  </div>
                  <Show
                    when={verificationChannel() === 'phone'}
                    fallback={
                      <>
                        <p class="auth-verify-notice__body">
                          {nb() ? 'Vi har sendt en seks-sifret engangskode til ' : 'We sent a six-digit one-time code to '}
                          <strong>{verifyEmailFor()}</strong>
                          {nb() ? '. Skriv inn koden for å fullføre registreringen.' : '. Enter the code to finish registration.'}
                        </p>
                        <label class="auth-verify-notice__field">
                          <span>{nb() ? 'Engangskode' : 'One-time code'}</span>
                          <input
                            class="auth-verify-notice__otp-input"
                            value={emailVerificationCode()}
                            onInput={(event) => setEmailVerificationCode(event.currentTarget.value.replace(/\D/g, '').slice(0, 6))}
                            type="text"
                            inputmode="numeric"
                            autocomplete="one-time-code"
                            maxlength="6"
                            placeholder="123456"
                          />
                        </label>
                      </>
                    }
                  >
                    <p class="auth-verify-notice__body">
                      {phoneResendState() === 'sent'
                        ? nb() ? 'Vi har sendt en seks-sifret SMS-kode til ' : 'We sent a six-digit SMS code to '
                        : nb() ? 'Vi sender en SMS-kode til ' : 'We will send an SMS code to '}
                      <strong>{verifyPhoneFor()}</strong>
                      .
                    </p>
                    <label class="auth-verify-notice__field">
                      <span>{nb() ? 'SMS-kode' : 'SMS code'}</span>
                      <input
                        class="auth-verify-notice__otp-input"
                        value={phoneVerificationCode()}
                        onInput={(event) => setPhoneVerificationCode(event.currentTarget.value.replace(/\D/g, '').slice(0, 6))}
                        type="text"
                        inputmode="numeric"
                        autocomplete="one-time-code"
                        maxlength="6"
                        placeholder="123456"
                      />
                    </label>
                  </Show>
                  <p class="auth-verify-notice__hint">
                    {nb() ? 'Koden utløper etter 5 minutter.' : 'The code expires after 5 minutes.'}
                  </p>
                  <Show when={authError()}>
                    {(msg) => <p class="onboarding-error" role="alert">{msg()}</p>}
                  </Show>
                  <button
                    type="submit"
                    class="auth-verify-notice__resend"
                    disabled={submitting() || !registrationVerificationCodeComplete()}
                  >
                    {submitting() ? nb() ? 'Bekrefter…' : 'Verifying…' : nb() ? 'Bekreft kode' : 'Verify code'}
                  </button>
                  <button
                    type="button"
                    class="auth-verify-notice__secondary"
                    disabled={selectedResendState() === 'sending'}
                    onClick={resendSelectedVerification}
                  >
                    {selectedResendState() === 'sent'
                      ? nb() ? 'Ny kode sendt' : 'New code sent'
                      : selectedResendState() === 'sending'
                        ? nb() ? 'Sender…' : 'Sending…'
                        : nb() ? 'Send kode på nytt' : 'Resend code'}
                  </button>
                  <button type="button" class="auth-verify-notice__back" onClick={backToSignIn}>
                    {nb() ? '← Tilbake til innlogging' : '← Back to sign in'}
                  </button>
                </form>
              }
            >
              <Show
                when={twoFactorMethods()}
                fallback={
                  <>
                    <Show when={authError()}>
                      {(msg) => <p class="onboarding-error" role="alert" style={{ padding: '0 1.5rem 0.5rem' }}>{msg()}</p>}
                    </Show>
                    <Show when={authNotice()}>
                      {(notice) => (
                        <p
                          class={`auth-status-notice auth-status-notice--${notice().tone}`}
                          role="status"
                          style={{ padding: '0 1.5rem 0.5rem' }}
                        >
                          {notice().message}
                        </p>
                      )}
                    </Show>
                    <AuthFormPanel
                      mode={mode}
                      locale={locale}
                      copy={copy}
                      email={email}
                      password={password}
                      name={name}
                      phoneCountryCode={phoneCountryCode}
                      phoneNumber={phoneNumber}
                      captchaSiteKey={captchaSiteKey}
                      showPassword={showPassword}
                      submitting={submitting}
                      contentHeight={animatedContentHeight()}
                      socialProviders={SOCIAL_PROVIDERS}
                      onSocialSignIn={handleSocialSignIn}
                      ssoEmail={ssoEmail}
                      ssoSubmitting={ssoSubmitting}
                      onSsoEmailInput={handleSsoEmailInput}
                      onSsoSignIn={handleSsoSignIn}
                      onSetMode={setAuthMode}
                      onToggleLocale={() => setLocale((current) => (current === 'nb' ? 'en' : 'nb'))}
                      onEmailInput={handleEmailInput}
                      onPasswordInput={handlePasswordInput}
                      onNameInput={handleNameInput}
                      onPhoneCountryChange={handlePhoneCountryChange}
                      onPhoneInput={handlePhoneInput}
                      onCaptchaToken={setCaptchaToken}
                      onTogglePassword={() => setShowPassword((current) => !current)}
                      onForgotPassword={() => void requestPasswordReset()}
                      onCompleteAuth={() => void completeAuth()}
                      passkeyEnabled={false}
                      onSubmit={handleSubmit}
                      onContentRef={(element) => {
                        contentRef = element
                      }}
                    />
                  </>
                }
              >
                <>
                  <form class="auth-verify-notice" aria-live="polite" onSubmit={handleTwoFactorSubmit}>
                    <h2 class="auth-verify-notice__title">
                      {nb() ? 'To-faktorbekreftelse' : 'Two-factor verification'}
                    </h2>
                    <p class="auth-verify-notice__body">
                      {nb()
                        ? 'Skriv inn koden fra authenticator-appen din for å fullføre innloggingen.'
                        : 'Enter the code from your authenticator app to complete sign in.'}
                    </p>
                    <Show when={availableTwoFactorTypes().length > 1}>
                      <div class="auth-verify-notice__methods" role="tablist" aria-label={nb() ? 'To-faktormetode' : 'Two-factor method'}>
                        <button
                          type="button"
                          class={`auth-verify-notice__method ${twoFactorType() === 'totp' ? 'auth-verify-notice__method--active' : ''}`}
                          onClick={() => setTwoFactorType('totp')}
                        >
                          {twoFactorLabel('totp')}
                        </button>
                        <button
                          type="button"
                          class={`auth-verify-notice__method ${twoFactorType() === 'backup-code' ? 'auth-verify-notice__method--active' : ''}`}
                          onClick={() => setTwoFactorType('backup-code')}
                        >
                          {twoFactorLabel('backup-code')}
                        </button>
                      </div>
                    </Show>
                    <label class="auth-verify-notice__field">
                      <span>{twoFactorLabel(twoFactorType())}</span>
                      <input
                        value={twoFactorCode()}
                        onInput={(event) => setTwoFactorCode(event.currentTarget.value)}
                        type="text"
                        inputmode={twoFactorType() === 'totp' ? 'numeric' : 'text'}
                        autocomplete="one-time-code"
                        placeholder={twoFactorType() === 'totp' ? '123456' : 'ABCD-EFGH'}
                      />
                    </label>
                    <label class="auth-verify-notice__trust">
                      <input
                        type="checkbox"
                        checked={trustDevice()}
                        onChange={(event) => setTrustDevice(event.currentTarget.checked)}
                      />
                      <span>{nb() ? 'Husk denne enheten i 30 dager' : 'Trust this device for 30 days'}</span>
                    </label>
                    <Show when={authError()}>
                      {(msg) => <p class="onboarding-error" role="alert">{msg()}</p>}
                    </Show>
                    <button
                      type="submit"
                      class="auth-verify-notice__resend"
                      disabled={submitting() || !availableTwoFactorTypes().length}
                    >
                      {submitting() ? nb() ? 'Bekrefter…' : 'Verifying…' : nb() ? 'Bekreft kode' : 'Verify code'}
                    </button>
                    <button type="button" class="auth-verify-notice__back" onClick={backToSignIn}>
                      {nb() ? '← Tilbake til innlogging' : '← Back to sign in'}
                    </button>
                  </form>
                </>
              </Show>
            </Show>
          }
        >
          <form class="auth-verify-notice" aria-live="polite" onSubmit={completePasswordReset}>
            <h2 class="auth-verify-notice__title">{nb() ? 'Tilbakestill passord' : 'Reset password'}</h2>
            <p class="auth-verify-notice__body">
              {nb()
                ? 'Velg et nytt passord for kontoen din.'
                : 'Choose a new password for your account.'}
            </p>
            <label class="auth-verify-notice__field">
              <span>{nb() ? 'Nytt passord' : 'New password'}</span>
              <input
                value={newPassword()}
                onInput={(event) => setNewPassword(event.currentTarget.value)}
                type="password"
                autocomplete="new-password"
                placeholder="••••••••"
              />
            </label>
            <label class="auth-verify-notice__field">
              <span>{nb() ? 'Bekreft nytt passord' : 'Confirm new password'}</span>
              <input
                value={newPasswordConfirm()}
                onInput={(event) => setNewPasswordConfirm(event.currentTarget.value)}
                type="password"
                autocomplete="new-password"
                placeholder="••••••••"
              />
            </label>
            <Show when={authError()}>
              {(msg) => <p class="onboarding-error" role="alert">{msg()}</p>}
            </Show>
            <button
              type="submit"
              class="auth-verify-notice__resend"
              disabled={submitting()}
            >
              {submitting()
                ? nb() ? 'Oppdaterer…' : 'Updating…'
                : nb() ? 'Oppdater passord' : 'Update password'}
            </button>
            <button type="button" class="auth-verify-notice__back" onClick={backToSignIn}>
              {nb() ? '← Tilbake til innlogging' : '← Back to sign in'}
            </button>
          </form>
        </Show>
      }
      right={
        <AuthVisualPanel
          copy={copy}
          showConsent={showConsent}
          onDismissConsent={() => setShowConsent(false)}
        />
      }
    />
  )
}
