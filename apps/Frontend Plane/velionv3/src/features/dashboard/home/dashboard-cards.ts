export type DashboardCard = {
  id: string
  category: string
  categoryEn: string
  title: string
  titleEn: string
  description: string
  descriptionEn: string
  image?: string
  href: string
  prompt: string
  promptEn: string
}

export const dashboardCards: DashboardCard[] = [
  {
    id: 'weather',
    category: 'Vær',
    categoryEn: 'Weather',
    title: 'Lokalt værbilde',
    titleEn: 'Local weather',
    description: 'Få rask status på temperatur, vind og nedbør direkte på Velion-flaten.',
    descriptionEn: 'Get a quick view of temperature, wind, and precipitation directly on the Velion surface.',
    href: '/dashboard',
    prompt: 'Oppsummer værbildet og hva teamet bør være obs på i dag.',
    promptEn: 'Summarize the weather and what the team should watch today.',
  },
  {
    id: 'traffic',
    category: 'Trafikk',
    categoryEn: 'Traffic',
    title: 'Trafikk rundt Oslo',
    titleEn: 'Traffic around Oslo',
    description: 'Se operative målepunkter, fart og belastning på nøkkelstrekninger.',
    descriptionEn: 'See operational sensors, speed, and load on key routes.',
    href: '/dashboard',
    prompt: 'Gi meg en kort trafikkstatus og eventuelle flaskehalser i området.',
    promptEn: 'Give me a short traffic status and any bottlenecks nearby.',
  },
  {
    id: 'news',
    category: 'Nyheter',
    categoryEn: 'News',
    title: 'Norske nyheter',
    titleEn: 'Norwegian news',
    description: 'Følg de ferskeste sakene fra utvalgte norske kilder uten å forlate dashboardet.',
    descriptionEn: 'Follow the latest stories from selected Norwegian sources without leaving the dashboard.',
    href: '/dashboard',
    prompt: 'Oppsummer de viktigste nyhetene akkurat nå i korte trekk.',
    promptEn: 'Briefly summarize the most important news right now.',
  },
  {
    id: 'sources',
    category: 'Datakilder',
    categoryEn: 'Data sources',
    title: 'Nettsider og tilkoblede kilder',
    titleEn: 'Websites and connected sources',
    description: 'Administrer nettsider, SharePoint-biblioteker og andre datakilder.',
    descriptionEn: 'Manage websites, SharePoint libraries, and other data sources.',
    image: '/imagens/curved-concrete-space.png',
    href: '/knowledge',
    prompt: 'Hvilke datakilder er koblet til systemet og hva er statusen?',
    promptEn: 'Which data sources are connected to the system, and what is their status?',
  },
  {
    id: 'documents',
    category: 'Dokumenter',
    categoryEn: 'Documents',
    title: 'Indekserte filer og dokumenter',
    titleEn: 'Indexed files and documents',
    description: 'Bla gjennom opplastede filer og automatisk hentet dokumentasjon.',
    descriptionEn: 'Browse uploaded files and automatically imported documentation.',
    image: '/imagens/curved-interior-sculpture.png',
    href: '/knowledge',
    prompt: 'Bla gjennom indekserte dokumenter og finn relevante filer.',
    promptEn: 'Browse indexed documents and find relevant files.',
  },
  {
    id: 'team',
    category: 'Team',
    categoryEn: 'Team',
    title: 'Teammedlemmer og tilganger',
    titleEn: 'Team members and access',
    description: 'Inviter kollegaer, administrer roller og se hvem som har tilgang til hva.',
    descriptionEn: 'Invite colleagues, manage roles, and see who has access to what.',
    image: '/imagens/arched-hallway-symmetry.jpeg',
    href: '/settings',
    prompt: 'Hvem er i teamet og hvilke tilganger har de?',
    promptEn: 'Who is on the team, and what access do they have?',
  },
] as const
