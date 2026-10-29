export const servicesIndexSeo = {
  canonical: "/tjenester",
  title: "Digitale tjenester | Webutvikling, KI og automatisering | TriodeLab",
  description:
    "TriodeLab bygger nettsider, nettbutikker og systemintegrasjoner, og bruker KI (AI) og automatisering der det gir reell verdi. Vi hjelper også med SEO og annonsering.",
} as const;

export type ServiceSeo = {
  canonical:
    | "/tjenester/web-losninger"
    | "/tjenester/nettbutikk"
    | "/tjenester/organisk-trafikk"
    | "/tjenester/integrasjoner"
    | "/tjenester/google-og-meta-ads"
    | "/tjenester/ki-og-automatisering"
    | "/tjenester/radgivning-og-konsulentbistand";
  description: string;
  image: string;
  lastUpdated: string;
  lastUpdatedLabel: string;
  name: string;
  serviceType: string;
  title: string;
};

export const webSolutionsSeo: ServiceSeo = {
  canonical: "/tjenester/web-losninger",
  title: "Nettside for bedrift | Webdesign og webutvikling | TriodeLab",
  description:
    "Trenger du en nettside for bedrift? TriodeLab kombinerer webdesign og webutvikling i en løsning som er enkel å bruke, oppdatere og utvikle videre.",
  image: "/images/services/web-solutions-pair-programming.jpg",
  lastUpdated: "2026-10-08",
  lastUpdatedLabel: "8. oktober 2026",
  name: "Nettside for bedrift",
  serviceType: "Webdesign og webutvikling",
};

export const commerceSeo: ServiceSeo = {
  canonical: "/tjenester/nettbutikk",
  title: "Nettbutikk for bedrifter | Design, utvikling og drift | TriodeLab",
  description:
    "Skal dere lage nettbutikk? TriodeLab bygger nettbutikker med en god kjøpsopplevelse, trygg drift og et fundament som kan utvikles videre.",
  image: "/images/services/commerce-online-shopping-lifestyle.jpg",
  lastUpdated: "2026-10-08",
  lastUpdatedLabel: "8. oktober 2026",
  name: "Nettbutikk for bedrifter",
  serviceType: "Nettbutikkdesign og netthandelsutvikling",
};

export const organicTrafficSeo: ServiceSeo = {
  canonical: "/tjenester/organisk-trafikk",
  title: "Organisk trafikk og SEO | Bli synlig på Google | TriodeLab",
  description:
    "Vil dere få mer organisk trafikk? TriodeLab hjelper med SEO, søkeordsanalyse, innhold og tekniske forbedringer som gjør dere synlige på Google.",
  image: "/images/services/organic-search-research.jpg",
  lastUpdated: "2026-10-08",
  lastUpdatedLabel: "8. oktober 2026",
  name: "Organisk trafikk og SEO",
  serviceType: "Søkemotoroptimalisering",
};

export const integrationsSeo: ServiceSeo = {
  canonical: "/tjenester/integrasjoner",
  title: "Systemintegrasjoner | Koble sammen systemene | TriodeLab",
  description:
    "TriodeLab bygger systemintegrasjoner som kobler sammen nettbutikk, CRM, ERP og andre systemer, slik at data flyter bedre og hverdagen blir enklere.",
  image: "/images/services/integrations-real-colleagues-lines.webp",
  lastUpdated: "2026-10-08",
  lastUpdatedLabel: "8. oktober 2026",
  name: "Systemintegrasjoner",
  serviceType: "Systemintegrasjoner",
};

export const paidAdsSeo: ServiceSeo = {
  canonical: "/tjenester/google-og-meta-ads",
  title: "Google Ads og Meta Ads | Annonsering som kan måles | TriodeLab",
  description:
    "TriodeLab hjelper dere med Google Ads og Meta Ads, fra kampanjer og budskap til sporing, måling og løpende forbedring.",
  image: "/images/services/google-ads-analytics.jpg",
  lastUpdated: "2026-10-08",
  lastUpdatedLabel: "8. oktober 2026",
  name: "Google Ads og Meta Ads",
  serviceType: "Digital annonsering",
};

export const aiAutomationSeo: ServiceSeo = {
  canonical: "/tjenester/ki-og-automatisering",
  title: "KI og automatisering for bedrifter | TriodeLab",
  description:
    "TriodeLab bygger KI-assistenter og AI-støttede arbeidsflyter for bedrifter. Bruk generativ KI og automatisering til konkrete oppgaver, med mennesker og kontroll i sentrum.",
  image: "/images/services/ai-automation-human-workflow.webp",
  lastUpdated: "2026-10-08",
  lastUpdatedLabel: "8. oktober 2026",
  name: "KI og automatisering",
  serviceType: "KI-assistenter og automatisering av arbeidsflyter",
};

export const consultancySeo: ServiceSeo = {
  canonical: "/tjenester/radgivning-og-konsulentbistand",
  title: "Digital rådgivning og IT-konsulentbistand | TriodeLab",
  description:
    "Få hjelp til teknologivalg, prosjektavklaring og utvikling. TriodeLab tilbyr digital rådgivning og IT-konsulenter i avgrensede oppdrag eller sammen med deres team.",
  image: "/images/services/consultancy-advisory-session.jpg",
  lastUpdated: "2026-10-08",
  lastUpdatedLabel: "8. oktober 2026",
  name: "Digital rådgivning og konsulentbistand",
  serviceType: "Digital rådgivning og IT-konsulentbistand",
};
