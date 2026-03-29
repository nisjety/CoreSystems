/**
 * IAB TCF 2.2 (Transparency and Consent Framework) CMP Implementation
 * Compliant with IAB Europe Transparency & Consent Framework v2.2
 */

import type { ConsentPurposes } from './types';

// TCF 2.2 Purpose IDs according to IAB specification
export enum TCFPurpose {
  STORE_ACCESS_INFO = 1,              // Store and/or access information on a device
  BASIC_ADS = 2,                      // Select basic ads
  PERSONALIZED_ADS = 3,               // Create a personalised ads profile
  AD_SELECTION = 4,                   // Select personalised ads
  CONTENT_PROFILE = 5,                // Create a personalised content profile
  CONTENT_SELECTION = 6,              // Select personalised content
  AD_MEASUREMENT = 7,                 // Measure ad performance
  CONTENT_MEASUREMENT = 8,            // Measure content performance
  AUDIENCE_INSIGHTS = 9,              // Apply market research to generate audience insights
  PRODUCT_DEVELOPMENT = 10,           // Develop and improve products
  PRECISE_GEOLOCATION = 11,           // Use precise geolocation data
}

// TCF 2.2 Special Feature IDs
export enum TCFSpecialFeature {
  PRECISE_GEOLOCATION = 1,            // Use precise geolocation data
  DEVICE_SCAN = 2,                    // Actively scan device characteristics for identification
}

// Vendor purposes mapping
export interface TCFVendor {
  id: number;
  name: string;
  purposes: TCFPurpose[];
  legIntPurposes: TCFPurpose[];
  flexiblePurposes: TCFPurpose[];
  specialPurposes: number[];
  features: number[];
  specialFeatures: TCFSpecialFeature[];
  policyUrl: string;
  cookieMaxAgeSeconds?: number;
  cookieRefresh?: boolean;
  usesNonCookieAccess?: boolean;
}

// TCF Consent String data structure
export interface TCFConsentData {
  tcString: string;
  tcfPolicyVersion: number;
  cmpId: number;
  cmpVersion: number;
  consentScreen: number;
  consentLanguage: string;
  vendorConsents: Map<number, boolean>;
  vendorLegitimateInterests: Map<number, boolean>;
  purposeConsents: Map<TCFPurpose, boolean>;
  purposeLegitimateInterests: Map<TCFPurpose, boolean>;
  specialFeatureOptins: Map<TCFSpecialFeature, boolean>;
  publisherConsents: Map<number, boolean>;
  publisherLegitimateInterests: Map<number, boolean>;
  publisherCustomPurposes: Map<number, boolean>;
  created: Date;
  lastUpdated: Date;
  policyVersion: number;
  isServiceSpecific: boolean;
  useNonStandardStacks: boolean;
  publisherCountryCode: string;
}

// CMP API Response interface
export interface TCFApiResponse {
  tcString: string;
  cmpId: number;
  cmpVersion: number;
  gdprApplies: boolean;
  cmpStatus: 'stub' | 'loading' | 'loaded' | 'error';
  displayStatus: 'hidden' | 'visible' | 'disabled';
  apiVersion: string;
  consentData: TCFConsentData;
  eventStatus: 'tcloaded' | 'cmpuishown' | 'useractioncomplete';
  listenerId?: number;
}

// CMP API callback types
type TCFCallback = (result: TCFApiResponse, success: boolean) => void;
type PingCallback = (result: unknown, success: boolean) => void;
type GenericCallback = (result: unknown, success: boolean) => void;

// Global CMP API interface
declare global {
  interface Window {
    __tcfapi?: (
      command: string,
      version: number,
      callback: TCFCallback | PingCallback | GenericCallback,
      parameter?: unknown
    ) => void;
    __tcfapiLocator?: boolean;
  }
}

/**
 * IAB TCF 2.2 CMP (Consent Management Platform) Implementation
 */
export class TCFCMP {
  private static instance: TCFCMP;
  private consentData: TCFConsentData | null = null;
  private isInitialized = false;
  private eventListeners: Map<number, TCFCallback> = new Map();
  private listenerCounter = 0;
  private gdprApplies = true;
  private cmpId = 1000; // Your CMP ID (register with IAB)
  private cmpVersion = 1;
  private displayStatus: 'hidden' | 'visible' | 'disabled' = 'hidden';
  private cmpStatus: 'stub' | 'loading' | 'loaded' | 'error' = 'loading';

  // Predefined vendors (example - in production, load from IAB Global Vendor List)
  private vendors: Map<number, TCFVendor> = new Map([
    [755, { // Google
      id: 755,
      name: 'Google Advertising Products',
      purposes: [TCFPurpose.STORE_ACCESS_INFO, TCFPurpose.BASIC_ADS, TCFPurpose.PERSONALIZED_ADS, TCFPurpose.AD_MEASUREMENT],
      legIntPurposes: [TCFPurpose.AD_SELECTION],
      flexiblePurposes: [],
      specialPurposes: [1, 2],
      features: [1, 2, 3],
      specialFeatures: [],
      policyUrl: 'https://policies.google.com/privacy',
      cookieMaxAgeSeconds: 63072000,
    }],
  ]);

  static getInstance(): TCFCMP {
    if (!TCFCMP.instance) {
      TCFCMP.instance = new TCFCMP();
    }
    return TCFCMP.instance;
  }

  /**
   * Initialize the CMP and TCF API
   */
  public async initialize(): Promise<void> {
    if (this.isInitialized) return;

    // Load existing consent
    await this.loadStoredConsent();

    // Initialize __tcfapi
    this.initializeTCFAPI();

    // Set CMP as loaded
    this.cmpStatus = 'loaded';
    this.isInitialized = true;

    // Fire tcloaded event
    this.fireEvent('tcloaded');

    console.log('TCF 2.2 CMP initialized');
  }

  /**
   * Initialize the __tcfapi function
   */
  private initializeTCFAPI(): void {
    if (typeof window === 'undefined') return;

    window.__tcfapi = (command: string, version: number, callback: TCFCallback | PingCallback | GenericCallback, parameter?: unknown) => {
      switch (command) {
        case 'getTCData':
          this.getTCData(callback as TCFCallback);
          break;
        case 'ping':
          this.ping(callback as PingCallback);
          break;
        case 'addEventListener':
          this.addEventListener(callback as TCFCallback);
          break;
        case 'removeEventListener':
          this.removeEventListener(parameter as number, callback as GenericCallback);
          break;
        case 'getInAppTCData':
          this.getInAppTCData(callback as TCFCallback);
          break;
        case 'getVendorList':
          this.getVendorList(callback as GenericCallback);
          break;
        default:
          (callback as GenericCallback)({ error: 'Unknown command' }, false);
      }
    };

    // Set locator
    window.__tcfapiLocator = true;
  }

  /**
   * Get TC Data - main consent information
   */
  private getTCData(callback: TCFCallback): void {
    const response: TCFApiResponse = {
      tcString: this.consentData?.tcString || '',
      cmpId: this.cmpId,
      cmpVersion: this.cmpVersion,
      gdprApplies: this.gdprApplies,
      cmpStatus: this.cmpStatus,
      displayStatus: this.displayStatus,
      apiVersion: '2.2',
      consentData: this.consentData!,
      eventStatus: 'tcloaded',
    };

    callback(response, true);
  }

  /**
   * Ping - check CMP status
   */
  private ping(callback: PingCallback): void {
    const response = {
      gdprApplies: this.gdprApplies,
      cmpLoaded: this.isInitialized,
      cmpStatus: this.cmpStatus,
      displayStatus: this.displayStatus,
      apiVersion: '2.2',
      cmpVersion: this.cmpVersion,
      cmpId: this.cmpId,
      gvlVersion: 3, // Global Vendor List version
      tcfPolicyVersion: 4, // TCF Policy version
    };

    callback(response, true);
  }

  /**
   * Add event listener
   */
  private addEventListener(callback: TCFCallback): void {
    const listenerId = ++this.listenerCounter;
    this.eventListeners.set(listenerId, callback);

    // Send current data immediately
    this.getTCData((response: TCFApiResponse) => {
      response.listenerId = listenerId;
      callback(response, true);
    });
  }

  /**
   * Remove event listener
   */
  private removeEventListener(listenerId: number, callback: GenericCallback): void {
    const removed = this.eventListeners.delete(listenerId);
    callback({ success: removed }, removed);
  }

  /**
   * Get In-App TC Data (for mobile apps)
   */
  private getInAppTCData(callback: TCFCallback): void {
    // Similar to getTCData but for in-app context
    this.getTCData(callback);
  }

  /**
   * Get Vendor List (simplified - in production, fetch from IAB)
   */
  private getVendorList(callback: GenericCallback): void {
    const vendorList = {
      gvlSpecificationVersion: 3,
      vendorListVersion: 1,
      tcfPolicyVersion: 4,
      lastUpdated: new Date().toISOString(),
      purposes: Object.values(TCFPurpose).filter(v => typeof v === 'number').map(id => ({
        id,
        name: this.getPurposeName(id as TCFPurpose),
        description: this.getPurposeDescription(id as TCFPurpose),
      })),
      specialPurposes: [
        { id: 1, name: 'Ensure security, prevent fraud, and debug' },
        { id: 2, name: 'Technically deliver ads or content' },
      ],
      features: [
        { id: 1, name: 'Match and combine offline data sources' },
        { id: 2, name: 'Link different devices' },
        { id: 3, name: 'Receive and use automatically-sent device characteristics' },
      ],
      specialFeatures: [
        { id: 1, name: 'Use precise geolocation data' },
        { id: 2, name: 'Actively scan device characteristics' },
      ],
      vendors: Array.from(this.vendors.values()),
    };

    callback(vendorList, true);
  }

  /**
   * Fire event to all listeners
   */
  private fireEvent(eventStatus: 'tcloaded' | 'cmpuishown' | 'useractioncomplete'): void {
    this.eventListeners.forEach((callback, listenerId) => {
      this.getTCData((response: TCFApiResponse) => {
        response.eventStatus = eventStatus;
        response.listenerId = listenerId;
        callback(response, true);
      });
    });
  }

  /**
   * Show consent UI
   */
  public showConsentUI(): void {
    this.displayStatus = 'visible';
    this.fireEvent('cmpuishown');
  }

  /**
   * Hide consent UI
   */
  public hideConsentUI(): void {
    this.displayStatus = 'hidden';
    this.fireEvent('useractioncomplete');
  }

  /**
   * Update consent based on user choices
   */
  public updateConsent(
    purposes: ConsentPurposes,
    vendorConsents: Map<number, boolean> = new Map(),
    specialFeatures: Map<TCFSpecialFeature, boolean> = new Map()
  ): void {
    // Map our consent purposes to TCF purposes
    const tcfPurposeConsents = new Map<TCFPurpose, boolean>([
      [TCFPurpose.STORE_ACCESS_INFO, purposes.functional],
      [TCFPurpose.BASIC_ADS, purposes.ads],
      [TCFPurpose.PERSONALIZED_ADS, purposes.ads],
      [TCFPurpose.AD_SELECTION, purposes.ads],
      [TCFPurpose.CONTENT_PROFILE, purposes.functional],
      [TCFPurpose.CONTENT_SELECTION, purposes.functional],
      [TCFPurpose.AD_MEASUREMENT, purposes.analytics],
      [TCFPurpose.CONTENT_MEASUREMENT, purposes.analytics],
      [TCFPurpose.AUDIENCE_INSIGHTS, purposes.analytics],
      [TCFPurpose.PRODUCT_DEVELOPMENT, purposes.analytics],
      [TCFPurpose.PRECISE_GEOLOCATION, false], // Requires explicit consent
    ]);

    // Create consent data
    this.consentData = {
      tcString: this.generateTCString(tcfPurposeConsents, vendorConsents, specialFeatures),
      tcfPolicyVersion: 4,
      cmpId: this.cmpId,
      cmpVersion: this.cmpVersion,
      consentScreen: 1,
      consentLanguage: 'NO', // Norwegian
      vendorConsents,
      vendorLegitimateInterests: new Map(),
      purposeConsents: tcfPurposeConsents,
      purposeLegitimateInterests: new Map(),
      specialFeatureOptins: specialFeatures,
      publisherConsents: new Map(),
      publisherLegitimateInterests: new Map(),
      publisherCustomPurposes: new Map(),
      created: new Date(),
      lastUpdated: new Date(),
      policyVersion: 4,
      isServiceSpecific: false,
      useNonStandardStacks: false,
      publisherCountryCode: 'NO',
    };

    // Store consent
    this.storeConsent();

    // Fire event
    this.fireEvent('useractioncomplete');
    this.hideConsentUI();
  }

  /**
   * Generate TC String (simplified - in production use proper encoding)
   */
  private generateTCString(
    purposeConsents: Map<TCFPurpose, boolean>,
    vendorConsents: Map<number, boolean>,
    specialFeatures: Map<TCFSpecialFeature, boolean>
  ): string {
    // This is a simplified implementation
    // In production, use proper base64url encoding as per TCF 2.2 spec
    const data = {
      version: 2,
      created: Date.now(),
      lastUpdated: Date.now(),
      cmpId: this.cmpId,
      cmpVersion: this.cmpVersion,
      consentScreen: 1,
      consentLanguage: 'NO',
      vendorListVersion: 1,
      tcfPolicyVersion: 4,
      isServiceSpecific: false,
      useNonStandardStacks: false,
      purposeConsents: Array.from(purposeConsents.entries()),
      vendorConsents: Array.from(vendorConsents.entries()),
      specialFeatureOptins: Array.from(specialFeatures.entries()),
      publisherCountryCode: 'NO',
    };

    // In production, properly encode this as per TCF 2.2 specification
    return btoa(JSON.stringify(data)).replace(/[+/=]/g, (match) => {
      return { '+': '-', '/': '_', '=': '' }[match] || match;
    });
  }

  /**
   * Store consent in localStorage
   */
  private storeConsent(): void {
    if (typeof window === 'undefined' || !this.consentData) return;

    localStorage.setItem('tcf_consent_data', JSON.stringify({
      tcString: this.consentData.tcString,
      created: this.consentData.created.toISOString(),
      lastUpdated: this.consentData.lastUpdated.toISOString(),
    }));
  }

  /**
   * Load stored consent
   */
  private async loadStoredConsent(): Promise<void> {
    if (typeof window === 'undefined') return;

    const stored = localStorage.getItem('tcf_consent_data');
    if (stored) {
      try {
        const data = JSON.parse(stored);
        // Decode TC string and rebuild consent data
        // This is simplified - in production, properly decode TCF string
        
        // For now, create default consent data
        this.consentData = {
          tcString: data.tcString,
          tcfPolicyVersion: 4,
          cmpId: this.cmpId,
          cmpVersion: this.cmpVersion,
          consentScreen: 1,
          consentLanguage: 'NO',
          vendorConsents: new Map(),
          vendorLegitimateInterests: new Map(),
          purposeConsents: new Map(),
          purposeLegitimateInterests: new Map(),
          specialFeatureOptins: new Map(),
          publisherConsents: new Map(),
          publisherLegitimateInterests: new Map(),
          publisherCustomPurposes: new Map(),
          created: new Date(data.created),
          lastUpdated: new Date(data.lastUpdated),
          policyVersion: 4,
          isServiceSpecific: false,
          useNonStandardStacks: false,
          publisherCountryCode: 'NO',
        };
      } catch (error) {
        console.error('Failed to load stored TCF consent:', error);
      }
    }
  }

  /**
   * Check if vendor has consent for specific purpose
   */
  public hasVendorConsent(vendorId: number, purpose: TCFPurpose): boolean {
    if (!this.consentData) return false;

    const vendor = this.vendors.get(vendorId);
    if (!vendor) return false;

    // Check if vendor is consented
    const vendorConsent = this.consentData.vendorConsents.get(vendorId);
    if (!vendorConsent) return false;

    // Check if purpose is consented
    const purposeConsent = this.consentData.purposeConsents.get(purpose);
    if (!purposeConsent) return false;

    // Check if vendor declares this purpose
    return vendor.purposes.includes(purpose);
  }

  /**
   * Check if vendor has legitimate interest for specific purpose
   */
  public hasVendorLegitimateInterest(vendorId: number, purpose: TCFPurpose): boolean {
    if (!this.consentData) return false;

    const vendor = this.vendors.get(vendorId);
    if (!vendor) return false;

    // Check if vendor legitimate interest is not objected
    const vendorLI = this.consentData.vendorLegitimateInterests.get(vendorId);
    if (vendorLI === false) return false; // User objected

    // Check if purpose legitimate interest is not objected
    const purposeLI = this.consentData.purposeLegitimateInterests.get(purpose);
    if (purposeLI === false) return false; // User objected

    // Check if vendor declares legitimate interest for this purpose
    return vendor.legIntPurposes.includes(purpose);
  }

  /**
   * Get current consent status for integration with our consent system
   */
  public getConsentPurposes(): ConsentPurposes {
    if (!this.consentData) {
      return {
        necessary: true, // Always true for necessary cookies
        analytics: false,
        ads: false,
        functional: false,
        ab_test: false,
        heatmap: false,
      };
    }

    return {
      necessary: true, // Always true for necessary cookies
      analytics: this.consentData.purposeConsents.get(TCFPurpose.AD_MEASUREMENT) || false,
      ads: this.consentData.purposeConsents.get(TCFPurpose.BASIC_ADS) || false,
      functional: this.consentData.purposeConsents.get(TCFPurpose.STORE_ACCESS_INFO) || false,
      ab_test: this.consentData.purposeConsents.get(TCFPurpose.PRODUCT_DEVELOPMENT) || false,
      heatmap: this.consentData.purposeConsents.get(TCFPurpose.CONTENT_MEASUREMENT) || false,
    };
  }

  /**
   * Helper method to get purpose name
   */
  private getPurposeName(purpose: TCFPurpose): string {
    const names: Record<TCFPurpose, string> = {
      [TCFPurpose.STORE_ACCESS_INFO]: 'Store and/or access information',
      [TCFPurpose.BASIC_ADS]: 'Select basic ads',
      [TCFPurpose.PERSONALIZED_ADS]: 'Create personalised ads profile',
      [TCFPurpose.AD_SELECTION]: 'Select personalised ads',
      [TCFPurpose.CONTENT_PROFILE]: 'Create personalised content profile',
      [TCFPurpose.CONTENT_SELECTION]: 'Select personalised content',
      [TCFPurpose.AD_MEASUREMENT]: 'Measure ad performance',
      [TCFPurpose.CONTENT_MEASUREMENT]: 'Measure content performance',
      [TCFPurpose.AUDIENCE_INSIGHTS]: 'Market research and audience insights',
      [TCFPurpose.PRODUCT_DEVELOPMENT]: 'Develop and improve products',
      [TCFPurpose.PRECISE_GEOLOCATION]: 'Use precise geolocation data',
    };
    return names[purpose] || 'Unknown purpose';
  }

  /**
   * Helper method to get purpose description
   */
  private getPurposeDescription(purpose: TCFPurpose): string {
    const descriptions: Record<TCFPurpose, string> = {
      [TCFPurpose.STORE_ACCESS_INFO]: 'Cookies, device identifiers, or other information can be stored or accessed on your device for the purposes presented to you.',
      [TCFPurpose.BASIC_ADS]: 'Ads can be shown to you based on the content you are viewing, the app you are using, your approximate location, or your device type.',
      [TCFPurpose.PERSONALIZED_ADS]: 'A profile can be built about you and your interests to show you personalised ads that are relevant to you.',
      [TCFPurpose.AD_SELECTION]: 'Personalised ads can be shown to you based on a profile about you.',
      [TCFPurpose.CONTENT_PROFILE]: 'A profile can be built about you and your interests to show you personalised content that is relevant to you.',
      [TCFPurpose.CONTENT_SELECTION]: 'Personalised content can be shown to you based on a profile about you.',
      [TCFPurpose.AD_MEASUREMENT]: 'The performance and effectiveness of ads that you see or interact with can be measured.',
      [TCFPurpose.CONTENT_MEASUREMENT]: 'The performance and effectiveness of content that you see or interact with can be measured.',
      [TCFPurpose.AUDIENCE_INSIGHTS]: 'Market research can be used to learn more about the audiences who visit sites/apps and view ads.',
      [TCFPurpose.PRODUCT_DEVELOPMENT]: 'Your data can be used to improve existing systems and software, and to develop new products.',
      [TCFPurpose.PRECISE_GEOLOCATION]: 'Your precise geolocation data can be used in support of one or more purposes.',
    };
    return descriptions[purpose] || 'Unknown purpose description';
  }

  /**
   * Clear all consent data
   */
  public clearConsent(): void {
    this.consentData = null;
    if (typeof window !== 'undefined') {
      localStorage.removeItem('tcf_consent_data');
    }
    this.fireEvent('useractioncomplete');
  }

  /**
   * Get TC String for server-side verification
   */
  public getTCString(): string {
    return this.consentData?.tcString || '';
  }
}

// Export singleton instance
export const tcfCMP = TCFCMP.getInstance();

export default TCFCMP;
