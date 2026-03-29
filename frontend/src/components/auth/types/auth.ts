export type AuthMode = 'signin' | 'signup' | 'enterprise-sso' | 'org';

export type ConsentKeys = 'necessary' | 'performance' | 'functional' | 'marketing';
export type ConsentState = Record<ConsentKeys, boolean>;
export type StoredConsent = { version: string; choices: ConsentState; timestamp: number };

export interface AuthPageProps {
  redirectTo?: string;
}

export interface AuthFormData {
  email: string;
  password: string;
  name: string;
  confirmPassword: string;
  // Enterprise SSO fields
  ssoEmail: string;
  ssoDomain: string;
  // Organization fields
  orgName: string;
  orgSlug: string;
  inviteEmail: string;
  inviteRole: 'admin' | 'member';
  // OAuth app fields
  appName: string;
  redirectURL: string;
}

export interface AuthFormErrors {
  email: string;
  password: string;
  name: string;
  confirmPassword: string;
}

export interface SocialProvider {
  name: string;
  color: string;
  active: boolean;
  icon: React.ReactNode;
}

export interface AuthState {
  mode: AuthMode;
  showPassword: boolean;
  showConfirmPassword: boolean;
  emailNeedsVerification: string | null;
  passkeySupported: boolean;
  formData: AuthFormData;
  fieldErrors: AuthFormErrors;
}
