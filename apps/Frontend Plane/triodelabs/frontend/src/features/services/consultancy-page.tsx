import type { Locale, Localized } from "@/lib/i18n/locales";
import { ServiceArticlePage } from "./service-article-page";
import type { ServiceArticleContent } from "./service-article-content";
import { consultancyFaqs, consultancyFaqsEn } from "./consultancy-page-data";
import { consultancySeo } from "./service-seo";
import { consultancySeoEn } from "./service-seo-en";

const consultancyContent: Localized<ServiceArticleContent> = {
  nb: {
    cta: {
      title: "Hva trenger dere å komme videre med?",
      body: "Fortell om løsningen, teamet og utfordringen. Vi kan ta utgangspunkt i et konkret oppdrag eller hjelpe dere med å avklare neste steg.",
    },
    editorial: {
      faqTitle: "Spørsmål om rådgivning og konsulentoppdrag.",
      introAside:
        "Et godt råd skal gjøre neste beslutning enklere å ta – og mulig å gjennomføre.",
      narrativeTitle: "Tydelig ansvar fra første arbeidsøkt.",
      narrative: [
        "Vi starter med samtaler og en gjennomgang av det som finnes: mål, systemer, arbeidsmåter og kjente hindringer. Det gjør det lettere å skille et teknisk problem fra noe som handler om innhold, prioritering eller ansvar.",
        "Deretter foreslår vi et avgrenset oppdrag. Det kan være et beslutningsgrunnlag, en teknisk undersøkelse, videreutvikling av en løsning eller en konsulent som arbeider sammen med teamet deres.",
        "Dere får synlig fremdrift og mulighet til å vurdere det som leveres underveis. Nye behov håndteres som bevisste valg om omfang og prioritet.",
      ],
      reflectionTitle: "Kompetanse som blir igjen hos dere.",
    },
    faqs: consultancyFaqs,
    hero: {
      imageSrc: "/images/services/consultancy-advisory-session.jpg",
      imageAlt:
        "To fagpersoner diskuterer en digital løsning ved en bærbar datamaskin.",
      title: ["Rådgivning", "som fører", "til handling."],
      summary:
        "Vi tilbyr digital rådgivning og IT-konsulentbistand innen web, netthandel, integrasjoner og KI. Fra avklaring og teknologivalg til utvikling sammen med teamet deres.",
    },
    intro: {
      kicker: "Rådgivning og konsulentbistand",
      title: "Riktig kompetanse rundt den konkrete oppgaven.",
      body: [
        "Noen virksomheter trenger et uavhengig blikk før en investering. Andre har en tydelig retning, men mangler tid eller kompetanse til å gjennomføre. Vi tilpasser oppdraget til hvor dere står og hva dere allerede har internt.",
        "Vi kan hjelpe med å vurdere en eksisterende nettside eller nettbutikk, planlegge systemkoblinger, avklare en KI-pilot eller videreutvikle en digital løsning. Anbefalingene tar hensyn til drift og forvaltning, i tillegg til det som skal bygges nå.",
        "Oppdrag kan leveres som en avgrenset kartlegging, prosjektbistand eller utvikling i samarbeid med deres team. Kompetansebehov, kapasitet og arbeidsform avklares før vi starter.",
      ],
    },
    label: "Tjenester / Rådgivning og konsulentbistand",
    reflection: {
      label: "Forståelige valg",
      body: [
        "En løsning skal være forståelig også når konsulentoppdraget er avsluttet. Derfor planlegger vi dokumentasjon og kunnskapsoverføring som en del av arbeidet.",
        "Dere skal vite hvorfor et valg ble tatt, hvilke begrensninger løsningen har, og hvem som kan ta den videre. Det gjør det enklere å prioritere senere endringer og samarbeide med andre leverandører.",
        "Vi kan fortsette med avtalt vedlikehold eller videreutvikling når det er nyttig. Arbeidsformen skal passe organisasjonen og gi dere et tydelig eierskap til det som leveres.",
      ],
    },
    seo: consultancySeo,
  },
  en: {
    cta: {
      title: "What do you need to move forward with?",
      body: "Tell us about the solution, the team and the challenge. We can start from a specific assignment or help you clarify the next step.",
    },
    editorial: {
      faqTitle: "Questions about advisory and consulting assignments.",
      introAside:
        "Good advice should make the next decision easier to take – and possible to carry out.",
      narrativeTitle: "Clear responsibilities from the first working session.",
      narrative: [
        "We start with conversations and a review of what is already in place: goals, systems, ways of working and known obstacles. That makes it easier to separate a technical problem from one that is really about content, priorities or responsibilities.",
        "Then we propose a defined assignment. That could be a decision basis, a technical investigation, further development of a solution or a consultant working alongside your team.",
        "You get visible progress and the chance to assess the work as it is delivered. New needs are handled as deliberate choices about scope and priority.",
      ],
      reflectionTitle: "Expertise that stays with you.",
    },
    faqs: consultancyFaqsEn,
    hero: {
      imageSrc: "/images/services/consultancy-advisory-session.jpg",
      imageAlt:
        "Two professionals discussing a digital solution at a laptop.",
      title: ["Advice", "that leads", "to action."],
      summary:
        "We offer digital advisory and IT consulting in web, e-commerce, integrations and AI. From scoping and technology choices to development alongside your team.",
    },
    intro: {
      kicker: "Advisory and consulting",
      title: "The right expertise for the task at hand.",
      body: [
        "Some organisations need an independent perspective before an investment. Others have a clear direction but lack the time or expertise to carry it out. We tailor the assignment to where you are and what you already have in-house.",
        "We can help assess an existing website or online store, plan system integrations, scope an AI pilot or further develop a digital solution. Our recommendations take operations and long-term management into account, as well as what needs to be built now.",
        "Assignments can be delivered as a defined assessment, project support or development in collaboration with your team. Skills needs, capacity and ways of working are agreed before we start.",
      ],
    },
    label: "Services / Advisory and consulting",
    reflection: {
      label: "Choices you can understand",
      body: [
        "A solution should remain understandable after the consulting assignment has ended. That is why we plan documentation and knowledge transfer as part of the work.",
        "You should know why a choice was made, what limitations the solution has and who can take it further. That makes it easier to prioritise later changes and to work with other vendors.",
        "We can continue with agreed maintenance or further development when that is useful. The way we work should suit your organisation and give you clear ownership of what is delivered.",
      ],
    },
    seo: consultancySeoEn,
  },
};

export function ConsultancyPage({ locale }: { locale: Locale }) {
  return (
    <ServiceArticlePage {...consultancyContent[locale]} locale={locale} />
  );
}
