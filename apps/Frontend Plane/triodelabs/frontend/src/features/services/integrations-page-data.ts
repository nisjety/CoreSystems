import type { ServiceFaq } from "./service-faq";

export const integrationsFaqs: readonly ServiceFaq[] = [
  {
    question: "Hva er en systemintegrasjon?",
    answer:
      "En systemintegrasjon gjør det mulig for to eller flere systemer å utveksle informasjon automatisk. Det kan for eksempel være at en ordre fra nettbutikken sendes videre til lager eller regnskap, uten at noen må registrere informasjonen på nytt.",
  },
  {
    question: "Må vi bytte systemene vi allerede bruker?",
    answer:
      "Som regel ikke. Vi starter med å se på systemene dere allerede har og undersøker hvordan de kan kobles sammen. Hvis dagens løsninger fungerer godt, bygger vi videre på dem. Hvis noe begrenser det dere ønsker å få til, sier vi fra og ser på alternativene sammen.",
  },
];

export const integrationsFaqsEn: readonly ServiceFaq[] = [
  {
    question: "What is a system integration?",
    answer:
      "A system integration lets two or more systems exchange information automatically. For example, an order from your online store can be passed on to stock management or accounting, without anyone having to enter the information again.",
  },
  {
    question: "Do we have to replace the systems we already use?",
    answer:
      "Usually not. We start by looking at the systems you already have and explore how they can be connected. If your current solutions work well, we build on them. If something limits what you want to achieve, we tell you and look at the alternatives together.",
  },
];
