// components/ui/LocaleSwitcher.tsx - Language Switcher Component
'use client';

import { useI18n, type Locale } from '@/components/chat/hooks/i18n';
import { Globe } from 'lucide-react';

const localeLabels: Record<Locale, string> = {
  en: 'English',
  no: 'Norsk',
};

const localeFlags: Record<Locale, string> = {
  en: '🇺🇸',
  no: '🇳🇴',
};

export function LocaleSwitcher() {
  const { locale, changeLocale } = useI18n();

  return (
    <div className="relative">
      <select
        value={locale}
        onChange={(e) => changeLocale(e.target.value as Locale)}
        className="appearance-none bg-white border border-gray-200 rounded-lg px-3 py-2 pr-8 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent"
      >
        {Object.entries(localeLabels).map(([localeKey, label]) => (
          <option key={localeKey} value={localeKey}>
            {localeFlags[localeKey as Locale]} {label}
          </option>
        ))}
      </select>
      
      <div className="absolute right-2 top-1/2 transform -translate-y-1/2 pointer-events-none">
        <Globe className="w-4 h-4 text-gray-500" />
      </div>
    </div>
  );
}
