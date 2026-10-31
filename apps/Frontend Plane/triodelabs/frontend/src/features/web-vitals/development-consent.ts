import { vitalsPage } from "./policy";

export const DEVELOPMENT_CONSENT_EVENT = "triodelab:development-vitals-consent";
// No cookies/localStorage/sessionStorage: a new document always starts off.
const grants = new WeakMap<Window, boolean>();

export function developmentConsentAvailable(
  win: Window,
  development: boolean,
): boolean {
  if (
    !development ||
    process.env.NODE_ENV !== "development" ||
    process.env.NEXT_PUBLIC_WEB_VITALS_ENABLED !== "true"
  )
    return false;
  try {
    return (
      ["http://localhost:3000", "http://localhost:3001"].includes(new URL(win.location.href).origin) &&
      vitalsPage(win.location.href, true) !== null
    );
  } catch {
    return false;
  }
}

export function privacySignal(win: Window): boolean {
  return (
    win.navigator.doNotTrack === "1" ||
    (win.navigator as Navigator & { globalPrivacyControl?: boolean })
      .globalPrivacyControl === true
  );
}

export function readDevelopmentConsent(
  win: Window,
  development: boolean,
): boolean | undefined {
  if (!developmentConsentAvailable(win, development)) return undefined;
  return !privacySignal(win) && grants.get(win) === true;
}

export function setDevelopmentConsent(
  win: Window,
  development: boolean,
  accepted: boolean,
): boolean {
  if (
    !developmentConsentAvailable(win, development) ||
    (accepted && privacySignal(win))
  )
    return false;
  grants.set(win, accepted);
  win.dispatchEvent(new Event(DEVELOPMENT_CONSENT_EVENT));
  return true;
}
