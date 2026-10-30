import type { Localized } from "../../lib/i18n/locales";

export type TeamMemberData = {
  name: string;
  role: string;
  image: string;
  imageAlt: string;
  imagePosition?: string;
  focusAreas: string[];
  linkedin?: string;
};

export const teamMembers: TeamMemberData[] = [
  {
    name: "Ima Da Costa",
    role: "Fullstack-utvikler",
    image: "/images/ima-office.webp",
    imageAlt: "Ima Da Costa på kontoret",
    imagePosition: "43% center",
    focusAreas: ["Design", "SEO", "KI", "Systemer"],
    linkedin: "https://linkedin.com/in/imadecosta",
  },
  {
    name: "Abdifatah Hassan",
    role: "Fullstack-utvikler",
    image: "/images/team-abdi.webp",
    imageAlt: "Abdifatah Hassan i et varmt, innredet kontorlokale",
    imagePosition: "58% center",
    focusAreas: ["Nett", "Mobil", "Apper"],
    linkedin: "https://linkedin.com/in/abdifatahhassan",
  },
  {
    name: "Jack Delamou",
    role: "Fullstack-utvikler",
    image: "/images/team-jack.webp",
    imageAlt: "Jack Delamou ved et bord på kontoret",
    imagePosition: "42% center",
    focusAreas: ["Automatisering", "Integrasjoner", "Sky"],
    linkedin: "https://linkedin.com/in/jackdelamou",
  },
];

export const teamMembersEn: TeamMemberData[] = [
  {
    ...teamMembers[0],
    role: "Full-stack developer",
    imageAlt: "Ima Da Costa at the office",
    focusAreas: ["Design", "SEO", "AI", "Systems"],
  },
  {
    ...teamMembers[1],
    role: "Full-stack developer",
    imageAlt: "Abdifatah Hassan in a warm, furnished office",
    focusAreas: ["Web", "Mobile", "Apps"],
  },
  {
    ...teamMembers[2],
    role: "Full-stack developer",
    imageAlt: "Jack Delamou at a table in the office",
    focusAreas: ["Automation", "Integrations", "Cloud"],
  },
];

export const teamContent: Localized<{
  body: string;
  headingLines: readonly [string, string];
  linkedin: (name: string) => string;
  members: readonly TeamMemberData[];
}> = {
  nb: {
    headingLines: ["Tre folk.", "Hver vår styrke."],
    body: "Vi bygger fullstack, alle tre — men har hver vår greie vi er best på.",
    linkedin: (name) => `${name} på LinkedIn`,
    members: teamMembers,
  },
  en: {
    headingLines: ["Three people.", "Three strengths."],
    body: "All three of us build full-stack — but each of us has something we do best.",
    linkedin: (name) => `${name} on LinkedIn`,
    members: teamMembersEn,
  },
};
