import type { Localized } from "../../lib/i18n/locales";
import type { ServiceFaq } from "./service-faq";
import type { ServiceSeo } from "./service-seo";

/** Everything a page rendered by ServiceArticlePage needs, in one language. */
export type ServiceArticleContent = {
  cta: { body: string; title: string };
  editorial: {
    faqTitle: string;
    introAside: string;
    narrative: readonly string[];
    narrativeTitle: string;
    reflectionTitle: string;
  };
  faqs: readonly ServiceFaq[];
  hero: {
    imageAlt: string;
    imagePosition?: string;
    imageSrc: string;
    summary: string;
    title: readonly string[];
  };
  intro: { body: readonly string[]; kicker: string; title: string };
  label: string;
  reflection: { body: readonly string[]; label: string };
  seo: ServiceSeo;
};

/** Fixed labels in the service page template. */
export const serviceArticleLabels: Localized<{
  breadcrumbHome: string;
  breadcrumbServices: string;
  contact: string;
  delivery: {
    collaboration: string;
    continuation: string;
    heading: string;
    intro: string;
    marker: string;
    quality: string;
  };
  faqMarker: string;
  location: string;
  narrativeMarker: string;
  servicePage: string;
  startProject: string;
  updated: (date: string) => string;
}> = {
  nb: {
    location: "Oslo / Norge",
    servicePage: "Tjenesteside",
    updated: (date) => `Oppdatert ${date}`,
    startProject: "Start et prosjekt",
    narrativeMarker: "02 / Slik jobber vi",
    faqMarker: "05 / Spørsmål og svar",
    contact: "Kontakt oss",
    breadcrumbHome: "Hjem",
    breadcrumbServices: "Tjenester",
    delivery: {
      marker: "03 / Leveranse og samarbeid",
      heading: "Dette kan inngå i leveransen.",
      intro:
        "Vi avtaler omfang, ansvar og kriterier for godkjenning før arbeidet starter. Dere kan velge et avgrenset oppdrag eller en leveranse med flere av delene under.",
      collaboration: "Samarbeid med deres team",
      quality: "Kvalitet og godkjenning",
      continuation: "Etter leveransen",
    },
  },
  en: {
    location: "Oslo / Norway",
    servicePage: "Service",
    updated: (date) => `Updated ${date}`,
    startProject: "Start a project",
    narrativeMarker: "02 / How we work",
    faqMarker: "05 / Questions and answers",
    contact: "Contact us",
    breadcrumbHome: "Home",
    breadcrumbServices: "Services",
    delivery: {
      marker: "03 / Delivery and collaboration",
      heading: "What the delivery can include.",
      intro:
        "We agree on scope, responsibilities and acceptance criteria before work starts. You can choose a defined assignment or a delivery that combines several of the parts below.",
      collaboration: "Working with your team",
      quality: "Quality and approval",
      continuation: "After delivery",
    },
  },
};
