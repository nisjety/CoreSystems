export const no = {
  common: {
    loading: 'Laster...',
    error: 'Feil',
    retry: 'Prøv igjen',
    refresh: 'Oppdater',
    updated: 'Oppdatert',
    close: 'Lukk',
    save: 'Lagre',
    cancel: 'Avbryt',
    delete: 'Slett',
    edit: 'Rediger',
    view: 'Vis',
    more: 'Mer'
  },
  dashboard: {
    greetings: {
      morning: 'God morgen',
      afternoon: 'Hei der',
      evening: 'God kveld'
    },
    subtitle: 'Hva vil du utforske i dag?',
    chatPlaceholder: 'Skriv meldingen din her...',
    welcomeTitle: 'Velkommen til Aquatiq Chat',
    welcomeText: 'Start din første samtale og oppdag kraften til AI-assistanse. Still spørsmål, få hjelp med oppgaver, eller utforsk kreative ideer.',
    startFirstChat: 'Start din første chat',
    exploreFeatures: 'Utforsk flere funksjoner og muligheter',
    features: {
      ai: 'Avanserte AI-modeller',
      fast: 'Lynraskt',
      search: 'Smart søk',
      creative: 'Kreative verktøy'
    }
  },
  weather: {
    title: 'Vær',
    location: 'Oslo, Norge',
    unavailable: 'Vær utilgjengelig',
    humidity: 'Fuktighet',
    wind: 'Vind',
    precipitation: 'Nedbør',
    pressure: 'Trykk',
    high: 'H',
    low: 'L',
    forecast: 'Værvarsling',
    tomorrow: 'I morgen',
    nextDays: 'Neste dager',
    useMyLocation: 'Bruk min posisjon',
    conditions: {
      clear: 'Klart',
      cloudy: 'Skyet',
      partlyCloudy: 'Delvis skyet',
      overcast: 'Overskyet',
      rain: 'Regn',
      lightRain: 'Lett regn',
      heavyRain: 'Kraftig regn',
      snow: 'Snø',
      lightSnow: 'Lett snø',
      heavySnow: 'Kraftig snø',
      sleet: 'Sludd',
      fog: 'Tåke',
      mist: 'Dis',
      thunderstorm: 'Tordenvær',
      unknown: 'Ukjent'
    }
  },
  news: {
    title: 'Siste nytt',
    unavailable: 'Nyheter utilgjengelig',
    timeout: 'Nyheter tok for lang tid å laste - prøv igjen',
    articles: 'artikler tilgjengelig',
    breaking: 'Siste nytt',
    source: 'Kilde',
    readMore: 'Les mer',
    categories: {
      all: 'Alle nyheter',
      general: 'Generelt',
      technology: 'Teknologi',
      business: 'Business',
      sports: 'Sport'
    },
    loadMore: 'Last flere artikler',
    showAll: 'Vis alle {{count}} artikler',
    showFewer: 'Vis færre',
    justNow: 'Akkurat nå',
    hoursAgo: '{{count}}t siden',
    daysAgo: '{{count}}d siden',
    filter: 'Filtrer nyheter',
    article: '{{count}} artikkel',
    articlePlural: '{{count}} artikler'
  },
  aquatiq: {
    title: 'Høydepunkter',
    unavailable: 'Utilgjengelig',
    offers: 'tilbud tilgjengelig',
    learnMore: 'Lær mer',
    priority: 'Prioritet',
    categories: {
      product: 'Produkt',
      event: 'Arrangement',
      business: 'Forretning'
    },
    fetchError: 'Kunne ikke hente Aquatiq-innhold',
    viewAll: 'Vis alle',
    priorityShort: 'P{{priority}}',
    offer: '{{count}} tilbud',
    offerPlural: '{{count}} tilbud'
  },
  traffic: {
    title: 'Trafikkdata',
    unavailable: 'Trafikk utilgjengelig',
    searchPlaceholder: 'Søk etter en lokasjon...',
    locationError: 'Kunne ikke hente posisjon',
    fetchError: 'Kunne ikke hente trafikkdata',
    noData: 'Ingen trafikkdata tilgjengelig',
    pointsFound: '{{count}} målepunkter',
    morePoints: '+{{count}} flere punkter',
    volume: 'Volum',
    speed: 'Hastighet',
    distance: 'Avstand',
    loading: 'Henter trafikkdata…',
    showAll: 'Vis alle',
    showFewer: 'Vis færre',
    accurateLocation: 'Prøv å få din nøyaktige posisjon',
    status: {
      operational: 'I drift',
      maintenance: 'Vedlikehold',
      offline: 'Ikke tilgjengelig'
    }
  }
} as const;

export type NoTranslations = typeof no;
