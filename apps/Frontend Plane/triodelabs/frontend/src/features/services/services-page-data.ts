import type { Localized } from "../../lib/i18n/locales";

export type ServiceStory = {
  annotation: string;
  description: string;
  focus: string;
  href?: string;
  image: string;
  imageAlt: string;
  imagePosition?: string;
  number: string;
  title: string;
};

export const serviceStories: ServiceStory[] = [
  {
    number: "01",
    title: "Webløsninger",
    focus: "Nettsider og digitale produkter som gir mening med en gang.",
    description:
      "Fra effektive markedsnettsider med hosting og drift i Norge til avanserte webapplikasjoner — vi bygger løsninger som passer behovet deres.",
    image: "/images/Triodelab-weblosning.jpg",
    imageAlt: "To hender holder en bærbar datamaskin som viser TriodeLabs nettside.",
    imagePosition: "50% 62%",
    annotation: "Fra første skisse til en løsning som virker.",
    href: "/tjenester/web-losninger",
  },
  {
    number: "02",
    title: "Nettbutikk",
    focus: "Handleopplevelser som er enkle å velge — og enkle å drifte.",
    description:
      "Vi er plattformuavhengige og finner den teknologien som passer hvert prosjekt best — Shopify, WooCommerce eller noe annet.",
    image: "/images/Nettbutikk.jpg",
    imageAlt: "En liten handlevogn med en pakke står foran en bærbar datamaskin.",
    imagePosition: "45% 46%",
    annotation: "Bygget rundt måten dere faktisk selger på.",
    href: "/tjenester/nettbutikk",
  },
  {
    number: "03",
    title: "Organisk trafikk",
    focus: "Kvalitetstrafikk som fortsetter å finne veien til dere.",
    description:
      "Få kvalitetstrafikk fra søkemotorer som Google. Med godt innhold og teknisk SEO bygger dere synlighet som kan fortsette å skape relevante besøk over tid.",
    image: "/images/OrganiskTraffikk.jpg",
    imageAlt: "En person søker på Google på en stasjonær datamaskin.",
    imagePosition: "60% 50%",
    annotation: "Innsikt som gjør det lettere å bli funnet.",
    href: "/tjenester/organisk-trafikk",
  },
  {
    number: "04",
    title: "Integrasjoner",
    focus: "Systemer som snakker sammen, uten ekstra friksjon.",
    description:
      "Integrasjons-Hub kobler Shopify og WooCommerce med Visma, Fiken og Tripletex — robuste, stabile og fleksible integrasjoner for driften deres.",
    image: "/images/Integrasjoner.jpg",
    imageAlt: "En hvit buebro med kabler som strekker seg mot en blå himmel.",
    imagePosition: "50% 52%",
    annotation: "Mindre manuelt arbeid. Mer sammenheng.",
    href: "/tjenester/integrasjoner",
  },
  {
    number: "05",
    title: "Google + Meta Ads",
    focus: "Betalt synlighet med et tydeligere neste steg.",
    description:
      "Vi hjelper dere med Google Ads og Meta Ads, fra budskap og målgrupper til sporing, måling og løpende forbedring.",
    image: "/images/Google-meta-Ads.jpg",
    imageAlt: "En bærbar datamaskin viser et TriodeLab-dashboard med resultater fra Google Ads og Meta Ads.",
    imagePosition: "50% 88%",
    annotation: "Relevant trafikk, målt og forbedret over tid.",
    href: "/tjenester/google-og-meta-ads",
  },
  {
    number: "06",
    title: "KI og automatisering",
    focus: "Assistanse og arbeidsflyter som gir folk mer rom til å tenke.",
    description:
      "Vi bygger KI-assistenter og automatisering rundt konkrete behov, med egne kilder, tydelige kontrollpunkter og mennesker i sentrum.",
    image: "/images/AI-automation.jpg",
    imageAlt: "En robotarm rekker en kaffekopp til en mann som leser i en lenestol.",
    imagePosition: "50% 55%",
    annotation: "Praktisk KI, med menneskelig kontroll.",
    href: "/tjenester/ki-og-automatisering",
  },
];

const storyText = (
  story: ServiceStory,
  text: Pick<
    ServiceStory,
    "annotation" | "description" | "focus" | "imageAlt" | "title"
  >,
): ServiceStory => ({ ...story, ...text });

export const serviceStoriesEn: ServiceStory[] = [
  storyText(serviceStories[0], {
    title: "Web solutions",
    focus: "Websites and digital products that make sense straight away.",
    description:
      "From effective marketing websites with hosting and operations in Norway to advanced web applications — we build solutions that fit your needs.",
    imageAlt: "Two hands holding a laptop showing the TriodeLab website.",
    annotation: "From the first sketch to a solution that works.",
  }),
  storyText(serviceStories[1], {
    title: "E-commerce",
    focus: "Shopping experiences that are easy to choose from — and easy to run.",
    description:
      "We are platform-independent and find the technology that suits each project best — Shopify, WooCommerce or something else.",
    imageAlt: "A small shopping trolley holding a parcel in front of a laptop.",
    annotation: "Built around the way you actually sell.",
  }),
  storyText(serviceStories[2], {
    title: "Organic traffic",
    focus: "Quality traffic that keeps finding its way to you.",
    description:
      "Get quality traffic from search engines such as Google. With strong content and technical SEO, you build visibility that can keep bringing relevant visits over time.",
    imageAlt: "A person searching on Google on a desktop computer.",
    annotation: "Insight that makes it easier to be found.",
  }),
  storyText(serviceStories[3], {
    title: "Integrations",
    focus: "Systems that talk to each other, without extra friction.",
    description:
      "Integrasjons-Hub connects Shopify and WooCommerce with Visma, Fiken and Tripletex — robust, stable and flexible integrations for your operations.",
    imageAlt: "A white arch bridge with cables stretching towards a blue sky.",
    annotation: "Less manual work. More coherence.",
  }),
  storyText(serviceStories[4], {
    title: "Google + Meta Ads",
    focus: "Paid visibility with a clearer next step.",
    description:
      "We help you with Google Ads and Meta Ads, from messaging and audiences to tracking, measurement and continuous improvement.",
    imageAlt:
      "A laptop showing a TriodeLab dashboard with results from Google Ads and Meta Ads.",
    annotation: "Relevant traffic, measured and improved over time.",
  }),
  storyText(serviceStories[5], {
    title: "AI and automation",
    focus: "Assistance and workflows that give people more room to think.",
    description:
      "We build AI assistants and automation around concrete needs, with your own sources, clear checkpoints and people at the centre.",
    imageAlt:
      "A robotic arm hands a cup of coffee to a man reading in an armchair.",
    annotation: "Practical AI, with human control.",
  }),
];

export const serviceStoriesByLocale: Localized<ServiceStory[]> = {
  nb: serviceStories,
  en: serviceStoriesEn,
};

type ServicesPageCopy = {
  approach: {
    body: string;
    caption: string;
    heading: string;
    imageAlt: string;
    label: string;
    steps: readonly { copy: string; title: string }[];
  };
  cta: { body: string; button: string; heading: string; label: string };
  editorial: {
    heading: string;
    label: string;
    notes: readonly { alt: string; label: string; note: string }[];
  };
  hero: { body: string; eyebrow: string; heading: string; imageAlt: string };
  overview: {
    advisoryLink: string;
    body: string;
    heading: string;
    label: string;
    readAbout: (title: string) => string;
    servicesLabel: string;
  };
};

export const servicesPageCopy: Localized<ServicesPageCopy> = {
  nb: {
    hero: {
      eyebrow: "Tjenester · Strategi, design og teknologi",
      heading: "Vi bygger digitale løsninger som gjør en forskjell.",
      body: "Fra nettsider og nettbutikker til integrasjoner, SEO og annonsering. Vi bruker også KI og automatisering når det løser konkrete behov – og bygger løsningen sammen med dere.",
      imageAlt:
        "To hender holder en bærbar datamaskin som viser TriodeLabs nettside.",
    },
    overview: {
      label: "Hva vi gjør",
      heading: "Rådgivning og utvikling for virksomheter med ambisjoner.",
      body: "Når en digital løsning må gjøre mer enn å se bra ut, samler vi strategi, design og teknologi rundt oppgaven. Fra retning og utvikling til integrasjoner, synlighet og KI — med løsninger som kan videreutvikles når behovene endrer seg.",
      advisoryLink: "Rådgivning og konsulentbistand for deres team",
      servicesLabel: "Våre tjenester",
      readAbout: (title) => `Les om ${title}`,
    },
    approach: {
      imageAlt:
        "To medlemmer av TriodeLab samarbeider rundt et bord med bærbare datamaskiner",
      caption: "Tett samarbeid gir færre overleveringer og bedre beslutninger.",
      label: "Slik arbeider vi",
      heading: "Samarbeid som gjør prosessen enkel.",
      body: "Vi jobber i tett samarbeid med dere. Det betyr færre overleveringer, bedre kommunikasjon og at dere er med på hver viktig beslutning.",
      steps: [
        {
          title: "Undersøk",
          copy: "Vi starter med å forstå bedriften, målene og hvem dere vil nå.",
        },
        {
          title: "Prosjekter",
          copy: "Vi bygger og tester løsningen sammen med dere, steg for steg.",
        },
        {
          title: "Vokse",
          copy: "Vi følger opp, måler resultatene og utvikler videre når dere er klar.",
        },
      ],
    },
    editorial: {
      label: "I arbeidet",
      heading: "Detaljene er en del av helheten.",
      notes: [
        {
          alt: "To medlemmer av TriodeLab i samtale ved et bord foran en bokhylle",
          label: "Tidlig retning",
          note: "De beste digitale løsningene begynner analogt: med spørsmål, skisser og prioriteringer.",
        },
        {
          alt: "TriodeLab-teamet jobber sammen ved en bærbar datamaskin",
          label: "Sammenheng",
          note: "Når systemer og flater henger sammen, blir hverdagen enklere for både teamet og kundene.",
        },
        {
          alt: "Et medlem av TriodeLab står og jobber ved en bærbar datamaskin",
          label: "Videreutvikling",
          note: "Etter lansering bruker vi data til å se hva som virker — og hva som bør bli bedre.",
        },
      ],
    },
    cta: {
      label: "Neste steg",
      heading: "Hva kan vi gjøre enklere?",
      body: "Fortell oss kort hvor dere står. Vi finner et konkret første steg sammen.",
      button: "Start en samtale",
    },
  },
  en: {
    hero: {
      eyebrow: "Services · Strategy, design and technology",
      heading: "We build digital solutions that make a difference.",
      body: "From websites and online stores to integrations, SEO and advertising. We also use AI and automation when it solves a concrete need – and we build the solution together with you.",
      imageAlt: "Two hands holding a laptop showing the TriodeLab website.",
    },
    overview: {
      label: "What we do",
      heading: "Advisory and development for ambitious businesses.",
      body: "When a digital solution needs to do more than look good, we bring strategy, design and technology together around the task. From direction and development to integrations, visibility and AI — with solutions that can evolve as your needs change.",
      advisoryLink: "Advisory and consulting for your team",
      servicesLabel: "Our services",
      readAbout: (title) => `Read about ${title}`,
    },
    approach: {
      imageAlt:
        "Two members of TriodeLab working together at a table with laptops",
      caption: "Close collaboration means fewer handovers and better decisions.",
      label: "How we work",
      heading: "Collaboration that keeps things simple.",
      body: "We work closely with you. That means fewer handovers, better communication and that you are part of every important decision.",
      steps: [
        {
          title: "Explore",
          copy: "We start by understanding your business, your goals and who you want to reach.",
        },
        {
          title: "Build",
          copy: "We build and test the solution together with you, step by step.",
        },
        {
          title: "Grow",
          copy: "We follow up, measure the results and keep developing when you are ready.",
        },
      ],
    },
    editorial: {
      label: "At work",
      heading: "The details are part of the whole.",
      notes: [
        {
          alt: "Two members of TriodeLab in conversation at a table in front of a bookshelf",
          label: "Early direction",
          note: "The best digital solutions start offline: with questions, sketches and priorities.",
        },
        {
          alt: "The TriodeLab team working together at a laptop",
          label: "Coherence",
          note: "When systems and channels fit together, everyday work gets easier for both your team and your customers.",
        },
        {
          alt: "A member of TriodeLab standing and working at a laptop",
          label: "Further development",
          note: "After launch, we use data to see what works — and what should get better.",
        },
      ],
    },
    cta: {
      label: "Next step",
      heading: "What can we make simpler?",
      body: "Tell us briefly where you are. We will find a concrete first step together.",
      button: "Start a conversation",
    },
  },
};
