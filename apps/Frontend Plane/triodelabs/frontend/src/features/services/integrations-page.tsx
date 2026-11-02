import type { Locale, Localized } from "@/lib/i18n/locales";
import { ServiceArticlePage } from "./service-article-page";
import type { ServiceArticleContent } from "./service-article-content";
import { integrationsFaqs, integrationsFaqsEn } from "./integrations-page-data";
import { integrationsSeo } from "./service-seo";
import { integrationsSeoEn } from "./service-seo-en";

const integrationsContent: Localized<ServiceArticleContent> = {
  nb: {
    cta: {
      title: "Bruker dere tid på arbeid systemene kunne gjort selv?",
      body: "Vis oss hvordan dere jobber i dag. Så ser vi på hva som kan kobles sammen, automatiseres og gjøres enklere.",
    },
    editorial: {
      faqTitle: "To spørsmål vi ofte får om integrasjoner.",
      introAside:
        "Gode integrasjoner fjerner dobbeltarbeid uten å gjøre hverdagen mer komplisert.",
      narrative: [
        "Før vi bygger noe, kartlegger vi systemene dere bruker og hvordan informasjon beveger seg mellom dem. Da ser vi raskt hvor det brukes unødvendig tid, og hva som kan automatiseres.",
        "Deretter bygger vi integrasjonen rundt arbeidsflyten deres. Vi tester at data sendes riktig, håndterer situasjoner der noe går galt og dokumenterer løsningen slik at den er forståelig også etter at prosjektet er ferdig.",
        "Dere skal vite hva som skjer i systemene – ikke sitte igjen med en teknisk løsning ingen forstår.",
      ],
      narrativeTitle: "Vi starter med hvordan dere jobber i dag.",
      reflectionTitle: "La systemene ta seg av det manuelle.",
    },
    faqs: integrationsFaqs,
    hero: {
      imageSrc: "/images/Integrasjoner.jpg",
      imageAlt:
        "En hvit buebro med kabler som strekker seg mot en blå himmel.",
      imagePosition: "50% 45%",
      title: ["Systemer", "som snakker", "sammen."],
      summary:
        "Vi bygger integrasjoner som kobler systemene deres sammen, automatiserer informasjonsflyten og reduserer unødvendig manuelt arbeid.",
    },
    intro: {
      kicker: "Koblet sammen",
      title: "Slipp å gjøre den samme jobben flere ganger.",
      body: [
        "Mange bedrifter bruker flere systemer for nettbutikk, økonomi, lager, CRM og andre deler av driften. Problemet oppstår når systemene ikke snakker sammen, og informasjon må kopieres eller registreres manuelt flere steder.",
        "Vi kobler systemene sammen slik at riktig informasjon kommer dit den skal, automatisk. Det kan for eksempel være ordre fra nettbutikken som sendes til regnskapet, lagerbeholdning som oppdateres eller kundeinformasjon som synkroniseres mellom ulike systemer.",
        "Målet er enkelt: mindre dobbeltarbeid, færre manuelle feil og en arbeidsflyt som fungerer bedre.",
      ],
    },
    label: "Tjenester / Integrasjoner",
    reflection: {
      label: "Bedre flyt",
      body: [
        "Når informasjon flyter automatisk mellom systemene, slipper teamet å bruke tid på å flytte de samme opplysningene fra ett sted til et annet.",
        "Det kan bety at ordre, kunder, produkter, lagerstatus eller regnskapsdata oppdateres automatisk der informasjonen trengs.",
        "Når det gir mening, kan vi også koble AI (kunstig intelligens) til arbeidsflyten – for eksempel for å finne svar i egne data eller sortere henvendelser. Gode integrasjoner og kontroll på dataene må fortsatt være på plass.",
        "Vi bygger integrasjoner som kan videreutvikles når bedriften vokser, nye behov dukker opp eller flere systemer skal kobles på.",
      ],
    },
    seo: integrationsSeo,
  },
  en: {
    cta: {
      title: "Spending time on work your systems could do themselves?",
      body: "Show us how you work today. We’ll look at what can be connected, automated and made simpler.",
    },
    editorial: {
      faqTitle: "Two questions we often hear about integrations.",
      introAside:
        "Good integrations remove duplicate work without making everyday work more complicated.",
      narrative: [
        "Before we build anything, we map the systems you use and how information moves between them. That quickly shows us where time is being wasted and what can be automated.",
        "Then we build the integration around your workflow. We test that data is sent correctly, handle situations where something goes wrong and document the solution so it remains understandable after the project is finished.",
        "You should know what is happening in your systems – not be left with a technical solution nobody understands.",
      ],
      narrativeTitle: "We start with how you work today.",
      reflectionTitle: "Let your systems handle the manual work.",
    },
    faqs: integrationsFaqsEn,
    hero: {
      imageSrc: "/images/Integrasjoner.jpg",
      imageAlt:
        "A white arch bridge with cables stretching towards a blue sky.",
      imagePosition: "50% 45%",
      title: ["Systems", "that talk", "to each other."],
      summary:
        "We build integrations that connect your systems, automate the flow of information and reduce unnecessary manual work.",
    },
    intro: {
      kicker: "Connected",
      title: "Stop doing the same job more than once.",
      body: [
        "Many businesses use several systems for their online store, accounting, stock, CRM and other parts of their operations. The problem arises when those systems don’t talk to each other, and information has to be copied or entered by hand in several places.",
        "We connect your systems so the right information reaches the right place, automatically. That could be orders from your online store being sent to accounting, stock levels being updated or customer information being synchronised between systems.",
        "The goal is simple: less duplicate work, fewer manual errors and a workflow that runs better.",
      ],
    },
    label: "Services / Integrations",
    reflection: {
      label: "Better flow",
      body: [
        "When information flows automatically between systems, your team no longer has to spend time moving the same details from one place to another.",
        "That can mean orders, customers, products, stock status or accounting data being updated automatically wherever the information is needed.",
        "Where it makes sense, we can also bring AI (artificial intelligence) into the workflow – for example to find answers in your own data or sort incoming enquiries. Solid integrations and control over your data still need to be in place.",
        "We build integrations that can be developed further as your business grows, new needs arise or more systems need to be connected.",
      ],
    },
    seo: integrationsSeoEn,
  },
};

export function IntegrationsPage({ locale }: { locale: Locale }) {
  return (
    <ServiceArticlePage {...integrationsContent[locale]} locale={locale} />
  );
}
