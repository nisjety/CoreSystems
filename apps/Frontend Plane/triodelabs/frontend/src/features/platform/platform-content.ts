import type { Localized } from "../../lib/i18n/locales";

export type PlatformProduct = {
  description: string;
  href: string;
  image: { alt: string; src: string };
  name: "Verevon" | "Agenci";
  number: string;
  status: "Abonnement" | "Subscription";
  summary: string;
  theme: "verevon" | "agenci";
};

export const platformProducts: readonly PlatformProduct[] = [
  {
    name: "Verevon",
    number: "01",
    status: "Abonnement",
    href: "https://www.verevon.com/",
    theme: "verevon",
    image: {
      src: "/images/Triodelab-bilder/verevon-macbook-mockup.jpg",
      alt: "En bærbar datamaskin på en oransje lenestol viser Verevons arbeidsflate.",
    },
    summary: "Kilder, svar og godkjenning i én arbeidsflyt.",
    description:
      "Verevon er et abonnementsbasert tillegg for team som vil gjøre signaler om til kontrollert handling, med synlig grunnlag og mennesker med i beslutningen.",
  },
  {
    name: "Agenci",
    number: "02",
    status: "Abonnement",
    href: "https://agenci.no/",
    theme: "agenci",
    image: {
      src: "/images/Triodelab-bilder/agenci-macbook-mockup.jpg",
      alt: "En bærbar datamaskin viser Agencis dashboard med samtaler og statistikk.",
    },
    summary: "KI-chatassistent for norske nettsteder.",
    description:
      "Agenci er et abonnementsbasert tillegg som svarer kunder automatisk, døgnet rundt, basert på virksomhetens egen kunnskapsbase. Den kan settes opp på under fem minutter uten koding.",
  },
];

export const platformProductsEn: readonly PlatformProduct[] = [
  {
    ...platformProducts[0],
    status: "Subscription",
    image: {
      src: platformProducts[0].image.src,
      alt: "A laptop on an orange armchair showing the Verevon workspace.",
    },
    summary: "Sources, answers and approval in one workflow.",
    description:
      "Verevon is a subscription add-on for teams that want to turn signals into controlled action, with the evidence visible and people involved in the decision.",
  },
  {
    ...platformProducts[1],
    status: "Subscription",
    image: {
      src: platformProducts[1].image.src,
      alt: "A laptop showing the Agenci dashboard with conversations and statistics.",
    },
    summary: "AI chat assistant for Norwegian websites.",
    description:
      "Agenci is a subscription add-on that answers customers automatically, around the clock, based on your organisation's own knowledge base. It can be set up in under five minutes, with no coding.",
  },
];

type PlatformPageContent = {
  cta: { body: string; heading: string; link: string };
  hero: { body: string; eyebrow: string; heading: string };
  models: {
    body: string;
    consulting: { body: string; heading: string; label: string };
    heading: string;
    subscription: { body: string; heading: string; label: string };
  };
  productLabel: (number: string) => string;
  products: readonly PlatformProduct[];
  productsHeading: string;
  visit: (name: string) => string;
};

export const platformPageContent: Localized<PlatformPageContent> = {
  nb: {
    hero: {
      eyebrow: "Plattform",
      heading: "Produkter vi bygger selv.",
      body: "TriodeLab er både konsulentbyrå og produktleverandør. Når et behov krever mer enn et enkelt prosjekt, kan kundene bruke produktene våre som abonnement.",
    },
    productsHeading: "Produktene",
    productLabel: (number) => `Produkt ${number}`,
    visit: (name) => `Besøk ${name}`,
    products: platformProducts,
    models: {
      heading: "Fra rådgivning til produkter som blir værende.",
      body: "Vi hjelper dere med å finne retning og bygge løsningen. Når Verevon eller Agenci passer oppgaven, blir produktet en fast del av samarbeidet – og vi forbedrer det med innsikt fra hvordan det faktisk brukes.",
      consulting: {
        label: "01 / Konsulentarbeid",
        heading: "Vi former og bygger det dere trenger.",
        body: "Strategi, design, utvikling og integrasjoner tilpasses virksomheten deres. Vi jobber tett med teamet deres fra første valg til løsningen er i bruk.",
      },
      subscription: {
        label: "02 / Produktabonnement",
        heading: "Dere bruker produktet videre i hverdagen.",
        body: "Kundene bruker produktene aktivt i egne arbeidsflyter, mens vi videreutvikler dem og hjelper når behovene endrer seg.",
      },
    },
    cta: {
      heading: "Usikker på hva som passer dere?",
      body: "Fortell oss kort om behovet, så anbefaler vi om dere trenger et prosjekt, et av produktene våre eller begge deler.",
      link: "Ta kontakt",
    },
  },
  en: {
    hero: {
      eyebrow: "Platform",
      heading: "Products we build ourselves.",
      body: "TriodeLab is both a consultancy and a product company. When a need calls for more than a one-off project, clients can use our products on a subscription.",
    },
    productsHeading: "The products",
    productLabel: (number) => `Product ${number}`,
    visit: (name) => `Visit ${name}`,
    products: platformProductsEn,
    models: {
      heading: "From advisory to products that stay.",
      body: "We help you find direction and build the solution. When Verevon or Agenci fits the task, the product becomes a lasting part of our work together – and we improve it with insight from how it is actually used.",
      consulting: {
        label: "01 / Consulting",
        heading: "We shape and build what you need.",
        body: "Strategy, design, development and integrations are tailored to your business. We work closely with your team from the first decision until the solution is in use.",
      },
      subscription: {
        label: "02 / Product subscription",
        heading: "You keep using the product day to day.",
        body: "Clients use the products actively in their own workflows, while we keep developing them and help when needs change.",
      },
    },
    cta: {
      heading: "Not sure what suits you?",
      body: "Tell us briefly what you need, and we will recommend whether you need a project, one of our products or both.",
      link: "Get in touch",
    },
  },
};

export const platformSeo: Localized<{ description: string; title: string }> = {
  nb: {
    title: "Agenci og Verevon | Produkter fra TriodeLab",
    description:
      "Se produktene TriodeLab utvikler: Agenci for svar fra egen kunnskap og Verevon for kontrollerte KI-arbeidsflyter.",
  },
  en: {
    title: "Agenci and Verevon | Products from TriodeLab",
    description:
      "Explore the products TriodeLab builds: Agenci for answers from your own knowledge and Verevon for controlled AI workflows.",
  },
};
