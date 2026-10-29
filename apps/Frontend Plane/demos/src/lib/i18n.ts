// Supported locales
export type SupportedLocale = 'en' | 'nb';

// Default locale
export const DEFAULT_LOCALE: SupportedLocale = 'nb';

// Available locales
export const AVAILABLE_LOCALES: SupportedLocale[] = ['en', 'nb'];

// Locale names
export const LOCALE_NAMES: Record<SupportedLocale, { name: string; nativeName: string }> = {
  en: {
    name: 'English',
    nativeName: 'English',
  },
  nb: {
    name: 'Norwegian Bokmål',
    nativeName: 'Norsk Bokmål',
  },
};

// Simple i18n manager
export const i18nManager = {
  getDefaultLocale: (): SupportedLocale => DEFAULT_LOCALE,
  
  isValidLocale: (locale: string): locale is SupportedLocale => {
    return AVAILABLE_LOCALES.includes(locale as SupportedLocale);
  },
  
  getLocaleName: (locale: SupportedLocale): string => {
    return LOCALE_NAMES[locale].name;
  },
  
  getNativeLocaleName: (locale: SupportedLocale): string => {
    return LOCALE_NAMES[locale].nativeName;
  },
};
