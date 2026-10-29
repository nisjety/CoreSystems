import type { Localized } from "../../lib/i18n/locales";

export type NavigationDropdownItem = {
  description: string;
  external?: boolean;
  /** Internal links use the Norwegian path and are localized when rendered. */
  href: string;
  image?: string;
  imagePosition?: string;
  label: string;
  number: string;
};

export type NavigationDropdownId = "services" | "products";

export type NavigationDropdown = {
  description: string;
  eyebrow: string;
  id: NavigationDropdownId;
  items: readonly NavigationDropdownItem[];
  overview: { href: string; label: string };
  title: string;
};

export const navigationDropdowns: Localized<
  Record<NavigationDropdownId, NavigationDropdown>
> = {
  nb: {
    services: {
      id: "services",
      title: "Tjenester",
      eyebrow: "Utforsk tjenester",
      description: "Fra første retning til en løsning som virker i hverdagen.",
      overview: { href: "/tjenester", label: "Se alle tjenester" },
      items: [
        {
          number: "01",
          label: "Webløsninger",
          description: "Nettsider og digitale produkter.",
          href: "/tjenester/web-losninger",
          image: "/images/Triodelab-weblosning.jpg",
          imagePosition: "50% 66%",
        },
        {
          number: "02",
          label: "Nettbutikk",
          description: "Handleopplevelser som er enkle å drifte.",
          href: "/tjenester/nettbutikk",
          image: "/images/Nettbutikk.jpg",
          imagePosition: "45% 44%",
        },
        {
          number: "03",
          label: "Organisk trafikk",
          description: "SEO og innhold som blir funnet.",
          href: "/tjenester/organisk-trafikk",
          image: "/images/OrganiskTraffikk.jpg",
          imagePosition: "60% 50%",
        },
        {
          number: "04",
          label: "Integrasjoner",
          description: "Systemer som jobber sammen.",
          href: "/tjenester/integrasjoner",
          image: "/images/Integrasjoner.jpg",
          imagePosition: "50% 62%",
        },
        {
          number: "05",
          label: "Google + Meta Ads",
          description: "Annonsering som kan måles.",
          href: "/tjenester/google-og-meta-ads",
          image: "/images/Google-meta-Ads.jpg",
          imagePosition: "50% 90%",
        },
        {
          number: "06",
          label: "KI og automatisering",
          description: "KI-assistenter og smartere arbeidsflyter.",
          href: "/tjenester/ki-og-automatisering",
          image: "/images/AI-automation.jpg",
          imagePosition: "50% 60%",
        },
      ],
    },
    products: {
      id: "products",
      title: "Produkter",
      eyebrow: "Utforsk produkter",
      description:
        "Abonnementsbaserte produkter som kundene bruker som en aktiv del av samarbeidet.",
      overview: { href: "/plattform", label: "Se plattformene" },
      items: [
        {
          number: "01",
          label: "Verevon",
          description: "Kilder, svar og godkjenning i én arbeidsflyt.",
          href: "https://www.verevon.com/",
          external: true,
          image: "/images/Triodelab-bilder/verevon-hero.png",
        },
        {
          number: "02",
          label: "Agenci",
          description: "KI-chatassistent for norske nettsteder.",
          href: "https://agenci.no/",
          external: true,
          image: "/images/Triodelab-bilder/agenci-dashboard.png",
        },
      ],
    },
  },
  en: {
    services: {
      id: "services",
      title: "Services",
      eyebrow: "Explore services",
      description: "From first direction to a solution that works day to day.",
      overview: { href: "/tjenester", label: "See all services" },
      items: [
        {
          number: "01",
          label: "Web solutions",
          description: "Websites and digital products.",
          href: "/tjenester/web-losninger",
          image: "/images/Triodelab-weblosning.jpg",
          imagePosition: "50% 66%",
        },
        {
          number: "02",
          label: "E-commerce",
          description: "Shopping experiences that are easy to run.",
          href: "/tjenester/nettbutikk",
          image: "/images/Nettbutikk.jpg",
          imagePosition: "45% 44%",
        },
        {
          number: "03",
          label: "Organic traffic",
          description: "SEO and content that gets found.",
          href: "/tjenester/organisk-trafikk",
          image: "/images/OrganiskTraffikk.jpg",
          imagePosition: "60% 50%",
        },
        {
          number: "04",
          label: "Integrations",
          description: "Systems that work together.",
          href: "/tjenester/integrasjoner",
          image: "/images/Integrasjoner.jpg",
          imagePosition: "50% 62%",
        },
        {
          number: "05",
          label: "Google + Meta Ads",
          description: "Advertising you can measure.",
          href: "/tjenester/google-og-meta-ads",
          image: "/images/Google-meta-Ads.jpg",
          imagePosition: "50% 90%",
        },
        {
          number: "06",
          label: "AI and automation",
          description: "AI assistants and smarter workflows.",
          href: "/tjenester/ki-og-automatisering",
          image: "/images/AI-automation.jpg",
          imagePosition: "50% 60%",
        },
      ],
    },
    products: {
      id: "products",
      title: "Products",
      eyebrow: "Explore products",
      description:
        "Subscription products our clients use as an active part of working with us.",
      overview: { href: "/plattform", label: "See the platforms" },
      items: [
        {
          number: "01",
          label: "Verevon",
          description: "Sources, answers and approval in one workflow.",
          href: "https://www.verevon.com/",
          external: true,
          image: "/images/Triodelab-bilder/verevon-hero.png",
        },
        {
          number: "02",
          label: "Agenci",
          description: "AI chat assistant for Norwegian websites.",
          href: "https://agenci.no/",
          external: true,
          image: "/images/Triodelab-bilder/agenci-dashboard.png",
        },
      ],
    },
  },
};
