'use client';

import React, { createContext, useContext, useCallback, useEffect, useState } from 'react';
import { i18nManager, type SupportedLocale } from '@/components/auth/lib/i18n';

// Global language types - using auth system as standard
export type GlobalLocale = SupportedLocale; // 'en' | 'nb'

// Mapping between different i18n systems
const LOCALE_MAPPING = {
  // Auth system uses 'nb', chat system uses 'no'
  authToChat: (locale: SupportedLocale): 'en' | 'no' => {
    return locale === 'nb' ? 'no' : 'en';
  },
  chatToAuth: (locale: 'en' | 'no'): SupportedLocale => {
    return locale === 'no' ? 'nb' : 'en';
  },
  // Sidebar system uses same as chat
  authToSidebar: (locale: SupportedLocale): 'en' | 'no' => {
    return locale === 'nb' ? 'no' : 'en';
  }
};

interface GlobalLanguageContextType {
  // Current language (auth system format)
  locale: GlobalLocale;
  
  // Change language globally
  changeLocale: (newLocale: GlobalLocale) => void;
  
  // Toggle between languages
  toggleLanguage: () => void;
  
  // Convenience getters
  isNorwegian: boolean;
  isEnglish: boolean;
  
  // Get locale in different formats for different systems
  getChatLocale: () => 'en' | 'no';
  getSidebarLocale: () => 'en' | 'no';
  getAuthLocale: () => SupportedLocale;
  
  // Language display names
  getLanguageName: () => string;
  getLanguageNativeName: () => string;
}

const GlobalLanguageContext = createContext<GlobalLanguageContextType | undefined>(undefined);

interface GlobalLanguageProviderProps {
  children: React.ReactNode;
}

export function GlobalLanguageProvider({ children }: GlobalLanguageProviderProps) {
  // Use auth i18n manager as the source of truth
  const [locale, setLocale] = useState<GlobalLocale>(() => {
    if (typeof window === 'undefined') {
      return 'nb'; // Default to Norwegian on server
    }
    return i18nManager.getLocale();
  });

  // Sync with auth i18n manager changes
  useEffect(() => {
    if (typeof window === 'undefined') return;

    const handleStorageChange = (e: StorageEvent) => {
      if (e.key === 'id-knuten-locale' && e.newValue) {
        const newLocale = e.newValue as SupportedLocale;
        if (newLocale !== locale) {
          setLocale(newLocale);
          // Dispatch custom event for other systems
          window.dispatchEvent(new CustomEvent('global-language-changed', {
            detail: { 
              locale: newLocale,
              chatLocale: LOCALE_MAPPING.authToChat(newLocale),
              sidebarLocale: LOCALE_MAPPING.authToSidebar(newLocale)
            }
          }));
        }
      }
    };

    const handleLocaleChange = (e: CustomEvent) => {
      const { locale: newLocale } = e.detail;
      if (newLocale !== locale) {
        setLocale(newLocale);
      }
    };

    // Listen to auth system changes
    window.addEventListener('storage', handleStorageChange);
    window.addEventListener('i18n-locale-changed', handleLocaleChange as EventListener);

    // Initial sync
    const currentLocale = i18nManager.getLocale();
    if (currentLocale !== locale) {
      // eslint-disable-next-line react-hooks/set-state-in-effect
      setLocale(currentLocale);
    }

    return () => {
      window.removeEventListener('storage', handleStorageChange);
      window.removeEventListener('i18n-locale-changed', handleLocaleChange as EventListener);
    };
  }, [locale]);

  // Change locale function
  const changeLocale = useCallback((newLocale: GlobalLocale) => {
    i18nManager.setLocale(newLocale);
    setLocale(newLocale);
    
    // Dispatch global event for other systems to sync
    if (typeof window !== 'undefined') {
      window.dispatchEvent(new CustomEvent('global-language-changed', {
        detail: { 
          locale: newLocale,
          chatLocale: LOCALE_MAPPING.authToChat(newLocale),
          sidebarLocale: LOCALE_MAPPING.authToSidebar(newLocale)
        }
      }));
    }
  }, []);

  // Toggle between languages
  const toggleLanguage = useCallback(() => {
    const newLocale = i18nManager.toggleLanguage();
    setLocale(newLocale);
    
    // Dispatch global event
    if (typeof window !== 'undefined') {
      window.dispatchEvent(new CustomEvent('global-language-changed', {
        detail: { 
          locale: newLocale,
          chatLocale: LOCALE_MAPPING.authToChat(newLocale),
          sidebarLocale: LOCALE_MAPPING.authToSidebar(newLocale)
        }
      }));
    }
    
    return newLocale;
  }, []);

  // Format converters
  const getChatLocale = useCallback(() => LOCALE_MAPPING.authToChat(locale), [locale]);
  const getSidebarLocale = useCallback(() => LOCALE_MAPPING.authToSidebar(locale), [locale]);
  const getAuthLocale = useCallback(() => locale, [locale]);

  // Language names
  const getLanguageName = useCallback(() => {
    return locale === 'nb' ? 'Norwegian' : 'English';
  }, [locale]);

  const getLanguageNativeName = useCallback(() => {
    return locale === 'nb' ? 'Norsk' : 'English';
  }, [locale]);

  const value: GlobalLanguageContextType = {
    locale,
    changeLocale,
    toggleLanguage,
    isNorwegian: locale === 'nb',
    isEnglish: locale === 'en',
    getChatLocale,
    getSidebarLocale,
    getAuthLocale,
    getLanguageName,
    getLanguageNativeName,
  };

  return (
    <GlobalLanguageContext.Provider value={value}>
      {children}
    </GlobalLanguageContext.Provider>
  );
}

// Hook to use global language context
export function useGlobalLanguage() {
  const context = useContext(GlobalLanguageContext);
  if (context === undefined) {
    throw new Error('useGlobalLanguage must be used within a GlobalLanguageProvider');
  }
  return context;
}

// Hook that provides compatibility with different i18n systems
export function useCompatibleLanguage() {
  const global = useGlobalLanguage();
  
  return {
    // Global context
    global,
    
    // For chat system compatibility
    chatLocale: global.getChatLocale(),
    
    // For sidebar system compatibility  
    sidebarLocale: global.getSidebarLocale(),
    
    // For auth system compatibility
    authLocale: global.getAuthLocale(),
    
    // Change handlers for different systems
    handleChatLocaleChange: useCallback((chatLocale: 'en' | 'no') => {
      const authLocale = LOCALE_MAPPING.chatToAuth(chatLocale);
      global.changeLocale(authLocale);
    }, [global]),
    
    handleSidebarLocaleChange: useCallback((sidebarLocale: 'en' | 'no') => {
      const authLocale = LOCALE_MAPPING.chatToAuth(sidebarLocale);
      global.changeLocale(authLocale);
    }, [global]),
  };
}

// Export locale mapping for use in other components
export { LOCALE_MAPPING };
