export const en = {
  common: {
    loading: 'Loading...',
    error: 'Error',
    retry: 'Try Again',
    refresh: 'Refresh',
    updated: 'Updated',
    close: 'Close',
    save: 'Save',
    cancel: 'Cancel',
    delete: 'Delete',
    edit: 'Edit',
    view: 'View',
    more: 'More'
  },
  dashboard: {
    greetings: {
      morning: 'Good morning',
      afternoon: 'Hello there',
      evening: 'Good evening'
    },
    subtitle: 'What would you like to explore today?',
    chatPlaceholder: 'Type your message here...',
    welcomeTitle: 'Welcome to Aquatiq Chat',
    welcomeText: 'Start your first conversation and discover the power of AI assistance. Ask questions, get help with tasks, or explore creative ideas.',
    startFirstChat: 'Start Your First Chat',
    exploreFeatures: 'Explore more features and capabilities',
    features: {
      ai: 'Advanced AI Models',
      fast: 'Lightning Fast',
      search: 'Smart Search',
      creative: 'Creative Tools'
    }
  },
  weather: {
    title: 'Weather',
    location: 'Oslo, Norway',
    unavailable: 'Weather Unavailable',
    humidity: 'Humidity',
    wind: 'Wind',
    precipitation: 'Precipitation',
    pressure: 'Pressure',
    high: 'H',
    low: 'L',
    forecast: 'Forecast',
    tomorrow: 'Tomorrow',
    nextDays: 'Next Days',
    useMyLocation: 'Use my location',
    conditions: {
      clear: 'Clear',
      cloudy: 'Cloudy',
      partlyCloudy: 'Partly Cloudy',
      overcast: 'Overcast',
      rain: 'Rain',
      lightRain: 'Light Rain',
      heavyRain: 'Heavy Rain',
      snow: 'Snow',
      lightSnow: 'Light Snow',
      heavySnow: 'Heavy Snow',
      sleet: 'Sleet',
      fog: 'Fog',
      mist: 'Mist',
      thunderstorm: 'Thunderstorm',
      unknown: 'Unknown'
    }
  },
  news: {
    title: 'Latest News',
    unavailable: 'News Unavailable',
    timeout: 'News loading timeout - please try again',
    articles: 'articles available',
    breaking: 'Breaking News',
    source: 'Source',
    readMore: 'Read More',
    categories: {
      all: 'All News',
      general: 'General',
      technology: 'Technology',
      business: 'Business',
      sports: 'Sports'
    },
    loadMore: 'Load more articles',
    showAll: 'Show all {{count}} articles',
    showFewer: 'Show fewer',
    justNow: 'Just now',
    hoursAgo: '{{count}}h ago',
    daysAgo: '{{count}}d ago',
    filter: 'Filter news',
    article: '{{count}} article',
    articlePlural: '{{count}} articles'
  },
  aquatiq: {
    title: 'Highlights',
    unavailable: 'Unavailable',
    offers: 'offers available',
    learnMore: 'Learn More',
    priority: 'Priority',
    categories: {
      product: 'Product',
      event: 'Event',
      business: 'Business'
    },
    fetchError: 'Failed to fetch Aquatiq content',
    viewAll: 'View All',
    priorityShort: 'P{{priority}}',
    offer: '{{count}} offer',
    offerPlural: '{{count}} offers'
  },
  traffic: {
    title: 'Traffic Data',
    unavailable: 'Traffic Unavailable',
    searchPlaceholder: 'Search for a location...',
    locationError: 'Unable to get location',
    fetchError: 'Failed to fetch traffic data',
    noData: 'No traffic data available',
    pointsFound: '{{count}} monitoring points',
    morePoints: '+{{count}} more points',
    volume: 'Volume',
    speed: 'Speed',
    distance: 'Distance',
    loading: 'Fetching traffic data...',
    showAll: 'Show all',
    showFewer: 'Show fewer',
    accurateLocation: 'Try to get precise position',
    status: {
      operational: 'Operational',
      maintenance: 'Maintenance',
      offline: 'Offline'
    }
  }
} as const;

export type EnTranslations = typeof en;
