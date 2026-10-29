import type { Locale } from "../../lib/i18n/locales";
import { localizedRoutes } from "../../lib/i18n/routes";
import type { ContactFormData } from "../../lib/validations";

export const contactSources = [
  "direct",
  "web-losninger",
  "nettbutikk",
  "organisk-trafikk",
  "integrasjoner",
  "google-og-meta-ads",
  "ki-og-automatisering",
  "radgivning-og-konsulentbistand",
] as const;

export type ContactSource = (typeof contactSources)[number];

const sourceProjectTypes: Record<
  ContactSource,
  ContactFormData["projectType"]
> = {
  direct: "annet",
  "web-losninger": "webutvikling",
  nettbutikk: "ehandel",
  "organisk-trafikk": "seo",
  integrasjoner: "integrasjoner",
  "google-og-meta-ads": "annonsering",
  "ki-og-automatisering": "ki",
  "radgivning-og-konsulentbistand": "konsultasjon",
};

export function getContactSource(value: unknown): ContactSource {
  return contactSources.find((source) => source === value) ?? "direct";
}

export function getProjectTypeForSource(source: ContactSource) {
  return sourceProjectTypes[source];
}

/**
 * The contact page's service query parameter: `?tjeneste=<norsk slug>` on the
 * Norwegian page and `?service=<English slug>` on the English one. Both map to
 * the same source, so the email to TriodeLab looks the same either way.
 */
export const contactSourceParam: Record<Locale, string> = {
  nb: "tjeneste",
  en: "service",
};

export function getContactSourceFromSlug(
  value: unknown,
  locale: Locale,
): ContactSource {
  if (locale === "nb") return getContactSource(value);

  const route = localizedRoutes.find(
    (entry) =>
      entry.nb.startsWith("/tjenester/") && entry.en.split("/").at(-1) === value,
  );
  // Fall back to the Norwegian slug so shared Norwegian links keep working.
  return getContactSource(route ? route.nb.split("/").at(-1) : value);
}

export function getContactSlugForSource(
  source: ContactSource,
  locale: Locale,
): string {
  const route = localizedRoutes.find(
    (entry) => entry.nb === `/tjenester/${source}`,
  );
  return route?.[locale].split("/").at(-1) ?? source;
}
