// utils/consent.ts
import type { ConsentState } from '../consent/useConsent';

// Define StoredConsent type
interface StoredConsent {
  version: string;
  choices: ConsentState;
  timestamp: number;
}

export const CONSENT_VERSION = 'v1';
export const LS_CONSENT_KEY = 'consent:choices';

export const DEFAULT_CONSENT: ConsentState = {
  necessary: true,
  performance: false,
  functional: false,
  marketing: false,
  analytics: false,
  social: false,
};

export function readConsent(): StoredConsent | null {
  if (typeof window === 'undefined') return null;
  try {
    const raw = localStorage.getItem(LS_CONSENT_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as StoredConsent;
    return parsed?.version ? parsed : null;
  } catch {
    return null;
  }
}

export function writeConsent(choices: ConsentState): boolean {
  if (typeof window === 'undefined') return false;
  try {
    const payload: StoredConsent = { version: CONSENT_VERSION, choices, timestamp: Date.now() };
    localStorage.setItem(LS_CONSENT_KEY, JSON.stringify(payload));
    return true;
  } catch {
    return false;
  }
}
