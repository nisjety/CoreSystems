import type { JSX } from 'solid-js'

export type AuthMode = 'signin' | 'signup'
export type Locale = 'nb' | 'en'

export type AuthCopy = {
  back: string
  signin: string
  signup: string
  title: string
  description: string
  emailLabel: string
  passwordLabel: string
  nameLabel: string
  primaryAction: string
  divider: string
  passkey: string
  ssoTitle: string
  ssoPlaceholder: string
  ssoAction: string
  ssoSubmitting: string
  terms: string
  support: string
  cookies: string
  decline: string
  accept: string
}

export type SocialProvider = {
  /** Better Auth provider id, used for the OAuth initiate endpoint. */
  id: string
  name: string
  active: boolean
  icon: JSX.Element
}

export const SOCIAL_PROVIDERS: readonly SocialProvider[] = [
  {
    id: 'microsoft',
    name: 'Microsoft',
    active: true,
    icon: (
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <path fill="#F25022" d="M2 2h9.25v9.25H2z" />
        <path fill="#7FBA00" d="M12.75 2H22v9.25h-9.25z" />
        <path fill="#00A4EF" d="M2 12.75h9.25V22H2z" />
        <path fill="#FFB900" d="M12.75 12.75H22V22h-9.25z" />
      </svg>
    ),
  },
  {
    id: 'google',
    name: 'Google',
    active: true,
    icon: (
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <path
          fill="#4285F4"
          d="M22.56 12.25c0-.78-.07-1.53-.2-2.25H12v4.26h5.92c-.26 1.37-1.04 2.53-2.21 3.31v2.77h3.57c2.08-1.92 3.28-4.74 3.28-8.09z"
        />
        <path
          fill="#34A853"
          d="M12 23c2.97 0 5.46-.98 7.28-2.66l-3.57-2.77c-.98.66-2.23 1.06-3.71 1.06-2.86 0-5.29-1.93-6.16-4.53H2.18v2.84C3.99 20.53 7.7 23 12 23z"
        />
        <path
          fill="#FBBC05"
          d="M5.84 14.09c-.22-.66-.35-1.36-.35-2.09s.13-1.43.35-2.09V7.07H2.18C1.43 8.55 1 10.22 1 12s.43 3.45 1.18 4.93l2.85-2.22.81-.62z"
        />
        <path
          fill="#EA4335"
          d="M12 5.38c1.62 0 3.06.56 4.21 1.64l3.15-3.15C17.45 2.09 14.97 1 12 1 7.7 1 3.99 3.47 2.18 7.07l3.66 2.84c.87-2.6 3.3-4.53 6.16-4.53z"
        />
      </svg>
    ),
  },
  {
    id: 'apple',
    name: 'Apple',
    active: false,
    icon: (
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <path
          fill="#111111"
          d="M16.3 2.3c0 1.2-.5 2.3-1.3 3.1-.8.9-2 1.5-3.1 1.4-.1-1.1.4-2.3 1.2-3.1.8-.9 2.1-1.5 3.2-1.4zM20.8 17.4c-.5 1.2-.8 1.8-1.5 2.9-1 1.5-2.4 3.4-4.1 3.4-1.5 0-1.9-1-3.9-1s-2.5 1-4 1c-1.7 0-3-1.7-4-3.2C.5 16.3.2 11.3 2.1 8.5 3.4 6.6 5.5 5.4 7.5 5.4c1.9 0 3.1 1 3.9 1 .7 0 2.2-1.2 4.6-1 1 .1 3.7.4 5.4 2.9-4.8 2.6-4 9.4-.6 9.1z"
        />
      </svg>
    ),
  },
  {
    id: 'okta',
    name: 'Okta',
    active: false,
    icon: (
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <path
          fill="currentColor"
          d="M12 1.5A10.5 10.5 0 1 0 22.5 12 10.512 10.512 0 0 0 12 1.5Zm0 16.2a5.7 5.7 0 1 1 5.7-5.7A5.707 5.707 0 0 1 12 17.7Zm0-2.7a3 3 0 1 0-3-3 3.005 3.005 0 0 0 3 3Z"
        />
      </svg>
    ),
  },
  {
    id: 'vipps',
    name: 'Vipps',
    active: false,
    icon: (
      <svg viewBox="43 24 28 23" aria-hidden="true">
        <path
          fill="#FF5B24"
          d="M57.3 40.6c3.7 0 5.8-1.8 7.8-4.4 1.1-1.4 2.5-1.7 3.5-.9s1.1 2.3 0 3.7c-2.9 3.8-6.6 6.1-11.3 6.1-5.1 0-9.6-2.8-12.7-7.7-.9-1.3-.7-2.7.3-3.4s2.5-.4 3.4 1c2.2 3.3 5.2 5.6 9 5.6zM64.2 28.3c0 1.8-1.4 3-3 3s-3-1.2-3-3 1.4-3 3-3c1.6 0 3 1.3 3 3z"
        />
      </svg>
    ),
  },
]

export function getAuthCopy(locale: Locale, mode: AuthMode): AuthCopy {
  if (locale === 'en') {
    return {
      back: 'BACK',
      signin: 'Sign in',
      signup: 'Register',
      title: mode === 'signin' ? 'Sign In' : 'Create Account',
      description:
        mode === 'signin'
          ? 'Welcome back! Sign in to your account.'
          : 'Create your account to continue into Verevon.',
      emailLabel: 'Email address *',
      passwordLabel: 'Password *',
      nameLabel: 'Full name *',
      primaryAction: mode === 'signin' ? 'Sign in' : 'Create account',
      divider: 'or with',
      passkey: 'Use passkey',
      ssoTitle: 'Single sign-on',
      ssoPlaceholder: 'you@company.com',
      ssoAction: 'Sign in with SSO',
      ssoSubmitting: 'Redirecting…',
      terms: 'By continuing you accept our terms of use and privacy policy.',
      support: 'Need help? Contact Support',
      cookies:
        'By clicking "Accept", you accept storage of information cookies on your device.',
      decline: 'Decline',
      accept: 'Accept',
    }
  }

  return {
    back: 'TILBAKE',
    signin: 'Logg inn',
    signup: 'Registrer',
    title: mode === 'signin' ? 'Logg Inn' : 'Opprett Konto',
    description:
      mode === 'signin'
        ? 'Velkommen tilbake! Logg inn på kontoen din.'
        : 'Opprett kontoen din for å fortsette inn i Verevon.',
    emailLabel: 'E-postadresse *',
    passwordLabel: 'Passord *',
    nameLabel: 'Fullt navn *',
    primaryAction: mode === 'signin' ? 'Logg inn' : 'Opprett konto',
    divider: 'eller med',
    passkey: 'Bruk passkey',
    ssoTitle: 'Enkel pålogging (SSO)',
    ssoPlaceholder: 'deg@bedrift.no',
    ssoAction: 'Logg inn med SSO',
    ssoSubmitting: 'Sender videre…',
    terms: 'Ved å fortsette aksepterer du våre brukervilkår og personvernregler.',
    support: 'Trenger du hjelp? Kontakt Support',
    cookies:
      'Ved å klikke «Godta», godtar du lagring av informasjonskapsler på enheten din.',
    decline: 'Avvis',
    accept: 'Godta',
  }
}
