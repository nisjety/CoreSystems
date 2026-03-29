import { useState, useEffect, useCallback } from 'react';
import { authClient } from '../lib/auth-client-enterprise';
import { authProviderClient } from '../lib/api/auth-provider-client';
import { useConsentTranslation } from '../lib/i18n/hooks';
import { useIsHydrated } from '../lib/hydration/HydrationGuard';

// Enhanced types for enterprise consent management
export type ConsentKeys = 'necessary' | 'performance' | 'functional' | 'marketing' | 'analytics' | 'social';
export type ConsentState = Record<ConsentKeys, boolean>;

// Enterprise consent interfaces (server-managed; local type removed to avoid unused warnings)

// Enhanced loading state management
interface ConsentLoadingState {
  saving: boolean;
  resetting: boolean;
  syncing: boolean;
  scriptLoading: Record<string, boolean>;
}

// Enhanced fallback implementations when providers not available
const defaultToastActions = {
  success: (title: string, description?: string) => console.log('✅ Consent Success:', title, description),
  error: (title: string, description?: string) => console.error('❌ Consent Error:', title, description),
  info: (title: string, description?: string) => console.log('ℹ️ Consent Info:', title, description),
  warning: (title: string, description?: string) => console.warn('⚠️ Consent Warning:', title, description),
};

// Constants
const CONSENT_VERSION = 'v1';
// Local storage key removed; server cookie is source of truth

const DEFAULT_CONSENT: ConsentState = {
  necessary: true,     // Always required
  performance: false,
  functional: false,
  marketing: false,
  analytics: false,
  social: false,
};

const DEFAULT_LOADING_STATE: ConsentLoadingState = {
  saving: false,
  resetting: false,
  syncing: false,
  scriptLoading: {},
};

// LocalStorage is deprecated for consent persistence; server cookie/API is the source of truth.

// Enhanced hook for consent management with enterprise features
export function useConsent() {
  const [consent, setConsent] = useState<ConsentState>(DEFAULT_CONSENT);
  const [showBanner, setShowBanner] = useState(false);
  const [showPreferencesModal, setShowPreferencesModal] = useState(false);
  const [isLoading, setIsLoading] = useState(true);
  const [loadingState, setLoadingState] = useState<ConsentLoadingState>(DEFAULT_LOADING_STATE);
  
  // Enterprise features integration
  const { consentT } = useConsentTranslation();
  const isHydrated = useIsHydrated();
  
  // Get current user session for backend sync
  const [currentUser, setCurrentUser] = useState<{ id: string; email: string } | null>(null);
  
  // Initialize from localStorage (fallback or non-authenticated users)
  const initializeLocalConsent = useCallback(() => {
    // No local storage fallback; default to banner for anonymous users
    setConsent(DEFAULT_CONSENT);
    setShowBanner(true);
    setIsLoading(false);
  }, []);

  // Sync with backend for authenticated users
  const syncConsentWithBackend = useCallback(async (choices: ConsentState, userInfo: { id: string; organizationId?: string }) => {
    if (!isHydrated) return;
    
    try {
      setLoadingState(prev => ({ ...prev, syncing: true }));
      
      await authProviderClient.updateConsent({
        necessary: choices.necessary,
        analytics: choices.analytics,
        marketing: choices.marketing,
        performance: choices.performance,
        functional: choices.functional,
      });
      
      // Create audit log entry - removing since we don't have audit log endpoint in provider client
      // TODO: Add audit logging when available
      
    } catch (error) {
      console.error('Error syncing consent with backend:', error);
      defaultToastActions.warning(
        consentT ? consentT.storage('error') : 'Could not sync with server',
        'Changes saved locally'
      );
    } finally {
      setLoadingState(prev => ({ ...prev, syncing: false }));
    }
  }, [isHydrated, consentT]);

  // Load consent from backend for authenticated users
  const loadConsentFromBackend = useCallback(async () => {
    try {
      setLoadingState(prev => ({ ...prev, syncing: true }));
      
      const backendConsent = await authProviderClient.getConsent();
      
  if (backendConsent) {
        // Ensure the backend consent has all required fields
        const validatedConsent: ConsentState = {
          necessary: backendConsent.necessary ?? true,
          performance: backendConsent.performance ?? false,
          functional: backendConsent.functional ?? false,
          marketing: backendConsent.marketing ?? false,
          analytics: backendConsent.analytics ?? false,
          social: false, // Not in new API contract, defaulting to false
        };
        setConsent(validatedConsent);
        setShowBanner(false);
  // Local persistence removed; server cookie holds the consent
      } else {
  // First time user, show banner
  setConsent(DEFAULT_CONSENT);
  setShowBanner(true);
      }
    } catch (error) {
      console.error('Error loading consent from backend:', error);
      defaultToastActions.error(
        consentT ? consentT.storage('error') : 'Could not load consent preferences',
        'Using local storage'
      );
  // Fall back to default banner when server not reachable
      initializeLocalConsent();
    } finally {
      setLoadingState(prev => ({ ...prev, syncing: false }));
    }
  }, [consentT, initializeLocalConsent]);
  
  // Initialize user session and consent
  useEffect(() => {
    const initializeUser = async () => {
      if (!isHydrated) return;
      
      try {
        const session = await authClient.getSession();
        if (session.data?.user) {
          setCurrentUser(session.data.user);
          // Load consent from backend for authenticated users
          await loadConsentFromBackend();
        } else {
          // Non-authenticated user, use localStorage
          initializeLocalConsent();
        }
      } catch (error) {
        console.error('Error getting user session:', error);
        // Fall back to localStorage for non-authenticated users
        initializeLocalConsent();
      }
    };
    
    initializeUser();
  }, [isHydrated, loadConsentFromBackend, initializeLocalConsent]);

  // Listen for custom events to show/hide banner
  useEffect(() => {
    const handleShowBanner = () => setShowBanner(true);
    const handleHideBanner = () => setShowBanner(false);
    
    if (typeof window !== 'undefined') {
      window.addEventListener('showConsentBanner', handleShowBanner);
      window.addEventListener('hideConsentBanner', handleHideBanner);
      
      return () => {
        window.removeEventListener('showConsentBanner', handleShowBanner);
        window.removeEventListener('hideConsentBanner', handleHideBanner);
      };
    }
  }, []);

  // Enhanced save consent with backend sync and error handling
  const saveConsent = useCallback(async (newConsent: ConsentState) => {
    setLoadingState(prev => ({ ...prev, saving: true }));
    
    try {
      // Ensure necessary cookies are always enabled
      const validatedConsent = {
        ...newConsent,
        necessary: true,
      };
      
      // Update state
      setConsent(validatedConsent);
      setShowBanner(false);
      setShowPreferencesModal(false);
      
      // Sync with backend if user is authenticated
      if (currentUser) {
        await syncConsentWithBackend(validatedConsent, currentUser);
      }
      
      defaultToastActions.success(
        consentT ? consentT.storage('success') : 'Consent choices saved',
        consentT ? consentT.storage('success') : 'Your preferences have been updated'
      );
      
    } catch (error) {
      console.error('Error saving consent:', error);
      defaultToastActions.error(
        consentT ? consentT.storage('error') : 'Could not save consent',
        'Please try again'
      );
    } finally {
      setLoadingState(prev => ({ ...prev, saving: false }));
    }
  }, [currentUser, syncConsentWithBackend, consentT]);

  // Enhanced accept all with loading state
  const acceptAll = useCallback(async () => {
    const allAccepted: ConsentState = {
      necessary: true,
      performance: true,
      functional: true,
      marketing: true,
      analytics: true,
      social: true,
    };
    
    await saveConsent(allAccepted);
  }, [saveConsent]);

  // Enhanced reject all with loading state  
  const rejectAll = useCallback(async () => {
    const onlyNecessary: ConsentState = {
      necessary: true,
      performance: false,
      functional: false,
      marketing: false,
      analytics: false,
      social: false,
    };
    
    await saveConsent(onlyNecessary);
  }, [saveConsent]);

  // Show preferences modal
  const showPreferences = useCallback(() => {
    setShowPreferencesModal(true);
  }, []);

  // Hide preferences modal
  const hidePreferences = useCallback(() => {
    setShowPreferencesModal(false);
  }, []);

  // Update a specific consent choice
  const updateConsent = useCallback((key: ConsentKeys, value: boolean) => {
    setConsent(prev => ({
      ...prev,
      [key]: key === 'necessary' ? true : value, // Force necessary to always be true
    }));
  }, []);

  // Check if a specific consent type is granted
  const hasConsent = useCallback((key: ConsentKeys): boolean => {
    return consent[key];
  }, [consent]);

  // Check if user has made any consent choice
  const hasConsentChoice = useCallback((): boolean => {
    // Without localStorage, determine choice by whether banner is hidden
    return !showBanner;
  }, [showBanner]);

  // Enhanced reset consent with backend sync and loading state
  const resetConsent = useCallback(async () => {
    setLoadingState(prev => ({ ...prev, resetting: true }));
    
    try {
      // Reset state
      setConsent(DEFAULT_CONSENT);
      setShowBanner(true);
      setShowPreferencesModal(false);
      
      // Create audit log for reset action - removing since we don't have audit log endpoint in provider client
      // TODO: Add audit logging when available
      if (currentUser) {
        console.log('Consent reset for user:', currentUser.id);
      }
      
      defaultToastActions.success(
        consentT ? consentT.storage('success') : 'Consent settings reset',
        consentT ? consentT.storage('success') : 'All preferences have been cleared'
      );
      
    } catch (error) {
      console.error('Error resetting consent:', error);
      defaultToastActions.error(
        consentT ? consentT.storage('error') : 'Could not reset consent',
        'Please try again'
      );
    } finally {
      setLoadingState(prev => ({ ...prev, resetting: false }));
    }
  }, [currentUser, consent, consentT]);

  // Get consent summary for debugging/display
  const getConsentSummary = useCallback(() => {
    return {
      current: consent,
      stored: null,
      timestamp: null,
      version: CONSENT_VERSION,
      hasChoice: hasConsentChoice(),
      loadingState,
      user: currentUser,
    };
  }, [consent, hasConsentChoice, loadingState, currentUser]);

  return {
    // State
    consent,
    showBanner,
    showPreferences: showPreferencesModal,
    isLoading: isLoading || loadingState.saving || loadingState.resetting || loadingState.syncing,
    loadingState,
    currentUser,

    // Actions
    saveConsent,
    acceptAll,
    rejectAll,
    showPreferencesModal: showPreferences,
    hidePreferences,
    updateConsent,
    resetConsent,

    // Utilities
    hasConsent,
    hasConsentChoice,
    getConsentSummary,
  };
}

// Hook for listening to consent changes across the app
export function useConsentListener(callback: (consent: ConsentState) => void) {
  useEffect(() => {
    const handleConsentChange = (event: CustomEvent<ConsentState>) => {
      callback(event.detail);
    };

    if (typeof window !== 'undefined') {
      window.addEventListener('consentChanged', handleConsentChange as EventListener);
      
      return () => {
        window.removeEventListener('consentChanged', handleConsentChange as EventListener);
      };
    }
  }, [callback]);
}

// Enhanced hook for conditional script loading based on consent with loading states
export function useConsentScript(
  consentType: ConsentKeys,
  scriptSrc: string,
  options?: {
    async?: boolean;
    defer?: boolean;
    onLoad?: () => void;
    onError?: () => void;
  }
) {
  const { hasConsent } = useConsent();
  const [isLoaded, setIsLoaded] = useState(false);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!hasConsent(consentType)) {
      // Remove script if consent was revoked
      const existingScript = document.querySelector(`script[src="${scriptSrc}"]`);
      if (existingScript) {
        existingScript.remove();
        // eslint-disable-next-line react-hooks/set-state-in-effect
        setIsLoaded(false);
        setIsLoading(false);
        setError(null);
      }
      return;
    }

    // Check if script is already loaded
    const existingScript = document.querySelector(`script[src="${scriptSrc}"]`);
    if (existingScript) {
      setIsLoaded(true);
      setIsLoading(false);
      return;
    }

    // Create and load script with enhanced error handling
    setIsLoading(true);
    setError(null);
    
    const script = document.createElement('script');
    script.src = scriptSrc;
    script.async = options?.async !== false;
    script.defer = options?.defer || false;

    script.onload = () => {
      setIsLoaded(true);
      setIsLoading(false);
      setError(null);
      options?.onLoad?.();
    };

    script.onerror = () => {
      const errorMsg = `Failed to load script: ${scriptSrc}`;
      setError(errorMsg);
      setIsLoading(false);
      defaultToastActions.error('Failed to load script', errorMsg);
      options?.onError?.();
    };

    document.head.appendChild(script);

    return () => {
      // Cleanup on unmount
      if (script.parentNode) {
        script.parentNode.removeChild(script);
      }
      setIsLoading(false);
    };
  }, [hasConsent, consentType, scriptSrc, options]);

  return { isLoaded, isLoading, error };
}

// Utility function for manually checking consent outside of React
export function checkConsent(consentType: ConsentKeys): boolean {
  // Use hook state if needed in components; here default to false except necessary
  return consentType === 'necessary';
}

// Utility function for getting all consent choices
export function getAllConsent(): ConsentState | null {
  return null;
}

// Utility functions for controlling consent UI programmatically
export function showConsentBanner() {
  if (typeof window !== 'undefined') {
    window.dispatchEvent(new CustomEvent('showConsentBanner'));
  }
}

export function hideConsentBanner() {
  if (typeof window !== 'undefined') {
    window.dispatchEvent(new CustomEvent('hideConsentBanner'));
  }
}
