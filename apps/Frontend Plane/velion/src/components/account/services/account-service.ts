/**
 * Account Service
 *
 * Aggregates calls to the control-plane services relevant to the account
 * settings page: user-service (profile, password), org-service (billing,
 * org info), and auth-core (linked providers, session).
 */
import { profileService } from '@/components/core/profile/lib/profile-service'
import { orgService } from '@/lib/services/org-service'
import type { AccountProfileFormValues, NotificationPreferences } from '../types'

class AccountServiceAPI {
  // ── Profile ────────────────────────────────────────────────────────────────

  async getCurrentUser() {
    return profileService.getCurrentUser()
  }

  async getLinkedProviders() {
    return profileService.getLinkedProviders()
  }

  async updateProfile(data: Partial<AccountProfileFormValues>): Promise<void> {
    await profileService.updateProfile({
      name: data.displayName,
      position: data.jobTitle,
      department: data.department,
    })
  }

  // ── Security ───────────────────────────────────────────────────────────────

  async changePassword(currentPassword: string, newPassword: string): Promise<void> {
    const res = await fetch('/api/auth/change-password', {
      method: 'POST',
      credentials: 'include',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ currentPassword, newPassword }),
    })
    if (!res.ok) {
      const body = await res.json().catch(() => ({}))
      throw new Error((body as { message?: string }).message ?? 'Password change failed')
    }
  }

  // ── Organisation / Billing ─────────────────────────────────────────────────

  async getMyOrganizations() {
    return orgService.getMyOrganizations()
  }

  async getOrganizationBilling(orgId: string) {
    return orgService.getOrganizationBilling(orgId)
  }

  async getOrganizationQuotas(orgId: string) {
    return orgService.getOrganizationQuotas(orgId)
  }

  // ── Notifications (real, U5-2) ─────────────────────────────────────────────
  //
  // U5-2 (ui-ux-verevon-gap.md §10): wired to notification-core via verevon's
  // proxy routes under /api/notifications/*. Previously this was hardcoded
  // mock data returning 5 boolean flags — replaced with a Novu-backed
  // preference matrix served from /preferences (per-user overrides) merged
  // with /channels/config (org-level defaults).
  //
  // The legacy `NotificationPreferences` shape is kept for the old caller
  // until the UI migrates; new code should call `getNotificationMatrix`
  // and `setChannelPreference` directly.

  async getNotificationMatrix(): Promise<NotificationMatrix> {
    const [prefsRes, configsRes] = await Promise.all([
      fetch('/api/notifications/preferences', {
        credentials: 'include',
        cache: 'no-store',
      }),
      fetch('/api/notifications/channels', {
        credentials: 'include',
        cache: 'no-store',
      }),
    ])

    if (!prefsRes.ok) {
      throw new Error(`load preferences: HTTP ${prefsRes.status}`)
    }
    if (!configsRes.ok) {
      throw new Error(`load channel configs: HTTP ${configsRes.status}`)
    }

    const prefsJson = (await prefsRes.json()) as {
      preferences?: NotificationPreferenceRow[]
    }
    const configsJson = (await configsRes.json()) as {
      configs?: NotificationChannelConfigRow[]
    }

    const prefs = prefsJson.preferences ?? []
    const configs = configsJson.configs ?? []

    return mergePreferenceMatrix(configs, prefs)
  }

  async setChannelPreference(
    eventType: string,
    channel: string,
    enabled: boolean,
  ): Promise<void> {
    const res = await fetch(
      `/api/notifications/preferences/${encodeURIComponent(eventType)}/${encodeURIComponent(channel)}`,
      {
        method: 'PUT',
        credentials: 'include',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ enabled }),
      },
    )
    if (!res.ok) {
      const body = await res.text().catch(() => '')
      throw new Error(`set preference: HTTP ${res.status} ${body}`)
    }
  }

  // Legacy shape retained for back-compat with the old NotificationsSection
  // until the UI migrates. Maps the matrix down to the original 5 booleans
  // using event_type → key conventions documented in code.
  async getNotificationPreferences(): Promise<NotificationPreferences> {
    try {
      const matrix = await this.getNotificationMatrix()
      return projectLegacyPreferences(matrix)
    } catch (err) {
      console.warn('[account-service] getNotificationPreferences fell back to legacy mock:', err)
      // Keep the page usable even when notification-core is down — the
      // matrix view will surface the real error.
      return {
        emailDigest: true,
        securityAlerts: true,
        productUpdates: false,
        teamInvites: true,
        mentionsOnly: false,
      }
    }
  }

  async updateNotificationPreferences(
    prefs: Partial<NotificationPreferences>,
  ): Promise<void> {
    // Translate legacy boolean keys back into channel-preference PUTs.
    const mapping: Array<[
      keyof NotificationPreferences,
      string /* event_type */,
      string /* channel */,
    ]> = [
      ['emailDigest', 'product.update', 'email'],
      ['securityAlerts', 'auth.user.security_alert', 'email'],
      ['productUpdates', 'product.update', 'in_app'],
      ['teamInvites', 'auth.user.invited', 'email'],
      // `mentionsOnly` is not a per-channel toggle — it's a global filter
      // that the matrix UI exposes separately. Skip it here.
    ]
    for (const [key, eventType, channel] of mapping) {
      if (prefs[key] === undefined) continue
      try {
        await this.setChannelPreference(eventType, channel, prefs[key] as boolean)
      } catch (err) {
        console.error(`[account-service] failed to update ${eventType}/${channel}:`, err)
      }
    }
  }

  // ── Danger zone ───────────────────────────────────────────────────────────

  async deleteAccount(): Promise<void> {
    const res = await fetch('/api/auth/delete-account', {
      method: 'DELETE',
      credentials: 'include',
    })
    if (!res.ok) {
      throw new Error('Account deletion failed. Please contact support.')
    }
  }
}

export const accountService = new AccountServiceAPI()

// ── U5-2 types + helpers ────────────────────────────────────────────────────

export interface NotificationPreferenceRow {
  user_id: string
  event_type: string
  channel: string
  enabled: boolean
  created_at: string
  updated_at: string
}

export interface NotificationChannelConfigRow {
  org_id: string
  event_type: string
  channel: string
  enabled: boolean
  default_for_subscribers: boolean
  label: string
  description: string
  created_at: string
  updated_at: string
}

// NotificationMatrixEntry is the row a single (event_type) gets in the UI:
// one label/description + a map of channel → effective enabled state +
// metadata about whether the channel is even available for the org.
export interface NotificationMatrixEntry {
  eventType: string
  label: string
  description: string
  channels: {
    [channel: string]: {
      available: boolean // org enables this channel for this event_type
      enabled: boolean   // effective: user override OR org default
      hasOverride: boolean // user has set an explicit preference
    }
  }
}

export interface NotificationMatrix {
  entries: NotificationMatrixEntry[]
}

// mergePreferenceMatrix combines org-level channel configs with the user's
// explicit preferences into the matrix the UI renders.
//
// Rules:
//   - `available` = the org-level row has `enabled: true`.
//   - `enabled`   = the user override when present, otherwise the org's
//                   `default_for_subscribers`.
//   - `hasOverride` = a user row exists for this (event_type, channel).
export function mergePreferenceMatrix(
  configs: NotificationChannelConfigRow[],
  prefs: NotificationPreferenceRow[],
): NotificationMatrix {
  type EntryKey = string // event_type
  type ChannelMap = Record<string, NotificationMatrixEntry['channels'][string]>

  const byEvent = new Map<EntryKey, { label: string; description: string; channels: ChannelMap }>()
  for (const cfg of configs) {
    let entry = byEvent.get(cfg.event_type)
    if (!entry) {
      entry = { label: cfg.label || cfg.event_type, description: cfg.description, channels: {} }
      byEvent.set(cfg.event_type, entry)
    } else if (!entry.label && cfg.label) {
      entry.label = cfg.label
      entry.description = cfg.description
    }
    entry.channels[cfg.channel] = {
      available: cfg.enabled,
      enabled: cfg.default_for_subscribers,
      hasOverride: false,
    }
  }

  for (const p of prefs) {
    const entry = byEvent.get(p.event_type)
    if (!entry) continue
    const channelEntry = entry.channels[p.channel]
    if (!channelEntry) continue
    channelEntry.enabled = p.enabled
    channelEntry.hasOverride = true
  }

  const entries: NotificationMatrixEntry[] = []
  for (const [eventType, value] of byEvent.entries()) {
    entries.push({
      eventType,
      label: value.label,
      description: value.description,
      channels: value.channels,
    })
  }
  entries.sort((a, b) => a.label.localeCompare(b.label))

  return { entries }
}

// projectLegacyPreferences renders the 5-boolean shape the old UI used
// from the matrix. Keep it close to NotificationsSection's expectations.
function projectLegacyPreferences(matrix: NotificationMatrix): NotificationPreferences {
  function effective(eventType: string, channel: string): boolean {
    const entry = matrix.entries.find((e) => e.eventType === eventType)
    return entry?.channels[channel]?.enabled ?? false
  }
  return {
    emailDigest: effective('product.update', 'email'),
    securityAlerts: effective('auth.user.security_alert', 'email'),
    productUpdates: effective('product.update', 'in_app'),
    teamInvites: effective('auth.user.invited', 'email'),
    mentionsOnly: effective('mention.received', 'in_app'),
  }
}
