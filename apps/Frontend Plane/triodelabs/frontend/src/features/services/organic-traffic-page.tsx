import type { Locale, Localized } from "@/lib/i18n/locales";
import { ServiceArticlePage } from "./service-article-page";
import type { ServiceArticleContent } from "./service-article-content";
import { organicTrafficFaqs, organicTrafficFaqsEn } from "./organic-traffic-page-data";
import { organicTrafficSeo } from "./service-seo";
import { organicTrafficSeoEn } from "./service-seo-en";

const organicTrafficContent: Localized<ServiceArticleContent> = {
  nb: {
    cta: {
      title: "Vil dere bli funnet når kundene søker?",
      body: "Fortell oss hva dere tilbyr og hvem dere ønsker å nå. Så ser vi på hvor dere står i dag og hvor det finnes muligheter for bedre synlighet.",
    },
    editorial: {
      faqTitle: "To spørsmål vi ofte får om SEO.",
      introAside:
        "Synlighet skjer når siden svarer godt på det folk faktisk leter etter.",
      narrative: [
        "Vi starter med å forstå bedriften, kundene og hva dere ønsker å bli funnet for. Deretter undersøker vi hvordan folk søker, hva konkurrentene gjør og hvor dere har mulighet til å ta mer plass.",
        "Så jobber vi med nettsiden. Det kan være nye landingssider, bedre innhold, tydeligere struktur, internlenker eller tekniske forbedringer som gjør det enklere for Google å forstå og indeksere innholdet.",
        "Vi prioriterer det som gir mest mening for bedriften, fremfor å gjøre SEO mer komplisert enn det trenger å være.",
      ],
      narrativeTitle: "Vi bygger synlighet på det kundene faktisk søker etter.",
      reflectionTitle: "God synlighet bygges over tid.",
    },
    faqs: organicTrafficFaqs,
    hero: {
      imageSrc: "/images/Triodelab-bilder/Imatrapper.jpg",
      imageAlt:
        "En person med rød caps og laptop går opp en trapp, sett ovenfra.",
      title: ["Bli funnet av", "de som leter", "etter det dere tilbyr."],
      summary:
        "Vi hjelper dere med å bli mer synlige i Google gjennom SEO, godt innhold og en nettside som er bygget for å bli funnet.",
    },
    intro: {
      kicker: "Søkemotoroptimalisering",
      title: "Vær synlig når kundene faktisk leter.",
      body: [
        "Hver dag søker potensielle kunder etter produkter, tjenester og svar på problemer dere kan løse. SEO handler om å gjøre det enklere for dem å finne akkurat dere når behovet oppstår.",
        "Vi finner ut hva kundene deres søker etter, hvilke spørsmål de stiller og hvor det finnes muligheter for bedre synlighet. Derfra forbedrer vi innholdet, strukturen og det tekniske på nettsiden.",
        "Målet er ikke bare mer trafikk. Det er å få flere av de riktige menneskene inn på nettsiden.",
      ],
    },
    label: "Tjenester / Organisk trafikk",
    reflection: {
      label: "Måling og forbedring",
      body: [
        "SEO er ikke noe man gjør én gang og legger bort. Søkemønstre endrer seg, konkurrenter utvikler seg og nye muligheter dukker opp.",
        "Derfor følger vi med på hvilke søk dere blir funnet på, hvilke sider som får trafikk og hvor det finnes rom for forbedring. Det gir oss et bedre grunnlag for hva vi bør prioritere videre.",
        "Over tid bygger dere opp synlighet og innhold som fortsetter å skape trafikk – uten at dere betaler for hvert eneste klikk.",
      ],
    },
    seo: organicTrafficSeo,
  },
  en: {
    cta: {
      title: "Want to be found when customers search?",
      body: "Tell us what you offer and who you want to reach. We’ll look at where you stand today and where there are opportunities for better visibility.",
    },
    editorial: {
      faqTitle: "Two questions we often hear about SEO.",
      introAside:
        "Visibility comes when a page answers what people are actually looking for.",
      narrative: [
        "We start by understanding your business, your customers and what you want to be found for. Then we look at how people search, what your competitors are doing and where you have room to claim more space.",
        "Then we work on the website. That might mean new landing pages, better content, a clearer structure, internal links or technical improvements that make it easier for Google to understand and index your content.",
        "We prioritise what makes the most sense for your business, rather than making SEO more complicated than it needs to be.",
      ],
      narrativeTitle: "We build visibility on what customers actually search for.",
      reflectionTitle: "Good visibility is built over time.",
    },
    faqs: organicTrafficFaqsEn,
    hero: {
      imageSrc: "/images/Triodelab-bilder/Imatrapper.jpg",
      imageAlt:
        "A person in a red cap carrying a laptop walks up a staircase, seen from above.",
      title: ["Get found by", "the people looking", "for what you offer."],
      summary:
        "We help you become more visible on Google through SEO, strong content and a website that is built to be found.",
    },
    intro: {
      kicker: "Search engine optimisation",
      title: "Be visible when customers are actually looking.",
      body: [
        "Every day, potential customers search for products, services and answers to problems you can solve. SEO is about making it easier for them to find you at the moment the need arises.",
        "We find out what your customers search for, which questions they ask and where there are opportunities for better visibility. From there, we improve the content, structure and technical side of your website.",
        "The goal isn’t just more traffic. It is getting more of the right people to your website.",
      ],
    },
    label: "Services / Organic traffic",
    reflection: {
      label: "Measurement and improvement",
      body: [
        "SEO isn’t something you do once and put aside. Search behaviour changes, competitors evolve and new opportunities appear.",
        "That is why we keep track of which searches you are found for, which pages get traffic and where there is room for improvement. It gives us a better basis for deciding what to prioritise next.",
        "Over time, you build up visibility and content that keep generating traffic – without paying for every single click.",
      ],
    },
    seo: organicTrafficSeoEn,
  },
};

export function OrganicTrafficPage({ locale }: { locale: Locale }) {
  return (
    <ServiceArticlePage {...organicTrafficContent[locale]} locale={locale} />
  );
}
