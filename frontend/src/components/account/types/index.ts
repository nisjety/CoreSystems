// ─── Account section identifiers ──────────────────────────────────────────────
export const ACCOUNT_NAV_SECTION_IDS = [
  'profile',
  'security',
  'linked-accounts',
  'notifications',
] as const

export const ACCOUNT_SECTION_IDS = [
  ...ACCOUNT_NAV_SECTION_IDS,
  'danger',
] as const

export type AccountNavSectionId = (typeof ACCOUNT_NAV_SECTION_IDS)[number]
export type AccountSectionId = (typeof ACCOUNT_SECTION_IDS)[number]

export function isAccountNavSectionId(value: string): value is AccountNavSectionId {
  return ACCOUNT_NAV_SECTION_IDS.includes(value as AccountNavSectionId)
}

// ─── Profile form ──────────────────────────────────────────────────────────────
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

// ─── Security form ─────────────────────────────────────────────────────────────
export interface SecurityFormValues {
  currentPassword: string
  newPassword: string
  confirmPassword: string
}

// ─── Notification preferences ─────────────────────────────────────────────────
export interface NotificationPreferences {
  emailDigest: boolean
  securityAlerts: boolean
  productUpdates: boolean
  teamInvites: boolean
  mentionsOnly: boolean
}

// ─── Billing summary (derived from OrgBilling) ────────────────────────────────
export interface BillingSummary {
  plan: 'free' | 'pro' | 'enterprise'
  status: string
  billingEmail: string | undefined
  currentPeriodEnd: Date | undefined
  trialEndsAt: Date | undefined
}
