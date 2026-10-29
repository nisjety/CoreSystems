/// <reference types="vite/client" />
import { normalizePulseSite } from './pulse-site-stack.ts'

type Mapping<S extends string> = { status: S; checkedAt?: string }
export type Website = {
  id: string; site: string; revision: number; createdAt: string; updatedAt: string
  /** True for the organisation's primary site, the one org-core holds. Absent for every other site. */
  primary?: boolean
  ownership: Mapping<'unverified' | 'google_owner' | 'dns_verified'>
  integrations: {
    searchConsole: Mapping<'unmapped' | 'connected' | 'needs_connection' | 'not_found' | 'source_unavailable'> & { property?: string; accessLevel?: string }
    ga4: Mapping<'not_connected' | 'requires_consent' | 'needs_selection' | 'connected' | 'not_found' | 'source_unavailable'> & { propertyId?: string; streamId?: string; measurementId?: string; reportingSite?: string }
    clarity: { status: 'not_connected' | 'pending_verification'; projectId?: string }
    pageSpeed: { status: 'available' }
  }
}
export type GoogleAuthorization = { sessionId: string; authorizationUrl: string; expiresAt: string }
const path = '/api/v1/visibility/websites'
const record = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v)
const timestamp = (v: unknown): v is string => typeof v === 'string' && v.length <= 40 && /^\d{4}-\d{2}-\d{2}T/.test(v) && Number.isFinite(Date.parse(v))
export const isWebsiteId = (v: unknown): v is string => typeof v === 'string' && /^web_[a-f0-9]{32}$/.test(v)
const bounded = (v: unknown, max = 300): v is string => typeof v === 'string' && v.length > 0 && v.length <= max && !/[\x00-\x1f\x7f]/.test(v)
const sessionId = (v: unknown): v is string => typeof v === 'string' && /^[A-Za-z0-9][A-Za-z0-9._:-]{0,199}$/.test(v)

/** GA4 can use the registered root or exactly its www twin; never another host. */
export function ga4ReportingSiteOptions(site: string): readonly string[] {
  const registeredSite = normalizePulseSite(site)
  const host = new URL(registeredSite).hostname
  if (host.startsWith('www.')) return [registeredSite, normalizePulseSite(`https://${host.slice(4)}/`)]
  return [registeredSite, normalizePulseSite(`https://www.${host}/`)]
}

export function isGA4ReportingSite(site: string, reportingSite: string): boolean {
  try { return ga4ReportingSiteOptions(site).includes(reportingSite) } catch { return false }
}

export function isGSCReportingProperty(site: string, property: unknown): property is string {
  if (typeof property !== 'string') return false
  try {
    const host = new URL(normalizePulseSite(site)).hostname
    if (property.startsWith('sc-domain:')) { const domain = property.slice(10); return domain === host || `www.${domain}` === host }
    return normalizePulseSite(property) === property && isGA4ReportingSite(site, property)
  } catch { return false }
}
function checked(value: Record<string, unknown>): { checkedAt?: string } {
  if (value.checkedAt === undefined) return {}
  if (!timestamp(value.checkedAt)) throw new Error('Invalid connection check')
  return { checkedAt: value.checkedAt }
}
function mapping<S extends string>(value: unknown, states: readonly S[]): Mapping<S> & Record<string, unknown> {
  if (!record(value) || !states.includes(value.status as S)) throw new Error('Invalid connection status')
  return { ...value, status: value.status as S, ...checked(value) }
}
function optional(value: Record<string, unknown>, key: string, pattern?: RegExp): Record<string, string> {
  const field = value[key]
  if (field === undefined) return {}
  if (!bounded(field) || pattern && !pattern.test(field)) throw new Error('Invalid mapping')
  return { [key]: field }
}
function reportingSite(value: Record<string, unknown>, site: string): { reportingSite?: string } {
  if (value.reportingSite === undefined) return {}
  if (typeof value.reportingSite !== 'string' || normalizePulseSite(value.reportingSite) !== value.reportingSite || !isGA4ReportingSite(site, value.reportingSite)) throw new Error('Invalid GA4 reporting site')
  return { reportingSite: value.reportingSite }
}

/** Only site-level evidence crosses the boundary; never org grants, tokens or connection IDs. */
export function sanitizeWebsite(value: unknown): Website {
  if (!record(value) || !isWebsiteId(value.id) || typeof value.site !== 'string' || normalizePulseSite(value.site) !== value.site || !Number.isSafeInteger(value.revision) || Number(value.revision) < 1 || !timestamp(value.createdAt) || !timestamp(value.updatedAt) || !record(value.integrations)) throw new Error('Invalid website')
  const ownership = mapping(value.ownership, ['unverified', 'google_owner', 'dns_verified'] as const)
  const gsc = mapping(value.integrations.searchConsole, ['unmapped', 'connected', 'needs_connection', 'not_found', 'source_unavailable'] as const)
  if (gsc.property !== undefined && !isGSCReportingProperty(value.site,gsc.property)) throw new Error('Invalid Search Console reporting property')
  const ga4 = mapping(value.integrations.ga4, ['not_connected', 'requires_consent', 'needs_selection', 'connected', 'not_found', 'source_unavailable'] as const)
  const clarity = mapping(value.integrations.clarity, ['not_connected', 'pending_verification'] as const)
  if (!record(value.integrations.pageSpeed) || value.integrations.pageSpeed.status !== 'available') throw new Error('Invalid PageSpeed mapping')
  return {
    id: value.id, site: value.site, revision: Number(value.revision), createdAt: value.createdAt, updatedAt: value.updatedAt,
    ...(value.primary === true ? { primary: true } : {}),
    ownership: { status: ownership.status, ...checked(ownership) },
    integrations: {
      searchConsole: { status: gsc.status, ...checked(gsc), ...optional(gsc, 'property'), ...optional(gsc, 'accessLevel', /^(?:siteOwner|siteFullUser|siteRestrictedUser|siteUnverifiedUser)$/) },
      ga4: { status: ga4.status, ...checked(ga4), ...optional(ga4, 'propertyId', /^\d{1,20}$/), ...optional(ga4, 'streamId', /^\d{1,20}$/), ...optional(ga4, 'measurementId', /^G-[A-Z0-9]{1,30}$/), ...reportingSite(ga4, value.site) },
      clarity: { status: clarity.status, ...optional(clarity, 'projectId', /^[a-z0-9]{4,32}$/) },
      pageSpeed: { status: 'available' },
    },
  }
}

export function sanitizeWebsiteList(value: unknown): Website[] {
  if (!record(value) || !Array.isArray(value.websites) || value.websites.length > 100) throw new Error('Invalid website list')
  const websites = value.websites.map(sanitizeWebsite)
  if (new Set(websites.map((site) => site.id)).size !== websites.length || new Set(websites.map((site) => site.site)).size !== websites.length) throw new Error('Duplicate websites')
  return websites
}

function googleOAuthCallback(): string {
  // Public deployment configuration only, never a request/website URL or secret.
  // Must match Marketing Core and Google's registered Integration Core callback.
  const raw: unknown = import.meta.env.VITE_MARKETING_GOOGLE_OAUTH_CALLBACK_URL ?? 'http://localhost:3026/oauth/callback/google'
  if (typeof raw !== 'string' || raw.length > 2048 || /[\x00-\x20\x7f?#%\\]/.test(raw)) throw new Error('Invalid Google callback configuration')
  const url = new URL(raw)
  if (url.href !== raw || url.username || url.password || url.pathname !== '/oauth/callback/google' || url.search || url.hash) throw new Error('Invalid Google callback configuration')
  if (url.protocol === 'https:' && !url.port && normalizePulseSite(url.origin + '/') === url.origin + '/') return raw
  if (url.protocol === 'http:' && ['localhost', '127.0.0.1', '[::1]'].includes(url.hostname) && (!url.port || Number(url.port) > 0)) return raw
  throw new Error('Invalid Google callback configuration')
}

/** Consent is an explicit human action. Strict callback/scopes reject redirects to arbitrary sites. */
export function sanitizeGoogleAuthorization(value: unknown): GoogleAuthorization {
  if (!record(value) || !sessionId(value.sessionId) || !timestamp(value.expiresAt) || !bounded(value.authorizationUrl, 6000)) throw new Error('Invalid Google connection')
  const url = new URL(value.authorizationUrl)
  const scopes = new Set(['openid', 'email', 'profile', 'https://www.googleapis.com/auth/userinfo.email', 'https://www.googleapis.com/auth/userinfo.profile', 'https://www.googleapis.com/auth/webmasters.readonly', 'https://www.googleapis.com/auth/analytics.readonly'])
  const query = url.searchParams
  if (url.origin !== 'https://accounts.google.com' || url.pathname !== '/o/oauth2/v2/auth' || url.username || url.password || url.hash || query.get('redirect_uri') !== googleOAuthCallback() || query.get('response_type') !== 'code' || !query.get('client_id') || !/^[A-Za-z0-9_-]{32,128}$/.test(query.get('state') ?? '') || query.get('code_challenge_method') !== 'S256' || !/^[A-Za-z0-9_-]{43}$/.test(query.get('code_challenge') ?? '')) throw new Error('Unexpected Google authorization URL')
  if ([...query.keys()].some((key) => query.getAll(key).length !== 1)) throw new Error('Duplicate authorization parameters')
  const requested = (query.get('scope') ?? '').split(' ')
  if (!requested.length || requested.some((scope) => !scopes.has(scope))) throw new Error('Unexpected Google permission')
  return { sessionId: value.sessionId, authorizationUrl: value.authorizationUrl, expiresAt: value.expiresAt }
}

export function sanitizeGoogleStatus(value: unknown): { status: 'pending' | 'completed' | 'failed' | 'expired' } {
  if (!record(value) || !['pending', 'completed', 'failed', 'expired'].includes(String(value.status))) throw new Error('Invalid connection status')
  return { status: value.status as 'pending' | 'completed' | 'failed' | 'expired' }
}

async function request(suffix: string, method = 'GET', body?: object, signal?: AbortSignal): Promise<unknown> {
  const response = await fetch(path + suffix, { method, credentials: 'include', signal, headers: { Accept: 'application/json', ...(body === undefined ? {} : { 'Content-Type': 'application/json' }) }, ...(body === undefined ? {} : { body: JSON.stringify(body) }) })
  if (!response.ok) throw new Error(response.status === 409 ? 'Website changed. Reload the website list and try again.' : 'Website service is unavailable. Please try again.')
  const value: unknown = await response.json()
  if (!record(value) || !('data' in value)) throw new Error('Invalid website service response')
  return value.data
}
function websitePath(id: string): string { if (!isWebsiteId(id)) throw new Error('Invalid website ID'); return '/' + id }
function revisionValue(revision: number) { if (!Number.isSafeInteger(revision) || revision < 1) throw new Error('Invalid revision'); return revision }
export const loadWebsites = async (signal?: AbortSignal) => sanitizeWebsiteList(await request('', 'GET', undefined, signal))
export const addWebsite = async (site: string, signal?: AbortSignal) => sanitizeWebsite(await request('', 'POST', { site: normalizePulseSite(site) }, signal))
export const saveWebsite = async (id: string, site: string, revision: number, signal?: AbortSignal) => sanitizeWebsite(await request(websitePath(id), 'PATCH', { site: normalizePulseSite(site), revision: revisionValue(revision) }, signal))
export const verifyWebsite = async (id: string, signal?: AbortSignal) => sanitizeWebsite(await request(websitePath(id) + '/verify', 'POST', {}, signal))
export const connectWebsiteGoogle = async (id: string, signal?: AbortSignal) => sanitizeGoogleAuthorization(await request(websitePath(id) + '/google/connect', 'POST', {}, signal))
export const websiteGoogleStatus = async (id: string, session: string, signal?: AbortSignal) => { if (!sessionId(session)) throw new Error('Invalid session'); return sanitizeGoogleStatus(await request(websitePath(id) + '/google/status?sessionId=' + encodeURIComponent(session), 'GET', undefined, signal)) }
export const saveClarityMapping = async (id: string, clarityProjectId: string, revision: number, signal?: AbortSignal) => {
  if (!/^[a-z0-9]{4,32}$/.test(clarityProjectId)) throw new Error('Enter a valid Clarity project ID.')
  return sanitizeWebsite(await request(websitePath(id) + '/mappings', 'PATCH', { clarityProjectId, revision: revisionValue(revision) }, signal))
}
export const saveGA4ReportingSite = async (id: string, site: string, ga4ReportingSite: string, revision: number, signal?: AbortSignal) => {
  const registeredSite = normalizePulseSite(site)
  const normalizedReportingSite = normalizePulseSite(ga4ReportingSite)
  if (!isGA4ReportingSite(registeredSite, normalizedReportingSite)) throw new Error('Choose this website or its exact www hostname.')
  return sanitizeWebsite(await request(websitePath(id) + '/mappings', 'PATCH', { ga4ReportingSite: normalizedReportingSite, revision: revisionValue(revision) }, signal))
}

let preferenceScope=''
/** Set by the keyed authenticated workspace entry, before rendering its children. */
export function setWebsitePreferenceScope(identity:string):void {preferenceScope=identity}
const preferenceKey=()=>preferenceScope?`engagement-visibility-website:${preferenceScope}`:'studio-pulse-active-website-id-v1'

export function readWebsitePreference(): string | undefined {
  try { const id = globalThis.localStorage?.getItem(preferenceKey()); return isWebsiteId(id) ? id : undefined } catch { return undefined }
}
export function saveWebsitePreference(id: string): void {
  if (!isWebsiteId(id)) return
  try { globalThis.localStorage?.setItem(preferenceKey(), id) } catch { /* A blocked preference store does not affect the backend registry. */ }
}
