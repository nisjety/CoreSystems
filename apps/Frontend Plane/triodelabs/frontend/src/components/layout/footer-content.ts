import type { Localized } from "../../lib/i18n/locales";

type FooterLink = { href: string; label: string };

/** Hrefs are Norwegian paths; the footer localizes them when rendering. */
export const footerContent: Localized<{
  address: readonly string[];
  backHome: string;
  columns: readonly { label: string; links: readonly FooterLink[] }[];
  contactHeading: string;
  home: string;
  legal: readonly FooterLink[];
  legalLabel: string;
  navLabel: (column: string) => string;
  officeHeading: string;
  tagline: string;
  disciplines: string;
}> = {
  nb: {
    home: "TriodeLab hjem",
    tagline:
      "TriodeLab gjør ambisiøse idéer til tydelige digitale opplevelser — fra første retning til noe som virker.",
    navLabel: (column) => `Bunntekst: ${column}`,
    columns: [
      {
        label: "Tjenester",
        links: [
          { href: "/tjenester", label: "Tjenester" },
          { href: "/tjenester/web-losninger", label: "Webløsninger" },
          { href: "/tjenester/nettbutikk", label: "Nettbutikk" },
          { href: "/tjenester/organisk-trafikk", label: "Organisk trafikk" },
          { href: "/tjenester/integrasjoner", label: "Integrasjoner" },
          { href: "/tjenester/google-og-meta-ads", label: "Google + Meta Ads" },
          {
            href: "/tjenester/ki-og-automatisering",
            label: "KI og automatisering",
          },
          {
            href: "/tjenester/radgivning-og-konsulentbistand",
            label: "Rådgivning og konsulentbistand",
          },
        ],
      },
      {
        label: "Selskap",
        links: [
          { href: "/plattform", label: "Plattform" },
          { href: "/innsikt", label: "Innsikt" },
          { href: "/om-oss", label: "Om oss" },
          { href: "/kontakt", label: "Kontakt" },
        ],
      },
    ],
    officeHeading: "Hovedkontor",
    address: [
      "Sigurd Hoels vei 114",
      "0655 Oslo, Norge",
      "Arbeider med team over hele landet",
    ],
    contactHeading: "Kontakt oss",
    backHome: "Til forsiden",
    disciplines: "Strategi / design / teknologi",
    legalLabel: "Personvern og vilkår",
    legal: [
      { href: "/personvern", label: "Personvern" },
      { href: "/cookies", label: "Informasjonskapsler" },
      { href: "/vilkar", label: "Vilkår" },
      { href: "/salgsvilkar", label: "Salgsvilkår" },
    ],
  },
  en: {
    home: "TriodeLab home",
    tagline:
      "TriodeLab turns ambitious ideas into clear digital experiences — from first direction to something that works.",
    navLabel: (column) => `Footer: ${column}`,
    columns: [
      {
        label: "Services",
        links: [
          { href: "/tjenester", label: "Services" },
          { href: "/tjenester/web-losninger", label: "Web solutions" },
          { href: "/tjenester/nettbutikk", label: "E-commerce" },
          { href: "/tjenester/organisk-trafikk", label: "Organic traffic" },
          { href: "/tjenester/integrasjoner", label: "Integrations" },
          { href: "/tjenester/google-og-meta-ads", label: "Google + Meta Ads" },
          {
            href: "/tjenester/ki-og-automatisering",
            label: "AI and automation",
          },
          {
            href: "/tjenester/radgivning-og-konsulentbistand",
            label: "Advisory and consulting",
          },
        ],
      },
      {
        label: "Company",
        links: [
          { href: "/plattform", label: "Platform" },
          { href: "/om-oss", label: "About" },
          { href: "/kontakt", label: "Contact" },
        ],
      },
    ],
    officeHeading: "Head office",
    address: [
      "Sigurd Hoels vei 114",
      "0655 Oslo, Norway",
      "Working with teams across Norway",
    ],
    contactHeading: "Contact us",
    backHome: "Back to the home page",
    disciplines: "Strategy / design / technology",
    legalLabel: "Privacy and terms",
    legal: [
      { href: "/personvern", label: "Privacy" },
      { href: "/cookies", label: "Cookies" },
      { href: "/vilkar", label: "Terms" },
      { href: "/salgsvilkar", label: "Terms of sale" },
    ],
  },
};
