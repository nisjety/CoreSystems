import type { Localized } from "../../lib/i18n/locales";

type HomeFaq = { readonly answer: string; readonly question: string };

export const homeFaqs = [
  {
    question: "Hva slags digitale løsninger lager TriodeLab?",
    answer:
      "Vi utvikler nettsider, nettbutikker og digitale produkter – og kobler systemene rundt slik at løsningen fungerer som en helhet.",
  },
  {
    question: "Hvordan starter et samarbeid?",
    answer:
      "Vi starter med en samtale om behovene, målene og rammene deres. Derfra finner vi en tydelig retning og avtaler hva som er viktigst å løse først.",
  },
  {
    question: "Kan dere ta ansvar for komplekse digitale prosjekter?",
    answer:
      "Ja. Vi kan bidra fra tidlig rådgivning og planlegging til design, utvikling, integrasjoner og videreutvikling. Omfanget deles inn i tydelige faser, slik at teamet deres har oversikt over prioriteringer, ansvar og neste steg underveis.",
  },
  {
    question: "Kan dere videreutvikle en løsning vi allerede har?",
    answer:
      "Ja. Vi kan forbedre eksisterende nettsider, nettbutikker og arbeidsflyter, eller hjelpe teamet deres med drift og videreutvikling etter lansering.",
  },
  {
    question: "Hva koster en nettside eller nettbutikk?",
    answer:
      "Prisen avhenger av omfang, innhold, integrasjoner og hvor mye som skal skreddersys. Vi avklarer behovene først og gir dere et konkret forslag med tydelige rammer.",
  },
  {
    question: "Hvor lang tid tar det å lage en digital løsning?",
    answer:
      "Det kommer an på løsningens størrelse og hvor raskt innhold og beslutninger er på plass. Vi deler arbeidet i tydelige faser og avtaler fremdrift før vi starter.",
  },
  {
    question: "Kan dere hjelpe oss å bli mer synlige på nett?",
    answer:
      "Ja. Vi kan forbedre teknisk SEO og innhold, og jobbe med Google- og Meta-annonser når betalt synlighet passer målene deres. Vi følger med på hva som gir relevante besøk og henvendelser.",
  },
] as const;

export const homeFaqsEn = [
  {
    question: "What kind of digital solutions does TriodeLab build?",
    answer:
      "We build websites, online stores and digital products – and connect the systems around them so the whole solution works as one.",
  },
  {
    question: "How does working together start?",
    answer:
      "We start with a conversation about your needs, goals and constraints. From there we agree on a clear direction and on what matters most to solve first.",
  },
  {
    question: "Can you take responsibility for complex digital projects?",
    answer:
      "Yes. We can contribute from early advisory and planning through design, development, integrations and further development. The scope is split into clear phases, so your team keeps an overview of priorities, responsibilities and next steps along the way.",
  },
  {
    question: "Can you improve a solution we already have?",
    answer:
      "Yes. We can improve existing websites, online stores and workflows, or support your team with operations and further development after launch.",
  },
  {
    question: "What does a website or online store cost?",
    answer:
      "The price depends on scope, content, integrations and how much needs to be tailored. We clarify your needs first and give you a concrete proposal with a clear scope and budget.",
  },
  {
    question: "How long does it take to build a digital solution?",
    answer:
      "It depends on the size of the solution and how quickly content and decisions are in place. We split the work into clear phases and agree on a timeline before we start.",
  },
  {
    question: "Can you help us become more visible online?",
    answer:
      "Yes. We can improve technical SEO and content, and run Google and Meta ads when paid visibility suits your goals. We keep track of what brings relevant visits and enquiries.",
  },
] as const;

export const homeFaqContent: Localized<{
  faqs: readonly HomeFaq[];
  heading: string;
  intro: string;
}> = {
  nb: {
    heading: "Spørsmål, besvart.",
    intro: "Korte svar om digitale prosjekter, samarbeid og veien videre.",
    faqs: homeFaqs,
  },
  en: {
    heading: "Questions, answered.",
    intro: "Short answers about digital projects, working together and what comes next.",
    faqs: homeFaqsEn,
  },
};
