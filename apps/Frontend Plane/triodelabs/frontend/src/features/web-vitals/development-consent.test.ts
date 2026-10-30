import { afterEach, describe, expect, it, vi } from "vitest";
import {
  developmentConsentAvailable,
  readDevelopmentConsent,
  setDevelopmentConsent,
  DEVELOPMENT_CONSENT_EVENT,
} from "./development-consent";

function localWindow(href = "http://localhost:3000/") {
  return Object.assign(new EventTarget(), {
    location: { href },
    navigator: { doNotTrack: "0", globalPrivacyControl: false },
    Cookiebot: { consent: { statistics: false } },
  }) as unknown as Window;
}
function enable() {
  vi.stubEnv("NODE_ENV", "development");
  vi.stubEnv("NEXT_PUBLIC_WEB_VITALS_ENABLED", "true");
}
afterEach(() => vi.unstubAllEnvs());

describe("isolated development Web Vitals consent", () => {
  it("defaults off per document and changes only its own consent authority", () => {
    enable();
    const win = localWindow();
    const event = vi.fn();
    const cookieEvent = vi.fn();
    win.addEventListener(DEVELOPMENT_CONSENT_EVENT, event);
    win.addEventListener("CookiebotOnAccept", cookieEvent);
    expect(readDevelopmentConsent(win, true)).toBe(false);
    expect(setDevelopmentConsent(win, true, true)).toBe(true);
    expect(readDevelopmentConsent(win, true)).toBe(true);
    expect(
      (win as Window & { Cookiebot: { consent: { statistics: boolean } } })
        .Cookiebot.consent.statistics,
    ).toBe(false);
    expect(cookieEvent).not.toHaveBeenCalled();
    expect(event).toHaveBeenCalledTimes(1);
    expect(readDevelopmentConsent(localWindow(), true)).toBe(false);
    setDevelopmentConsent(win, true, false);
    expect(readDevelopmentConsent(win, true)).toBe(false);
  });
  it.each(["production", "test"])(
    "rejects runtime %s even at localhost",
    (runtime) => {
      enable();
      vi.stubEnv("NODE_ENV", runtime);
      const win = localWindow();
      expect(developmentConsentAvailable(win, true)).toBe(false);
      expect(setDevelopmentConsent(win, true, true)).toBe(false);
      expect(readDevelopmentConsent(win, true)).toBeUndefined();
    },
  );
  it.each([
    "https://www.triodelab.no/",
    "http://127.0.0.1:3000/",
    "http://localhost:3002/",
    "http://localhost:3000/kontakt",
    "http://localhost:3000/?private=x",
    "http://localhost:3000/#private",
  ])("rejects unsupported origin/path %s", (href) => {
    enable();
    const win = localWindow(href);
    expect(setDevelopmentConsent(win, true, true)).toBe(false);
  });
  it("supports the separate Triodelab development port without granting other trackers", () => {
    enable(); const win = localWindow("http://localhost:3001/");
    expect(readDevelopmentConsent(win, true)).toBe(false);
    expect(setDevelopmentConsent(win, true, true)).toBe(true);
    expect(readDevelopmentConsent(win, true)).toBe(true);
    expect((win as Window & { Cookiebot: { consent: { statistics: boolean } } }).Cookiebot.consent.statistics).toBe(false);
    vi.stubEnv("NODE_ENV", "production");
    expect(developmentConsentAvailable(win, true)).toBe(false);
  });
  it("requires both caller development qualification and the public enabled flag", () => {
    enable();
    const win = localWindow();
    expect(setDevelopmentConsent(win, false, true)).toBe(false);
    vi.stubEnv("NEXT_PUBLIC_WEB_VITALS_ENABLED", "false");
    expect(setDevelopmentConsent(win, true, true)).toBe(false);
  });
  it.each(["doNotTrack", "globalPrivacyControl"])(
    "does not override %s",
    (signal) => {
      enable();
      const win = localWindow();
      Object.defineProperty(win.navigator, signal, {
        value: signal === "doNotTrack" ? "1" : true,
      });
      expect(setDevelopmentConsent(win, true, true)).toBe(false);
      expect(readDevelopmentConsent(win, true)).toBe(false);
    },
  );
});
