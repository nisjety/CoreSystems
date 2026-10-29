import type { Locale } from "./locales";

/**
 * Every public page and its address in each language. Content files link to
 * the Norwegian path; `localizeHref` turns it into the address for the page
 * being rendered. A new page needs a row here before it gets an English URL.
 */
export const localizedRoutes = [
  { nb: "/", en: "/en" },
  { nb: "/tjenester", en: "/en/services" },
  { nb: "/tjenester/web-losninger", en: "/en/services/web-solutions" },
  { nb: "/tjenester/nettbutikk", en: "/en/services/ecommerce" },
  { nb: "/tjenester/organisk-trafikk", en: "/en/services/organic-traffic" },
  { nb: "/tjenester/integrasjoner", en: "/en/services/integrations" },
  {
    nb: "/tjenester/google-og-meta-ads",
    en: "/en/services/google-and-meta-ads",
  },
  {
    nb: "/tjenester/ki-og-automatisering",
    en: "/en/services/ai-and-automation",
  },
  {
    nb: "/tjenester/radgivning-og-konsulentbistand",
    en: "/en/services/advisory-and-consulting",
  },
  { nb: "/plattform", en: "/en/platform" },
  { nb: "/om-oss", en: "/en/about" },
  { nb: "/kontakt", en: "/en/contact" },
  { nb: "/personvern", en: "/en/privacy" },
  { nb: "/cookies", en: "/en/cookies" },
  { nb: "/vilkar", en: "/en/terms" },
  { nb: "/salgsvilkar", en: "/en/terms-of-sale" },
] as const;

export type LocalizedRoute = (typeof localizedRoutes)[number];
export type RoutePath =
  | LocalizedRoute["nb"]
  | "/innsikt"
  | `/innsikt/${string}`;

function stripTrailingSlash(path: string) {
  return path.length > 1 && path.endsWith("/") ? path.slice(0, -1) : path;
}

/** The address of a Norwegian route in the given language. */
export function localizePath(path: RoutePath, locale: Locale): string {
  const route = localizedRoutes.find((entry) => entry.nb === path);
  return route ? route[locale] : path;
}

/**
 * Localizes an internal href written with a Norwegian path, keeping any query
 * string or hash. External links, mailto/tel and same-page anchors pass
 * through unchanged.
 */
export function localizeHref(href: string, locale: Locale): string {
  if (locale === "nb" || !href.startsWith("/")) return href;

  const match = /^([^?#]*)(.*)$/.exec(href);
  const path = stripTrailingSlash(match?.[1] || "/");
  const suffix = match?.[2] ?? "";
  const route = localizedRoutes.find((entry) => entry.nb === path);

  return route ? `${route.en}${suffix}` : href;
}

export function getLocaleFromPathname(pathname: string): Locale {
  return pathname === "/en" || pathname.startsWith("/en/") ? "en" : "nb";
}

/**
 * The same page in another language, for the language switcher and hreflang.
 * Unknown paths fall back to that language's home page.
 */
export function getAlternatePath(pathname: string, target: Locale): string {
  const path = stripTrailingSlash(pathname.split(/[?#]/)[0] || "/");
  const current = getLocaleFromPathname(path);
  const route = localizedRoutes.find((entry) => entry[current] === path);

  return route ? route[target] : localizePath("/", target);
}
