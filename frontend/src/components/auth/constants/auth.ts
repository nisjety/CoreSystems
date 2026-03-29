export const AUTH_MODES = {
  SIGNIN: 'signin',
  SIGNUP: 'signup',
  FORGOT_PASSWORD: 'forgot-password',
  ENTERPRISE_SSO: 'enterprise-sso',
  ORGANIZATION: 'organization'
} as const;

export const VERIFICATION_METHODS = {
  EMAIL: 'email',
  SMS: 'sms',
  TOTP: 'totp'
} as const;

export const VERIFICATION_STEPS = {
  METHOD_SELECTION: 'method-selection',
  CODE_ENTRY: 'code-entry',
  SUCCESS: 'success'
} as const;

export const FORM_CONSTRAINTS = {
  MIN_PASSWORD_LENGTH: 8,
  VERIFICATION_CODE_LENGTH: 6,
  RESEND_TIMEOUT_SECONDS: 60
} as const;

export const DEFAULT_REDIRECT_URL = '/dashboard';

export const SOCIAL_PROVIDERS = [
  'google',
  'microsoft',
  'okta',
  'vipps'
] as const;
