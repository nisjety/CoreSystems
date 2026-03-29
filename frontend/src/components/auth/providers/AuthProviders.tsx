'use client';

import * as React from 'react';
import { ToastProvider } from '../hooks/use-toast';
import { LoadingProvider } from '../hooks/use-loading';
import { I18nProvider } from '../hooks/use-i18n';

export interface AuthProvidersProps {
  children: React.ReactNode;
  locale?: 'nb' | 'no' | 'en' | 'sv' | 'da';
}

export function AuthProviders({ children, locale = 'nb' }: AuthProvidersProps) {
  // Convert new locale system to old locale system
  const convertedLocale = React.useMemo(() => {
    if (locale === 'nb') return 'no'; // Convert nb to no for the old system
    return locale as 'no' | 'en' | 'sv' | 'da';
  }, [locale]);

  return (
    <I18nProvider defaultLocale={convertedLocale}>
      <LoadingProvider>
        <ToastProvider>
          {children}
        </ToastProvider>
      </LoadingProvider>
    </I18nProvider>
  );
}
