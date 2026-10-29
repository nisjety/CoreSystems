import type { Locale, Localized } from "@/lib/i18n/locales";
import { ServiceArticlePage } from "./service-article-page";
import type { ServiceArticleContent } from "./service-article-content";
import { webSolutionFaqs, webSolutionFaqsEn } from "./web-solutions-page-data";
import { webSolutionsSeo } from "./service-seo";
import { webSolutionsSeoEn } from "./service-seo-en";

const webSolutionsContent: Localized<ServiceArticleContent> = {
  nb: {
    cta: {
      title: "Har dere en nettside som bør gjøre mer?",
      body: "Fortell oss hvor dere står i dag og hva dere ønsker å få til. Så tar vi en prat om hvordan vi kan hjelpe dere videre.",
    },
    editorial: {
      faqTitle: "To spørsmål vi ofte får før vi setter i gang.",
      introAside:
        "En nettside virker best når den gjør riktig neste steg enkelt for de rette menneskene.",
      narrative: [
        "Før vi designer eller bygger noe, setter vi oss inn i bedriften, kundene og hva dere ønsker å oppnå. Det gir oss et godt grunnlag for å finne riktig struktur, innhold og uttrykk.",
        "Deretter designer og utvikler vi nettsiden. Dere er med gjennom hele prosessen, ser fremdriften og kan komme med tilbakemeldinger underveis.",
        "Før lansering tester vi at alt fungerer som det skal på mobil, nettbrett og PC.",
      ],
      narrativeTitle: "Vi starter med å forstå hva dere trenger.",
      reflectionTitle: "En nettside skal fungere i dag – og være klar for i morgen.",
    },
    faqs: webSolutionFaqs,
    hero: {
      imagePosition: "50% 0%",
      imageSrc: "/images/Triodelab-bilder/JackStaende.jpg",
      imageAlt:
        "En person med caps står bøyd over en bærbar datamaskin ved en høy pult.",
      title: ["Nettsider", "for bedrifter", "som virker."],
      summary:
        "Vi lager nettsider som gjør det enkelt å forstå hvem dere er, hva dere tilbyr og hvorfor kunden skal velge dere.",
    },
    intro: {
      kicker: "Et godt førsteinntrykk",
      title: "Nettsiden skal gjøre mer enn å se bra ut.",
      body: [
        "For mange er nettsiden det første møtet med bedriften. Da må den raskt gi svar på det kunden lurer på, skape tillit og gjøre veien videre enkel.",
        "Vi kombinerer design og utvikling for å lage en nettside som passer bedriften og målene deres. Dere trenger ikke vite hvilken teknologi dere trenger eller ha alle svarene klare på forhånd. Det finner vi ut av sammen.",
        "Resultatet skal være en nettside som representerer dere godt – og som faktisk gjør jobben den er laget for.",
      ],
    },
    label: "Tjenester / Webløsninger",
    reflection: {
      label: "Bygget for veien videre",
      body: [
        "Bedrifter utvikler seg, og nettsiden bør kunne gjøre det samme. Nye tjenester kommer til, innhold endres og behovene kan se annerledes ut om ett år enn de gjør i dag.",
        "Derfor bygger vi løsninger som er enkle å oppdatere og utvikle videre. Dere skal ikke måtte starte på nytt hver gang bedriften tar et nytt steg.",
        "Trenger dere hjelp etter lansering, kan vi også ta oss av drift, vedlikehold og videreutvikling.",
      ],
    },
    seo: webSolutionsSeo,
  },
  en: {
    cta: {
      title: "Does your website need to do more?",
      body: "Tell us where you are today and what you want to achieve. Then let’s talk about how we can help you move forward.",
    },
    editorial: {
      faqTitle: "Two questions we often hear before we get started.",
      introAside:
        "A website works best when it makes the right next step easy for the right people.",
      narrative: [
        "Before we design or build anything, we get to know your business, your customers and what you want to achieve. That gives us a solid basis for finding the right structure, content and look.",
        "Then we design and develop the website. You are involved throughout, can follow the progress and give feedback along the way.",
        "Before launch, we test that everything works as it should on mobile, tablet and desktop.",
      ],
      narrativeTitle: "We start by understanding what you need.",
      reflectionTitle: "A website should work today – and be ready for tomorrow.",
    },
    faqs: webSolutionFaqsEn,
    hero: {
      imagePosition: "50% 0%",
      imageSrc: "/images/Triodelab-bilder/JackStaende.jpg",
      imageAlt:
        "A person in a cap leans over a laptop at a standing desk.",
      title: ["Websites", "for businesses", "that work."],
      summary:
        "We build websites that make it easy to understand who you are, what you offer and why customers should choose you.",
    },
    intro: {
      kicker: "A strong first impression",
      title: "A website should do more than look good.",
      body: [
        "For many people, your website is their first encounter with your business. It has to answer their questions quickly, build trust and make the next step easy.",
        "We combine design and development to create a website that fits your business and your goals. You don’t need to know which technology you need or have every answer ready in advance. We work that out together.",
        "The result should be a website that represents you well – and actually does the job it was built for.",
      ],
    },
    label: "Services / Web solutions",
    reflection: {
      label: "Built for what comes next",
      body: [
        "Businesses evolve, and your website should be able to evolve with them. New services are added, content changes and your needs may look different a year from now.",
        "That is why we build solutions that are easy to update and develop further. You shouldn’t have to start over every time your business takes a new step.",
        "If you need help after launch, we can also take care of hosting, maintenance and further development.",
      ],
    },
    seo: webSolutionsSeoEn,
  },
};

export function WebSolutionsPage({ locale }: { locale: Locale }) {
  return (
    <ServiceArticlePage {...webSolutionsContent[locale]} locale={locale} />
  );
}
