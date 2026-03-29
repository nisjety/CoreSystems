/**
 * Shared Consent Types
 * These types can be safely imported by both client and server code
 */

export interface ConsentPurposes {
  necessary: boolean;
  analytics: boolean;
  ads: boolean;
  ab_test: boolean;
  heatmap: boolean;
  functional: boolean;
}

export interface ServerConsentCookie {
  version: string;
  timestamp: number;
  purposes: ConsentPurposes;
  consentId: string;
  userAgent?: string;
  ipHash?: string;
  geoLocation?: string;
  tcString?: string; // TCF 2.2 compliance
  additionalConsent?: string; // Google Additional Consent
}

export interface ConsentAuditLog {
  id: string;
  userId?: string;
  sessionId: string;
  purposes: ConsentPurposes;
  version: string;
  timestamp: number;
  action: 'granted' | 'denied' | 'updated' | 'withdrawn';
  method: 'banner' | 'preferences' | 'api' | 'tcf' | 'user' | 'gdpr_request';
  userAgent: string;
  ipAddress: string;
  geoLocation?: string;
  tcString?: string;
  additionalConsent?: string;
  ttl: number; // Time to live in seconds
}

export type ConsentMethod = 'banner' | 'preferences' | 'api' | 'tcf' | 'user' | 'gdpr_request';
export type ConsentAction = 'granted' | 'denied' | 'updated' | 'withdrawn';
export type ConsentPurpose = keyof ConsentPurposes;
