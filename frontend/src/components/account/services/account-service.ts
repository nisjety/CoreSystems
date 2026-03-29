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

  // ── Notifications (stub — extend when backend endpoint exists) ─────────────

  async getNotificationPreferences(): Promise<NotificationPreferences> {
    // TODO: wire to user-service preferences endpoint
    return {
      emailDigest: true,
      securityAlerts: true,
      productUpdates: false,
      teamInvites: true,
      mentionsOnly: false,
    }
  }

  async updateNotificationPreferences(
    _prefs: Partial<NotificationPreferences>,
  ): Promise<void> {
    // TODO: wire to user-service preferences endpoint
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
