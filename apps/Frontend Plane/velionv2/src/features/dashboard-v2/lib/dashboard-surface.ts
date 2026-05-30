export type DashboardCard = {
  id: string;
  category: string;
  title: string;
  description: string;
  image: string;
  href: string;
  prompt: string;
};

export const dashboardCards: DashboardCard[] = [
  {
    id: "search",
    category: "Søk",
    title: "Søk i selskapets kunnskap",
    description: "Søk på tvers av nettsider, dokumenter og kanaler i én felles søkemotor.",
    image: "/imagens/arched-corridor-1.jpeg",
    href: "/knowledge",
    prompt: "Søk i selskapets kunnskap og finn relevant informasjon.",
  },
  {
    id: "chat",
    category: "Chat",
    title: "Chat med organisasjons-AI",
    description: "Still spørsmål og få svar med kildehenvisninger direkte fra selskapsinnholdet.",
    image: "/imagens/arched-hallway-symmetry.jpeg",
    href: "/chat",
    prompt: "Hva kan du hjelpe meg med i dag?",
  },
  {
    id: "knowledge",
    category: "Kunnskapsbase",
    title: "Oversikt over indeksert innhold",
    description: "Se hva AI-en vet om selskapet — statusoversikt over alle koblede kilder.",
    image: "/imagens/arched-interior-modern.png",
    href: "/knowledge",
    prompt: "Gi meg en oversikt over hva som er indeksert i kunnskapsbasen.",
  },
  {
    id: "sources",
    category: "Datakilder",
    title: "Nettsider og tilkoblede kilder",
    description: "Administrer nettsider, SharePoint-biblioteker og andre datakilder.",
    image: "/imagens/curved-concrete-space.png",
    href: "/knowledge",
    prompt: "Hvilke datakilder er koblet til systemet og hva er statusen?",
  },
  {
    id: "documents",
    category: "Dokumenter",
    title: "Indekserte filer og dokumenter",
    description: "Bla gjennom opplastede filer og automatisk hentet dokumentasjon.",
    image: "/imagens/curved-interior-sculpture.png",
    href: "/knowledge",
    prompt: "Bla gjennom indekserte dokumenter og finn relevante filer.",
  },
  {
    id: "team",
    category: "Team",
    title: "Teammedlemmer og tilganger",
    description: "Inviter kollegaer, administrer roller og se hvem som har tilgang til hva.",
    image: "/imagens/arched-hallway-symmetry.jpeg",
    href: "/settings",
    prompt: "Hvem er i teamet og hvilke tilganger har de?",
  },
] as const;
