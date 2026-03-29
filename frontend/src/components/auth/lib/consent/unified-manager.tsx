/**
 * Unified Consent Manager
 * Integrates server-side consent, middleware blocking, Google Consent Mode v2, and TCF 2.2 CMP
 */

import React, { createContext, useContext, useEffect, useState, ReactNode } from 'react';
import type { ConsentPurposes } from './types';
import { ConsentServerActions } from './server-cookie';
import { GoogleConsentMode } from './google-consent-mode';
import { TCFCMP, TCFPurpose } from './tcf-cmp';

export interface ConsentManagerConfig {
  enableTCF?: boolean;
  enableGoogleConsentMode?: boolean;
  googleTagId?: string;
  cmpId?: number;
  serverEndpoint?: string;
  defaultConsent?: Partial<ConsentPurposes>;
  autoShow?: boolean;
  showOnLoad?: boolean;
  respectDNT?: boolean; // Respect Do Not Track browser setting
}

export interface ConsentState {
  purposes: ConsentPurposes;
  isInitialized: boolean;
  isLoading: boolean;
  hasUserInteracted: boolean;
  showBanner: boolean;
  tcString?: string;
  lastUpdated?: Date;
}

interface ConsentManagerContextValue {
  state: ConsentState;
  updateConsent: (purposes: Partial<ConsentPurposes>) => Promise<void>;
  withdrawConsent: () => Promise<void>;
  showConsentManager: () => void;
  hideConsentManager: () => void;
  checkVendorConsent: (vendorId: number, purpose: TCFPurpose) => boolean;
  getConsentString: () => string;
}

const ConsentManagerContext = createContext<ConsentManagerContextValue | null>(null);

/**
 * Unified Consent Manager Class
 */
export class UnifiedConsentManager {
  private static instance: UnifiedConsentManager;
  private config: ConsentManagerConfig;
  private serverActions: ConsentServerActions;
  private googleConsentMode?: GoogleConsentMode;
  private tcfCMP?: TCFCMP;
  private state: ConsentState = {
    purposes: {
      necessary: true,
      analytics: false,
      ads: false,
      functional: false,
      ab_test: false,
      heatmap: false,
    },
    isInitialized: false,
    isLoading: true,
    hasUserInteracted: false,
    showBanner: false,
  };
  private listeners: Set<(state: ConsentState) => void> = new Set();

  constructor(config: ConsentManagerConfig) {
    this.config = config;
    this.serverActions = new ConsentServerActions();
  }

  static getInstance(config?: ConsentManagerConfig): UnifiedConsentManager {
    if (!UnifiedConsentManager.instance && config) {
      UnifiedConsentManager.instance = new UnifiedConsentManager(config);
    }
    return UnifiedConsentManager.instance;
  }

  /**
   * Initialize all consent systems
   */
  public async initialize(): Promise<void> {
    this.setState({ isLoading: true });

    try {
      // Check Do Not Track setting
      if (this.config.respectDNT && this.isDNTEnabled()) {
        await this.handleDNT();
        return;
      }

      // Load existing consent
      await this.loadExistingConsent();

      // Initialize Google Consent Mode
      if (this.config.enableGoogleConsentMode) {
        this.googleConsentMode = GoogleConsentMode.getInstance();
        this.googleConsentMode.initialize(this.config.googleTagId);
        this.googleConsentMode.updateConsentFromPurposes(this.state.purposes);
      }

      // Initialize TCF CMP
      if (this.config.enableTCF) {
        this.tcfCMP = TCFCMP.getInstance();
        await this.tcfCMP.initialize();
      }

      // Determine if we should show banner
      const shouldShow = this.shouldShowConsentBanner();
      
      this.setState({
        isInitialized: true,
        isLoading: false,
        showBanner: shouldShow,
      });

      // Auto-show if configured
      if (shouldShow && this.config.autoShow) {
        this.showConsentManager();
      }

      console.log('Unified Consent Manager initialized');
    } catch (error) {
      console.error('Failed to initialize Unified Consent Manager:', error);
      this.setState({ isLoading: false, isInitialized: false });
    }
  }

  /**
   * Update consent across all systems
   */
  public async updateConsent(purposes: Partial<ConsentPurposes>): Promise<void> {
    const newPurposes: ConsentPurposes = {
      ...this.state.purposes,
      ...purposes,
      necessary: true, // Always true
    };

    try {
      // Update server-side consent (in browser context, we'll need to make API call)
      if (typeof window !== 'undefined') {
        await fetch('/api/consent', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ purposes: newPurposes }),
          credentials: 'include', // Include cookies
        });
      }

      // Update Google Consent Mode
      if (this.googleConsentMode) {
        this.googleConsentMode.updateConsentFromPurposes(newPurposes);
      }

      // Update TCF CMP
      if (this.tcfCMP) {
        const vendorConsents = this.generateVendorConsents(newPurposes);
        this.tcfCMP.updateConsent(newPurposes, vendorConsents);
      }

      // Update state
      this.setState({
        purposes: newPurposes,
        hasUserInteracted: true,
        showBanner: false,
        lastUpdated: new Date(),
        tcString: this.tcfCMP?.getTCString(),
      });

      // Trigger page reload for middleware to take effect
      if (typeof window !== 'undefined') {
        // Dispatch event for other parts of the app
        window.dispatchEvent(new CustomEvent('consent-updated', {
          detail: { purposes: newPurposes }
        }));

        // Optional: Reload page to ensure middleware applies new consent
        // window.location.reload();
      }

    } catch (error) {
      console.error('Failed to update consent:', error);
      throw error;
    }
  }

  /**
   * Withdraw all consent
   */
  public async withdrawConsent(): Promise<void> {
    const withdrawnPurposes: ConsentPurposes = {
      necessary: true,
      analytics: false,
      ads: false,
      functional: false,
      ab_test: false,
      heatmap: false,
    };

    await this.updateConsent(withdrawnPurposes);

    // Clear analytics data
    if (this.googleConsentMode) {
      this.googleConsentMode.clearAnalyticsData();
    }

    // Clear TCF data
    if (this.tcfCMP) {
      this.tcfCMP.clearConsent();
    }
  }

  /**
   * Show consent manager UI
   */
  public showConsentManager(): void {
    this.setState({ showBanner: true });
    
    if (this.tcfCMP) {
      this.tcfCMP.showConsentUI();
    }
  }

  /**
   * Hide consent manager UI
   */
  public hideConsentManager(): void {
    this.setState({ showBanner: false });
    
    if (this.tcfCMP) {
      this.tcfCMP.hideConsentUI();
    }
  }

  /**
   * Check vendor consent via TCF
   */
  public checkVendorConsent(vendorId: number, purpose: TCFPurpose): boolean {
    if (!this.tcfCMP) return false;
    return this.tcfCMP.hasVendorConsent(vendorId, purpose);
  }

  /**
   * Get consent string for server verification
   */
  public getConsentString(): string {
    return this.tcfCMP?.getTCString() || '';
  }

  /**
   * Add state change listener
   */
  public addListener(listener: (state: ConsentState) => void): void {
    this.listeners.add(listener);
  }

  /**
   * Remove state change listener
   */
  public removeListener(listener: (state: ConsentState) => void): void {
    this.listeners.delete(listener);
  }

  /**
   * Get current state
   */
  public getState(): ConsentState {
    return { ...this.state };
  }

  // Private methods

  private setState(updates: Partial<ConsentState>): void {
    this.state = { ...this.state, ...updates };
    this.listeners.forEach(listener => listener(this.state));
  }

  private async loadExistingConsent(): Promise<void> {
    try {
      // In browser context, load from localStorage or make API call
      if (typeof window !== 'undefined') {
        const response = await fetch('/api/consent', {
          method: 'GET',
          credentials: 'include',
        });
        
        if (response.ok) {
          const existingConsent = await response.json();
          if (existingConsent?.purposes) {
            this.setState({
              purposes: existingConsent.purposes,
              hasUserInteracted: true,
              lastUpdated: new Date(existingConsent.timestamp),
            });
          }
        }
      }
      
      if (!this.state.hasUserInteracted && this.config.defaultConsent) {
        this.setState({
          purposes: { ...this.state.purposes, ...this.config.defaultConsent },
        });
      }
    } catch (error) {
      console.error('Failed to load existing consent:', error);
    }
  }

  private shouldShowConsentBanner(): boolean {
    // Don't show if user has already interacted
    if (this.state.hasUserInteracted) return false;

    // Show on load if configured
    if (this.config.showOnLoad) return true;

    // Show if no consent exists
    return !this.state.lastUpdated;
  }

  private isDNTEnabled(): boolean {
    if (typeof navigator !== 'undefined') {
      return navigator.doNotTrack === '1' || 
             (navigator as { msDoNotTrack?: string }).msDoNotTrack === '1';
    }
    return false;
  }

  private async handleDNT(): Promise<void> {
    const dntPurposes: ConsentPurposes = {
      necessary: true,
      analytics: false,
      ads: false,
      functional: false,
      ab_test: false,
      heatmap: false,
    };

    this.setState({
      purposes: dntPurposes,
      hasUserInteracted: true,
      isInitialized: true,
      isLoading: false,
      showBanner: false,
    });

    // Update server-side consent for DNT
    if (typeof window !== 'undefined') {
      await fetch('/api/consent', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ purposes: dntPurposes, method: 'dnt' }),
        credentials: 'include',
      });
    }
    
    console.log('Do Not Track detected - all non-necessary consent denied');
  }

  private generateVendorConsents(purposes: ConsentPurposes): Map<number, boolean> {
    const vendorConsents = new Map<number, boolean>();

    // Google (vendor ID 755)
    if (purposes.analytics || purposes.ads) {
      vendorConsents.set(755, true);
    }

    // Add other vendor mappings based on purposes
    // This would be configured based on your specific vendors

    return vendorConsents;
  }
}

/**
 * React Hook for Unified Consent Manager
 */
export function useConsentManager() {
  const context = useContext(ConsentManagerContext);
  if (!context) {
    throw new Error('useConsentManager must be used within ConsentManagerProvider');
  }
  return context;
}

/**
 * React Provider for Unified Consent Manager
 */
interface ConsentManagerProviderProps {
  children: ReactNode;
  config: ConsentManagerConfig;
}

export function ConsentManagerProvider({ children, config }: ConsentManagerProviderProps) {
  const [state, setState] = useState<ConsentState>({
    purposes: {
      necessary: true,
      analytics: false,
      ads: false,
      functional: false,
      ab_test: false,
      heatmap: false,
    },
    isInitialized: false,
    isLoading: true,
    hasUserInteracted: false,
    showBanner: false,
  });

  const [manager] = useState(() => UnifiedConsentManager.getInstance(config));

  useEffect(() => {
    // Add listener for state changes
    const handleStateChange = (newState: ConsentState) => {
      setState(newState);
    };

    manager.addListener(handleStateChange);

    // Initialize manager
    manager.initialize();

    return () => {
      manager.removeListener(handleStateChange);
    };
  }, [manager]);

  const updateConsent = async (purposes: Partial<ConsentPurposes>) => {
    await manager.updateConsent(purposes);
  };

  const withdrawConsent = async () => {
    await manager.withdrawConsent();
  };

  const showConsentManager = () => {
    manager.showConsentManager();
  };

  const hideConsentManager = () => {
    manager.hideConsentManager();
  };

  const checkVendorConsent = (vendorId: number, purpose: TCFPurpose) => {
    return manager.checkVendorConsent(vendorId, purpose);
  };

  const getConsentString = () => {
    return manager.getConsentString();
  };

  const contextValue: ConsentManagerContextValue = {
    state,
    updateConsent,
    withdrawConsent,
    showConsentManager,
    hideConsentManager,
    checkVendorConsent,
    getConsentString,
  };

  return (
    <ConsentManagerContext.Provider value={contextValue}>
      {children}
    </ConsentManagerContext.Provider>
  );
}

export default UnifiedConsentManager;
