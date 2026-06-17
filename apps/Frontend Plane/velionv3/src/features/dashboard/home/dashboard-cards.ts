export type DashboardCard = {
  id: string
  category: string
  title: string
  description: string
  image?: string
  href: string
  prompt: string
}

export const dashboardCards: DashboardCard[] = [
  {
    id: 'weather',
    category: 'Weather',
    title: 'Lokalt værbilde',
    description: 'Få rask status på temperatur, vind og nedbør direkte på Velion-flaten.',
    href: '/dashboard',
    prompt: 'Oppsummer værbildet og hva teamet bør være obs på i dag.',
  },
  {
    id: 'traffic',
    category: 'Traffic',
    title: 'Trafikk rundt Oslo',
    description: 'Se operative målepunkter, fart og belastning på nøkkelstrekninger.',
    href: '/dashboard',
    prompt: 'Gi meg en kort trafikkstatus og eventuelle flaskehalser i området.',
  },
  {
    id: 'news',
    category: 'News',
    title: 'Norske nyheter',
    description: 'Følg de ferskeste sakene fra utvalgte norske kilder uten å forlate dashboardet.',
    href: '/dashboard',
    prompt: 'Oppsummer de viktigste nyhetene akkurat nå i korte trekk.',
  },
  {
    id: 'sources',
    category: 'Datakilder',
    title: 'Nettsider og tilkoblede kilder',
    description: 'Administrer nettsider, SharePoint-biblioteker og andre datakilder.',
    image: '/imagens/curved-concrete-space.png',
    href: '/knowledge',
    prompt: 'Hvilke datakilder er koblet til systemet og hva er statusen?',
  },
  {
    id: 'documents',
    category: 'Dokumenter',
    title: 'Indekserte filer og dokumenter',
    description: 'Bla gjennom opplastede filer og automatisk hentet dokumentasjon.',
    image: '/imagens/curved-interior-sculpture.png',
    href: '/knowledge',
    prompt: 'Bla gjennom indekserte dokumenter og finn relevante filer.',
  },
  {
    id: 'team',
    category: 'Team',
    title: 'Teammedlemmer og tilganger',
    description: 'Inviter kollegaer, administrer roller og se hvem som har tilgang til hva.',
    image: '/imagens/arched-hallway-symmetry.jpeg',
    href: '/settings',
    prompt: 'Hvem er i teamet og hvilke tilganger har de?',
  },
] as const
