export type InformationWeatherPayload = {
  current: {
    temperature: number;
    humidity: number;
    windSpeed: number;
    windDirection: number;
    precipitation: number;
    pressure: number;
    condition: string;
    icon: string;
    location: string;
    lastUpdated: string;
  };
  forecast: Array<{
    date: string;
    temperature: {
      min: number;
      max: number;
    };
    condition: string;
    icon: string;
    precipitation: number;
  }>;
};

export type InformationTrafficStation = {
  id: string;
  name: string;
  locationName: string;
  county: string;
  roadReference: string;
  trafficVolume: number;
  averageSpeed: number;
  status: string;
  distanceKm?: number;
  lastUpdated: string;
};

export type InformationTrafficPayload = {
  success: boolean;
  data: InformationTrafficStation[];
  timestamp: string;
};

export type InformationNewsArticle = {
  id: string;
  title: string;
  description: string;
  link: string;
  source: string;
  publishDate: string;
  image?: string;
  categories: string[];
};

export type InformationNewsPayload = {
  articles: InformationNewsArticle[];
  lastUpdated: string;
  totalCount: number;
  hasMore: boolean;
};

export type InformationDashboardPayload = {
  news: InformationNewsPayload | null;
  newsError: string | null;
  traffic: InformationTrafficPayload | null;
  trafficError: string | null;
  weather: InformationWeatherPayload | null;
  weatherError: string | null;
};

export type BrowserLocationCoordinates = {
  latitude: number;
  longitude: number;
  altitude?: number | null;
};

export const newsCategoryOptions = [
  { value: "all", label: "Alle" },
  { value: "General", label: "General" },
  { value: "Business", label: "Business" },
  { value: "Technology", label: "Technology" },
] as const;

let browserLocationPromise: Promise<BrowserLocationCoordinates | null> | null = null;

export function formatRelativeNorwegianTime(value: string) {
  const date = new Date(value);
  const deltaSeconds = Math.max(0, Math.floor((Date.now() - date.getTime()) / 1000));
  if (deltaSeconds < 60) return "nå";
  if (deltaSeconds < 3600) return `${Math.floor(deltaSeconds / 60)}m siden`;
  if (deltaSeconds < 86400) return `${Math.floor(deltaSeconds / 3600)}t siden`;
  return `${Math.floor(deltaSeconds / 86400)}d siden`;
}

export function weatherGlyph(condition: string) {
  const lower = condition.toLowerCase();
  if (lower.includes("clear")) return "☀";
  if (lower.includes("partly")) return "⛅";
  if (lower.includes("cloud")) return "☁";
  if (lower.includes("rain")) return "☔";
  if (lower.includes("snow")) return "❄";
  if (lower.includes("fog")) return "〰";
  if (lower.includes("thunder")) return "⚡";
  return "○";
}

export function requestBrowserLocation(forceRefresh = false): Promise<BrowserLocationCoordinates | null> {
  if (typeof window === "undefined" || !("geolocation" in navigator)) {
    return Promise.resolve(null);
  }

  if (!forceRefresh && browserLocationPromise) {
    return browserLocationPromise;
  }

  browserLocationPromise = new Promise((resolve) => {
    navigator.geolocation.getCurrentPosition(
      (position) => {
        resolve({
          latitude: position.coords.latitude,
          longitude: position.coords.longitude,
          altitude: position.coords.altitude,
        });
      },
      () => resolve(null),
      {
        enableHighAccuracy: true,
        maximumAge: 5 * 60 * 1000,
        timeout: 8000,
      },
    );
  });

  return browserLocationPromise;
}
