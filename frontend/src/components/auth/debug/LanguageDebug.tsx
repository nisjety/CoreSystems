'use client';

import { useTranslation, useLanguageSwitch } from '../lib/i18n/hooks';

export function LanguageDebug() {
  const { t, locale, isHydrated } = useTranslation();
  const { isNorwegian, toggleLanguage } = useLanguageSwitch();

  return (
    <div className="fixed top-4 right-4 bg-white p-4 border rounded shadow-lg z-50 text-sm">
      <h3 className="font-bold mb-2">🔍 Language Debug</h3>
      <div className="space-y-1">
        <div>Current locale: <strong>{locale}</strong></div>
        <div>Is Norwegian: <strong>{isNorwegian ? 'Yes' : 'No'}</strong></div>
        <div>Is Hydrated: <strong>{isHydrated ? 'Yes' : 'No'}</strong></div>
        <div>Test translation: <strong>{t('consent.banner.accept')}</strong></div>
        <button 
          onClick={toggleLanguage}
          className="mt-2 px-2 py-1 bg-blue-500 text-white rounded text-xs"
        >
          Toggle Language
        </button>
      </div>
    </div>
  );
}
