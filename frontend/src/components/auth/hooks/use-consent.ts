/**
 * DEPRECATED: This lightweight consent hook is superseded by the enterprise
 * consent system located at `@/components/auth/consent` which provides:
 *  - Extended consent categories (including analytics & social)
 *  - Persistence + versioning + migration layer
 *  - Translation integration via consentT
 *  - Event dispatch & listener API
 *  - Script gating helpers (useConsentScript)
 *  - Analytics gating utilities (hasConsent)
 *
 * This file remains temporarily to avoid breaking external imports during
 * migration. New code MUST import from the enterprise module:
 *    import { useConsent } from '@/components/auth/consent';
 *
 * Pending removal once all legacy imports are eliminated.
 */
import { useState, useEffect } from 'react';
import { 
  readConsent, 
  writeConsent, 
  shouldShowConsentBanner,
  clearConsent,
  type ConsentState,
  DEFAULT_CONSENT 
} from '../utils/consent';

export function useConsent() {
  const [consent, setConsent] = useState<ConsentState>(DEFAULT_CONSENT);
  const [showBanner, setShowBanner] = useState(false);
  const [showPreferences, setShowPreferences] = useState(false);

  useEffect(() => {
    const shouldShow = shouldShowConsentBanner();
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setShowBanner(shouldShow);
    
    if (!shouldShow) {
      const stored = readConsent();
      if (stored) {
        setConsent(stored.choices);
      }
    }
  }, []);

  const acceptAll = () => {
    const allConsent: ConsentState = {
      necessary: true,
      performance: true,
      functional: true,
      marketing: true,
    };
    setConsent(allConsent);
    writeConsent(allConsent);
    setShowBanner(false);
    setShowPreferences(false);
  };

  const rejectAll = () => {
    const minimalConsent: ConsentState = {
      necessary: true,
      performance: false,
      functional: false,
      marketing: false,
    };
    setConsent(minimalConsent);
    writeConsent(minimalConsent);
    setShowBanner(false);
    setShowPreferences(false);
  };

  const savePreferences = () => {
    writeConsent(consent);
    setShowBanner(false);
    setShowPreferences(false);
  };

  const updateConsent = (key: keyof ConsentState, value: boolean) => {
    setConsent(prev => ({
      ...prev,
      [key]: value
    }));
  };

  const resetConsent = () => {
    clearConsent();
    setConsent(DEFAULT_CONSENT);
    setShowBanner(true);
    setShowPreferences(false);
  };

  return {
    consent,
    showBanner,
    showPreferences,
    acceptAll,
    rejectAll,
    savePreferences,
    updateConsent,
    setShowPreferences,
    openPreferences: () => setShowPreferences(true), // Add this function
    resetConsent, // Add this for testing
  };
}
