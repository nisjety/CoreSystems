import React, { ReactNode, createContext, useContext } from 'react';
import { ConsentSystem } from './ConsentSystem';
import { useConsent, ConsentState, ConsentKeys } from './useConsent';

/**
 * Enterprise Consent Context for sharing consent state across the application
 */
interface ConsentContextValue {
  consent: ConsentState;
  showBanner: boolean;
  showPreferences: boolean;
  isLoading: boolean;
  currentUser: { id: string; email: string } | null;
  
  // Actions
  saveConsent: (newConsent: ConsentState) => Promise<void>;
  acceptAll: () => Promise<void>;
  rejectAll: () => Promise<void>;
  showPreferencesModal: () => void;
  hidePreferences: () => void;
  updateConsent: (key: ConsentKeys, value: boolean) => void;
  resetConsent: () => Promise<void>;
  
  // Utilities
  hasConsent: (key: ConsentKeys) => boolean;
  hasConsentChoice: () => boolean;
  getConsentSummary: () => {
    current: ConsentState;
    stored: ConsentState | null;
    timestamp: number | null;
    version: string | null;
    hasChoice: boolean;
    loadingState: {
      saving: boolean;
      resetting: boolean;
      syncing: boolean;
      scriptLoading: Record<string, boolean>;
    };
    user: { id: string; email: string } | null;
  };
}

const ConsentContext = createContext<ConsentContextValue | null>(null);

/**
 * Main Consent Provider that sets up the complete enterprise consent system
 * Includes backend integration, i18n support, analytics, and SSR safety
 */
interface ConsentProviderProps {
  children: ReactNode;
}

export function ConsentProvider({ children }: ConsentProviderProps) {
  const consentHook = useConsent();

  const contextValue: ConsentContextValue = {
    consent: consentHook.consent,
    showBanner: consentHook.showBanner,
    showPreferences: consentHook.showPreferences,
    isLoading: consentHook.isLoading,
    currentUser: consentHook.currentUser,
    
    // Actions
    saveConsent: consentHook.saveConsent,
    acceptAll: consentHook.acceptAll,
    rejectAll: consentHook.rejectAll,
    showPreferencesModal: consentHook.showPreferencesModal,
    hidePreferences: consentHook.hidePreferences,
    updateConsent: consentHook.updateConsent,
    resetConsent: consentHook.resetConsent,
    
    // Utilities
    hasConsent: consentHook.hasConsent,
    hasConsentChoice: consentHook.hasConsentChoice,
    getConsentSummary: consentHook.getConsentSummary,
  };

  return (
    <ConsentContext.Provider value={contextValue}>
      {children}
      {/* Enterprise Consent UI Components with backend integration */}
      <ConsentSystem />
    </ConsentContext.Provider>
  );
}

/**
 * Hook to access the consent context
 * Provides enterprise consent functionality with backend sync and analytics
 */
export function useConsentContext(): ConsentContextValue {
  const context = useContext(ConsentContext);
  if (!context) {
    throw new Error('useConsentContext must be used within a ConsentProvider');
  }
  return context;
}

export default ConsentProvider;
