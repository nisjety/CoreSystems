import type { Localized } from "../../lib/i18n/locales";

export const heroContent: Localized<{
  bodyAfterLink: string;
  bodyBeforeLink: string;
  bodyLink: string;
  cta: string;
  headlineLines: readonly string[];
  kicker: readonly string[];
  location: string;
  mobileBodyAfterLink: string;
  mobileBodyLink: string;
  scroll: string;
}> = {
  nb: {
    kicker: [
      "Digital rådgivning, design og teknologi",
      "Fra første valg til en løsning dere kan utvikle videre.",
    ],
    headlineLines: ["Digitale løsninger", "for virksomheter", "som vil videre."],
    bodyBeforeLink:
      "TriodeLab er en teknisk og strategisk partner for virksomheter med digitale ambisjoner. Vi hjelper team med ",
    bodyLink: "rådgivning",
    bodyAfterLink:
      ", utvikling og videre forbedring — fra nettsider og nettbutikk til integrasjoner, synlighet og KI.",
    mobileBodyLink: "Rådgivning og utvikling",
    mobileBodyAfterLink: " for digitale løsninger som er bygget for å vare.",
    cta: "Fortell oss om prosjektet ditt",
    location: "Oslo / 59.9139° N",
    scroll: "(Scroll)",
  },
  en: {
    kicker: [
      "Digital advisory, design and technology",
      "From the first decision to a solution you can keep building on.",
    ],
    headlineLines: ["Digital solutions", "for businesses", "that aim higher."],
    bodyBeforeLink:
      "TriodeLab is a technical and strategic partner for businesses with digital ambitions. We help teams with ",
    bodyLink: "advisory",
    bodyAfterLink:
      ", development and continuous improvement — from websites and online stores to integrations, search visibility and AI.",
    mobileBodyLink: "Advisory and development",
    mobileBodyAfterLink: " for digital solutions built to last.",
    cta: "Tell us about your project",
    location: "Oslo / 59.9139° N",
    scroll: "(Scroll)",
  },
};
