import type { ServiceFaq } from "./service-faq";

export const aiAutomationFaqs: readonly ServiceFaq[] = [
  {
    question: "Hva kan KI og automatisering brukes til i en bedrift?",
    answer:
      "KI kan hjelpe med å finne svar i egen kunnskap, lage utkast og sortere henvendelser. Automatisering kan flytte en oppgave videre i en arbeidsflyt når bestemte vilkår er oppfylt. Vi starter med én konkret oppgave, tester nytten og bestemmer hvor mennesker skal kontrollere resultatet.",
  },
  {
    question: "Hvordan beholder vi kontrollen over data og svar?",
    answer:
      "Vi kartlegger hvilke kilder løsningen skal bruke, hvem som trenger tilgang, og hvilke svar eller handlinger som må godkjennes av et menneske. Deretter tester vi løsningen på reelle spørsmål og følger opp feil og usikkerhet. En KI-assistent skal støtte arbeidet, ikke skjule hvordan beslutninger tas.",
  },
];

export const aiAutomationFaqsEn: readonly ServiceFaq[] = [
  {
    question: "What can AI and automation be used for in a business?",
    answer:
      "AI can help find answers in your own knowledge, write drafts and sort incoming enquiries. Automation can move a task forward in a workflow when specific conditions are met. We start with one concrete task, test its value and decide where people should check the result.",
  },
  {
    question: "How do we stay in control of data and answers?",
    answer:
      "We map which sources the solution should use, who needs access, and which answers or actions must be approved by a person. We then test the solution on real questions and follow up errors and uncertainty. An AI assistant should support the work, not hide how decisions are made.",
  },
];
