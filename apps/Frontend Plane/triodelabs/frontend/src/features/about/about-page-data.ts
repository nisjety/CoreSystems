import type { Localized } from "../../lib/i18n/locales";

export const aboutPrinciples = [
  {
    number: "01",
    title: "Nysgjerrighet",
    description:
      "Vi undersøker før vi konkluderer, og lar gode spørsmål åpne nye muligheter.",
  },
  {
    number: "02",
    title: "Nærhet",
    description:
      "Vi jobber tett nok på til å forstå nyansene og ta ansvar for helheten.",
  },
  {
    number: "03",
    title: "Mot",
    description: "Vi tør å velge bort, prøve nytt og stå for det vi tror på.",
  },
] as const;

export const aboutMethod = [
  [
    "01",
    "Lytte",
    "Vi setter av tid til menneskene, historien og hverdagen bak oppgaven.",
  ],
  [
    "02",
    "Samskape",
    "Vi deler tidlige tanker, gjør ideer konkrete og bygger tillit i rommet.",
  ],
  [
    "03",
    "Lære",
    "Vi tar med oss det vi oppdager og lar erfaring forme neste kapittel.",
  ],
] as const;

export const aboutPrinciplesEn = [
  {
    number: "01",
    title: "Curiosity",
    description:
      "We investigate before we conclude, and let good questions open up new possibilities.",
  },
  {
    number: "02",
    title: "Closeness",
    description:
      "We work closely enough to understand the nuances and take responsibility for the whole.",
  },
  {
    number: "03",
    title: "Courage",
    description:
      "We dare to leave things out, try something new and stand by what we believe in.",
  },
] as const;

export const aboutMethodEn = [
  [
    "01",
    "Listen",
    "We make time for the people, the history and the everyday reality behind the task.",
  ],
  [
    "02",
    "Co-create",
    "We share early thinking, make ideas concrete and build trust in the room.",
  ],
  [
    "03",
    "Learn",
    "We take what we discover with us and let experience shape the next chapter.",
  ],
] as const;

type AboutContent = {
  cta: {
    body: string;
    eyebrow: string;
    headingLines: readonly [string, string];
    link: string;
  };
  hero: {
    imageAlt: string;
    location: string;
    title: string;
    titleParts: readonly [string, string];
  };
  method: {
    eyebrow: string;
    heading: string;
    steps: readonly (readonly [string, string, string])[];
  };
  principles: {
    body: string;
    eyebrow: string;
    headingLines: readonly [string, string];
    items: readonly { description: string; number: string; title: string }[];
  };
};

export const aboutContent: Localized<AboutContent> = {
  nb: {
    hero: {
      title: "Om oss",
      titleParts: ["Om", "oss."],
      imageAlt:
        "De tre medlemmene i TriodeLab samarbeider rundt et bord med bærbare datamaskiner",
      location: "Oslo / Norge",
    },
    principles: {
      eyebrow: "Det vi tar med oss",
      headingLines: ["Mennesker først.", "Arbeid med mening."],
      body: "Vi er et lite, uavhengig studio som tror på nysgjerrighet, omtanke og ideer som får tid til å bli gode.",
      items: aboutPrinciples,
    },
    method: {
      eyebrow: "Slik blir vi klokere",
      heading: "Vi lar arbeidet utvikle seg.",
      steps: aboutMethod,
    },
    cta: {
      eyebrow: "Neste steg",
      headingLines: ["La oss begynne", "med det som betyr noe."],
      body: "En god første prat trenger ikke være ferdig formulert.",
      link: "Bli kjent",
    },
  },
  en: {
    hero: {
      title: "About us",
      titleParts: ["About", "us."],
      imageAlt:
        "The three members of TriodeLab working together around a table with laptops",
      location: "Oslo / Norway",
    },
    principles: {
      eyebrow: "What we bring",
      headingLines: ["People first.", "Work that matters."],
      body: "We are a small, independent studio that believes in curiosity, care and ideas that are given time to become good.",
      items: aboutPrinciplesEn,
    },
    method: {
      eyebrow: "How we grow wiser",
      heading: "We let the work evolve.",
      steps: aboutMethodEn,
    },
    cta: {
      eyebrow: "Next step",
      headingLines: ["Let’s start", "with what matters."],
      body: "A good first conversation doesn’t need to be fully formed.",
      link: "Get to know us",
    },
  },
};
