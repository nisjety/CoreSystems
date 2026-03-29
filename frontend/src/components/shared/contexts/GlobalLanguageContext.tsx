'use client';

import React, { createContext, useContext, useState, ReactNode } from 'react';

type Locale = 'en' | 'nb';
type ChatLocale = 'en' | 'no';

export interface GlobalLanguageContextType {
  locale: Locale;
  changeLocale: (locale: Locale) => void;
  getChatLocale: () => ChatLocale;
}

const GlobalLanguageContext = createContext<GlobalLanguageContextType | undefined>(undefined);

export function GlobalLanguageProvider({ children }: { children: ReactNode }) {
  const [locale, setLocale] = useState<Locale>('en');

  const changeLocale = (newLocale: Locale) => {
    setLocale(newLocale);
  };

  const getChatLocale = (): ChatLocale => {
    return locale === 'nb' ? 'no' : 'en';
  };

  return (
    <GlobalLanguageContext.Provider value={{ locale, changeLocale, getChatLocale }}>
      {children}
    </GlobalLanguageContext.Provider>
  );
}

export function useGlobalLanguage() {
  const context = useContext(GlobalLanguageContext);
  if (context === undefined) {
    // Return a default context instead of throwing error to prevent crashes if provider is missing
    // This is useful for standalone components that might be used outside the main app context
    return {
      locale: 'en' as Locale,
      changeLocale: () => {},
      getChatLocale: () => 'en' as ChatLocale
    };
  }
  return context;
}
