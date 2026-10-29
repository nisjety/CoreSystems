import type { Locale, Localized } from "@/lib/i18n/locales";
import { ServiceArticlePage } from "./service-article-page";
import type { ServiceArticleContent } from "./service-article-content";
import { commerceFaqs, commerceFaqsEn } from "./commerce-page-data";
import { commerceSeo } from "./service-seo";
import { commerceSeoEn } from "./service-seo-en";

const commerceContent: Localized<ServiceArticleContent> = {
  nb: {
    cta: {
      title: "Har dere produkter dere vil selge på nett?",
      body: "Fortell oss hva dere selger og hvor dere står i dag. Så finner vi ut hvordan vi kan bygge en nettbutikk som passer bedriften og kundene deres.",
    },
    editorial: {
      faqTitle: "To spørsmål vi ofte får før vi setter i gang.",
      introAside:
        "En nettbutikk virker best når kjøpet føles enkelt og driften henger med.",
      narrative: [
        "Vi starter med å forstå hva dere selger, hvem kundene er og hvordan dere jobber i dag. Deretter finner vi riktig struktur og lager en tydelig vei fra første besøk til gjennomført kjøp.",
        "Vi jobber med alt fra kategorier og produktsider til handlekurv, betaling og levering. Underveis får dere følge prosessen og komme med tilbakemeldinger.",
        "Før lansering tester vi hele kjøpsreisen på mobil, nettbrett og PC, slik at butikken fungerer som den skal når kundene kommer.",
      ],
      narrativeTitle: "Vi bygger rundt hvordan kundene faktisk handler.",
      reflectionTitle: "Salget slutter ikke når kunden trykker «betal».",
    },
    faqs: commerceFaqs,
    hero: {
      imageSrc: "/images/Triodelab-bilder/JackogAbdi.jpg",
      imageAlt:
        "To personer sett ovenfra arbeider sammen med bærbare datamaskiner og bøker på et bord, i svart-hvitt.",
      title: ["Nettbutikker", "som gjør det", "enklere å kjøpe."],
      summary:
        "Vi bygger nettbutikker som gjør det enkelt å finne riktig produkt, føle seg trygg på valget og fullføre kjøpet.",
    },
    intro: {
      kicker: "Fra besøk til kjøp",
      title: "En god nettbutikk fjerner det som står i veien for salget.",
      body: [
        "Kunden skal raskt finne det de leter etter, forstå produktet og føle seg trygg nok til å kjøpe. Jo flere unødvendige steg og spørsmål som dukker opp underveis, desto lettere er det å miste salget.",
        "Vi bygger nettbutikker rundt produktene, kundene og måten dere driver på. Sammen finner vi riktig plattform og setter opp en løsning som fungerer både for de som handler og de som skal håndtere butikken.",
        "Målet er en nettbutikk som er enkel å handle i, enkel å drive og klar for å vokse.",
      ],
    },
    label: "Tjenester / Nettbutikk",
    reflection: {
      label: "Mer enn selve nettbutikken",
      body: [
        "Bak en god nettbutikk ligger det mye som kunden aldri ser. Betalinger skal registreres, ordre håndteres, varer sendes og informasjon holdes oppdatert.",
        "Derfor ser vi også på hvordan nettbutikken passer inn i resten av driften. Ved behov kan vi koble den mot systemene dere allerede bruker og automatisere oppgaver som ellers må gjøres manuelt.",
        "Resultatet er mindre dobbeltarbeid og en nettbutikk som er enklere å håndtere når salget øker.",
      ],
    },
    seo: commerceSeo,
  },
  en: {
    cta: {
      title: "Do you have products you want to sell online?",
      body: "Tell us what you sell and where you are today. Then we’ll work out how to build an online store that suits your business and your customers.",
    },
    editorial: {
      faqTitle: "Two questions we often hear before we get started.",
      introAside:
        "An online store works best when buying feels simple and operations keep up.",
      narrative: [
        "We start by understanding what you sell, who your customers are and how you work today. Then we find the right structure and create a clear path from the first visit to a completed purchase.",
        "We work on everything from categories and product pages to the basket, payment and delivery. Along the way, you can follow the process and give feedback.",
        "Before launch, we test the entire buying journey on mobile, tablet and desktop, so the store works as it should when customers arrive.",
      ],
      narrativeTitle: "We build around how your customers actually shop.",
      reflectionTitle: "A sale doesn’t end when the customer clicks “pay”.",
    },
    faqs: commerceFaqsEn,
    hero: {
      imageSrc: "/images/Triodelab-bilder/JackogAbdi.jpg",
      imageAlt:
        "Two people seen from above working together with laptops and books at a table, in black and white.",
      title: ["Online stores", "that make", "buying easier."],
      summary:
        "We build online stores that make it easy to find the right product, feel confident in the choice and complete the purchase.",
    },
    intro: {
      kicker: "From visit to purchase",
      title: "A good online store removes what stands in the way of a sale.",
      body: [
        "Customers should quickly find what they are looking for, understand the product and feel confident enough to buy. The more unnecessary steps and questions that come up along the way, the easier it is to lose the sale.",
        "We build online stores around your products, your customers and the way you run your business. Together, we find the right platform and set up a solution that works both for the people who shop and the people who run the store.",
        "The goal is an online store that is easy to shop in, easy to run and ready to grow.",
      ],
    },
    label: "Services / E-commerce",
    reflection: {
      label: "More than the store itself",
      body: [
        "Behind a good online store lies a lot that customers never see. Payments must be recorded, orders handled, goods shipped and information kept up to date.",
        "That is why we also look at how the store fits into the rest of your operations. Where needed, we can connect it to the systems you already use and automate tasks that would otherwise be done by hand.",
        "The result is less duplicate work and an online store that is easier to manage as sales grow.",
      ],
    },
    seo: commerceSeoEn,
  },
};

export function CommercePage({ locale }: { locale: Locale }) {
  return (
    <ServiceArticlePage {...commerceContent[locale]} locale={locale} />
  );
}
