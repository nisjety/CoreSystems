'use client';

import { useState, useEffect, useCallback, useMemo } from 'react';
import type { SupportedLocale, TranslationDict } from './index';
import { i18nManager } from './index';
import { norwegianTranslations } from './translations/nb';
import { englishTranslations } from './translations/en';

/**
 * Translation map for all supported locales
 */
const translations: Record<SupportedLocale, TranslationDict> = {
  nb: norwegianTranslations,
  en: englishTranslations,
};

/**
 * Type-safe translation key extraction
 */
type DeepKeys<T> = T extends object
  ? {
      [K in keyof T]: K extends string
        ? T[K] extends object
          ? `${K}.${DeepKeys<T[K]>}`
          : K
        : never;
    }[keyof T]
  : never;

type TranslationKey = DeepKeys<TranslationDict>;

/**
 * Get nested value from object using dot notation
 */
function getNestedValue(obj: unknown, path: string): string {
  try {
    const value = path.split('.').reduce((current: unknown, key: string) => {
      return current && typeof current === 'object' && key in current
        ? (current as Record<string, unknown>)[key]
        : undefined;
    }, obj);
    
    return typeof value === 'string' ? value : path;
  } catch {
    return path;
  }
}

/**
 * Main translation hook with type safety and fallback
 * Includes hydration safety to prevent server/client mismatches
 */
export function useTranslation() {
  // Use a hydrated state to prevent SSR/client mismatches
  const [isHydrated, setIsHydrated] = useState(false);
  // Always start from the same locale that SSR uses to avoid hydration mismatches.
  const [currentLocale, setCurrentLocale] = useState<SupportedLocale>('nb');

  // Hydration effect - runs only on client
  useEffect(() => {
    const actualLocale = i18nManager.getLocale();
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setCurrentLocale((previousLocale) =>
      previousLocale === actualLocale ? previousLocale : actualLocale
    );
    setIsHydrated(true);
  }, []);

  // Subscribe to locale changes after hydration
  useEffect(() => {
    if (!isHydrated) return;

    const handleStorageChange = (e: StorageEvent) => {
      if (e.key === 'id-knuten-locale' && e.newValue) {
        const newLocale = e.newValue as SupportedLocale;
        if (newLocale !== currentLocale) {
          setCurrentLocale(newLocale);
        }
      }
    };

    const handleLocaleChange = (e: CustomEvent) => {
      const { locale: newLocale } = e.detail;
      if (newLocale !== currentLocale) {
        setCurrentLocale(newLocale);
      }
    };

    window.addEventListener('storage', handleStorageChange);
    window.addEventListener('i18n-locale-changed', handleLocaleChange as EventListener);
    
    return () => {
      window.removeEventListener('storage', handleStorageChange);
      window.removeEventListener('i18n-locale-changed', handleLocaleChange as EventListener);
    };
  }, [currentLocale, isHydrated]);

  // Get current translations
  const currentTranslations = useMemo(() => {
    return translations[currentLocale] || translations.en;
  }, [currentLocale]);

  // Translation function with fallback
  const t = useCallback((key: TranslationKey | string, fallback?: string): string => {
    const value = getNestedValue(currentTranslations, key);
    
    // If value not found, try fallback locale
    if (value === key && currentLocale !== 'en') {
      const fallbackValue = getNestedValue(translations.en, key);
      if (fallbackValue !== key) {
        return fallbackValue;
      }
    }
    
    return value || fallback || key;
  }, [currentTranslations, currentLocale]);

  // Change locale function
  const changeLocale = useCallback((locale: SupportedLocale) => {
    i18nManager.setLocale(locale);
    setCurrentLocale(locale);
  }, []);

  // Toggle between languages
  const toggleLanguage = useCallback(() => {
    const newLocale = i18nManager.toggleLanguage();
    setCurrentLocale(newLocale);
    return newLocale;
  }, []);

  return {
    t,
    locale: currentLocale,
    changeLocale,
    toggleLanguage,
    isNorwegian: currentLocale === 'nb',
    isEnglish: currentLocale === 'en',
    direction: 'ltr' as const,
    htmlLang: i18nManager.getHtmlLang(),
    languageName: i18nManager.getLanguageName(),
    isHydrated, // Expose hydration state
  };
}

/**
 * Enhanced translation function with interpolation support
 */
function useTranslationWithInterpolation() {
  const { t, ...rest } = useTranslation();

  const tWithParams = useCallback((
    key: TranslationKey | string,
    params?: Record<string, string | number>,
    fallback?: string
  ): string => {
    let translation = t(key, fallback);
    
    if (params) {
      Object.entries(params).forEach(([paramKey, value]) => {
        translation = translation.replace(
          new RegExp(`{{\\s*${paramKey}\\s*}}`, 'g'),
          String(value)
        );
      });
    }
    
    return translation;
  }, [t]);

  return {
    t: tWithParams,
    tSimple: t,
    ...rest,
  };
}

/**
 * Hook specifically for authentication components
 */
export function useAuthTranslation() {
  const { t, ...rest } = useTranslation();

  // Convenience methods for common auth translations
  const authT = useMemo(() => ({
    // Fields
    field: (field: 'name' | 'email' | 'password' | 'confirmPassword') => 
      t(`auth.fields.${field}` as TranslationKey),
    
    // Actions
    action: (action: 'signin' | 'signup' | 'signout' | 'forgotPassword' | 'resetPassword') => 
      t(`auth.actions.${action}` as TranslationKey),
    
    // Messages
    message: (message: 'emailSent' | 'passwordReset' | 'accountCreated' | 'loginSuccess' | 'logoutSuccess' | 'invalidCredentials' | 'emailExists' | 'passwordMismatch' | 'weakPassword' | 'networkError') => 
      t(`auth.messages.${message}` as TranslationKey),
    
    // Validation
  validation: (validation: 'emailRequired' | 'emailInvalid' | 'passwordRequired' | 'passwordMinLength' | 'passwordMismatch' | 'nameRequired' | 'nameMinLength' | 'emailTooLong' | 'passwordUpper' | 'passwordLower' | 'passwordNumber' | 'passwordSpecial' | 'confirmPasswordRequired' | 'nameInvalidChars' | 'orgNameRequired' | 'orgSlugRequired' | 'orgSlugInvalid') => 
      t(`auth.validation.${validation}` as TranslationKey),
    
    // Placeholders
    placeholder: (placeholder: 'name' | 'email' | 'password' | 'ssoEmail' | 'domain' | 'orgName' | 'orgSlug' | 'inviteEmail') => 
      t(`auth.placeholders.${placeholder}` as TranslationKey),
    
    // SSO
    sso: (key: 'businessEmail' | 'organizationDomain' | 'continue' | 'connecting' | 'description' | 'domainDescription' | 'title' | 'businessEmailRequired' | 'discoveryError' | 'noProvidersFound' | 'redirecting' | 'authError' | 'emailHelp' | 'continueHelp' | 'aboutSso' | 'aboutDescription' | 'selectProvider' | 'selectDescription' | 'providersListLabel' | 'defaultProviderDescription' | 'disabledProvidersNotice' | 'authenticating' | 'authenticatingDescription' | 'checking' | 'domainTitle') => 
      t(`auth.sso.${key}` as TranslationKey),
    
    // Organization
    org: (key: 'name' | 'slug' | 'inviteEmail' | 'inviteRole' | 'create' | 'creating' | 'nameDescription' | 'slugDescription' | 'inviteDescription') => 
      t(`auth.organization.${key}` as TranslationKey),
    
    // Organization Management
    orgMgmt: (key: 'title' | 'description' | 'createNew' | 'createTitle' | 'createDescription' | 'manageTitle' | 'noOrganizations' | 'noOrganizationsDescription' | 'nameLabel' | 'slugLabel' | 'slugHelper' | 'inviteUser' | 'sendInvitation' | 'sending' | 'copyLink' | 'deleteInvitation' | 'pendingInvitations' | 'expires' | 'oauthTitle' | 'appNameLabel' | 'redirectUrlLabel' | 'registerApp' | 'members' | 'member' | 'createSuccess' | 'inviteSuccess' | 'linkCopied' | 'inviteDeleted' | 'createError' | 'inviteError' | 'deleteError' | 'copyError' | 'nameRequired' | 'inviteRequired') => 
      t(`auth.organizationManagement.${key}` as TranslationKey),
    
    // Roles
    role: (role: 'member' | 'admin' | 'owner' | 'guest') => 
      t(`auth.roles.${role}` as TranslationKey),
    
    // Modes
    mode: (mode: 'signin' | 'signup' | 'forgot', prop: 'title' | 'description' | 'action') => 
      t(`auth.modes.${mode}.${prop}` as TranslationKey),

    // Social providers
    social: (key: 'orWith') => t(`social.${key}` as TranslationKey),
    socialWithProvider: (key: 'signInWith' | 'unavailable', provider: string) => 
      t(`social.${key}` as TranslationKey).replace('{{provider}}', provider),

    // Passkey
    passkey: (key: 'title' | 'register' | 'authenticate' | 'manage' | 'delete' | 'creating' | 'authenticating' | 'deleting' | 'unsupported' | 'unsupportedMessage' | 'emailRequired' | 'description' | 'quickAuth' | 'noPasskeys' | 'noPasskeysDescription' | 'deleteConfirm' | 'lastUsed' | 'created') => 
      t(`passkey.${key}` as TranslationKey),
    passkeyError: (key: 'registrationFailed' | 'authenticationFailed' | 'deleteFailed') =>
      t(`passkey.errors.${key}` as TranslationKey),

    // Callback
    callback: (key: 'title' | 'processing' | 'success' | 'redirecting' | 'error' | 'redirectingToSignIn') =>
      t(`callback.${key}` as TranslationKey),
    callbackOAuth: (key: 'error' | 'cancelled' | 'denied' | 'timeout') =>
      t(`callback.oauth.${key}` as TranslationKey),

    // Password strength
    passwordStrength: (key: 'veryWeak' | 'weak' | 'ok' | 'strong' | 'veryStrong' | 'enterPassword' | 'missing') =>
      t(`auth.passwordStrength.${key}` as TranslationKey),

    // Guard messages (generic access via key)
    guard: (key: string) => t(`auth.guards.${key}` as TranslationKey),
    // Guard messages with interpolation params
    guardFmt: (key: string, params?: Record<string, string | number>) => {
      let base = t(`auth.guards.${key}` as TranslationKey);
      if (params) {
        Object.entries(params).forEach(([k, v]) => {
          base = base.replace(new RegExp(`{{\\s*${k}\\s*}}`, 'g'), String(v));
        });
      }
      return base;
    },
  }), [t]);

  return {
    t,
    authT,
    ...rest,
  };
}

/**
 * Hook for common translations
 */
export function useCommonTranslation() {
  const { t, ...rest } = useTranslation();

  const commonT = useMemo(() => ({
    required: () => t('common.required'),
    optional: () => t('common.optional'),
    loading: () => t('common.loading'),
    error: () => t('common.error'),
    success: () => t('common.success'),
    cancel: () => t('common.cancel'),
    confirm: () => t('common.confirm'),
    close: () => t('common.close'),
    save: () => t('common.save'),
    delete: () => t('common.delete'),
    edit: () => t('common.edit'),
    back: () => t('common.back'),
    next: () => t('common.next'),
    previous: () => t('common.previous'),
    continue: () => t('common.continue'),
    yes: () => t('common.yes'),
    no: () => t('common.no'),
  }), [t]);

  return {
    t,
    commonT,
    ...rest,
  };
}

/**
 * Hook for language switching
 */
export function useLanguageSwitch() {
  const { locale, changeLocale, toggleLanguage, languageName } = useTranslation();

  const switchToNorwegian = useCallback(() => changeLocale('nb'), [changeLocale]);
  const switchToEnglish = useCallback(() => changeLocale('en'), [changeLocale]);

  return {
    currentLocale: locale,
    currentLanguageName: languageName,
    isNorwegian: locale === 'nb',
    isEnglish: locale === 'en',
    switchToNorwegian,
    switchToEnglish,
    toggleLanguage,
    changeLocale,
  };
}

/**
 * Get all available locales with their display names
 */
export function useAvailableLocales() {
  return useMemo(() => [
    { code: 'nb' as const, name: 'Norsk', nativeName: 'Norsk' },
    { code: 'en' as const, name: 'English', nativeName: 'English' },
  ], []);
}

/**
 * Hook for consent translations
 */
export function useConsentTranslation() {
  const { t, ...rest } = useTranslation();

  // Convenience methods for consent translations
  const consentT = useMemo(() => ({
    // Banner
    banner: (key: 'message' | 'messageMobile' | 'settings' | 'reject' | 'accept' | 'settingsLabel') =>
      t(`consent.banner.${key}` as TranslationKey),
    
    // Preferences
    preferences: (key: 'title' | 'close' | 'intro' | 'allowAll' | 'rejectAll' | 'acceptAll' | 'saveChoices' | 'showDetails' | 'hideDetails') =>
      t(`consent.preferences.${key}` as TranslationKey),
    
    // Categories
    category: (category: 'necessary' | 'performance' | 'functional' | 'marketing', prop: 'title' | 'description') =>
      t(`consent.categories.${category}.${prop}` as TranslationKey),
    
    // Actions
    action: (action: 'accepted' | 'rejected' | 'settingsOpened' | 'allAccepted' | 'allRejected' | 'choicesSaved') =>
      t(`consent.actions.${action}` as TranslationKey),
    
    // Storage
    storage: (key: 'error' | 'success') =>
      t(`consent.storage.${key}` as TranslationKey),
    
    // Script
    script: (key: 'loadError' | 'loadSuccess') =>
      t(`consent.script.${key}` as TranslationKey),
    
    // Validation
    validation: (key: 'error') =>
      t(`consent.validation.${key}` as TranslationKey),
    
    // Reset
    reset: (key: 'success') =>
      t(`consent.reset.${key}` as TranslationKey),
  }), [t]);

  // Social provider translations with template support
  const socialT = useMemo(() => ({
    orWith: () => t('social.orWith' as TranslationKey),
    signInWith: (provider: string) => 
      t('social.signInWith' as TranslationKey).replace('{{provider}}', provider),
    unavailable: (provider: string) => 
      t('social.unavailable' as TranslationKey).replace('{{provider}}', provider),
    provider: (key: 'google' | 'microsoft' | 'okta' | 'vipps') =>
      t(`social.providers.${key}` as TranslationKey),
  }), [t]);

  // Passkey translations
  const passkeyT = useMemo(() => ({
    title: () => t('passkey.title' as TranslationKey),
    register: () => t('passkey.register' as TranslationKey),
    authenticate: () => t('passkey.authenticate' as TranslationKey),
    manage: () => t('passkey.manage' as TranslationKey),
    delete: () => t('passkey.delete' as TranslationKey),
    creating: () => t('passkey.creating' as TranslationKey),
    authenticating: () => t('passkey.authenticating' as TranslationKey),
    deleting: () => t('passkey.deleting' as TranslationKey),
    unsupported: () => t('passkey.unsupported' as TranslationKey),
    unsupportedMessage: () => t('passkey.unsupportedMessage' as TranslationKey),
    emailRequired: () => t('passkey.emailRequired' as TranslationKey),
    description: () => t('passkey.description' as TranslationKey),
    quickAuth: () => t('passkey.quickAuth' as TranslationKey),
    noPasskeys: () => t('passkey.noPasskeys' as TranslationKey),
    noPasskeysDescription: () => t('passkey.noPasskeysDescription' as TranslationKey),
    deleteConfirm: () => t('passkey.deleteConfirm' as TranslationKey),
    lastUsed: () => t('passkey.lastUsed' as TranslationKey),
    created: () => t('passkey.created' as TranslationKey),
    error: (key: 'registrationFailed' | 'authenticationFailed' | 'deleteFailed') =>
      t(`passkey.errors.${key}` as TranslationKey),
  }), [t]);

  return {
    t,
    consentT,
    socialT,
    passkeyT,
    ...rest,
  };
}
