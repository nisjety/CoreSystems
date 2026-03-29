'use client';

import * as React from 'react';

// Supported languages
export type Locale = 'no' | 'en' | 'sv' | 'da';

// Translation keys structure
export interface Translations {
  auth: {
    signin: {
      title: string;
      description: string;
      email: string;
      password: string;
      button: string;
      forgotPassword: string;
      loading: string;
    };
    signup: {
      title: string;
      description: string;
      name: string;
      email: string;
      password: string;
      confirmPassword: string;
      button: string;
      loading: string;
    };
    forgotPassword: {
      title: string;
      description: string;
      email: string;
      button: string;
      backToSignin: string;
      loading: string;
    };
    sso: {
      title: string;
      description: string;
      businessEmail: string;
      organizationDomain: string;
      button: string;
      connecting: string;
    };
    organization: {
      title: string;
      description: string;
      orgName: string;
      orgSlug: string;
      inviteEmail: string;
      inviteRole: string;
      roles: {
        member: string;
        admin: string;
      };
      button: string;
      creating: string;
    };
    tabs: {
      signin: string;
      signup: string;
      sso: string;
      organization: string;
    };
    social: {
      continueWith: string;
      google: string;
      microsoft: string;
    };
    passkey: {
      authenticate: string;
      register: string;
      usePasskey: string;
      createPasskey: string;
    };
    errors: {
      required: string;
      invalidEmail: string;
      passwordTooShort: string;
      passwordMismatch: string;
      general: string;
    };
    legal: {
      termsAndPrivacy: string;
      terms: string;
      privacy: string;
      gdprNotice: string;
      deleteCookie: string;
    };
  };
  common: {
    loading: string;
    error: string;
    success: string;
    cancel: string;
    save: string;
    delete: string;
    edit: string;
    close: string;
    back: string;
    next: string;
    optional: string;
    required: string;
  };
  toasts: {
    success: {
      authSuccess: string;
      profileUpdated: string;
      passwordChanged: string;
      organizationCreated: string;
    };
    errors: {
      authFailed: string;
      networkError: string;
      serverError: string;
      invalidCredentials: string;
      sessionExpired: string;
    };
  };
  demo: {
    title: string;
    description: string;
    buttons: {
      toast: string;
      startLoading: string;
      loading: string;
      language: string;
      error: string;
    };
    features: {
      title: string;
      toast: string;
      loading: string;
      i18n: string;
      responsive: string;
    };
    toast: {
      success: {
        title: string;
        description: string;
      };
    };
    loading: {
      starting: string;
      stage1: string;
      stage2: string;
      stage3: string;
      stage4: string;
      progress25: string;
      progress50: string;
      progress75: string;
      complete: {
        title: string;
        description: string;
      };
    };
    language: {
      switched: {
        title: string;
        description: string;
      };
    };
    error: {
      title: string;
      description: string;
    };
  };
}

// Norwegian translations (default)
const norwegianTranslations: Translations = {
  auth: {
    signin: {
      title: 'Logg inn på din konto',
      description: 'Vennligst oppgi dine innloggingsdetaljer',
      email: 'E-postadresse',
      password: 'Passord',
      button: 'Logg inn',
      forgotPassword: 'Glemt passordet?',
      loading: 'Logger inn...',
    },
    signup: {
      title: 'Opprett din konto',
      description: 'Opprett en ny konto for å komme i gang',
      name: 'Fullt navn',
      email: 'E-postadresse',
      password: 'Passord',
      confirmPassword: 'Bekreft passord',
      button: 'Opprett konto',
      loading: 'Oppretter konto...',
    },
    forgotPassword: {
      title: 'Tilbakestill passord',
      description: 'Vi sender deg en lenke for å tilbakestille passordet',
      email: 'E-postadresse',
      button: 'Send tilbakestillingslenke',
      backToSignin: 'Tilbake til innlogging',
      loading: 'Sender...',
    },
    sso: {
      title: 'Enterprise SSO',
      description: 'Logg inn med din organisasjons identitetsleverandør',
      businessEmail: 'Arbeids-e-post',
      organizationDomain: 'Organisasjonsdomene',
      button: 'Fortsett med SSO',
      connecting: 'Kobler til...',
    },
    organization: {
      title: 'Organisasjonsstyring',
      description: 'Opprett og administrer organisasjoner og medlemmer',
      orgName: 'Organisasjonsnavn',
      orgSlug: 'Organisasjons-URL',
      inviteEmail: 'Inviter teammedlem',
      inviteRole: 'Rolle for invitert medlem',
      roles: {
        member: 'Medlem',
        admin: 'Administrator',
      },
      button: 'Opprett organisasjon',
      creating: 'Oppretter organisasjon...',
    },
    tabs: {
      signin: 'Logg inn',
      signup: 'Registrer',
      sso: 'SSO',
      organization: 'Org',
    },
    social: {
      continueWith: 'Fortsett med',
      google: 'Google',
      microsoft: 'Microsoft',
    },
    passkey: {
      authenticate: 'Autentiser med passkey',
      register: 'Registrer passkey',
      usePasskey: 'Bruk passkey',
      createPasskey: 'Opprett passkey',
    },
    errors: {
      required: 'Dette feltet er påkrevd',
      invalidEmail: 'Ugyldig e-postadresse',
      passwordTooShort: 'Passordet må være minst 8 tegn',
      passwordMismatch: 'Passordene stemmer ikke overens',
      general: 'En feil oppstod. Prøv igjen.',
    },
    legal: {
      termsAndPrivacy: 'Ved å fortsette aksepterer du våre brukervilkår og personvernregler',
      terms: 'brukervilkår',
      privacy: 'personvernregler',
      gdprNotice: 'Vi lagrer kun nødvendig innloggingsdata lokalt på din enhet, med automatisk utløp. Du kan når som helst slette dette',
      deleteCookie: 'Slett Cookie',
    },
  },
  common: {
    loading: 'Laster...',
    error: 'Feil',
    success: 'Suksess',
    cancel: 'Avbryt',
    save: 'Lagre',
    delete: 'Slett',
    edit: 'Rediger',
    close: 'Lukk',
    back: 'Tilbake',
    next: 'Neste',
    optional: 'Valgfritt',
    required: 'Påkrevd',
  },
  toasts: {
    success: {
      authSuccess: 'Innlogging vellykket!',
      profileUpdated: 'Profilen din er oppdatert',
      passwordChanged: 'Passordet ditt er endret',
      organizationCreated: 'Organisasjonen er opprettet',
    },
    errors: {
      authFailed: 'Innlogging feilet',
      networkError: 'Nettverksfeil. Sjekk tilkoblingen din.',
      serverError: 'Serverfeil. Prøv igjen senere.',
      invalidCredentials: 'Ugyldig e-post eller passord',
      sessionExpired: 'Sesjonen din har utløpt. Vennligst logg inn igjen.',
    },
  },
  demo: {
    title: 'Forbedret Autentiseringsside Demo',
    description: 'Test de nye funksjonene: Toast-varsler, lastingstilstander og flerspråklig støtte',
    buttons: {
      toast: 'Vis Toast',
      startLoading: 'Start Lasting',
      loading: 'Laster...',
      language: 'Bytt Språk',
      error: 'Vis Feil',
    },
    features: {
      title: 'Aktiverte Funksjoner',
      toast: 'Toast Varsler',
      loading: 'Lastingstilstander',
      i18n: 'Flerspråklig Støtte',
      responsive: 'Responsiv Design',
    },
    toast: {
      success: {
        title: 'Demo Toast!',
        description: 'Dette er et eksempel på toast-varsel systemet.',
      },
    },
    loading: {
      starting: 'Starter demo lasting...',
      stage1: 'Initialiserer',
      stage2: 'Behandler data',
      stage3: 'Validerer informasjon',
      stage4: 'Fullfører',
      progress25: 'Første trinn fullført',
      progress50: 'Halvveis ferdig',
      progress75: 'Nesten ferdig',
      complete: {
        title: 'Demo Fullført!',
        description: 'Lastingsdemo er fullført suksessfullt.',
      },
    },
    language: {
      switched: {
        title: 'Språk Endret',
        description: 'Språket ble endret suksessfullt.',
      },
    },
    error: {
      title: 'Demo Feil',
      description: 'Dette er et eksempel på feilmelding.',
    },
  },
};

// English translations
const englishTranslations: Translations = {
  auth: {
    signin: {
      title: 'Sign in to your account',
      description: 'Please enter your login details',
      email: 'Email address',
      password: 'Password',
      button: 'Sign in',
      forgotPassword: 'Forgot password?',
      loading: 'Signing in...',
    },
    signup: {
      title: 'Create your account',
      description: 'Create a new account to get started',
      name: 'Full name',
      email: 'Email address',
      password: 'Password',
      confirmPassword: 'Confirm password',
      button: 'Create account',
      loading: 'Creating account...',
    },
    forgotPassword: {
      title: 'Reset password',
      description: 'We\'ll send you a link to reset your password',
      email: 'Email address',
      button: 'Send reset link',
      backToSignin: 'Back to sign in',
      loading: 'Sending...',
    },
    sso: {
      title: 'Enterprise SSO',
      description: 'Sign in with your organization\'s identity provider',
      businessEmail: 'Business email',
      organizationDomain: 'Organization domain',
      button: 'Continue with SSO',
      connecting: 'Connecting...',
    },
    organization: {
      title: 'Organization Management',
      description: 'Create and manage organizations and members',
      orgName: 'Organization name',
      orgSlug: 'Organization URL',
      inviteEmail: 'Invite team member',
      inviteRole: 'Role for invited member',
      roles: {
        member: 'Member',
        admin: 'Admin',
      },
      button: 'Create organization',
      creating: 'Creating organization...',
    },
    tabs: {
      signin: 'Sign In',
      signup: 'Sign Up',
      sso: 'SSO',
      organization: 'Org',
    },
    social: {
      continueWith: 'Continue with',
      google: 'Google',
      microsoft: 'Microsoft',
    },
    passkey: {
      authenticate: 'Authenticate with passkey',
      register: 'Register passkey',
      usePasskey: 'Use passkey',
      createPasskey: 'Create passkey',
    },
    errors: {
      required: 'This field is required',
      invalidEmail: 'Invalid email address',
      passwordTooShort: 'Password must be at least 8 characters',
      passwordMismatch: 'Passwords do not match',
      general: 'An error occurred. Please try again.',
    },
    legal: {
      termsAndPrivacy: 'By continuing you accept our terms and privacy policy',
      terms: 'terms',
      privacy: 'privacy policy',
      gdprNotice: 'We only store necessary login data locally on your device, with automatic expiration. You can delete this at any time',
      deleteCookie: 'Delete Cookie',
    },
  },
  common: {
    loading: 'Loading...',
    error: 'Error',
    success: 'Success',
    cancel: 'Cancel',
    save: 'Save',
    delete: 'Delete',
    edit: 'Edit',
    close: 'Close',
    back: 'Back',
    next: 'Next',
    optional: 'Optional',
    required: 'Required',
  },
  toasts: {
    success: {
      authSuccess: 'Sign in successful!',
      profileUpdated: 'Your profile has been updated',
      passwordChanged: 'Your password has been changed',
      organizationCreated: 'Organization has been created',
    },
    errors: {
      authFailed: 'Sign in failed',
      networkError: 'Network error. Check your connection.',
      serverError: 'Server error. Please try again later.',
      invalidCredentials: 'Invalid email or password',
      sessionExpired: 'Your session has expired. Please sign in again.',
    },
  },
  demo: {
    title: 'Enhanced Authentication Page Demo',
    description: 'Test the new features: Toast notifications, loading states, and multilingual support',
    buttons: {
      toast: 'Show Toast',
      startLoading: 'Start Loading',
      loading: 'Loading...',
      language: 'Switch Language',
      error: 'Show Error',
    },
    features: {
      title: 'Active Features',
      toast: 'Toast Notifications',
      loading: 'Loading States',
      i18n: 'Multilingual Support',
      responsive: 'Responsive Design',
    },
    toast: {
      success: {
        title: 'Demo Toast!',
        description: 'This is an example of the toast notification system.',
      },
    },
    loading: {
      starting: 'Starting demo loading...',
      stage1: 'Initializing',
      stage2: 'Processing data',
      stage3: 'Validating information',
      stage4: 'Completing',
      progress25: 'First step completed',
      progress50: 'Halfway done',
      progress75: 'Almost finished',
      complete: {
        title: 'Demo Complete!',
        description: 'Loading demo completed successfully.',
      },
    },
    language: {
      switched: {
        title: 'Language Changed',
        description: 'Language was changed successfully.',
      },
    },
    error: {
      title: 'Demo Error',
      description: 'This is an example error message.',
    },
  },
};

// Translations map
const translations: Record<Locale, Translations> = {
  no: norwegianTranslations,
  en: englishTranslations,
  sv: norwegianTranslations, // TODO: Add Swedish translations
  da: norwegianTranslations, // TODO: Add Danish translations
};

interface I18nContextType {
  locale: Locale;
  setLocale: (locale: Locale) => void;
  t: (key: string) => string;
  translations: Translations;
}

const I18nContext = React.createContext<I18nContextType | undefined>(undefined);

// Helper function to get nested object value by dot notation key
function getNestedValue(obj: Record<string, unknown>, key: string): string {
  return key.split('.').reduce((o: unknown, i: string) => {
    return (o as Record<string, unknown>)?.[i];
  }, obj) as string || key;
}

export function I18nProvider({ 
  children, 
  defaultLocale = 'no' 
}: { 
  children: React.ReactNode;
  defaultLocale?: Locale;
}) {
  const [locale, setLocale] = React.useState<Locale>(defaultLocale);

  const currentTranslations = translations[locale];

  const t = React.useCallback((key: string): string => {
    return getNestedValue(currentTranslations as unknown as Record<string, unknown>, key);
  }, [currentTranslations]);

  const value = React.useMemo(() => ({
    locale,
    setLocale,
    t,
    translations: currentTranslations,
  }), [locale, setLocale, t, currentTranslations]);

  return React.createElement(
    I18nContext.Provider,
    { value },
    children
  );
}

export function useI18n() {
  const context = React.useContext(I18nContext);
  if (context === undefined) {
    throw new Error('useI18n must be used within an I18nProvider');
  }
  return context;
}

// Convenience hook for common translation patterns
export function useTranslation() {
  const { t } = useI18n();
  return { t };
}
