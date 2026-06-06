"use client";

/**
 * Shared NO/EN locale provider for Velion v2.
 *
 * Replaces the cosmetic language switcher that previously only fired a
 * toast. The selected locale is persisted to the `velion_locale` cookie
 * (1-year max-age) so it survives reloads and is readable server-side if
 * we later want to pre-render localized copy.
 *
 * Both the auth page and the onboarding wizard read from this provider:
 *   - onboarding i18n calls `useLanguageSwitch()` (V1-compatible shape)
 *   - the auth page can use `useLocale()` directly.
 */

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
} from "react";

export type Locale = "nb" | "en";

export const LOCALE_COOKIE = "velion_locale";
const DEFAULT_LOCALE: Locale = "nb";

interface LocaleContextValue {
  currentLocale: Locale;
  setLocale: (locale: Locale) => void;
  toggle: () => void;
}

const LocaleContext = createContext<LocaleContextValue | null>(null);

function readLocaleCookie(): Locale | null {
  if (typeof document === "undefined") return null;
  const match = document.cookie
    .split("; ")
    .find((row) => row.startsWith(`${LOCALE_COOKIE}=`));
  if (!match) return null;
  const value = match.slice(LOCALE_COOKIE.length + 1);
  return value === "en" || value === "nb" ? value : null;
}

function writeLocaleCookie(locale: Locale): void {
  if (typeof document === "undefined") return;
  document.cookie = `${LOCALE_COOKIE}=${locale}; path=/; max-age=31536000; SameSite=Lax`;
}

export function LocaleProvider({
  children,
  initialLocale = DEFAULT_LOCALE,
}: {
  children: React.ReactNode;
  initialLocale?: Locale;
}) {
  const [currentLocale, setCurrentLocale] = useState<Locale>(initialLocale);

  // Hydrate from the cookie after mount so the server-rendered markup
  // (which has no access to the cookie unless passed in) and the first
  // client render agree on `initialLocale`, then we reconcile.
  useEffect(() => {
    // SSR renders with `initialLocale` (no cookie access); after mount we
    // reconcile from the persisted cookie. This is the legitimate "sync from
    // an external store after mount" case the rule warns about.
    const stored = readLocaleCookie();
    if (stored && stored !== currentLocale) {
      // eslint-disable-next-line react-hooks/set-state-in-effect
      setCurrentLocale(stored);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const setLocale = useCallback((locale: Locale) => {
    setCurrentLocale(locale);
    writeLocaleCookie(locale);
  }, []);

  const toggle = useCallback(() => {
    setCurrentLocale((prev) => {
      const next: Locale = prev === "nb" ? "en" : "nb";
      writeLocaleCookie(next);
      return next;
    });
  }, []);

  const value = useMemo<LocaleContextValue>(
    () => ({ currentLocale, setLocale, toggle }),
    [currentLocale, setLocale, toggle],
  );

  return <LocaleContext.Provider value={value}>{children}</LocaleContext.Provider>;
}

export function useLocale(): LocaleContextValue {
  const context = useContext(LocaleContext);
  if (!context) {
    // Degrade gracefully outside a provider (e.g. isolated tests) instead
    // of throwing — callers still get a usable, NO-default value.
    return {
      currentLocale: DEFAULT_LOCALE,
      setLocale: () => undefined,
      toggle: () => undefined,
    };
  }
  return context;
}

/** V1-compatible shape consumed by the onboarding i18n helpers. */
export function useLanguageSwitch(): {
  currentLocale: Locale;
  switchLanguage: (locale: Locale) => void;
} {
  const { currentLocale, setLocale } = useLocale();
  return { currentLocale, switchLanguage: setLocale };
}
