'use client';

import { useEffect } from 'react';
import { i18nManager } from '../lib/i18n';

/**
 * Component to initialize Norwegian as the default language
 * This ensures that the app always starts with Norwegian unless user explicitly changes it
 */
export function LanguageInitializer() {
  useEffect(() => {
    // Only run on client side
    if (typeof window === 'undefined') return;

    try {
      // Check if there's a stored locale
      const storedLocale = localStorage.getItem('id-knuten-locale');
      
      // If no locale is stored, or if it's set to English without user intention,
      // set it to Norwegian (nb) as the default
      if (!storedLocale) {
        console.log('No stored locale found, setting to Norwegian (nb)');
        i18nManager.setLocale('nb');
      } else if (storedLocale === 'en') {
        // If English is set, we respect that choice
        console.log('English locale found, keeping user preference');
      } else if (storedLocale !== 'nb') {
        // If any other locale is set, default to Norwegian
        console.log('Unknown locale found, defaulting to Norwegian (nb)');
        i18nManager.setLocale('nb');
      }
    } catch (error) {
      // If localStorage fails, just log the error
      console.warn('Failed to initialize language settings:', error);
    }
  }, []);

  // This component doesn't render anything
  return null;
}
