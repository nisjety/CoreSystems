import type { Localized } from "../../lib/i18n/locales";

export type PortfolioItem = {
  number: string;
  title: string;
  category: string;
  href?: string;
};

export type OwnedProduct = PortfolioItem & {
  description: string;
  href: string;
  image: string;
  video?: string;
  accentColor: string;
  accentTextColor: string;
};

export type ServiceHighlight = PortfolioItem & {
  description: string;
  action?: string;
};

export const ownedProducts: readonly OwnedProduct[] = [
  {
    number: "01",
    title: "Agenci",
    accentColor: "#427655",
    accentTextColor: "#FFFFFF",
    category: "Eget produkt · KI-chatassistent",
    href: "https://agenci.no/",
    description:
      "En KI-chatassistent som gjør virksomhetens egen kunnskap tilgjengelig hele døgnet.",
    image: "/images/Triodelab-bilder/agenci-macbook-mockup.jpg",
  },
  {
    number: "02",
    title: "Verevon",
    accentColor: "#EE7A50",
    accentTextColor: "#161616",
    category: "Eget produkt · KI-arbeidsflyt",
    href: "https://www.verevon.com/",
    description:
      "En KI-arbeidsflyt som gjør signaler om til tydelige, kontrollerte handlinger.",
    image: "/images/Triodelab-bilder/verevon-hover-mockup.jpg",
    video: "/videos/verevon-mockup-preview.mp4",
  },
];

export const ownedProductsEn: readonly OwnedProduct[] = [
  {
    ...ownedProducts[0],
    category: "Our own product · AI chat assistant",
    description:
      "An AI chat assistant that makes your organisation's own knowledge available around the clock.",
  },
  {
    ...ownedProducts[1],
    category: "Our own product · AI workflow",
    description:
      "An AI workflow that turns signals into clear, controlled actions.",
  },
];

export const ownedProductsByLocale: Localized<readonly OwnedProduct[]> = {
  nb: ownedProducts,
  en: ownedProductsEn,
};

export const portfolioCopy: Localized<{
  body: string;
  explore: (product: string) => string;
  heading: string;
  imageAlt: (product: string) => string;
  tablist: string;
  viewProduct: string;
}> = {
  nb: {
    heading: "Field of Dreams",
    body: "Agenci gir teamet raske svar fra egen kunnskap. Verevon gjør signaler om til tydelige, kontrollerte handlinger. Utforsk to KI-produkter som gjør hverdagen enklere.",
    tablist: "Velg produkt",
    explore: (product) => `Utforsk ${product}`,
    viewProduct: "Se produktet",
    imageAlt: (product) => `${product} produktforhåndsvisning`,
  },
  en: {
    heading: "Field of Dreams",
    body: "Agenci gives teams fast answers from their own knowledge. Verevon turns signals into clear, controlled actions. Explore two AI products that make everyday work easier.",
    tablist: "Choose a product",
    explore: (product) => `Explore ${product}`,
    viewProduct: "View the product",
    imageAlt: (product) => `${product} product preview`,
  },
};

export const serviceHighlights: readonly ServiceHighlight[] = [
  {
    number: "01",
    title: "WordPress",
    category: "Nettsider",
    description:
      "Redaktørvennlige nettsteder som gir teamet frihet til å publisere, forbedre og vokse.",
    action: "Utforsk WordPress",
    href: "/tjenester/web-losninger",
  },
  {
    number: "02",
    title: "Nettbutikk",
    category: "E-handel",
    description:
      "Handleopplevelser som gjør det enkelt å finne, velge og kjøpe – på alle skjermer.",
    action: "Utforsk nettbutikk",
    href: "/tjenester/nettbutikk",
  },
  {
    number: "03",
    title: "Portefølje",
    category: "Utvalgte arbeider",
    description:
      "Digitale prosjekter vi har utviklet sammen med team som trenger tydelige løsninger.",
  },
];
