import type { Locale, Localized } from "@/lib/i18n/locales";
import { ServiceArticlePage } from "./service-article-page";
import type { ServiceArticleContent } from "./service-article-content";
import { aiAutomationFaqs, aiAutomationFaqsEn } from "./ai-automation-page-data";
import { aiAutomationSeo } from "./service-seo";
import { aiAutomationSeoEn } from "./service-seo-en";
import { AiWorkflowExample } from "./ai-workflow-example";

const aiAutomationContent: Localized<ServiceArticleContent> = {
  nb: {
    cta: {
      title: "Hva kunne teamet brukt mindre tid på?",
      body: "Fortell oss om oppgaven dere vil løse. Sammen finner vi ut om KI, enkel automatisering eller en annen løsning gir mest verdi.",
    },
    editorial: {
      faqTitle: "To spørsmål vi ofte får om KI i arbeidsflyten.",
      introAside:
        "God KI begynner med et tydelig behov, tilgjengelig kunnskap og mennesker som vet når de skal ta over.",
      narrative: [
        "Vi begynner med å forstå arbeidet: hvilke spørsmål går igjen, hvor ligger kunnskapen, og hva tar unødvendig tid? Da kan vi velge en oppgave med tydelig nytte før vi velger teknologi.",
        "Deretter lager vi en avgrenset løsning og tester den med ekte eksempler. Vi ser på kildene, vurderer kvaliteten på svarene og bestemmer hvilke steg som skal kreve menneskelig kontroll.",
        "Når arbeidsflyten fungerer, kan vi koble den til eksisterende systemer der det er nødvendig. Da bygger vi videre på et forståelig grunnlag – ikke en svart boks som ingen tør å bruke.",
      ],
      narrativeTitle: "Først oppgaven. Så teknologien.",
      reflectionTitle: "Mer tid til det som trenger mennesker.",
    },
    faqs: aiAutomationFaqs,
    hero: {
      imageSrc: "/images/AI-automation.jpg",
      imageAlt:
        "En robotarm rekker en kaffekopp til en mann som leser i en lenestol, mens ikoner viser oppgaver som håndteres automatisk.",
      imagePosition: "50% 45%",
      title: ["KI som", "gjør arbeid", "enklere."],
      summary:
        "Vi bygger KI-assistenter og automatiserte arbeidsflyter rundt konkrete oppgaver – med virksomhetens kunnskap og mennesker i sentrum.",
    },
    intro: {
      kicker: "Praktisk KI",
      title: "Teknologi som hjelper i hverdagen.",
      body: [
        "Generativ KI, også kalt AI, kan gjøre det raskere å finne informasjon, foreslå svar og starte på oppgaver som ellers stjeler tid. AI-trender endrer seg raskt, men en ny modell er ikke målet i seg selv. Målet er en arbeidsflyt som blir enklere og mer pålitelig.",
        "Det kan være en assistent som finner svar i virksomhetens egen kunnskap, eller en prosess som sorterer henvendelser og foreslår neste steg. Vi velger hvor det er nyttig å automatisere, og hvor en kollega skal vurdere resultatet.",
        "Våre egne produkter Agenci og Verevon viser hvordan KI kan brukes til henholdsvis samtaler på nettstedet og kunnskapsarbeid med kilder, svar og godkjenning. I et nytt prosjekt tar vi utgangspunkt i deres behov og eksisterende systemer.",
      ],
    },
    label: "Tjenester / KI og automatisering",
    reflection: {
      label: "Mennesker i sentrum",
      body: [
        "En god KI-løsning gjør det lettere å bruke det virksomheten allerede vet. Den kan hjelpe ansatte og kunder med å finne frem, uten at alle må lete i de samme dokumentene hver gang.",
        "Automatisering kan ta hånd om rutinen rundt arbeidet: motta en henvendelse, hente relevant informasjon, foreslå et svar og sende saken videre til riktig person.",
        "Vi skiller mellom KI som tolker eller foreslår, og integrasjoner som flytter data mellom systemer. Ofte virker de best sammen, men de løser ulike problemer.",
        "Mennesker skal fortsatt kunne se grunnlaget, rette feil og ta beslutninger der det betyr noe.",
      ],
    },
    seo: aiAutomationSeo,
  },
  en: {
    cta: {
      title: "What could your team spend less time on?",
      body: "Tell us about the task you want to solve. Together, we’ll find out whether AI, simple automation or another solution adds the most value.",
    },
    editorial: {
      faqTitle: "Two questions we often hear about AI in the workflow.",
      introAside:
        "Good AI starts with a clear need, accessible knowledge and people who know when to take over.",
      narrative: [
        "We start by understanding the work: which questions keep coming up, where does the knowledge live, and what takes unnecessary time? That lets us choose a task with clear value before we choose the technology.",
        "Then we build a defined solution and test it with real examples. We look at the sources, assess the quality of the answers and decide which steps should require human review.",
        "Once the workflow works, we can connect it to existing systems where needed. That way, we build on a foundation people understand – not a black box nobody dares to use.",
      ],
      narrativeTitle: "The task first. Then the technology.",
      reflectionTitle: "More time for what needs people.",
    },
    faqs: aiAutomationFaqsEn,
    hero: {
      imageSrc: "/images/AI-automation.jpg",
      imageAlt:
        "A robotic arm hands a cup of coffee to a man reading in an armchair, while icons show tasks being handled automatically.",
      imagePosition: "50% 45%",
      title: ["AI that", "makes work", "easier."],
      summary:
        "We build AI assistants and automated workflows around concrete tasks – with your organisation’s knowledge and people at the centre.",
    },
    intro: {
      kicker: "Practical AI",
      title: "Technology that helps in everyday work.",
      body: [
        "Generative AI can make it faster to find information, suggest answers and get started on tasks that would otherwise eat up time. AI trends change quickly, but a new model is not a goal in itself. The goal is a workflow that becomes simpler and more reliable.",
        "That might be an assistant that finds answers in your organisation’s own knowledge, or a process that sorts incoming enquiries and suggests the next step. We choose where automation is useful, and where a colleague should assess the result.",
        "Our own products, Agenci and Verevon, show how AI can be used for website conversations and for knowledge work with sources, answers and approval, respectively. In a new project, we start from your needs and existing systems.",
      ],
    },
    label: "Services / AI and automation",
    reflection: {
      label: "People at the centre",
      body: [
        "A good AI solution makes it easier to use what your organisation already knows. It can help employees and customers find their way, without everyone having to search through the same documents every time.",
        "Automation can take care of the routine around the work: receiving an enquiry, fetching relevant information, suggesting a reply and passing the case on to the right person.",
        "We distinguish between AI that interprets or suggests, and integrations that move data between systems. They often work best together, but they solve different problems.",
        "People should still be able to see the basis for an answer, correct mistakes and make decisions where it matters.",
      ],
    },
    seo: aiAutomationSeoEn,
  },
};

export function AiAutomationPage({ locale }: { locale: Locale }) {
  return (
    <ServiceArticlePage {...aiAutomationContent[locale]} locale={locale}>
      <AiWorkflowExample locale={locale} />
    </ServiceArticlePage>
  );
}
