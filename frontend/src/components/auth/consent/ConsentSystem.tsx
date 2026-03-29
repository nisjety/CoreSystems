import React from 'react';
import { ConsentBanner } from './ConsentBanner';
import { ConsentPreferences } from './ConsentPreferences';
import { useConsent } from './useConsent';

/**
 * Main Consent System Component
 * Integrates banner and preferences modal for complete consent management
 */
export function ConsentSystem() {
  const {
    consent,
    showBanner,
    showPreferences,
    isLoading,
    acceptAll,
    rejectAll,
    showPreferencesModal,
    hidePreferences,
    updateConsent,
    saveConsent,
  } = useConsent();

  const handleSave = async () => {
    await saveConsent(consent);
  };

  return (
    <>
      {/* Consent Banner - appears when user hasn't made choice */}
      {showBanner && (
        <ConsentBanner
          onAcceptAll={acceptAll}
          onRejectAll={rejectAll}
          onShowPreferences={showPreferencesModal}
          isLoading={isLoading}
        />
      )}

      {/* Consent Preferences Modal - appears when user wants to customize */}
      <ConsentPreferences
        isOpen={showPreferences}
        onClose={hidePreferences}
        consent={consent}
        onConsentChange={updateConsent}
        onSave={handleSave}
        onAcceptAll={acceptAll}
        onRejectAll={rejectAll}
        isLoading={isLoading}
      />
    </>
  );
}

export default ConsentSystem;
