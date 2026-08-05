import { requestJson } from './http'

export type InformationWeatherPayload = {
  current: {
    condition: string
    humidity: number
    icon: string
    lastUpdated: string
    location: string
    precipitation: number
    pressure: number
    temperature: number
    windDirection: number
    windSpeed: number
  }
  forecast: Array<{
    condition: string
    date: string
    icon: string
    precipitation: number
    temperature: {
      max: number
      min: number
    }
  }>
}

export type InformationObservationType =
  | 'measured'
  | 'estimated'
  | 'synthetic'
  | 'unavailable'

export type InformationObservation = {
  confidence: number | null
  fetchedAt: string
  freshness: string
  observationType: InformationObservationType
  observedAt: string | null
  provider: string
  quality: string
  source: string
  unavailableReason?: string
  unit: string
  value: number | null
}

export type InformationTrafficStation = {
  averageSpeed: InformationObservation
  county: string
  countyProvenance: InformationDerivedFieldProvenance
  distanceKm?: number
  id: string
  lastUpdated: string
  locationName: string
  name: string
  roadReference: string
  roadReferenceProvenance: InformationDerivedFieldProvenance
  status: string
  trafficVolume: InformationObservation
}

export type InformationDerivedFieldProvenance = {
  observationType: 'estimated'
  quality: string
  source: string
}

export type InformationTrafficPayload = {
  data: InformationTrafficStation[]
  success: boolean
  timestamp: string
}

export type InformationNewsArticle = {
  categories: string[]
  description: string
  id: string
  image?: string
  link: string
  publishDate: string
  source: string
  title: string
}

export type InformationNewsPayload = {
  articles: InformationNewsArticle[]
  hasMore: boolean
  lastUpdated: string
  totalCount: number
}

export type InformationDashboardPayload = {
  news: InformationNewsPayload | null
  newsError: string | null
  traffic: InformationTrafficPayload | null
  trafficError: string | null
  weather: InformationWeatherPayload | null
  weatherError: string | null
}

export type BrowserLocationCoordinates = {
  altitude?: number | null
  latitude: number
  longitude: number
}

export function formatTrafficObservation(
  observation: InformationObservation,
  locale: string,
): string {
  const norwegian = /^(nb|nn|no)(-|$)/i.test(locale)
  const labels: Record<InformationObservationType, string> = norwegian
    ? {
      measured: 'Målt',
      estimated: 'Estimert',
      synthetic: 'Syntetisk',
      unavailable: 'Utilgjengelig',
    }
    : {
      measured: 'Measured',
      estimated: 'Estimated',
      synthetic: 'Synthetic',
      unavailable: 'Unavailable',
    }
  const provider = observation.provider === 'statens_vegvesen_atlas'
    ? 'Statens vegvesen'
    : observation.provider
  const effectiveType = observation.value === null
    ? 'unavailable'
    : observation.observationType
  const status = observation.freshness === 'stale'
    ? `${labels[effectiveType]} · ${norwegian ? 'Foreldet' : 'Stale'}`
    : labels[effectiveType]

  if (observation.value === null) {
    return `${status} · ${provider}`
  }

  const unit = observation.unit === 'vehicles_per_hour'
    ? norwegian ? 'kjøretøy/time' : 'vehicles/hour'
    : observation.unit
  return `${observation.value.toLocaleString(locale)} ${unit} · ${status} · ${provider}`
}

export function formatDerivedTrafficMetadata(
  metadata: Pick<InformationTrafficStation, 'county' | 'roadReference'>,
  locale: string,
): string {
  const values = [metadata.roadReference, metadata.county].filter((value) => value.trim())
  const norwegian = /^(nb|nn|no)(-|$)/i.test(locale)
  if (values.length === 0) {
    return norwegian ? 'Metadata utilgjengelig' : 'Metadata unavailable'
  }
  return `${values.join(' · ')} · ${norwegian ? 'Estimert metadata' : 'Estimated metadata'}`
}

type DashboardInformationSnapshot = InformationDashboardPayload & {
  usingLiveLocation: boolean
}

const dashboardInformationTtlMs = 30 * 1000

let browserLocationPromise: Promise<BrowserLocationCoordinates | null> | null = null
let cachedDashboardInformation:
  | {
    expiresAt: number
    key: string
    value: DashboardInformationSnapshot
  }
  | null = null
let pendingDashboardInformation:
  | {
    key: string
    promise: Promise<DashboardInformationSnapshot>
  }
  | null = null

export const newsCategoryOptions = [
  { value: 'all', label: 'Alle' },
  { value: 'General', label: 'General' },
  { value: 'Business', label: 'Business' },
  { value: 'Technology', label: 'Technology' },
] as const

export function formatRelativeNorwegianTime(value: string): string {
  const date = new Date(value)
  const deltaSeconds = Math.max(0, Math.floor((Date.now() - date.getTime()) / 1000))
  if (deltaSeconds < 60) return 'nå'
  if (deltaSeconds < 3600) return `${Math.floor(deltaSeconds / 60)}m siden`
  if (deltaSeconds < 86400) return `${Math.floor(deltaSeconds / 3600)}t siden`
  return `${Math.floor(deltaSeconds / 86400)}d siden`
}

export function weatherGlyph(condition: string): string {
  const lower = condition.toLowerCase()
  if (lower.includes('clear')) return '☀'
  if (lower.includes('partly')) return '⛅'
  if (lower.includes('cloud')) return '☁'
  if (lower.includes('rain')) return '☔'
  if (lower.includes('snow')) return '❄'
  if (lower.includes('fog')) return '〰'
  if (lower.includes('thunder')) return '⚡'
  return '○'
}

export function requestBrowserLocation(forceRefresh = false): Promise<BrowserLocationCoordinates | null> {
  if (typeof window === 'undefined' || !('geolocation' in navigator)) {
    return Promise.resolve(null)
  }

  if (!forceRefresh && browserLocationPromise) {
    return browserLocationPromise
  }

  browserLocationPromise = new Promise((resolve) => {
    navigator.geolocation.getCurrentPosition(
      (position) => {
        resolve({
          altitude: position.coords.altitude,
          latitude: position.coords.latitude,
          longitude: position.coords.longitude,
        })
      },
      () => resolve(null),
      {
        enableHighAccuracy: true,
        maximumAge: 5 * 60 * 1000,
        timeout: 8000,
      },
    )
  })

  return browserLocationPromise
}

export async function loadDashboardInformationSnapshot(): Promise<DashboardInformationSnapshot> {
  const coordinates = await requestBrowserLocation()
  const key = buildDashboardInformationKey(coordinates)
  const now = Date.now()

  if (cachedDashboardInformation?.key === key && cachedDashboardInformation.expiresAt > now) {
    return cachedDashboardInformation.value
  }

  if (pendingDashboardInformation?.key === key) {
    return pendingDashboardInformation.promise
  }

  const params = new URLSearchParams({
    newsLimit: '8',
    newsMaxAge: '24',
    trafficRadius: '40',
  })
  appendCoordinates(params, coordinates)

  const promise = requestJson<InformationDashboardPayload>(`/api/v1/information/dashboard?${params}`)
    .then((payload) => {
      const snapshot = {
        ...payload,
        usingLiveLocation: Boolean(coordinates),
      }
      cachedDashboardInformation = {
        expiresAt: Date.now() + dashboardInformationTtlMs,
        key,
        value: snapshot,
      }
      return snapshot
    })
    .finally(() => {
      if (pendingDashboardInformation?.key === key) {
        pendingDashboardInformation = null
      }
    })

  pendingDashboardInformation = { key, promise }
  return promise
}

export async function loadWeather(forceRefreshLocation = false): Promise<{
  data: InformationWeatherPayload
  usingLiveLocation: boolean
}> {
  const coordinates = await requestBrowserLocation(forceRefreshLocation)
  const params = new URLSearchParams()
  appendCoordinates(params, coordinates)
  const data = await requestJson<InformationWeatherPayload>(
    `/api/v1/information/weather${params.size ? `?${params}` : ''}`,
  )
  return { data, usingLiveLocation: Boolean(coordinates) }
}

export async function loadTraffic(forceRefreshLocation = false): Promise<{
  data: InformationTrafficPayload
  usingLiveLocation: boolean
}> {
  const coordinates = await requestBrowserLocation(forceRefreshLocation)
  const params = new URLSearchParams({ radius: '40' })
  appendCoordinates(params, coordinates)
  const data = await requestJson<InformationTrafficPayload>(`/api/v1/information/traffic?${params}`)
  return { data, usingLiveLocation: Boolean(coordinates) }
}

export function loadNews(category: string): Promise<InformationNewsPayload> {
  const params = new URLSearchParams({
    limit: '8',
    maxAge: '24',
  })
  if (category !== 'all') {
    params.set('category', category)
  }
  return requestJson<InformationNewsPayload>(`/api/v1/information/news?${params}`)
}

function appendCoordinates(params: URLSearchParams, coordinates: BrowserLocationCoordinates | null): void {
  if (!coordinates) return
  params.set('lat', coordinates.latitude.toString())
  params.set('lon', coordinates.longitude.toString())
  if (typeof coordinates.altitude === 'number') {
    params.set('altitude', Math.round(coordinates.altitude).toString())
  }
}

function buildDashboardInformationKey(coordinates: BrowserLocationCoordinates | null): string {
  if (!coordinates) return 'dashboard-information:default'
  return [
    'dashboard-information',
    coordinates.latitude.toFixed(4),
    coordinates.longitude.toFixed(4),
    typeof coordinates.altitude === 'number'
      ? Math.round(coordinates.altitude).toString()
      : 'na',
  ].join(':')
}
