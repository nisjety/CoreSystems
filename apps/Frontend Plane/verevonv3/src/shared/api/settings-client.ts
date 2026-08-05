import { requestJson } from './http'

export interface UserProfile {
  id: string
  name: string
  displayName: string
  email: string
  emailVerified: boolean
  avatarUrl: string | null
  firstName: string
  lastName: string
  phoneNumber: string
  officeLocation: string
  timezone: string
  position: string
  department: string
  status: string
  accountStatus: string
  createdAt?: string
  updatedAt?: string
  lastLoginAt?: string | null
}

export interface UpdateUserProfileRequest {
  name?: string
  displayName?: string
  avatar?: string
  firstName?: string
  lastName?: string
  phoneNumber?: string
  officeLocation?: string
  timezone?: string
  position?: string
  department?: string
  status?: string
}

export interface UserSettings {
  key: string
  value: unknown
}

export type CrawlIngestMode = 'auto' | 'never' | 'prompt'

export interface UserPreferences {
  theme?: string
  language?: string
  timezone?: string
  notifications?: Record<string, boolean>
  // Phase 6 selective ingest: whether crawl/scrape results are saved to the
  // knowledge base. Unset defaults to 'auto' (always save) — an explicit
  // "add website to Knowledge" crawl persisting is the only non-surprising
  // behavior; 'never' = browsing only; 'prompt' = ask after each crawl. Drives
  // the `ingest` flag on crawl requests. See resolveCrawlIngest.
  crawlIngestMode?: CrawlIngestMode
}

/**
 * The default crawl-ingest mode when a user has never set the preference.
 * 'auto' so an explicit "add this website to my Knowledge base" crawl actually
 * persists (owner=user, private until shared) instead of silently discarding
 * every page — the failure the add-source card's "flow into Knowledge" copy
 * promised against. A user who explicitly picked 'never' in Settings is always
 * respected: this default only fills a genuinely-unset preference.
 */
export const DEFAULT_CRAWL_INGEST_MODE: CrawlIngestMode = 'auto'

/**
 * Resolve the user's crawl-ingest preference into the boolean `ingest` flag a
 * crawl request carries. Shared by every explicit add-to-Knowledge crawl entry
 * point (dashboard composer, page-picker, Knowledge add-source modal) so they
 * cannot drift — before this, the add-source modal path sent no flag at all and
 * quarry defaulted to never-persist. Reads preferences once; on lookup failure
 * falls back to the default mode rather than blocking the crawl.
 *
 * @param confirmPrompt invoked only for the 'prompt' mode; return true to persist.
 */
export async function resolveCrawlIngest(confirmPrompt: () => boolean): Promise<boolean> {
  const mode = (await getPreferences().catch(() => null))?.crawlIngestMode ?? DEFAULT_CRAWL_INGEST_MODE
  return mode === 'auto' || (mode === 'prompt' && confirmPrompt())
}

export interface ApiKey {
  id: string
  name: string
  prefix: string
  createdAt: string
  expiresAt?: string
}

export interface CreateApiKeyRequest {
  name: string
  expiresAt?: string
}

export interface CreateApiKeyResponse extends ApiKey {
  secret: string
}

export interface SessionSnapshot {
  sessionId: string
  userId: string
  orgId: string
  expiresAt: string
}

export interface WorkflowPolicySetting {
  workflowPolicies?: Record<string, boolean>
}

function asRecord(value: unknown): Record<string, unknown> | null {
  return value && typeof value === 'object' ? value as Record<string, unknown> : null
}

function innerRecord(payload: unknown, key: string): Record<string, unknown> {
  const record = asRecord(payload)
  const nested = asRecord(record?.[key])
  return nested ?? record ?? {}
}

function stringField(record: Record<string, unknown>, ...keys: string[]): string {
  for (const key of keys) {
    const value = record[key]
    if (typeof value === 'string') return value
  }
  return ''
}

function nullableStringField(record: Record<string, unknown>, ...keys: string[]): string | null {
  for (const key of keys) {
    const value = record[key]
    if (typeof value === 'string') return value
    if (value === null) return null
  }
  return null
}

function booleanField(record: Record<string, unknown>, ...keys: string[]): boolean {
  for (const key of keys) {
    const value = record[key]
    if (typeof value === 'boolean') return value
  }
  return false
}

function displayNameFromEmail(email: string): string {
  return email.split('@')[0] ?? ''
}

function normalizeUserProfile(payload: unknown): UserProfile {
  const user = innerRecord(payload, 'user')
  const email = stringField(user, 'email')
  const displayName = stringField(user, 'display_name', 'displayName', 'name') || displayNameFromEmail(email)
  const name = stringField(user, 'name', 'display_name', 'displayName') || displayName
  return {
    id: stringField(user, 'id'),
    name,
    displayName,
    email,
    emailVerified: booleanField(user, 'email_verified', 'emailVerified'),
    avatarUrl: nullableStringField(user, 'avatarUrl', 'avatar', 'image'),
    firstName: stringField(user, 'first_name', 'firstName'),
    lastName: stringField(user, 'last_name', 'lastName'),
    phoneNumber: stringField(user, 'phoneNumber', 'phone_number', 'phone'),
    officeLocation: stringField(user, 'officeLocation', 'office_location', 'location'),
    timezone: stringField(user, 'timezone'),
    position: stringField(user, 'position'),
    department: stringField(user, 'department'),
    status: stringField(user, 'status'),
    accountStatus: stringField(user, 'account_status', 'accountStatus') || 'active',
    createdAt: stringField(user, 'created_at', 'createdAt') || undefined,
    updatedAt: stringField(user, 'updated_at', 'updatedAt') || undefined,
    lastLoginAt: nullableStringField(user, 'last_login_at', 'lastLoginAt'),
  }
}

function normalizePreferences(payload: unknown): UserPreferences {
  const preferences = innerRecord(payload, 'preferences')
  const notifications = asRecord(preferences.notifications)
  const rawMode = stringField(preferences, 'crawlIngestMode')
  const crawlIngestMode: CrawlIngestMode | undefined =
    rawMode === 'auto' || rawMode === 'never' || rawMode === 'prompt' ? rawMode : undefined
  return {
    theme: stringField(preferences, 'theme') || undefined,
    language: stringField(preferences, 'language') || undefined,
    timezone: stringField(preferences, 'timezone') || undefined,
    crawlIngestMode,
    notifications: notifications
      ? Object.fromEntries(
          Object.entries(notifications).filter((entry): entry is [string, boolean] => typeof entry[1] === 'boolean'),
        )
      : undefined,
  }
}

export function getMe(): Promise<UserProfile> {
  return requestJson<unknown>('/api/v1/me').then(normalizeUserProfile)
}

export function updateMe(patch: UpdateUserProfileRequest): Promise<UserProfile> {
  return requestJson<unknown>('/api/v1/me', {
    method: 'PATCH',
    body: JSON.stringify(patch),
  }).then(normalizeUserProfile)
}

export function getSetting(key: string): Promise<UserSettings> {
  return requestJson<UserSettings>(`/api/v1/settings/${encodeURIComponent(key)}`)
}

export function updateSetting(key: string, value: unknown): Promise<UserSettings> {
  return requestJson<UserSettings>(`/api/v1/settings/${encodeURIComponent(key)}`, {
    method: 'PUT',
    body: JSON.stringify({ key, value }),
  })
}

export function getPreferences(): Promise<UserPreferences> {
  return requestJson<unknown>('/api/v1/preferences').then(normalizePreferences)
}

export function updatePreferences(patch: Partial<UserPreferences>): Promise<UserPreferences> {
  return requestJson<unknown>('/api/v1/preferences', {
    method: 'PATCH',
    body: JSON.stringify(patch),
  }).then(() => getPreferences())
}

export function listApiKeys(): Promise<ApiKey[]> {
  return requestJson<ApiKey[]>('/api/v1/api-keys')
}

export function createApiKey(req: CreateApiKeyRequest): Promise<CreateApiKeyResponse> {
  return requestJson<CreateApiKeyResponse>('/api/v1/api-keys', {
    method: 'POST',
    body: JSON.stringify(req),
  })
}

export function deleteApiKey(id: string): Promise<void> {
  return requestJson<void>(`/api/v1/api-keys/${encodeURIComponent(id)}`, {
    method: 'DELETE',
  })
}

export function getSessionSnapshot(): Promise<SessionSnapshot> {
  return requestJson<SessionSnapshot>('/api/v1/session/current')
}

export function refreshSession(body?: Record<string, unknown>): Promise<SessionSnapshot> {
  return requestJson<SessionSnapshot>('/api/v1/session/refresh', {
    method: 'POST',
    body: JSON.stringify(body ?? {}),
  })
}
