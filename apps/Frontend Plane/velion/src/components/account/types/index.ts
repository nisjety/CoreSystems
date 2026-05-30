export interface AccountProfileFormValues {
  displayName: string
  bio: string
  location: string
  timezone: string
  jobTitle: string
  department: string
  // Social / web links
  website: string
  linkedIn: string
  github: string
  twitter: string
}

// ─── Notification preferences ─────────────────────────────────────────────────
export interface NotificationPreferences {
  emailDigest: boolean
  securityAlerts: boolean
  productUpdates: boolean
  teamInvites: boolean
  mentionsOnly: boolean
}
