import type { Localized } from "../../lib/i18n/locales";

export const contactPageContent = {
  eyebrow: "Kontakt / Oslo",
  heading: "KONTAKT",
  introduction:
    "Fortell oss hvor dere vil. Vi lytter, stiller de riktige spørsmålene og finner ut om vi er riktig team for veien videre.",
  responseTime: "Vi svarer normalt innen én arbeidsdag.",
  email: "post@triodelab.no",
  phone: "+47 988 46 460",
  address: ["Sigurd Hoels vei 114", "0655 Oslo, Norge"],
  image: {
    src: "https://images.unsplash.com/photo-1640006588210-df5dba064198?auto=format&fit=crop&w=1800&q=85",
    alt: "Glassbygning ved Operaen i Oslo",
  },
} as const;

export const contactProjectTypes = [
  { value: "webutvikling", label: "Nettside eller digital løsning" },
  { value: "design", label: "Design og merkevare" },
  { value: "konsultasjon", label: "Strategi og rådgivning" },
  { value: "ehandel", label: "Nettbutikk" },
  { value: "app", label: "Produkt eller app" },
  { value: "integrasjoner", label: "Systemintegrasjoner" },
  { value: "ki", label: "KI og automatisering" },
  { value: "seo", label: "SEO og organisk trafikk" },
  { value: "annonsering", label: "Google og Meta Ads" },
  { value: "annet", label: "Noe annet" },
] as const;

export const contactPageContentEn = {
  eyebrow: "Contact / Oslo",
  heading: "CONTACT",
  introduction:
    "Tell us where you want to go. We listen, ask the right questions and find out whether we are the right team for the road ahead.",
  responseTime: "We usually reply within one working day.",
  email: "post@triodelab.no",
  phone: "+47 988 46 460",
  address: ["Sigurd Hoels vei 114", "0655 Oslo, Norway"],
  image: {
    src: contactPageContent.image.src,
    alt: "Glass building by the Opera House in Oslo",
  },
} as const;

export const contactProjectTypesEn = [
  { value: "webutvikling", label: "Website or digital solution" },
  { value: "design", label: "Design and brand" },
  { value: "konsultasjon", label: "Strategy and advisory" },
  { value: "ehandel", label: "Online store" },
  { value: "app", label: "Product or app" },
  { value: "integrasjoner", label: "System integrations" },
  { value: "ki", label: "AI and automation" },
  { value: "seo", label: "SEO and organic traffic" },
  { value: "annonsering", label: "Google and Meta Ads" },
  { value: "annet", label: "Something else" },
] as const;

type ContactFormCopy = {
  companyLabel: string;
  companyPlaceholder: string;
  emailLabel: string;
  emailPlaceholder: string;
  errorGeneric: string;
  errorRateLimited: string;
  emailFallback: string;
  formHeading: string;
  honeypot: string;
  location: string;
  messageLabel: string;
  messagePlaceholder: string;
  nameLabel: string;
  namePlaceholder: string;
  privacyAfter: string;
  privacyBefore: string;
  privacyLink: string;
  projectTypeLabel: string;
  sending: string;
  submit: string;
  success: string;
};

export const contactContent: Localized<{
  form: ContactFormCopy;
  page: {
    email: string;
    eyebrow: string;
    heading: string;
    image: { alt: string; src: string };
    introduction: string;
    responseTime: string;
  };
  projectTypes: readonly { label: string; value: string }[];
}> = {
  nb: {
    page: contactPageContent,
    projectTypes: contactProjectTypes,
    form: {
      formHeading: "Send en henvendelse",
      honeypot: "La dette feltet stå tomt",
      nameLabel: "Navn",
      namePlaceholder: "Hva heter du?",
      emailLabel: "E-post",
      emailPlaceholder: "deg@bedrift.no",
      companyLabel: "Bedrift (valgfritt)",
      companyPlaceholder: "Bedriftens navn",
      projectTypeLabel: "Hva gjelder det?",
      messageLabel: "Fortell kort om hvor dere vil",
      messagePlaceholder:
        "Mål, utfordring eller idé — det holder med noen linjer.",
      privacyBefore:
        "Vi bruker opplysningene til å svare på henvendelsen. Ikke send sensitive opplysninger. Les vår ",
      privacyLink: "personvernerklæring",
      privacyAfter: ".",
      submit: "Send henvendelse",
      sending: "Sender …",
      success:
        "Takk! Henvendelsen er sendt. Vi svarer normalt innen én arbeidsdag.",
      errorRateLimited:
        "Det er sendt flere henvendelser på kort tid. Prøv igjen om litt, eller send oss en e-post.",
      emailFallback: "Send e-post til post@triodelab.no",
      errorGeneric:
        "Vi kunne ikke bekrefte innsendingen. Prøv igjen, eller send oss en e-post. Teksten din er bevart.",
      location: "Oslo / Norge",
    },
  },
  en: {
    page: contactPageContentEn,
    projectTypes: contactProjectTypesEn,
    form: {
      formHeading: "Send an enquiry",
      honeypot: "Leave this field empty",
      nameLabel: "Name",
      namePlaceholder: "What is your name?",
      emailLabel: "Email",
      emailPlaceholder: "you@company.com",
      companyLabel: "Company (optional)",
      companyPlaceholder: "Company name",
      projectTypeLabel: "What is it about?",
      messageLabel: "Tell us briefly where you want to go",
      messagePlaceholder: "A goal, a challenge or an idea — a few lines is enough.",
      privacyBefore:
        "We use this information to reply to your enquiry. Please do not send sensitive information. Read our ",
      privacyLink: "privacy policy",
      privacyAfter: ".",
      submit: "Send enquiry",
      sending: "Sending …",
      success:
        "Thank you! Your enquiry has been sent. We usually reply within one working day.",
      errorRateLimited:
        "Several enquiries have been sent in a short time. Please try again shortly, or email us.",
      emailFallback: "Email post@triodelab.no",
      errorGeneric:
        "We could not confirm that your enquiry was sent. Please try again, or email us. Your text has been kept.",
      location: "Oslo / Norway",
    },
  },
};

export const contactSeo: Localized<{ description: string; title: string }> = {
  nb: {
    title: "Kontakt TriodeLab | Start et digitalt prosjekt",
    description:
      "Ta kontakt med TriodeLab for digital rådgivning, webutvikling, nettbutikk, integrasjoner, synlighet eller KI. Vi jobber fra Oslo med virksomheter i hele Norge.",
  },
  en: {
    title: "Contact TriodeLab | Start a digital project",
    description:
      "Get in touch with TriodeLab for digital advisory, web development, e-commerce, integrations, search visibility or AI. We work from Oslo with businesses across Norway.",
  },
};
