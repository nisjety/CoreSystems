/**
 * Google Consent Mode v2 Implementation with Script Gating
 * Handles Google Analytics, Google Ads, and other Google services consent
 */

import React from 'react';
import type { ConsentPurposes } from './types';

// Extend Window interface for Google Analytics and advertising SDKs
declare global {
  interface Window {
    dataLayer: unknown[];
    gtag: (
      command: 'consent' | 'js' | 'config' | 'event',
      target: string | Date | GoogleConsentModeConfig,
      config?: unknown
    ) => void;
    fbq: {
      (command: string, ...args: unknown[]): void;
      q?: unknown[];
    };
  }
}

export interface GoogleConsentModeConfig {
  ad_storage: 'granted' | 'denied';
  ad_user_data: 'granted' | 'denied';
  ad_personalization: 'granted' | 'denied';
  analytics_storage: 'granted' | 'denied';
  functionality_storage: 'granted' | 'denied';
  personalization_storage: 'granted' | 'denied';
  security_storage: 'granted' | 'denied';
  wait_for_update?: number;
}

export interface ScriptGateConfig {
  src: string;
  purpose: keyof ConsentPurposes;
  fallback?: string;
  async?: boolean;
  defer?: boolean;
  onload?: () => void;
  onerror?: () => void;
}

/**
 * Google Consent Mode v2 Manager
 */
export class GoogleConsentMode {
  private static instance: GoogleConsentMode;
  private isInitialized = false;
  private pendingScripts: ScriptGateConfig[] = [];
  private loadedScripts = new Set<string>();

  static getInstance(): GoogleConsentMode {
    if (!GoogleConsentMode.instance) {
      GoogleConsentMode.instance = new GoogleConsentMode();
    }
    return GoogleConsentMode.instance;
  }

  /**
   * Initialize Google Consent Mode with default denied state
   */
  public initialize(gtagId?: string): void {
    if (this.isInitialized) return;
    
    // Initialize dataLayer
    if (typeof window !== 'undefined') {
      window.dataLayer = window.dataLayer || [];
      window.gtag = window.gtag || function(...args: unknown[]) {
        window.dataLayer.push(args);
      };
    }

    // Set default consent to denied (GDPR compliant)
    this.updateConsent({
      ad_storage: 'denied',
      ad_user_data: 'denied',
      ad_personalization: 'denied',
      analytics_storage: 'denied',
      functionality_storage: 'denied',
      personalization_storage: 'denied',
      security_storage: 'granted', // Security cookies are typically always allowed
      wait_for_update: 500,
    }, 'default');

    // Load Google Tag if ID provided
    if (gtagId) {
      this.loadGoogleTag(gtagId);
    }

    this.isInitialized = true;
  }

  /**
   * Update consent based on user choices
   */
  public updateConsentFromPurposes(purposes: ConsentPurposes): void {
    const consentConfig: GoogleConsentModeConfig = {
      // Analytics storage
      analytics_storage: purposes.analytics ? 'granted' : 'denied',
      
      // Advertising
      ad_storage: purposes.ads ? 'granted' : 'denied',
      ad_user_data: purposes.ads ? 'granted' : 'denied',
      ad_personalization: purposes.ads ? 'granted' : 'denied',
      
      // Functional
      functionality_storage: purposes.functional ? 'granted' : 'denied',
      personalization_storage: purposes.functional ? 'granted' : 'denied',
      
      // Security (always granted for necessary functionality)
      security_storage: 'granted',
    };

    this.updateConsent(consentConfig, 'update');
    
    // Process pending scripts based on new consent
    this.processPendingScripts(purposes);
  }

  /**
   * Update Google Consent Mode
   */
  private updateConsent(
    config: GoogleConsentModeConfig, 
    type: 'default' | 'update'
  ): void {
    if (typeof window === 'undefined' || !('gtag' in window)) return;

    const gtag = window.gtag;
    gtag('consent', type, config);

    // Log consent changes for debugging
    console.log(`Google Consent Mode ${type}:`, config);
    
    // Dispatch custom event
    if (type === 'update') {
      window.dispatchEvent(new CustomEvent('google-consent-updated', {
        detail: config
      }));
    }
  }

  /**
   * Load Google Tag script
   */
  private loadGoogleTag(gtagId: string): void {
    if (typeof window === 'undefined') return;

    const script = document.createElement('script');
    script.async = true;
    script.src = `https://www.googletagmanager.com/gtag/js?id=${gtagId}`;
    
    script.onload = () => {
      const gtag = window.gtag;
      gtag('js', new Date());
      gtag('config', gtagId, {
        anonymize_ip: true,
        allow_google_signals: false, // Disabled by default, enabled with consent
      });
    };

    document.head.appendChild(script);
  }

  /**
   * Add script to gating queue
   */
  public gateScript(config: ScriptGateConfig): void {
    this.pendingScripts.push(config);
  }

  /**
   * Process pending scripts based on consent
   */
  private processPendingScripts(purposes: ConsentPurposes): void {
    this.pendingScripts = this.pendingScripts.filter(script => {
      if (purposes[script.purpose] && !this.loadedScripts.has(script.src)) {
        this.loadScript(script);
        this.loadedScripts.add(script.src);
        return false; // Remove from pending
      }
      return true; // Keep in pending
    });
  }

  /**
   * Load a script
   */
  private loadScript(config: ScriptGateConfig): void {
    if (typeof window === 'undefined') return;

    const script = document.createElement('script');
    script.src = config.src;
    
    if (config.async) script.async = true;
    if (config.defer) script.defer = true;
    if (config.onload) script.onload = config.onload;
    if (config.onerror) script.onerror = config.onerror;

    document.head.appendChild(script);
    
    console.log(`Loaded script: ${config.src} for purpose: ${config.purpose}`);
  }

  /**
   * Block/unblock scripts in DOM based on consent
   */
  public processExistingScripts(purposes: ConsentPurposes): void {
    if (typeof window === 'undefined') return;

    const scripts = document.querySelectorAll('script[data-consent-purpose]');
    
    scripts.forEach((script) => {
      const purpose = script.getAttribute('data-consent-purpose') as keyof ConsentPurposes;
      const originalSrc = script.getAttribute('data-original-src');
      
      if (purposes[purpose]) {
        // Enable script
        if (originalSrc && !script.getAttribute('src')) {
          script.setAttribute('src', originalSrc);
          // Reload script by replacing it
          const newScript = script.cloneNode(true);
          script.parentNode?.replaceChild(newScript, script);
        }
      } else {
        // Disable script
        if (script.getAttribute('src')) {
          script.setAttribute('data-original-src', script.getAttribute('src')!);
          script.removeAttribute('src');
        }
      }
    });
  }

  /**
   * Enable Google Signals (enhanced demographics) after consent
   */
  public enableGoogleSignals(enable: boolean): void {
    if (typeof window === 'undefined' || !('gtag' in window)) return;

    const gtag = window.gtag;
    gtag('config', 'GA_MEASUREMENT_ID', {
      allow_google_signals: enable
    });
  }

  /**
   * Clear Google Analytics data
   */
  public clearAnalyticsData(): void {
    if (typeof window === 'undefined') return;

    // Clear Google Analytics cookies
    const gaCookies = ['_ga', '_ga_', '_gid', '_gat', '_gtag_GA_'];
    gaCookies.forEach(cookiePrefix => {
      document.cookie.split(';').forEach(cookie => {
        const eqPos = cookie.indexOf('=');
        const name = eqPos > -1 ? cookie.substr(0, eqPos).trim() : cookie.trim();
        if (name.startsWith(cookiePrefix)) {
          document.cookie = `${name}=;expires=Thu, 01 Jan 1970 00:00:00 GMT;path=/`;
          document.cookie = `${name}=;expires=Thu, 01 Jan 1970 00:00:00 GMT;path=/;domain=.${window.location.hostname}`;
        }
      });
    });

    // Clear localStorage GA data
    Object.keys(localStorage).forEach(key => {
      if (key.startsWith('_ga') || key.startsWith('ga_')) {
        localStorage.removeItem(key);
      }
    });
  }

  /**
   * Send custom event with consent awareness
   */
  public sendEvent(
    eventName: string, 
    parameters: Record<string, unknown>,
    requiredPurpose: keyof ConsentPurposes,
    purposes: ConsentPurposes
  ): void {
    if (!purposes[requiredPurpose]) {
      console.log(`Event '${eventName}' blocked: ${requiredPurpose} consent not granted`);
      return;
    }

    if (typeof window !== 'undefined' && 'gtag' in window) {
      const gtag = window.gtag;
      gtag('event', eventName, parameters);
    }
  }
}

/**
 * React Hook for Google Consent Mode
 */
export function useGoogleConsentMode() {
  const manager = GoogleConsentMode.getInstance();

  const initialize = (gtagId?: string) => {
    manager.initialize(gtagId);
  };

  const updateConsent = (purposes: ConsentPurposes) => {
    manager.updateConsentFromPurposes(purposes);
    manager.processExistingScripts(purposes);
  };

  const gateScript = (config: ScriptGateConfig) => {
    manager.gateScript(config);
  };

  const sendEvent = (
    eventName: string,
    parameters: Record<string, unknown>,
    requiredPurpose: keyof ConsentPurposes,
    purposes: ConsentPurposes
  ) => {
    manager.sendEvent(eventName, parameters, requiredPurpose, purposes);
  };

  const clearData = () => {
    manager.clearAnalyticsData();
  };

  return {
    initialize,
    updateConsent,
    gateScript,
    sendEvent,
    clearData,
  };
}

/**
 * Script Gate Component for React
 */
interface ScriptGateProps {
  src: string;
  purpose: keyof ConsentPurposes;
  purposes: ConsentPurposes;
  async?: boolean;
  defer?: boolean;
  fallback?: string;
  onLoad?: () => void;
  onError?: () => void;
  children?: React.ReactNode;
}

export function ScriptGate({
  src,
  purpose,
  purposes,
  async = true,
  defer = false,
  fallback,
  onLoad,
  onError,
  children,
}: ScriptGateProps) {
  const { gateScript } = useGoogleConsentMode();

  React.useEffect(() => {
    if (purposes[purpose]) {
      // Load immediately if consent is granted
      const script = document.createElement('script');
      script.src = src;
      script.async = async;
      script.defer = defer;
      if (onLoad) script.onload = onLoad;
      if (onError) script.onerror = onError;
      document.head.appendChild(script);
    } else {
      // Gate the script for later loading
      gateScript({
        src,
        purpose,
        async,
        defer,
        fallback,
        onload: onLoad,
        onerror: onError,
      });
    }
  }, [src, purpose, purposes, async, defer, onLoad, onError, gateScript, fallback]);

  // Render children only if consent is granted
  if (purposes[purpose]) {
    return React.createElement(React.Fragment, null, children);
  }

  // Render fallback if no consent
  if (fallback) {
    return React.createElement('div', { dangerouslySetInnerHTML: { __html: fallback } });
  }

  return null;
}

/**
 * Predefined script configurations for common services
 */
export const COMMON_SCRIPTS = {
  googleAnalytics: (id: string): ScriptGateConfig => ({
    src: `https://www.googletagmanager.com/gtag/js?id=${id}`,
    purpose: 'analytics',
    async: true,
  }),
  
  googleAds: (id: string): ScriptGateConfig => ({
    src: `https://www.googletagmanager.com/gtag/js?id=${id}`,
    purpose: 'ads',
    async: true,
  }),
  
  hotjar: (id: string): ScriptGateConfig => ({
    src: `https://static.hotjar.com/c/hotjar-${id}.js`,
    purpose: 'heatmap',
    async: true,
  }),
  
  intercom: (id: string): ScriptGateConfig => ({
    src: `https://widget.intercom.io/widget/${id}`,
    purpose: 'functional',
    async: true,
  }),
};

export default GoogleConsentMode;
