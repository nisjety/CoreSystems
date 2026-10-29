import type { ServiceFaq } from "./service-faq";

export type CommerceProcessStep = {
  description: string;
  title: string;
};

export const commerceProcess: readonly CommerceProcessStep[] = [
  {
    title: "Forretning og sortiment",
    description:
      "Vi starter med hva dere selger, hvem dere skal nå og hva som må fungere i marginer, sortiment og kundereise.",
  },
  {
    title: "Plattform og integrasjoner",
    description:
      "Vi velger en løsning som passer ambisjonsnivået og kobler den til systemene som skal støtte lager, ordre, økonomi og drift.",
  },
  {
    title: "Kjøpsopplevelse",
    description:
      "Kategorier, søk, produktinformasjon og utsjekk formes for å gjøre det enklere å velge — på mobil og større skjermer.",
  },
  {
    title: "Betaling, frakt og retur",
    description:
      "Vi gjør valgene og vilkårene tydelige før kjøpet. En ryddig leverings- og returflyt skaper trygghet også etter bestilling.",
  },
  {
    title: "Innhold og synlighet",
    description:
      "Produktdata, kategorier og redaksjonelt innhold struktureres for kundene som søker — og for kanalene som skal drive relevant trafikk.",
  },
  {
    title: "Måling og forbedring",
    description:
      "Etter lansering følger vi med på hva som blir funnet, lagt i kurven og kjøpt, slik at forbedringer kan prioriteres med grunnlag i bruk.",
  },
];

export const commercePrinciples = [
  {
    title: "Trygghet i kjøpet",
    description:
      "Produktinformasjon, pris, levering og retur må være lett å forstå før kunden skal bestemme seg.",
  },
  {
    title: "Flyt i driften",
    description:
      "Butikken skal fungere sammen med arbeidsmåtene og systemene deres, ikke skape flere manuelle oppgaver.",
  },
  {
    title: "Vekst som kan følges",
    description:
      "Når måling og struktur er på plass, kan dere lære av etterspørselen og utvikle butikken steg for steg.",
  },
] as const;

export const commerceFaqs: readonly ServiceFaq[] = [
  {
    question: "Hva trenger vi før vi lager nettbutikk?",
    answer:
      "Dere trenger ikke ha alt klart. Det er likevel nyttig å ha en idé om hvilke produkter dere skal selge, priser, levering og hvordan dere ønsker å håndtere bestillinger. Resten kan vi finne ut av sammen, inkludert hvilken plattform og teknisk løsning som passer best.",
  },
  {
    question: "Hva gjør en nettbutikk enkel å bruke?",
    answer:
      "Kunden bør raskt finne riktig produkt, forstå hva de får, se hva det koster og komme seg gjennom betalingen uten unødvendige hindringer. Vi bygger derfor hele kjøpsreisen med kunden i fokus – spesielt på mobil, hvor mye av netthandelen skjer.",
  },
];

export const commerceFaqsEn: readonly ServiceFaq[] = [
  {
    question: "What do we need before building an online store?",
    answer:
      "You don't need to have everything ready. It does help to have an idea of which products you will sell, your prices, delivery and how you want to handle orders. We can work out the rest together, including which platform and technical solution fit best.",
  },
  {
    question: "What makes an online store easy to use?",
    answer:
      "Customers should quickly find the right product, understand what they are getting, see what it costs and get through checkout without unnecessary obstacles. That is why we design the whole buying journey around the customer – especially on mobile, where much of online shopping happens.",
  },
];
