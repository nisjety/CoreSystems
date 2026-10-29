import type { ServiceFaq } from "./service-faq";

export type WebSolutionProcessStep = {
  description: string;
  title: string;
};

export const webSolutionProcess: readonly WebSolutionProcessStep[] = [
  {
    title: "Innsikt",
    description:
      "Vi starter med forretningen, menneskene dere vil nå og oppgavene nettstedet skal løse. Det gir et bedre grunnlag for hvert valg som følger.",
  },
  {
    title: "Plan og struktur",
    description:
      "Vi definerer mål, innhold, prioriteringer og brukerreiser før vi former løsningen. Slik blir nettstedet enkelt å forstå og enkelt å bruke.",
  },
  {
    title: "Design og innhold",
    description:
      "Vi gjør merkevaren tydelig i grensesnittet og hjelper med en struktur som gjør budskapet lett å finne, lese og huske.",
  },
  {
    title: "Utvikling",
    description:
      "Vi velger en plattform som passer oppgaven og bygger for hastighet, tilgjengelighet, sikkerhet og videre drift.",
  },
  {
    title: "Testing og lansering",
    description:
      "Før publisering kontrollerer vi viktige brukerreiser, innhold, mobilvisning og teknisk kvalitet. Lanseringen skal føles trygg — ikke som en test.",
  },
  {
    title: "Videreutvikling",
    description:
      "Etter lansering bruker vi innsikt fra søk, bruk og målinger til å forbedre det som faktisk har betydning over tid.",
  },
];

export const webSolutionCriteria = [
  {
    title: "Behov først",
    description:
      "Løsningen skal hjelpe besøkende med å finne, forstå eller gjøre noe konkret — uten unødvendige hindringer.",
  },
  {
    title: "Et tydelig uttrykk",
    description:
      "Designet skal gjøre avsenderen gjenkjennelig og troverdig, samtidig som innholdet får være hovedpersonen.",
  },
  {
    title: "Lett å bruke",
    description:
      "God struktur, lesbart innhold og tilgjengelige grensesnitt gjør det enklere for flere å lykkes på nettstedet.",
  },
] as const;

export const webSolutionFaqs: readonly ServiceFaq[] = [
  {
    question: "Hva bør en nettside for bedrift inneholde?",
    answer:
      "Det kommer an på bedriften, men det viktigste er at kunden raskt forstår hva dere tilbyr og hvordan de kommer videre. En god nettside bør ha tydelig innhold, enkel navigasjon, fungere godt på mobil og gjøre det lett å kontakte dere eller utføre ønsket handling.",
  },
  {
    question: "Må vi kunne mye om webdesign og webutvikling?",
    answer:
      "Nei. Dere kjenner bedriften og kundene deres – vi tar oss av det digitale. Vi forklarer valgene underveis og anbefaler det vi mener passer best, slik at dere slipper å sette dere inn i alt det tekniske.",
  },
];

export const webSolutionFaqsEn: readonly ServiceFaq[] = [
  {
    question: "What should a business website include?",
    answer:
      "It depends on the business, but the most important thing is that customers quickly understand what you offer and how to take the next step. A good website has clear content and simple navigation, works well on mobile and makes it easy to contact you or complete the action you want.",
  },
  {
    question: "Do we need to know much about web design and development?",
    answer:
      "No. You know your business and your customers – we take care of the digital side. We explain the choices along the way and recommend what we think fits best, so you don't have to get to grips with all the technical details.",
  },
];
