import type { ServiceSeo } from "./service-seo";

// English search copy. `canonical` stays the Norwegian route key; the page
// metadata turns it into the English URL (see lib/i18n/routes.ts).

export const servicesIndexSeoEn = {
  canonical: "/tjenester",
  title: "Digital services | Web development, AI and automation | TriodeLab",
  description:
    "TriodeLab builds websites, online stores and system integrations, and uses AI and automation where it adds real value. We also help with SEO and advertising.",
} as const;

export const webSolutionsSeoEn: ServiceSeo = {
  canonical: "/tjenester/web-losninger",
  title: "Business websites | Web design and development | TriodeLab",
  description:
    "Need a website for your business? TriodeLab combines web design and web development in a solution that is easy to use, update and build on.",
  image: "/images/services/web-solutions-pair-programming.jpg",
  lastUpdated: "2026-10-08",
  lastUpdatedLabel: "8 October 2026",
  name: "Business websites",
  serviceType: "Web design and web development",
};

export const commerceSeoEn: ServiceSeo = {
  canonical: "/tjenester/nettbutikk",
  title: "E-commerce development for businesses | TriodeLab",
  description:
    "Planning an online store? TriodeLab builds online stores with a great buying experience, reliable operations and a foundation you can keep developing.",
  image: "/images/services/commerce-online-shopping-lifestyle.jpg",
  lastUpdated: "2026-10-08",
  lastUpdatedLabel: "8 October 2026",
  name: "E-commerce for businesses",
  serviceType: "Online store design and e-commerce development",
};

export const organicTrafficSeoEn: ServiceSeo = {
  canonical: "/tjenester/organisk-trafikk",
  title: "Organic traffic and SEO | Get found on Google | TriodeLab",
  description:
    "Want more organic traffic? TriodeLab helps with SEO, keyword research, content and technical improvements that make you visible on Google.",
  image: "/images/services/organic-search-research.jpg",
  lastUpdated: "2026-10-08",
  lastUpdatedLabel: "8 October 2026",
  name: "Organic traffic and SEO",
  serviceType: "Search engine optimisation",
};

export const integrationsSeoEn: ServiceSeo = {
  canonical: "/tjenester/integrasjoner",
  title: "System integrations | Connect your systems | TriodeLab",
  description:
    "TriodeLab builds system integrations that connect your online store, CRM, ERP and other systems, so data flows better and everyday work gets easier.",
  image: "/images/services/integrations-real-colleagues-lines.webp",
  lastUpdated: "2026-10-08",
  lastUpdatedLabel: "8 October 2026",
  name: "System integrations",
  serviceType: "System integrations",
};

export const paidAdsSeoEn: ServiceSeo = {
  canonical: "/tjenester/google-og-meta-ads",
  title: "Google Ads and Meta Ads | Advertising you can measure | TriodeLab",
  description:
    "TriodeLab helps you with Google Ads and Meta Ads, from campaigns and messaging to tracking, measurement and continuous improvement.",
  image: "/images/services/google-ads-analytics.jpg",
  lastUpdated: "2026-10-08",
  lastUpdatedLabel: "8 October 2026",
  name: "Google Ads and Meta Ads",
  serviceType: "Digital advertising",
};

export const aiAutomationSeoEn: ServiceSeo = {
  canonical: "/tjenester/ki-og-automatisering",
  title: "AI and automation for businesses | TriodeLab",
  description:
    "TriodeLab builds AI assistants and AI-supported workflows for businesses. Use generative AI and automation for concrete tasks, with people and control at the centre.",
  image: "/images/services/ai-automation-human-workflow.webp",
  lastUpdated: "2026-10-08",
  lastUpdatedLabel: "8 October 2026",
  name: "AI and automation",
  serviceType: "AI assistants and workflow automation",
};

export const consultancySeoEn: ServiceSeo = {
  canonical: "/tjenester/radgivning-og-konsulentbistand",
  title: "Digital advisory and IT consulting | TriodeLab",
  description:
    "Get help with technology choices, project scoping and development. TriodeLab offers digital advisory and IT consultants for defined assignments or alongside your team.",
  image: "/images/services/consultancy-advisory-session.jpg",
  lastUpdated: "2026-10-08",
  lastUpdatedLabel: "8 October 2026",
  name: "Digital advisory and consulting",
  serviceType: "Digital advisory and IT consulting",
};
