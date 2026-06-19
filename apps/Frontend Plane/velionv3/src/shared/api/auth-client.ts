import { gatewayBaseUrl } from './config'
import { ApiError, requestJson } from './http'

export type SignUpInput = {
  email: string
  password: string
  name?: string
  phoneNumber?: string
  captchaToken?: string
}

export type SignInInput = {
  email: string
  password: string
}

export type VerifyTwoFactorInput = {
  code: string
  type?: 'totp' | 'backup-code'
  trustDevice?: boolean
}

export type EmailVerificationOtpInput = {
  email: string
  otp: string
}

export type PhoneVerificationOtpInput = {
  phoneNumber: string
  otp: string
}

export type AuthUser = {
  id: string
  email: string
  name: string
  emailVerified: boolean
  image?: string | null
  role?: string | null
}

export type OnboardingStatus = 'CREATED' | 'PROFILE_READY' | 'COMPLETED'

export type SessionData = {
  user: AuthUser
  org: { id: string; name: string; role: string } | null
  permissions?: string[]
  onboardingStatus?: OnboardingStatus
  status: 'authenticated'
}

export type PasswordStrength = {
  score: number
  isStrong?: boolean
  isCompromised?: boolean
  feedback?: string[]
}

export type SignInResult =
  | { user: AuthUser; twoFactorRedirect?: false }
  | { twoFactorRedirect: true; twoFactorMethods?: string[]; user?: never }

export async function signUp(input: SignUpInput): Promise<{ user: AuthUser }> {
  const { captchaToken, ...body } = input
  const headers = captchaToken ? { 'x-captcha-response': captchaToken } : undefined
  return requestJson('/api/v1/auth/sign-up', {
    method: 'POST',
    headers,
    body: JSON.stringify(body),
  })
}

export async function signIn(input: SignInInput): Promise<SignInResult> {
  return requestJson('/api/v1/auth/sign-in', {
    method: 'POST',
    body: JSON.stringify(input),
  })
}

export async function verifyTwoFactor(input: VerifyTwoFactorInput): Promise<{ success: true }> {
  const result = await requestJson<{ success: boolean; error?: string }>('/api/v1/auth/2fa/verify', {
    method: 'POST',
    body: JSON.stringify(input),
  })
  if (!result.success) {
    throw new Error(result.error || 'Invalid two-factor authentication code')
  }
  return { success: true }
}

export async function signOut(): Promise<void> {
  await requestJson('/api/v1/auth/sign-out', { method: 'POST' })
}

export async function getAuthSession(): Promise<{ user: AuthUser } | null> {
  try {
    return await requestJson('/api/v1/auth/session')
  } catch {
    return null
  }
}

export async function getCurrentSession(): Promise<SessionData> {
  return requestJson('/api/v1/session/current')
}

export async function getMe(): Promise<AuthUser> {
  return requestJson('/api/v1/me')
}

export async function getSessionContext(): Promise<{
  userId: string
  email?: string
  name?: string
  orgId?: string | null
  role?: string
  permissions?: string[]
  onboardingStatus?: OnboardingStatus
  orgs: Array<{ id: string; name: string; role: string }>
}> {
  return requestJson('/api/v1/me/session-context')
}

export async function sendEmailVerification(email: string, callbackUrl?: string): Promise<void> {
  await requestJson('/api/v1/auth/email-verification/send', {
    method: 'POST',
    body: JSON.stringify({ email, callbackURL: callbackUrl }),
  })
}

export async function verifyEmail(token: string): Promise<void> {
  await requestJson('/api/v1/auth/email-verification/verify', {
    method: 'POST',
    body: JSON.stringify({ token }),
  })
}

export async function sendEmailVerificationOtp(email: string): Promise<void> {
  const result = await requestJson<{ success: boolean; error?: string }>('/api/v1/auth/email-verification/otp/send', {
    method: 'POST',
    body: JSON.stringify({ email }),
  })
  if (!result.success) {
    throw new Error(result.error || 'Could not send verification code')
  }
}

export async function verifyEmailVerificationOtp(input: EmailVerificationOtpInput): Promise<{ user?: AuthUser }> {
  const result = await requestJson<{ success: boolean; user?: AuthUser; error?: string }>('/api/v1/auth/email-verification/otp/verify', {
    method: 'POST',
    body: JSON.stringify(input),
  }).catch((error: unknown) => {
    if (error instanceof ApiError && (error.status === 401 || error.code === 'INVALID_OTP')) {
      throw new Error('Invalid or expired verification code')
    }
    throw error
  })
  if (!result.success) {
    throw new Error(result.error || 'Invalid or expired verification code')
  }
  return { user: result.user }
}

export async function sendPhoneVerificationOtp(phoneNumber: string): Promise<void> {
  const result = await requestJson<{ success: boolean; error?: string }>('/api/v1/auth/phone-verification/otp/send', {
    method: 'POST',
    body: JSON.stringify({ phoneNumber }),
  })
  if (!result.success) {
    throw new Error(result.error || 'Could not send SMS verification code')
  }
}

export async function verifyPhoneVerificationOtp(input: PhoneVerificationOtpInput): Promise<{ user?: AuthUser }> {
  const result = await requestJson<{ success: boolean; user?: AuthUser; error?: string }>('/api/v1/auth/phone-verification/otp/verify', {
    method: 'POST',
    body: JSON.stringify(input),
  })
  if (!result.success) {
    throw new Error(result.error || 'Invalid or expired SMS verification code')
  }
  return { user: result.user }
}

export async function checkPasswordStrength(password: string): Promise<PasswordStrength> {
  return requestJson('/api/v1/auth/password/check-strength', {
    method: 'POST',
    body: JSON.stringify({ password }),
  })
}

export async function sendPasswordReset(email: string, redirectTo?: string): Promise<void> {
  await requestJson('/api/v1/auth/password/send-reset', {
    method: 'POST',
    body: JSON.stringify({ email, redirectTo }),
  })
}

export async function resetPassword(token: string, newPassword: string): Promise<void> {
  await requestJson('/api/v1/auth/password/reset', {
    method: 'POST',
    body: JSON.stringify({ token, newPassword }),
  })
}

// --- Enterprise SSO ------------------------------------------------------

/**
 * Build the gateway SSO-initiate URL. SSO is a server-side redirect flow (the
 * gateway calls Better Auth's `sign-in/sso` and 302s the browser to the IdP),
 * so callers navigate to this URL rather than fetching it — keeping the session
 * cookie first-party. The provider is resolved from the email's domain by the
 * SSO plugin. `callbackURL` is kept on the SPA origin so the onboarding-gated
 * router decides the landing route after the IdP round-trip.
 */
export function ssoInitiateUrl(email: string, callbackUrl: string): string {
  const params = new URLSearchParams({ email, callbackURL: callbackUrl })
  return `${gatewayBaseUrl()}/api/v1/auth/sso/initiate?${params.toString()}`
}

// --- Two-factor (TOTP) enrollment ---------------------------------------

export type TwoFactorEnableInput = {
  /** Account password — Better Auth requires re-auth to enable 2FA. */
  password: string
}

export type TwoFactorEnrollment = {
  /** otpauth:// URI to render as a QR code (or show as a manual key). */
  totpURI: string
  /** Single-use recovery codes shown once at enrollment. */
  backupCodes: string[]
}

type RawEnrollment = {
  totpURI?: string
  uri?: string
  backupCodes?: string[]
  error?: string
}

function normalizeEnrollment(raw: RawEnrollment): TwoFactorEnrollment {
  return {
    totpURI: raw.totpURI ?? raw.uri ?? '',
    backupCodes: Array.isArray(raw.backupCodes) ? raw.backupCodes : [],
  }
}

/**
 * Begin TOTP enrollment. Returns the otpauth URI to scan plus the initial set
 * of backup codes. Better Auth keeps the factor unverified until a TOTP code is
 * confirmed via {@link verifyTotpEnrollment}.
 */
export async function enableTwoFactor(input: TwoFactorEnableInput): Promise<TwoFactorEnrollment> {
  const raw = await requestJson<RawEnrollment>('/api/v1/auth/2fa/enable', {
    method: 'POST',
    body: JSON.stringify(input),
  })
  return normalizeEnrollment(raw)
}

/**
 * Fetch the otpauth URI for an in-progress enrollment without re-running enable
 * (e.g. to re-render the QR). Requires the account password.
 */
export async function getTotpUri(input: TwoFactorEnableInput): Promise<{ totpURI: string }> {
  const raw = await requestJson<RawEnrollment>('/api/v1/auth/2fa/get-totp-uri', {
    method: 'POST',
    body: JSON.stringify(input),
  })
  return { totpURI: raw.totpURI ?? raw.uri ?? '' }
}

/**
 * Confirm TOTP enrollment with a code from the authenticator app. On success
 * the factor is active and required at subsequent sign-ins.
 */
export async function verifyTotpEnrollment(code: string): Promise<{ success: true }> {
  await requestJson('/api/v1/auth/2fa/verify-totp', {
    method: 'POST',
    body: JSON.stringify({ code }),
  })
  return { success: true }
}

/** (Re)generate single-use backup codes. Requires the account password. */
export async function regenerateBackupCodes(input: TwoFactorEnableInput): Promise<string[]> {
  const raw = await requestJson<RawEnrollment>('/api/v1/auth/2fa/generate-backup-codes', {
    method: 'POST',
    body: JSON.stringify(input),
  })
  return Array.isArray(raw.backupCodes) ? raw.backupCodes : []
}
