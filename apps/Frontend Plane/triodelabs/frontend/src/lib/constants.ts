// Site Configuration
export const siteConfig = {
  name: "TriodeLab",
  title: "Digital rådgivning, web og KI | TriodeLab",
  description:
    "TriodeLab er en digital partner for ambisiøse virksomheter. Vi kombinerer rådgivning, webutvikling, nettbutikk, integrasjoner, synlighet og KI.",
  url: "https://www.triodelab.no",
  ogImage: "/opengraph-image",
  links: {
    email: "post@triodelab.no",
    phone: "+47 988 46 460",
    linkedin: "https://linkedin.com/company/triodelab",
    github: "https://github.com/triodelab",
  },
  address: {
    street: "Sigurd Hoels vei 114",
    city: "0655 Oslo",
    country: "Norge",
  }
};

// Service Types
export const serviceTypes = {
  strategisk: {
    title: "Strategisk Rådgivning",
    description: "Digital strategi og vekstplanlegging",
    icon: "BarChart",
  },
  teknisk: {
    title: "Teknisk Konsultasjon", 
    description: "Teknisk analyse og implementering",
    icon: "Code",
  },
  transformasjon: {
    title: "Digital Transformasjon",
    description: "Komplett digital omstilling",
    icon: "Zap",
  },
};

// Project Types
export const projectTypes = {
  webutvikling: "Webutvikling",
  design: "Design & UX",
  konsultasjon: "Teknisk konsultasjon",
  ehandel: "E-handel",
  app: "App-utvikling",
  annet: "Annet",
};

// Budget Ranges
export const budgetRanges = {
  "under-50k": "Under 50.000 NOK",
  "50k-150k": "50.000 - 150.000 NOK", 
  "150k-500k": "150.000 - 500.000 NOK",
  "over-500k": "Over 500.000 NOK",
};

// Timelines
export const timelines = {
  asap: "Så snart som mulig",
  "1-month": "Innen 1 måned",
  "2-3-months": "2-3 måneder",
  "3-6-months": "3-6 måneder", 
  flexible: "Fleksibel",
};

// Team Members
export const teamMembers = [
  {
    name: "Erik Nordahl",
    role: "Grunnlegger & Senior Konsulent",
    image: "https://images.unsplash.com/photo-1507003211169-0a1dd7228f2d?w=400&h=400&fit=crop&crop=face",
    bio: "Med over 10 år som konsulent har Erik hjulpet bedrifter med digital transformasjon. Han startet sin karriere hos McKinsey Digital og har ledet store transformasjonsprosjekter for Fortune 500-selskaper.",
    skills: ["Digital Strategy", "Change Management", "Technology Consulting", "Team Leadership"],
    achievements: ["Ex-McKinsey Digital", "MBA from BI", "10+ år erfaring"],
    email: "erik@triodelab.no",
    linkedin: "https://linkedin.com/in/eriknordahl",
  },
  {
    name: "Marie Haugen", 
    role: "Strategisk Konsulent",
    image: "https://images.unsplash.com/photo-1494790108755-2616b612b5bb?w=400&h=400&fit=crop&crop=face",
    bio: "Marie spesialiserer seg på digital strategi og vekst. Hun har 8 år med erfaring fra Bain & Company og har hjulpet over 50 bedrifter med digital transformasjon.",
    skills: ["Digital Strategy", "Growth Consulting", "Business Analysis", "Market Research"],
    achievements: ["Ex-Bain & Company", "Strategy Expert", "8+ år erfaring"],
    email: "marie@triodelab.no",
    linkedin: "https://linkedin.com/in/mariehaugen",
  },
  {
    name: "Olav Kristensen",
    role: "Teknisk Konsulent", 
    image: "https://images.unsplash.com/photo-1472099645785-5658abf4ff4e?w=400&h=400&fit=crop&crop=face",
    bio: "Olav er eksperten vi ringer når det gjelder teknisk implementering. Med bakgrunn som CTO hos flere scale-ups har han dyp forståelse for teknisk arkitektur og digital infrastruktur.",
    skills: ["Technical Architecture", "Digital Infrastructure", "Cloud Strategy", "System Design"],
    achievements: ["Ex-CTO Scale-ups", "Technical Expert", "12+ år erfaring"],
    email: "olav@triodelab.no",
    linkedin: "https://linkedin.com/in/olavkristensen",
  },
];

// Pricing Plans
export const pricingPlans = [
  {
    name: "Starter",
    price: 15000,
    period: "NOK",
    description: "Perfekt for små bedrifter som ønsker en profesjonell nettside",
    features: [
      "Responsiv design",
      "SEO-optimalisering",
      "Kontaktskjema",
      "Google Analytics",
      "1 måned support",
    ],
    popular: false,
  },
  {
    name: "Professional", 
    price: 35000,
    period: "NOK",
    description: "Ideell for voksende bedrifter med avanserte behov",
    features: [
      "Alt i Starter",
      "CMS integration",
      "E-handel funksjonalitet",
      "Avansert SEO",
      "3 måneder support",
      "Performance optimalisering",
    ],
    popular: true,
  },
  {
    name: "Enterprise",
    price: 75000,
    period: "NOK+",
    description: "Skreddersydde løsninger for store organisasjoner",
    features: [
      "Alt i Professional",
      "Skreddersydd funksjonalitet",
      "API-integrasjoner",
      "Dedikert prosjektleder",
      "6 måneder support",
      "Sikkerhetstesting",
      "Performance monitoring",
    ],
    popular: false,
  },
];

// Portfolio Projects
export const portfolioProjects = [
  {
    title: "E-handel Platform",
    client: "Nordic Fashion AS",
    description: "Moderne e-handelsløsning som økte konverteringen med 340%",
    image: "https://images.unsplash.com/photo-1556742049-0cfed4f6a45d?w=600&h=400&fit=crop",
    tags: ["E-handel", "React", "Node.js"],
    stats: { conversion: "+340%", users: "50k+", revenue: "+2.5M NOK" },
    year: 2024,
    category: "E-handel",
  },
  {
    title: "Fintech Dashboard", 
    client: "Oslo Invest",
    description: "Avansert dashboard for investering med real-time data",
    image: "https://images.unsplash.com/photo-1551288049-bebda4e38f71?w=600&h=400&fit=crop",
    tags: ["Fintech", "Dashboard", "API"],
    stats: { users: "10k+", data: "Real-time", uptime: "99.9%" },
    year: 2024,
    category: "Fintech",
  },
  {
    title: "SaaS Platform",
    client: "TechStart Norge", 
    description: "Skalerbar SaaS-plattform fra idé til 1000+ brukere",
    image: "https://images.unsplash.com/photo-1460925895917-afdab827c52f?w=600&h=400&fit=crop",
    tags: ["SaaS", "Cloud", "Skalering"],
    stats: { growth: "+500%", users: "1000+", mrr: "€50k" },
    year: 2023,
    category: "SaaS",
  },
];

// Process Steps
export const processSteps = [
  {
    step: 1,
    title: "Analyse & Forståelse",
    description: "Vi starter med å analysere din virksomhet, utfordringer og mål gjennom dybdeintervjuer og omfattende research.",
    duration: "1-2 uker",
    icon: "Search",
  },
  {
    step: 2,
    title: "Strategi & Planlegging",
    description: "Basert på analysen utvikler vi en skreddersydd digital strategi med klare mål og implementeringsplan.",
    duration: "2-3 uker", 
    icon: "Lightbulb",
  },
  {
    step: 3,
    title: "Implementering",
    description: "Vårt team av eksperter implementerer løsningene med fokus på kvalitet, effektivitet og målbare resultater.",
    duration: "4-12 uker",
    icon: "Code",
  },
  {
    step: 4,
    title: "Lansering & Overgang",
    description: "Vi sikrer en smidig overgang til nye prosesser og systemer med grundig opplæring og support.",
    duration: "1-2 uker",
    icon: "Rocket",
  },
  {
    step: 5,
    title: "Oppfølging & Optimalisering",
    description: "Kontinuerlig oppfølging, måling av resultater og optimalisering for å sikre langsiktig suksess.",
    duration: "Løpende",
    icon: "TrendingUp",
  },
];

// Statistics
export const statistics = [
  { value: "150+", label: "Fornøyde klienter" },
  { value: "500+", label: "Prosjekter levert" },
  { value: "99%", label: "Tilfredshet" },
];

// Company Stats
export const companyStats = [
  { number: "3", label: "År i markedet" },
  { number: "25", label: "Bransjepartnere" },
];

// SEO Keywords
export const seoKeywords = [
  "webutvikling oslo",
  "webdesign norge", 
  "digital strategi",
  "next.js utvikling",
  "react utvikling",
  "e-handel løsninger",
  "skreddersydd utvikling",
  "ui ux design oslo",
  "digital transformasjon",
  "moderne nettsider",
];
