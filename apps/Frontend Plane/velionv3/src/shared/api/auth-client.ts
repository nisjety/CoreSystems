import { requestJson } from './http'

export type SignUpInput = {
  email: string
  password: string
  name?: string
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
  return requestJson('/api/v1/auth/sign-up', {
    method: 'POST',
    body: JSON.stringify(input),
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

export async function checkPasswordStrength(password: string): Promise<PasswordStrength> {
  return requestJson('/api/v1/auth/password/check-strength', {
    method: 'POST',
    body: JSON.stringify({ password }),
  })
}

export async function sendPasswordReset(email: string, callbackUrl?: string): Promise<void> {
  await requestJson('/api/v1/auth/password/send-reset', {
    method: 'POST',
    body: JSON.stringify({ email, callbackURL: callbackUrl }),
  })
}

export async function resetPassword(token: string, newPassword: string): Promise<void> {
  await requestJson('/api/v1/auth/password/reset', {
    method: 'POST',
    body: JSON.stringify({ token, newPassword }),
  })
}
