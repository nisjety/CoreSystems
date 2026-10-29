import type { Localized } from "../../lib/i18n/locales";

export const studioIntroduction = {
  heading: "Dere setter retningen. Vi gjør den digital.",
  aside: "Vi tar oss tid til å forstå før vi bygger.",
  paragraphs: [
    "TriodeLab er et digitalt studio for virksomheter som trenger mer enn en pen overflate. Vi samler strategi, design og teknologi i løsninger som er tydelige å bruke og enkle å eie.",
    "Vi jobber tett med team som vil komme videre – enten det handler om en ny plattform, en bedre kjøpsreise eller et produkt som må fungere fra første dag.",
  ],
} as const;

export const studioIntroductionEn = {
  heading: "You set the direction. We make it digital.",
  aside: "We take the time to understand before we build.",
  paragraphs: [
    "TriodeLab is a digital studio for businesses that need more than a polished surface. We bring strategy, design and technology together in solutions that are clear to use and easy to own.",
    "We work closely with teams that want to move forward – whether that means a new platform, a better buying journey or a product that has to work from day one.",
  ],
} as const;

export const studioIntroductionContent: Localized<{
  aside: string;
  heading: string;
  paragraphs: readonly string[];
}> = { nb: studioIntroduction, en: studioIntroductionEn };
