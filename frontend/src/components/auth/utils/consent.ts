export type ConsentKeys = 'necessary' | 'performance' | 'functional' | 'marketing';
export type ConsentState = Record<ConsentKeys, boolean>;
export type StoredConsent = { version: string; choices: ConsentState; timestamp: number };

export const CONSENT_VERSION = 'v1';
export const LS_CONSENT_KEY = 'consent:choices';

export const DEFAULT_CONSENT: ConsentState = {
  necessary: true,
  performance: false,
  functional: false,
  marketing: false,
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

export function writeConsent(choices: ConsentState): void {
  if (typeof window === 'undefined') return;
  const payload: StoredConsent = { version: CONSENT_VERSION, choices, timestamp: Date.now() };
  localStorage.setItem(LS_CONSENT_KEY, JSON.stringify(payload));
  // TODO: Load/remove scripts dynamically based on choices
}

export function hasValidConsent(): boolean {
  const stored = readConsent();
  return stored !== null && stored.version === CONSENT_VERSION;
}

export function shouldShowConsentBanner(): boolean {
  return !hasValidConsent();
}

export function clearConsent(): void {
  if (typeof window === 'undefined') return;
  localStorage.removeItem(LS_CONSENT_KEY);
}
