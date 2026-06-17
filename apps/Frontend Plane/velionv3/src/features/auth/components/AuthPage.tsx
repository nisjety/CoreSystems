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
import { gatewayBaseUrl } from '@/shared/api/config'
import { sendEmailVerification, signIn, signUp, verifyTwoFactor } from '@/shared/api/auth-client'
import { ApiError } from '@/shared/api/http'
import { getSession, loadSession, setSessionUser } from '@/shared/session/session-store'

const EMAIL_RE = /^[^\s@]+@[^\s@]+\.[^\s@]+$/
type TwoFactorType = 'totp' | 'backup-code'

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
  const [showPassword, setShowPassword] = createSignal(false)
  const [viewportHeight, setViewportHeight] = createSignal<number | null>(null)
  const [contentHeight, setContentHeight] = createSignal<number | null>(null)
  const [pageVisible, setPageVisible] = createSignal(false)
  const [showConsent, setShowConsent] = createSignal(true)
  const [submitting, setSubmitting] = createSignal(false)
  const [authError, setAuthError] = createSignal<string | null>(null)
  const [verifyEmailFor, setVerifyEmailFor] = createSignal<string | null>(null)
  const [twoFactorMethods, setTwoFactorMethods] = createSignal<string[] | null>(null)
  const [twoFactorCode, setTwoFactorCode] = createSignal('')
  const [twoFactorType, setTwoFactorType] = createSignal<TwoFactorType>('totp')
  const [trustDevice, setTrustDevice] = createSignal(true)
  const [resendState, setResendState] = createSignal<'idle' | 'sending' | 'sent'>('idle')
  let contentRef: HTMLDivElement | undefined

  const nb = () => locale() === 'nb'
  const validate = (): string | null => {
    if (!EMAIL_RE.test(email().trim())) return nb() ? 'Skriv inn en gyldig e-postadresse.' : 'Enter a valid email address.'
    if (!password()) return nb() ? 'Skriv inn passordet ditt.' : 'Enter your password.'
    if (mode() === 'signup' && password().length < 8) {
      return nb() ? 'Passordet må være minst 8 tegn.' : 'Password must be at least 8 characters.'
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
    return Math.max(0, height + (mode() === 'signin' ? 6 : -24))
  })

  onMount(() => {
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
      setAuthError(validationError)
      return
    }
    setSubmitting(true)
    setAuthError(null)
    setTwoFactorMethods(null)
    const cleanEmail = email().trim()
    try {
      if (mode() === 'signup') {
        await signUp({ email: cleanEmail, password: password(), name: name().trim() || undefined })
        // auth-core (Better Auth) requires a verified email before sign-in, and
        // sign-up does not mint a session cookie. Surface the verify state rather
        // than attempting a cookie-less session load that would bounce to /login.
        setResendState('idle')
        setVerifyEmailFor(cleanEmail)
        return
      }
      const result = await signIn({ email: cleanEmail, password: password() })
      if (result.twoFactorRedirect) {
        const methods = result.twoFactorMethods ?? []
        const supportedTypes = supportedTwoFactorTypes(methods)
        setTwoFactorMethods(methods)
        setTwoFactorType(supportedTypes[0] ?? 'totp')
        setTwoFactorCode('')
        setAuthError(null)
        return
      }
      setSessionUser(result.user)
      // Load the full session (org + onboarding status). Routing is onboarding-gated:
      // finished users land on /dashboard, everyone else resumes onboarding.
      await loadSession()
      navigate(getSession().onboardingStatus === 'COMPLETED' ? '/dashboard' : '/onboarding', { replace: true })
    } catch (err) {
      if (err instanceof ApiError && err.code === 'EMAIL_NOT_VERIFIED') {
        setResendState('idle')
        setVerifyEmailFor(cleanEmail)
        return
      }
      setAuthError(err instanceof Error ? err.message : nb() ? 'Autentisering feilet' : 'Authentication failed')
    } finally {
      setSubmitting(false)
    }
  }

  const resendVerification = async () => {
    const target = verifyEmailFor()
    if (!target || resendState() === 'sending') return
    setResendState('sending')
    setAuthError(null)
    try {
      await sendEmailVerification(target, `${window.location.origin}/login`)
      setResendState('sent')
    } catch {
      setResendState('idle')
      setAuthError(nb() ? 'Kunne ikke sende verifiseringse-post.' : 'Could not send verification email.')
    }
  }

  const backToSignIn = () => {
    setVerifyEmailFor(null)
    setTwoFactorMethods(null)
    setTwoFactorCode('')
    setTwoFactorType('totp')
    setResendState('idle')
    setAuthError(null)
    setMode('signin')
  }

  const completeTwoFactor = async () => {
    if (submitting()) return
    const supportedTypes = availableTwoFactorTypes()
    if (!supportedTypes.length) {
      setAuthError(
        nb()
          ? 'Denne to-faktormetoden støttes ikke i Velion ennå.'
          : 'This two-factor method is not supported in Velion yet.',
      )
      return
    }

    const code = twoFactorCode().replace(/\s/g, '')
    if (!code) {
      setAuthError(nb() ? 'Skriv inn to-faktorkoden.' : 'Enter your two-factor code.')
      return
    }

    setSubmitting(true)
    setAuthError(null)
    try {
      await verifyTwoFactor({
        code,
        type: twoFactorType(),
        trustDevice: trustDevice(),
      })
      await loadSession()
      navigate(getSession().onboardingStatus === 'COMPLETED' ? '/dashboard' : '/onboarding', { replace: true })
    } catch (err) {
      setAuthError(err instanceof Error ? err.message : nb() ? 'Ugyldig to-faktorkode.' : 'Invalid two-factor code.')
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

  // Social sign-in is an OAuth redirect flow, not the email/password path. Send
  // the browser to the gateway's same-origin initiate endpoint, which hands off
  // to the provider. (Inactive providers are disabled and never reach here.)
  const handleSocialSignIn = (provider: SocialProvider) => {
    if (!provider.active || submitting()) return
    // callbackURL is where Better Auth lands the user after the provider round-trip;
    // keep it on the SPA origin so the session cookie stays first-party and the
    // onboarding-gated router decides /onboarding vs /dashboard.
    const callbackURL = `${window.location.origin}/onboarding`
    window.location.href = `${gatewayBaseUrl()}/api/v1/auth/oauth/${provider.id}?callbackURL=${encodeURIComponent(callbackURL)}`
  }

  const twoFactorLabel = (type: TwoFactorType) => {
    if (type === 'backup-code') return nb() ? 'Gjenopprettingskode' : 'Backup code'
    return nb() ? 'Authenticator-kode' : 'Authenticator code'
  }

  return (
    <AuthScreen
      cardScale={cardScale()}
      pageVisible={pageVisible()}
      left={
        <Show
          when={!verifyEmailFor()}
          fallback={
            <div class="auth-verify-notice" role="status" aria-live="polite">
              <h2 class="auth-verify-notice__title">{nb() ? 'Bekreft e-posten din' : 'Verify your email'}</h2>
              <p class="auth-verify-notice__body">
                {nb() ? 'Vi har sendt en bekreftelseslenke til ' : 'We sent a confirmation link to '}
                <strong>{verifyEmailFor()}</strong>
                {nb() ? '. Bekreft den for å logge inn.' : '. Confirm it to sign in.'}
              </p>
              <Show when={authError()}>
                {(msg) => <p class="onboarding-error" role="alert">{msg()}</p>}
              </Show>
              <button
                type="button"
                class="auth-verify-notice__resend"
                disabled={resendState() === 'sending'}
                onClick={() => void resendVerification()}
              >
                {resendState() === 'sent'
                  ? nb() ? 'Sendt på nytt ✓' : 'Resent ✓'
                  : resendState() === 'sending'
                    ? nb() ? 'Sender…' : 'Sending…'
                    : nb() ? 'Send bekreftelse på nytt' : 'Resend verification'}
              </button>
              <button type="button" class="auth-verify-notice__back" onClick={backToSignIn}>
                {nb() ? '← Tilbake til innlogging' : '← Back to sign in'}
              </button>
            </div>
          }
        >
          <Show
            when={twoFactorMethods()}
            fallback={
              <>
                <Show when={authError()}>
                  {(msg) => <p class="onboarding-error" role="alert" style={{ padding: '0 1.5rem 0.5rem' }}>{msg()}</p>}
                </Show>
                <AuthFormPanel
                  mode={mode}
                  locale={locale}
                  copy={copy}
                  email={email}
                  password={password}
                  name={name}
                  showPassword={showPassword}
                  contentHeight={animatedContentHeight()}
                  socialProviders={SOCIAL_PROVIDERS}
                  onSocialSignIn={handleSocialSignIn}
                  onSetMode={setMode}
                  onToggleLocale={() => setLocale((current) => (current === 'nb' ? 'en' : 'nb'))}
                  onEmailInput={setEmail}
                  onPasswordInput={setPassword}
                  onNameInput={setName}
                  onTogglePassword={() => setShowPassword((current) => !current)}
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
