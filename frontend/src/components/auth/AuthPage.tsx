'use client';

import React, { useState, useCallback } from 'react';
import Link from 'next/link';
import { useRouter } from 'next/navigation';
import { Fingerprint, Lock, Mail, ShieldCheck } from 'lucide-react';

import { useAuth } from './hooks/use-auth';
import { useAuthForm } from './hooks/use-auth-form';
import { usePasskey } from './hooks/use-passkey';
import { useConsent } from './consent/useConsent';
import { useTwoFactor } from './hooks/useTwoFactor';
import { useVerification } from './hooks/useVerification';
import { useSecuritySettings } from './hooks/useSecuritySettings';

import { useTranslation, useLanguageSwitch } from './lib/i18n/hooks';

import { AuthTabs } from './core/AuthTabs';
import { AuthForms } from './core/AuthForms';
import { SocialProviders } from './providers/SocialProviders';
import { PasskeyButtons } from './providers/PasskeyButtons';
import { OrganizationManagement } from './providers/OrganizationManagement';
import { LanguageSwitcher } from './ui/LanguageSwitcher';
import { AuthGuard } from './guards/AuthGuard';
import { RoleGuard } from './guards/RoleGuard';
import { ConsentBanner } from './consent/ConsentBanner';
import { ConsentPreferences } from './consent/ConsentPreferences';
import { CallbackModal } from './modals/CallbackModal';

import { AuthMode } from './types/auth';

const defaultToastActions = {
  error: (title: string, description?: string) => console.log('Error:', title, description),
  info: (message: string) => console.log('Info:', message),
};

export interface AuthPageProps {
  redirectTo?: string;
  initialMode?: AuthMode;
  features?: {
    socialAuth?: boolean;
    passkeys?: boolean;
    enterpriseSSO?: boolean;
    organizationManagement?: boolean;
    twoFactor?: boolean;
    multiChannelVerification?: boolean;
    gdprConsent?: boolean;
    accessibilityMode?: boolean;
  };
  customization?: {
    theme?: 'light' | 'dark' | 'auto';
    primaryColor?: string;
    logo?: string;
    companyName?: string;
    supportEmail?: string;
  };
  onAuthSuccess?: (user: unknown) => void;
  onAuthError?: (error: Error) => void;
  onModeChange?: (mode: AuthMode) => void;
}

export function AuthPage({
  redirectTo = '/',
  initialMode = 'signin',
  features = {
    socialAuth: true,
    passkeys: true,
    enterpriseSSO: true,
    organizationManagement: true,
    twoFactor: true,
    multiChannelVerification: true,
    gdprConsent: true,
    accessibilityMode: true,
  },
  customization = {
    theme: 'auto',
    companyName: 'ID-Knuten',
    supportEmail: 'support@id-knuten.no',
  },
  onAuthSuccess,
  onAuthError,
  onModeChange,
}: AuthPageProps) {
  const toCamelCaseText = (value: string) =>
    value
      .toLowerCase()
      .replace(/(^|\s)\S/g, (character) => character.toUpperCase());

  // Core hooks
  const normalizedInitialMode: AuthMode =
    initialMode === 'enterprise-sso' || initialMode === 'org' ? 'signin' : initialMode;

  const { user, isLoading: authLoading } = useAuth();
  const router = useRouter();
  const {
    formData,
    errors,
    isLoading: formLoading,
    setMode,
    setField,
    handleSubmit,
    reset,
  } = useAuthForm({ mode: normalizedInitialMode, redirectTo });

  // i18n hooks
  const { t } = useTranslation();
  const { isNorwegian } = useLanguageSwitch();

  // Specialized hooks
  const {
    isSupported: passkeySupported,
    isLoading: passkeyLoading,
  } = usePasskey();

  const {
    consent,
    showBanner: showConsentBanner,
    showPreferences: consentPreferencesOpen,
    acceptAll: acceptAllConsent,
    rejectAll: rejectAllConsent,
    showPreferencesModal: openConsentPreferences,
    hidePreferences: hideConsentPreferences,
    updateConsent,
    saveConsent,
  } = useConsent();

  const { isLoading: twoFactorLoading } = useTwoFactor();
  const { isLoading: verificationLoading } = useVerification();
  const { isLoading: securityLoading } = useSecuritySettings();

  // Enhanced hooks fallback
  const { error, info } = defaultToastActions;

  // Local state
  const [currentMode, setCurrentMode] = useState<AuthMode>(normalizedInitialMode);
  const [showCallbackModal, setShowCallbackModal] = useState(false);
  const [isHydrated, setIsHydrated] = useState(false);
  const [isPageVisible, setIsPageVisible] = useState(false);
  const [viewportHeight, setViewportHeight] = useState<number | null>(null);
  const [contentHeight, setContentHeight] = useState<number | null>(null);
  const cardRef = React.useRef<HTMLDivElement | null>(null);
  const contentRef = React.useRef<HTMLDivElement | null>(null);

  // Check for callback scenario in URL
  React.useEffect(() => {
    if (typeof window !== 'undefined') {
      const urlParams = new URLSearchParams(window.location.search);
      const isCallback =
        urlParams.has('code') ||
        urlParams.has('state') ||
        window.location.pathname.includes('/callback');

      if (isCallback) {
        setShowCallbackModal(true);
      }
    }
  }, []);

  React.useEffect(() => {
    if (typeof window === 'undefined') return;

    const updateViewportHeight = () => {
      setViewportHeight(window.innerHeight);
    };

    updateViewportHeight();
    setIsHydrated(true);

    window.addEventListener('resize', updateViewportHeight);
    return () => {
      window.removeEventListener('resize', updateViewportHeight);
    };
  }, []);

  React.useEffect(() => {
    if (!isHydrated || viewportHeight === null || contentHeight === null) return;

    const fadeFrame = window.requestAnimationFrame(() => {
      setIsPageVisible(true);
    });

    return () => window.cancelAnimationFrame(fadeFrame);
  }, [isHydrated, viewportHeight, contentHeight]);

  React.useEffect(() => {
    if (user && !authLoading) {
      // Already authenticated — redirect instead of showing a blank/hidden form
      router.replace(redirectTo);
    }
  }, [user, authLoading, router, redirectTo]);

  React.useEffect(() => {
    if (!contentRef.current) return;

    const measureContent = () => {
      if (contentRef.current) {
        setContentHeight(contentRef.current.scrollHeight);
      }
    };

    measureContent();
    const animationFrame = window.requestAnimationFrame(measureContent);

    return () => window.cancelAnimationFrame(animationFrame);
  }, [currentMode, authLoading, formLoading, passkeyLoading, passkeySupported, features.passkeys, features.socialAuth]);

  // Handle mode changes
  const handleModeChange = useCallback(
    (newMode: AuthMode) => {
      const allowedMode =
        newMode === 'enterprise-sso' || newMode === 'org' ? 'signin' : newMode;
      setCurrentMode(allowedMode);
      setMode(allowedMode);
      onModeChange?.(allowedMode);
      reset();
    },
    [onModeChange, reset, setMode]
  );

  // Authentication handlers
  const handleSocialSignIn = async (provider: string) => {
    try {
      if (provider !== 'microsoft' && provider !== 'google') {
        throw new Error(`Unsupported provider: ${provider}`);
      }

      info(`Initiating ${provider} sign-in via Better Auth...`);

      const redirectTo = `${window.location.origin}/auth/callback`;
      const endpoint = `${window.location.origin}/api/auth/oauth/initiate`;

      const response = await fetch(endpoint, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
        credentials: 'include',
        redirect: 'manual',
        body: JSON.stringify({
          provider,
          redirectTo,
        }),
      });

      if (!response.ok) {
        const errorText = await response.text();
        console.error('OAuth initiation failed:', errorText);
        throw new Error(`Failed to initiate social sign-in: ${response.statusText}`);
      }

      const data = await response.json().catch(() => ({} as { url?: string }));
      if (data?.url) {
        window.location.href = data.url;
        return;
      }

      throw new Error('No redirect URL received from authentication service');
    } catch (err) {
      error('Social sign-in error', err instanceof Error ? err.message : String(err));
      onAuthError?.(err instanceof Error ? err : new Error('Social sign-in failed'));
    }
  };

  // Loading state
  const isLoading =
    authLoading ||
    formLoading ||
    passkeyLoading ||
    twoFactorLoading ||
    verificationLoading ||
    securityLoading;

  // Header text
  const getHeaderText = () => {
    switch (currentMode) {
      case 'signup':
        return {
          title: t('auth.modes.signup.title') || 'Create Account',
          description: t('auth.modes.signup.description') || 'Set up your account securely.',
        };
      case 'signin':
        return {
          title: t('auth.modes.signin.title') || 'Sign In',
          description: t('auth.modes.signin.description') || 'Access your account securely.',
        };
      case 'enterprise-sso':
        return {
          title: t('auth.sso.title') || 'Sign In',
          description: t('auth.sso.description') || 'Access your account securely.',
        };
      case 'org':
        return {
          title: t('auth.organizationManagement.title') || 'Organization',
          description: t('auth.organizationManagement.description') || 'Manage your organization.',
        };
      default:
        return {
          title: t('auth.modes.default.title') || 'Sign In',
          description: t('auth.modes.default.description') || 'Access your account securely.',
        };
    }
  };

  const headerText = getHeaderText();

  const baseCardHeight = 1080;

  const cardScale = viewportHeight
    ? Math.min(1, Math.max(0.52, (viewportHeight - 18) / baseCardHeight))
    : 1;

  const animatedContentHeight = contentHeight
    ? Math.max(
        0,
        contentHeight + (currentMode === 'signin' ? 6 : currentMode === 'signup' ? -32 : 0)
      )
    : undefined;

  // Localized UI strings
  const uiTexts = {
    termsPrefixSignin: t('auth.page.terms.prefixSignin' as string),
    termsAnd: t('auth.page.terms.and' as string),
    termsUser: t('auth.page.terms.termsOfUse' as string),
    termsPrivacy: t('auth.page.terms.privacyPolicy' as string),
    signupDataNotice: t('auth.page.terms.dataNotice' as string),
    deleteCookie: t('auth.page.terms.deleteCookie' as string),
    supportNeedHelp: t('auth.page.support.needHelp' as string),
    supportContact: t('auth.page.support.contactSupport' as string),
    helpLabel: t('auth.page.support.helpLabel' as string),
    imprint: isNorwegian ? 'Om oss' : 'Imprint',
    privacy: isNorwegian ? 'Personvern' : 'Privacy',
    copyright: isNorwegian ? 'Opphavsrett' : 'Copyright',
    cookieSettings: isNorwegian ? 'Cookie-innstillinger' : 'Cookie settings',
  } as const;

  return (
    <div
      className={`relative isolate z-40 flex h-[100dvh] min-h-[100dvh] items-center justify-center overflow-hidden px-3 py-0 transition-opacity duration-800 ease-out sm:px-4 md:px-5 lg:px-6 xl:px-10 ${
        isPageVisible ? 'opacity-100' : 'opacity-0'
      }`}
      style={
        {
          '--primary': '#111111',
          '--primary-foreground': '#ffffff',
          '--ring': '#111111',
        } as React.CSSProperties
      }
    >
      <div
        ref={cardRef}
        className="relative z-[120] grid w-full max-w-[70.5rem] overflow-visible rounded-[24px] border border-[#D6D2CB] bg-[#EDEBE7] shadow-[0_20px_50px_rgba(0,0,0,0.14)] md:grid-cols-[1.15fr_0.85fr] xl:max-w-[72rem]"
        style={{
          transform: `scale(${cardScale})`,
          transformOrigin: 'center center',
        }}
      >
        <div className="flex items-center justify-center rounded-l-[24px] bg-white px-5 py-6 sm:px-7 sm:py-7 md:px-8 md:py-8 lg:px-10 lg:py-9 xl:px-16 xl:py-10">
          <div className="w-full max-w-[21rem] sm:max-w-[22rem] lg:max-w-[22.75rem] xl:max-w-[25rem]">
            <div className="mb-4 flex items-center justify-between xl:mb-6">
              <Link
                href="/intro"
                className="text-xs font-bold tracking-tight text-[#111111] transition-colors hover:text-[#FF2E63]"
              >
                ← TILBAKE
              </Link>
              <div className="flex items-center">
                <LanguageSwitcher variant="dropdown" size="sm" className="opacity-90 hover:opacity-100" />
                <span className="ml-2 inline-block h-1.5 w-1.5 rounded-full bg-[#FF2E63]/70" aria-hidden="true" />
              </div>
            </div>

            <div className="mb-3 xl:mb-4">
              <AuthTabs
                currentMode={currentMode}
                onModeChange={handleModeChange}
                disabled={isLoading}
                features={{
                  enterpriseSSO: false,
                  organizationManagement: false,
                }}
              />
            </div>

            <div className="mb-4 text-left xl:mb-5">
              <h1
                className="text-[clamp(40px,5vw,72px)] font-normal leading-[1.1] tracking-[-0.01em] text-[#1C1C1C]"
                style={{ fontFamily: 'var(--font-cormorant-garamond), serif' }}
              >
                {toCamelCaseText(headerText.title)}
              </h1>
              <p className="mt-3 font-inter text-[15px] leading-[1.6] text-[#66615B] xl:mt-4">
                {headerText.description}
              </p>
            </div>

            <div
              className="relative overflow-hidden transition-[height] duration-[520ms] ease-in-out"
              style={{ height: animatedContentHeight ? `${animatedContentHeight}px` : undefined }}
            >
              <div
                ref={contentRef}
                className="transform-gpu"
              >
                <div className="mb-2.5 xl:mb-3">
                {currentMode === 'org' ? (
                  <OrganizationManagement
                    onSuccess={(result) => {
                      onAuthSuccess?.(result);
                    }}
                    onError={(organizationError) => {
                      onAuthError?.(new Error(organizationError));
                    }}
                  />
                ) : (
                  <AuthForms
                    mode={currentMode}
                    formData={formData}
                    errors={errors}
                    isLoading={isLoading}
                    onFieldChange={setField}
                    onSubmit={handleSubmit}
                  />
                )}
              </div>

              {features.socialAuth && (currentMode === 'signin' || currentMode === 'signup') && (
                <div className="mb-2.5 xl:mb-3">
                  <SocialProviders onProviderClick={handleSocialSignIn} isLoading={isLoading} />
                </div>
              )}

              {features.passkeys && passkeySupported && currentMode !== 'org' && (
                <div className="mb-2.5 xl:mb-3">
                  <PasskeyButtons
                    mode={currentMode === 'signin' ? 'authenticate' : currentMode === 'signup' ? 'register' : 'manage'}
                    onSuccess={() => {}}
                    onError={(passkeyErrorMessage) => onAuthError?.(new Error(passkeyErrorMessage))}
                    email={formData.email}
                    displayName={formData.name}
                    disabled={isLoading}
                    className="text-sm"
                  />
                </div>
              )}

              <div className="mb-1.5 space-y-1 text-left xl:mb-2" />

              <div className="mb-1.5 text-left xl:mb-2">
                {currentMode === 'signin' && (
                  <p className="font-inter text-xs leading-[1.65] tracking-[0.02em] text-[#6A655F]" aria-label="terms-and-privacy">
                    {uiTexts.termsPrefixSignin}{' '}
                    <button type="button" className="text-[#3E3A35] underline hover:text-[#1C1C1C] focus:outline-none">
                      {uiTexts.termsUser}
                    </button>{' '}
                    {uiTexts.termsAnd}{' '}
                    <button type="button" className="text-[#3E3A35] underline hover:text-[#1C1C1C] focus:outline-none">
                      {uiTexts.termsPrivacy}
                    </button>
                    .
                  </p>
                )}
                {currentMode === 'signup' && (
                  <p className="font-inter text-xs leading-[1.65] tracking-[0.02em] text-[#6A655F]" aria-label="signup-data-notice">
                    {uiTexts.signupDataNotice}{' '}
                    <button
                      type="button"
                      className="text-[#3E3A35] underline hover:text-[#1C1C1C] focus:outline-none"
                      onClick={() => {
                        document.cookie.split(';').forEach((cookiePart) => {
                          document.cookie = cookiePart
                            .replace(/^ +/, '')
                            .replace(/=.*/, `=;expires=${new Date().toUTCString()};path=/`);
                        });
                        localStorage.clear();
                        sessionStorage.clear();
                      }}
                    >
                      {uiTexts.deleteCookie}
                    </button>
                    .
                  </p>
                )}
              </div>
            </div>
            </div>

            <div className="mt-2 hidden text-left sm:block xl:mt-3">
              <p className="font-inter text-xs tracking-[0.02em] text-[#6A655F]">
                {uiTexts.supportNeedHelp}{' '}
                <a href={`mailto:${customization.supportEmail}`} className="text-[#3E3A35] transition-colors hover:text-[#1C1C1C]">
                  {uiTexts.supportContact}
                </a>
              </p>
            </div>

            <div className="mt-1 flex space-x-3 sm:hidden">
              {!showConsentBanner && (
                <a
                  href={`mailto:${customization.supportEmail}`}
                  className="flex items-center gap-1 font-inter text-xs tracking-[0.02em] text-[#6A655F] transition-colors hover:text-[#1C1C1C]"
                  aria-label={uiTexts.helpLabel}
                >
                  <Mail className="w-3 h-3" />
                  {uiTexts.helpLabel}
                </a>
              )}
            </div>
          </div>
        </div>

        <div className="relative hidden min-h-[560px] overflow-hidden rounded-r-[24px] md:block lg:min-h-[600px] xl:min-h-[640px]">
          <div
            className="absolute inset-0 bg-cover bg-center"
            style={{
              backgroundImage: "url('/imagens/curved-interior-sculpture.png')",
            }}
          />
          <div className="pointer-events-none absolute bottom-8 left-5 top-8 z-20">
            <div className="absolute inset-y-0 left-0 w-px bg-[#FF2E63]/90" />
            <div className="absolute inset-y-0 -left-[3px] w-[8px] bg-[#FF3B5C]/35 blur-[7px]" />
            <div className="scanner-dot absolute -left-[4px] top-0 h-[20px] w-[9px] rounded-lg bg-gradient-to-b from-[#FF3B5C]/15 via-[#FF3B5C]/40 to-[#FF3B5C]/15 shadow-[0_0_8px_rgba(255,59,92,0.5),0_0_16px_rgba(255,59,92,0.3),0_0_32px_rgba(255,59,92,0.15)]" />
          </div>

          <div className="absolute left-4 top-1/2 z-30 -translate-y-1/2 xl:left-5">
            <div className="flex flex-col items-center gap-10 text-white/90 lg:gap-11 xl:gap-12">
              <ShieldCheck strokeWidth={1.4} className="h-4 w-4 text-[#10B981] lg:h-4 lg:w-4 xl:h-5 xl:w-5" />
              <Lock strokeWidth={1.4} className="h-4 w-4 lg:h-4 lg:w-4 xl:h-5 xl:w-5" />
              <Fingerprint strokeWidth={1.4} className="h-4 w-4 text-[#FF2E63]/80 lg:h-4 lg:w-4 xl:h-5 xl:w-5" />
            </div>
          </div>
          {features.gdprConsent && showConsentBanner && (
            <ConsentBanner
              onAcceptAll={acceptAllConsent}
              onRejectAll={rejectAllConsent}
              onShowPreferences={openConsentPreferences}
              embedded
            />
          )}
        </div>
      </div>

      <div className="pointer-events-none absolute bottom-3 left-0 right-0 hidden text-center md:block">
        <div className="pointer-events-auto flex flex-wrap items-center justify-center gap-6 font-inter text-xs tracking-[0.02em] text-[#6A655F]">
          <button className="transition-colors hover:text-[#1C1C1C]">{uiTexts.imprint}</button>
          <button className="transition-colors hover:text-[#1C1C1C]">{uiTexts.privacy}</button>
          <button className="transition-colors hover:text-[#1C1C1C]">{uiTexts.copyright}</button>
          <button className="transition-colors hover:text-[#1C1C1C]" onClick={openConsentPreferences}>
            {uiTexts.cookieSettings}
          </button>
        </div>
      </div>

      <ConsentPreferences
        isOpen={consentPreferencesOpen}
        onClose={hideConsentPreferences}
        consent={consent}
        onConsentChange={updateConsent}
        onSave={() => saveConsent(consent)}
        onAcceptAll={acceptAllConsent}
        onRejectAll={rejectAllConsent}
      />

      <CallbackModal
        isOpen={showCallbackModal}
        onClose={() => setShowCallbackModal(false)}
      />

      <style jsx>{`
        .scanner-dot {
          animation: scannerMove 5.5s ease-in-out infinite alternate;
        }

        @keyframes scannerMove {
          0% {
            top: 0;
          }
          100% {
            top: calc(100% - 20px);
          }
        }
      `}</style>
    </div>
  );
}

export const ProtectedAuthPage = () => (
  <AuthGuard allowedStates={['unauthenticated']}>
    <AuthPage />
  </AuthGuard>
);

export const AdminAuthPage = () => (
  <RoleGuard allowedRoles={['admin']}>
    <AuthPage features={{ organizationManagement: true }} />
  </RoleGuard>
);

export default AuthPage;
