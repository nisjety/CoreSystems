import type { Locale, Localized } from "@/lib/i18n/locales";
import { ServiceArticlePage } from "./service-article-page";
import type { ServiceArticleContent } from "./service-article-content";
import { paidAdsFaqs, paidAdsFaqsEn } from "./paid-ads-page-data";
import { paidAdsSeo } from "./service-seo";
import { paidAdsSeoEn } from "./service-seo-en";

const paidAdsContent: Localized<ServiceArticleContent> = {
  nb: {
    cta: {
      title: "Vil dere få mer ut av annonsebudsjettet?",
      body: "Fortell oss hva dere ønsker å oppnå og hvem dere vil nå. Så ser vi på hvordan Google Ads, Meta Ads eller en kombinasjon kan brukes for å komme dit.",
    },
    editorial: {
      faqTitle: "To spørsmål vi ofte får om annonsering.",
      introAside:
        "God annonsering starter med et tydelig tilbud, ikke bare et større budsjett.",
      narrative: [
        "Flere henvendelser, mer salg eller økt synlighet? Før vi setter opp annonser, må vi vite hva et godt resultat faktisk betyr for bedriften deres.",
        "Deretter finner vi kanalene, målgruppene og budskapene som passer målet. Google Ads lar oss nå mennesker som allerede søker etter det dere tilbyr, mens Meta Ads gir oss muligheten til å nå relevante målgrupper på Facebook og Instagram.",
        "Vi setter opp kampanjene, sørger for riktig sporing og følger med på resultatene fra start.",
      ],
      narrativeTitle: "Vi starter med hva dere ønsker å oppnå.",
      reflectionTitle: "Vi bruker resultatene til å gjøre annonseringen bedre.",
    },
    faqs: paidAdsFaqs,
    hero: {
      imageSrc: "/images/Google-meta-Ads.jpg",
      imageAlt:
        "En bærbar datamaskin viser et TriodeLab-dashboard med resultater fra Google Ads og Meta Ads.",
      imagePosition: "50% 100%",
      title: ["Annonser som", "når de riktige", "menneskene."],
      summary:
        "Vi hjelper dere med Google Ads og Meta Ads for å nå potensielle kunder, skape resultater og få mer ut av annonsebudsjettet.",
    },
    intro: {
      kicker: "Betalt annonsering",
      title: "Det handler ikke om flest klikk, men de riktige.",
      body: [
        "Det er enkelt å bruke penger på annonser. Det vanskeligere er å sørge for at de faktisk når mennesker som kan bli kunder.",
        "Vi hjelper dere med strategi, budskap, målgrupper og oppsett av kampanjer på Google, Facebook og Instagram. Samtidig sørger vi for at det som møter kunden etter klikket henger sammen med annonsen.",
        "Målet er å bruke budsjettet der det har størst mulighet til å skape verdi.",
      ],
    },
    label: "Tjenester / Google + Meta Ads",
    reflection: {
      label: "Måling og forbedring",
      body: [
        "Når kampanjene er i gang, ser vi på mer enn hvor mange som har sett eller klikket på en annonse. Vi følger med på hva som faktisk skjer etter klikket – som henvendelser, kjøp og andre handlinger som betyr noe for bedriften.",
        "Det gir oss et bedre grunnlag for å se hva som fungerer, hva som bør endres og hvor budsjettet bør brukes.",
        "Vi tester og forbedrer kampanjene underveis, slik at beslutningene bygger på faktiske resultater fremfor antakelser.",
      ],
    },
    seo: paidAdsSeo,
  },
  en: {
    cta: {
      title: "Want to get more out of your ad budget?",
      body: "Tell us what you want to achieve and who you want to reach. We’ll look at how Google Ads, Meta Ads or a combination of both can get you there.",
    },
    editorial: {
      faqTitle: "Two questions we often hear about advertising.",
      introAside:
        "Good advertising starts with a clear offer, not just a bigger budget.",
      narrative: [
        "More enquiries, more sales or greater visibility? Before we set up any ads, we need to know what a good result actually means for your business.",
        "Then we find the channels, audiences and messages that fit the goal. Google Ads lets us reach people who are already searching for what you offer, while Meta Ads lets us reach relevant audiences on Facebook and Instagram.",
        "We set up the campaigns, make sure tracking is in place and follow the results from day one.",
      ],
      narrativeTitle: "We start with what you want to achieve.",
      reflectionTitle: "We use the results to make your advertising better.",
    },
    faqs: paidAdsFaqsEn,
    hero: {
      imageSrc: "/images/Google-meta-Ads.jpg",
      imageAlt:
        "A laptop showing a TriodeLab dashboard with results from Google Ads and Meta Ads.",
      imagePosition: "50% 100%",
      title: ["Ads that", "reach the right", "people."],
      summary:
        "We help you with Google Ads and Meta Ads to reach potential customers, create results and get more out of your ad budget.",
    },
    intro: {
      kicker: "Paid advertising",
      title: "It’s not about the most clicks, but the right ones.",
      body: [
        "Spending money on ads is easy. The harder part is making sure they actually reach people who could become customers.",
        "We help you with strategy, messaging, audiences and campaign setup on Google, Facebook and Instagram. At the same time, we make sure that what customers see after the click matches the ad.",
        "The goal is to spend your budget where it has the greatest chance of creating value.",
      ],
    },
    label: "Services / Google + Meta Ads",
    reflection: {
      label: "Measurement and improvement",
      body: [
        "Once the campaigns are running, we look at more than how many people have seen or clicked an ad. We follow what actually happens after the click – such as enquiries, purchases and other actions that matter to your business.",
        "That gives us a better basis for seeing what works, what should change and where the budget should go.",
        "We test and improve the campaigns as we go, so decisions are based on actual results rather than assumptions.",
      ],
    },
    seo: paidAdsSeoEn,
  },
};

export function PaidAdsPage({ locale }: { locale: Locale }) {
  return (
    <ServiceArticlePage {...paidAdsContent[locale]} locale={locale} />
  );
}
