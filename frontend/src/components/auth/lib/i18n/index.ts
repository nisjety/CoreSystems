/**
 * Complete Internationalization System
 * 
 * Features:
 * - Norwegian (nb) and English (en) support
 * - Browser language detection
 * - Local storage persistence
 * - Fallback system
 * - Type-safe translation keys
 * - React hooks for easy integration
 */

export type SupportedLocale = 'nb' | 'en';

export interface I18nConfig {
  defaultLocale: SupportedLocale;
  fallbackLocale: SupportedLocale;
  storageKey: string;
  detectBrowserLanguage: boolean;
}

export const defaultI18nConfig: I18nConfig = {
  defaultLocale: 'nb', // Norwegian first
  fallbackLocale: 'en',
  storageKey: 'id-knuten-locale',
  detectBrowserLanguage: true,
};

/**
 * Translation dictionary structure for type safety
 */
export interface TranslationDict {
  // Common translations
  common: {
    required: string;
    optional: string;
    loading: string;
    error: string;
    success: string;
    cancel: string;
    confirm: string;
    close: string;
    save: string;
    delete: string;
    edit: string;
    back: string;
    next: string;
    previous: string;
    continue: string;
    yes: string;
  no: string;
  selected?: string;
  };

  // Authentication specific
  auth: {
    // Field labels
    fields: {
      name: string;
      email: string;
      password: string;
      confirmPassword: string;
    };

    // Actions
    actions: {
      signin: string;
      signup: string;
      signout: string;
      forgotPassword: string;
      resetPassword: string;
      loading: string;
      success: string;
      error: string;
    };

    // Modes and tabs
    modes: {
      signin: {
        title: string;
        description: string;
        action: string;
      };
      signup: {
        title: string;
        description: string;
        action: string;
      };
      forgot: {
        title: string;
        description: string;
        action: string;
      };
      default: {
        title: string;
        description: string;
      };
    };

    // SSO and enterprise
    sso: {
      title: string;
      businessEmail: string;
      organizationDomain: string;
      continue: string;
      connecting: string;
      description: string;
      domainDescription: string;
      businessEmailRequired: string;
      discoveryError: string;
      noProvidersFound: string;
      redirecting: string;
      authError: string;
      emailHelp: string;
      continueHelp: string;
      aboutSso: string;
      aboutDescription: string;
      selectProvider: string;
      selectDescription: string;
      providersListLabel: string;
      defaultProviderDescription: string;
      disabledProvidersNotice: string;
      authenticating: string;
      authenticatingDescription: string;
      checking: string;
      domainTitle: string;
    };

    // Organization management
    organization: {
      name: string;
      slug: string;
      inviteEmail: string;
      inviteRole: string;
      create: string;
      creating: string;
      nameDescription: string;
      slugDescription: string;
      inviteDescription: string;
    };

    // Organization management interface
    organizationManagement: {
      title: string;
      description: string;
      createNew: string;
      createTitle: string;
      createDescription: string;
      manageTitle: string;
      noOrganizations: string;
      noOrganizationsDescription: string;
      
      // Form labels
      nameLabel: string;
      slugLabel: string;
      slugHelper: string;
      
      // Actions
      inviteUser: string;
      sendInvitation: string;
      sending: string;
      copyLink: string;
      deleteInvitation: string;
      
      // Invitations
      pendingInvitations: string;
      expires: string;
      
      // OAuth
      oauthTitle: string;
      appNameLabel: string;
      redirectUrlLabel: string;
      registerApp: string;
      
      // Statistics
      members: string;
      member: string;
      
      // Messages
      createSuccess: string;
      inviteSuccess: string;
      linkCopied: string;
      inviteDeleted: string;
      
      // Errors
      createError: string;
      inviteError: string;
      deleteError: string;
      copyError: string;
      nameRequired: string;
      inviteRequired: string;
    };

    // Placeholders
    placeholders: {
      name: string;
      email: string;
      password: string;
      ssoEmail: string;
      domain: string;
      orgName: string;
      orgSlug: string;
      inviteEmail: string;
    };

    // Roles
    roles: {
      member: string;
      admin: string;
      owner: string;
      guest: string;
    };

    // Messages
    messages: {
      emailSent: string;
      passwordReset: string;
      accountCreated: string;
      loginSuccess: string;
      logoutSuccess: string;
      invalidCredentials: string;
      emailExists: string;
      passwordMismatch: string;
      weakPassword: string;
      networkError: string;
    };

    // Validation
    validation: {
      emailRequired: string;
      emailInvalid: string;
      passwordRequired: string;
      passwordMinLength: string;
      passwordMismatch: string;
      nameRequired: string;
      nameMinLength: string;
      orgNameRequired: string;
      orgSlugRequired: string;
      orgSlugInvalid: string;
      emailTooLong?: string;
      passwordUpper?: string;
      passwordLower?: string;
      passwordNumber?: string;
      passwordSpecial?: string;
      confirmPasswordRequired?: string;
      nameInvalidChars?: string;
    };

    // Password strength labels
    passwordStrength?: {
      veryWeak: string;
      weak: string;
      ok: string;
      strong: string;
      veryStrong: string;
      enterPassword: string;
      missing: string;
    };

    // Guard / access control messages
    guards?: Record<string, string>;

    // Tab labels (auth mode switcher)
    tabs?: {
      signin: { label: string; shortLabel: string; description?: string };
      signup: { label: string; shortLabel: string; description?: string };
      enterpriseSso: { label: string; shortLabel: string; description?: string };
      org: { label: string; shortLabel: string; description?: string };
    };

    // Navigation / aria labels
    navigation?: {
      authTabsLabel: string; // aria-label for tablist
      authOptionsLabel?: string; // alternative wording
    };

    // Security info section
    security?: {
      title: string;
      mfa: { title: string; description: string };
      passwordless: { title: string; description: string };
      sso: { title: string; description: string };
      gdpr: { title: string; description: string };
      notifications?: {
        securitySettingsUpdated: string;
        securityPreferencesUpdated: string;
        settingsReset: string;
        deviceTrusted: string;
        deviceUntrusted: string;
        deviceRemoved: string;
        deviceRenamed: string;
        sessionTerminated: string;
        sessionsTerminated: string;
        sessionExtended: string;
        passwordChanged: string;
        trustedIpAdded: string;
        trustedIpRemoved: string;
        dataExported: string;
        auditLogDownloadStarted: string;
      };
    };

    // TOTP setup & verification
    totp?: {
      loading: { title: string; description: string };
      error: { title: string; description: string; unexpected: string; retry: string; cancel: string };
      setup: {
        title: string; description: string; step1Title: string; copyQrUrl: string; copied: string;
        manualEntry: string; secretLabel: string; instructions: string; continue: string; showSecret: string; hideSecret: string; copySecretAria: string; copyQrUrlAria: string;
      };
      verify: {
        title: string; description: string; step2Title: string; codeLabel: string; codeHelp: string; invalidCode: string; verifying: string; complete: string; back: string; cancel: string;
      };
    };

    // Auth page miscellaneous (terms, support)
    page?: {
      terms: {
        prefixSignin: string; and: string; termsOfUse: string; privacyPolicy: string; dataNotice: string; deleteCookie: string;
      };
      support: {
        needHelp: string; contactSupport: string; helpLabel: string;
      };
    };

    // Two-factor verification (multi-channel)
    twoFactor?: {
      title: string;
      titleLogin: string;
      descriptionLogin: string;
      descriptionGeneric: string;
      methods: { totp: string; email: string; sms: string; recovery: string };
      badges: { totp: string; email: string; sms: string; recovery: string };
      instructions: {
        totp: string;
        emailSendTo: string; // {{email}}
        smsSendTo: string;   // {{phone}}
        recovery: string;
      };
      labels: {
        verificationCode: string;
        emailVerificationCode: string;
        smsVerificationCode: string;
        recoveryCode: string;
      };
      actions: {
        sendEmail: string;
        sendSms: string;
        verifyCode: string;
        verifyEmailCode: string;
        verifySmsCode: string;
        useRecoveryCode: string;
        back: string;
      };
      status: { sending: string; verifying: string };
      success: { emailCodeSent: string; smsCodeSent: string };
      errors: { invalidCode: string; sendEmailFailed: string; sendSmsFailed: string; invalidRecoveryCode: string };
      management?: {
        headerDescription: string;
        active: string; inactive: string;
        enablePrompt: { title: string; description: string };
        availableMethods: { title: string; description: string };
        methodDescriptions: { totp: string; email: string; sms: string; default: string };
        method: { enabled: string; lastUsedPrefix: string; setUp: string };
        addAuthenticator: { title: string; description: string; action: string };
        toggleError: string;
        recovery: {
          title: string; description: string; introTitle: string; introDescription: string;
          viewCodes: string; generateNew: string; hide: string; loading: string; yourCodes: string;
          storeSafelyTitle: string; storeSafelyDescription: string;
          copyCodes: string; copied: string; loadError: string; regenerateError: string;
          confirmRegenerate: string;
        };
        tips: { title: string; tip1: string; tip2: string; tip3: string; tip4: string };
      };
      recoveryCodes?: {
        loadingTitle: string; loadingDescription: string;
        title: string; description: string; remainingLabel: string; lowCodesBadge: string; noCodesBadge: string;
        lowCodesTitle: string; lowCodesDescription: string; // {{count}}
        noCodesTitle: string; noCodesDescription: string;
        manageTitle: string; manageDescription: string;
        viewCodes: string; hideCodes: string; generateCodes: string; generateNewCodes: string; generating: string; confirmRegenerate: string; regenerate: string; cancel: string;
        warningTitle: string; warningDescription: string; // regenerate warning
        copyAll: string; copied: string; export: string;
        headerYourCodes: string; headerCodesDescription: string;
        noCodesAvailableTitle: string; noCodesAvailableDescription: string;
        totalCodesLabel: string; remaining: string;
        loadError: string; regenerateError: string;
        exportFile: {
          title: string; intro1: string; intro2: string; generatedLabel: string; codesHeader: string; notesHeader: string; note1: string; note2: string; note3: string; note4: string; fileNamePrefix: string;
        };
      };
      notifications?: {
        setupComplete: string;
        enableSuccess: string;
        disableSuccess: string;
        methodEnabled: string; // {{method}}
        methodDisabled: string; // {{method}}
        primarySet: string; // {{method}}
        backupCodesGenerated: string;
        codeSentVia: string; // {{method}}
        allDisabled: string;
        resetSuccess: string;
      };
    };

    // Email verification status & help
    emailVerification?: {
      status: {
        verified: { title: string; message: string };
        pending: { title: string; message: string };
        expired: { title: string; message: string };
        failed: { title: string; message: string };
        notSent: { title: string; message: string };
        loading: { title: string; message: string };
        unknown: { title: string; message: string };
      };
      interface: {
        refreshStatus: string; emailLabel: string; sending: string; resendIn: string; sendVerification: string; resend: string; lastSent: string; emailNotFound: string; checkSpam: string; checkCorrect: string; waitDelivery: string;
      };
      badges: { verified: string; pending: string; expired: string; failed: string; notSent: string; loading: string; unknown: string };
      help: { title: string; reason1: string; reason2: string; reason3: string; reason4: string; needHelp: string; supportGuide: string; supportUrl?: string };
    };
  };

  // Modal system
  modal: {
    auth: {
      close: string;
      opened: string;
      closed: string;
      loading: {
        title: string;
        message: string;
      };
      success: {
        login: string;
        register: string;
        default: string;
      };
      error: {
        title: string;
        message: string;
        close: string;
      };
      mode: {
        signin: {
          title: string;
          description: string;
        };
        signup: {
          title: string;
          description: string;
        };
        forgot: {
          title: string;
          description: string;
        };
        verify: {
          title: string;
          description: string;
        };
        sso: {
          title: string;
          description: string;
        };
        organization: {
          title: string;
          description: string;
        };
        default: {
          title: string;
          description: string;
        };
      };
    };

    confirmation: {
      opened: string;
      closed: string;
      cancelled: string;
      success: {
        action: string;
        deleted: string;
        confirmed: string;
        logout: string;
        permission: string;
        cookies: string;
      };
      error: {
        action: string;
        network: string;
        permission: string;
      };
    };
  };

  // Consent management
  consent: {
    banner: {
      message: string;
      messageMobile: string;
      settings: string;
      reject: string;
      accept: string;
      settingsLabel: string;
    };

    preferences: {
      title: string;
      close: string;
      intro: string;
      allowAll: string;
      rejectAll: string;
      acceptAll: string;
      saveChoices: string;
      showDetails: string;
      hideDetails: string;
    };

    categories: {
      necessary: {
        title: string;
        description: string;
      };
      performance: {
        title: string;
        description: string;
      };
      functional: {
        title: string;
        description: string;
      };
      marketing: {
        title: string;
        description: string;
      };
    };

    actions: {
      accepted: string;
      rejected: string;
      settingsOpened: string;
      allAccepted: string;
      allRejected: string;
      choicesSaved: string;
    };

    storage: {
      error: string;
      success: string;
    };

    script: {
      loadError: string;
      loadSuccess: string;
    };

    validation: {
      error: string;
    };

    reset: {
      success: string;
    };
  };

  // Language selector
  language: {
    current: string;
    select: string;
    norwegian: string;
    english: string;
    changed: string;
  };

  // Social providers
  social: {
    orWith: string;
    signInWith: string;
    unavailable: string;
    providers: {
      google: string;
      microsoft: string;
      okta: string;
      vipps: string;
    };
  };

  // Passkey authentication
  passkey: {
    title: string;
    register: string;
    authenticate: string;
    manage: string;
    delete: string;
    creating: string;
    authenticating: string;
    deleting: string;
    unsupported: string;
    unsupportedMessage: string;
    emailRequired: string;
    description: string;
    quickAuth: string;
    noPasskeys: string;
    noPasskeysDescription: string;
    deleteConfirm: string;
    lastUsed: string;
    created: string;
    errors: {
      registrationFailed: string;
      authenticationFailed: string;
      deleteFailed: string;
    };
  };

  // Authentication callback handling
  callback: {
    title: string;
    processing: string;
    success: string;
    redirecting: string;
    error: string;
    redirectingToSignIn: string;
    oauth: {
      error: string;
      cancelled: string;
      denied: string;
      timeout: string;
    };
  };

  // Dashboard translations
  dashboard: {
    aquatiqCard: {
      publishedBy: string;
      readMore: string;
      viewAllCollaborators: string;
      noUpdates: string;
      categories: {
        announcement: string;
        event: string;
        product: string;
        business: string;
      };
    };
    weather: {
      title: string;
      humidity: string;
      wind: string;
      pressure: string;
      noData: string;
    };
    news: {
      title: string;
      noNews: string;
    };
    traffic: {
      title: string;
      noData: string;
    };
  };
}

/**
 * Utility functions for language detection and management
 */
export class I18nManager {
  private config: I18nConfig;
  private currentLocale: SupportedLocale;

  constructor(config: I18nConfig = defaultI18nConfig) {
    this.config = config;
    this.currentLocale = this.detectInitialLocale();
  }

  /**
   * Detect initial locale from browser, storage, or default
   * Safe for SSR - always returns default locale on server
   */
  private detectInitialLocale(): SupportedLocale {
    // During SSR, always use default locale to prevent hydration mismatch
    if (typeof window === 'undefined') {
      return this.config.defaultLocale;
    }

    // Check local storage first
    try {
      const stored = localStorage.getItem(this.config.storageKey) as SupportedLocale;
      if (stored && this.isValidLocale(stored)) {
        return stored;
      }
    } catch (error) {
      // localStorage might not be available
      console.warn('localStorage not available:', error);
    }

    // Always default to Norwegian unless explicitly set to English
    // This ensures Norwegian is the primary language
    return this.config.defaultLocale; // Which is 'nb' (Norwegian)
  }

  /**
   * Validate locale
   */
  private isValidLocale(locale: string): locale is SupportedLocale {
    return locale === 'nb' || locale === 'en';
  }

  /**
   * Get current locale
   */
  getLocale(): SupportedLocale {
    return this.currentLocale;
  }

  /**
   * Set locale and persist to storage
   * Safe for SSR
   */
  setLocale(locale: SupportedLocale): void {
    if (!this.isValidLocale(locale)) {
      console.warn(`Invalid locale: ${locale}. Using fallback: ${this.config.fallbackLocale}`);
      locale = this.config.fallbackLocale;
    }

    const previousLocale = this.currentLocale;
    this.currentLocale = locale;
    
    // Only access localStorage on client
    if (typeof window !== 'undefined') {
      try {
        localStorage.setItem(this.config.storageKey, locale);
        
        // Dispatch custom event to notify all hooks about locale change
        window.dispatchEvent(new CustomEvent('i18n-locale-changed', {
          detail: { locale, previousLocale }
        }));
      } catch (error) {
        console.warn('Failed to save locale to localStorage:', error);
      }
    }
  }

  /**
   * Get direction for locale (all supported locales are LTR)
   */
  getDirection(): 'ltr' | 'rtl' {
    return 'ltr';
  }

  /**
   * Format locale for HTML lang attribute
   */
  getHtmlLang(): string {
    return this.currentLocale === 'nb' ? 'nb-NO' : 'en-US';
  }

  /**
   * Get native language name
   */
  getLanguageName(locale?: SupportedLocale): string {
    const targetLocale = locale || this.currentLocale;
    return targetLocale === 'nb' ? 'Norsk' : 'English';
  }

  /**
   * Toggle between languages
   */
  toggleLanguage(): SupportedLocale {
    const newLocale = this.currentLocale === 'nb' ? 'en' : 'nb';
    this.setLocale(newLocale);
    return newLocale;
  }
}

// Global instance
export const i18nManager = new I18nManager();

/**
 * Get current locale
 */
export const getCurrentLocale = (): SupportedLocale => i18nManager.getLocale();

/**
 * Set current locale
 */
export const setCurrentLocale = (locale: SupportedLocale): void => i18nManager.setLocale(locale);

/**
 * Toggle language
 */
export const toggleLanguage = (): SupportedLocale => i18nManager.toggleLanguage();

// Re-export translations and components only (avoid circular re-export of hooks)
export * from './translations/nb';
export * from './translations/en';
export * from './components/LanguageSelector';
