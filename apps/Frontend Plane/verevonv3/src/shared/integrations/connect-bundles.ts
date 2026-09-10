/**
 * The ONE consent-bundle policy for integration-core v2 connect sessions.
 *
 * Onboarding, Settings → Integrations, Support ("Koble til Outlook") and Inbox
 * all start the same `POST /providers/{key}/connect-session`, and the OAuth
 * callback upgrades the org's existing connection in place. Until this module
 * each surface picked bundles on its own — onboarding asked for `onboarding`
 * (Microsoft: profile + SharePoint read + Teams *metadata*, no mail scope),
 * Support/Inbox asked for `full`, Settings had a third table — so a Microsoft
 * 365 connection made in onboarding left Support saying "Ingen e-postkonto
 * tilkoblet ennå" and asked for a second connection.
 *
 * The policy is driven by the product sources the user picked (the ids on the
 * onboarding connector cards / Knowledge provider list): each source maps to
 * the smallest catalog bundle that grants it, and the bundles are unioned.
 * When a surface connects a provider as a whole (Settings "Koble til", Support
 * "Koble til Outlook") it passes no sources and gets the provider's default
 * card sources — so a connection made anywhere carries what every surface
 * needs, and a reconnect from any surface upgrades to the same set.
 *
 * Bundle keys mirror integration-corev2 `internal/providers/catalog.go`.
 * Providers whose scopes need separate product review (Meta, LinkedIn) keep a
 * fixed narrow bundle regardless of sources — see notes inline.
 */

type SourceGroup = {
  /** Product source ids (onboarding card `sources`, Knowledge provider `sources`). */
  sources: readonly string[]
  /** Catalog bundle key that grants this group. */
  bundle: string
}

type ProviderBundlePolicy = {
  /** Sources assumed when a surface connects the provider as a whole. */
  defaultSources: readonly string[]
  groups: readonly SourceGroup[]
  /** Bundle used for sources no group claims (identity-only preview). */
  fallback?: string
}

const providerPolicies: Record<string, ProviderBundlePolicy> = {
  microsoft: {
    // The "Microsoft 365" card: Teams, Outlook, SharePoint, OneDrive.
    defaultSources: ['sharepoint', 'onedrive', 'outlook', 'teams'],
    groups: [
      // `onboarding` = profile.read + sharepoint.read + teams.read — the
      // Knowledge/finspo-core content grant (catalog `knowledge` is identical).
      { sources: ['sharepoint', 'onedrive', 'documents', 'files'], bundle: 'onboarding' },
      // `inbox` = profile.read + mail.read + mail.send + teams.read +
      // teams.messages.read — what Support/Inbox mailbox and Teams lanes need.
      { sources: ['outlook', 'mail', 'email', 'teams', 'teams_messages'], bundle: 'inbox' },
    ],
    fallback: 'onboarding',
  },
  google: {
    // The "Google Workspace" card: Drive, documents, Gmail and calendar.
    defaultSources: ['google_drive', 'documents', 'gmail', 'calendar'],
    groups: [
      { sources: ['google_drive', 'drive', 'documents', 'files'], bundle: 'knowledge' },
      { sources: ['gmail', 'mail', 'email'], bundle: 'inbox' },
      // Calendar read only exists in Google's `full` bundle.
      { sources: ['calendar'], bundle: 'full' },
    ],
    fallback: 'onboarding',
  },
  slack: {
    defaultSources: ['messages'],
    groups: [
      { sources: ['channels', 'files', 'threads'], bundle: 'knowledge' },
      { sources: ['messages', 'direct_messages', 'inbox'], bundle: 'inbox' },
    ],
    fallback: 'onboarding',
  },
  notion: {
    defaultSources: ['pages', 'databases'],
    groups: [{ sources: ['pages', 'databases', 'content'], bundle: 'knowledge' }],
    fallback: 'onboarding',
  },
  github: {
    defaultSources: ['repos', 'readme', 'issues'],
    groups: [{ sources: ['repos', 'readme', 'issues', 'pulls', 'commits'], bundle: 'knowledge' }],
    fallback: 'onboarding',
  },
  shopify: {
    defaultSources: ['products', 'orders', 'customers'],
    groups: [{ sources: ['products', 'orders', 'customers', 'content'], bundle: 'commerce' }],
    fallback: 'onboarding',
  },
  stripe: {
    defaultSources: ['customers', 'subscriptions', 'invoices'],
    groups: [{ sources: ['customers', 'subscriptions', 'invoices', 'billing'], bundle: 'billing' }],
    fallback: 'onboarding',
  },
}

/** Providers whose consent is fixed by product review, not by chosen sources. */
const fixedBundles: Record<string, readonly string[]> = {
  // Meta reviews product permissions independently; its `full` bundle mixes
  // Page, Instagram, WhatsApp, Ads, Catalog and Threads and is rejected for a
  // Messenger-only app. Feature-specific consent is requested later.
  meta: ['messenger'],
  // Instagram Inbox is a distinct Meta product and OAuth client.
  instagram: ['inbox'],
  // LinkedIn's marketing scopes are reviewed/partner-only; the first
  // connection may only request identity access.
  linkedin: ['onboarding'],
  // Discord's `inbox` bundle is its whole read surface (profile + guilds +
  // messages); `full` is identical.
  discord: ['inbox'],
  // X: DM/mention signals live in `inbox` (which includes profile read);
  // publishing is a separate, explicit consent.
  x: ['inbox'],
}

/**
 * Bundle keys to request for `provider`, derived from the product sources the
 * user picked. Pass no sources (or an empty list) to connect the provider as a
 * whole — the provider's default card sources apply.
 */
export function connectBundlesForSources(provider: string, sources?: readonly string[]): string[] {
  const key = normalizeProviderKey(provider)
  const fixed = fixedBundles[key]
  if (fixed) return [...fixed]

  const policy = providerPolicies[key]
  const requested = (sources ?? [])
    .map((source) => source.trim().toLowerCase())
    .filter(Boolean)
  if (!policy) {
    // Unknown catalog entry: an explicit source pick gets the safe preview
    // bundle; a whole-provider connect keeps the established `full` grant.
    return requested.length > 0 ? ['onboarding'] : ['full']
  }

  const effective = requested.length > 0 ? requested : [...policy.defaultSources]
  // Emit bundles in policy order (content grant before inbox grant) so the
  // result is stable regardless of how a caller ordered its sources.
  const bundles: string[] = []
  for (const group of policy.groups) {
    if (effective.some((source) => group.sources.includes(source)) && !bundles.includes(group.bundle)) {
      bundles.push(group.bundle)
    }
  }
  const unclaimed = effective.some((source) => !policy.groups.some((group) => group.sources.includes(source)))
  if (bundles.includes('full')) return ['full']
  if (unclaimed && policy.fallback && !bundles.includes(policy.fallback)) bundles.push(policy.fallback)
  if (bundles.length === 0) bundles.push(policy.fallback ?? 'onboarding')
  return bundles
}

/** Instagram Inbox is a distinct Meta product and OAuth client. Never add
 * its permissions to the Messenger app's consent URL. */
export function instagramInboxConnectionRequest(): { bundles: string[]; providerKey: 'instagram' } {
  return { providerKey: 'instagram', bundles: connectBundlesForSources('instagram') }
}

function normalizeProviderKey(value: string): string {
  switch (value.trim().toLowerCase().replaceAll('_', '-')) {
    case 'microsoft-365':
    case 'microsoft365':
    case 'microsoft-graph':
    case 'm365':
    case 'outlook':
    case 'teams':
    case 'sharepoint':
    case 'onedrive':
      return 'microsoft'
    case 'google-workspace':
    case 'gmail':
    case 'gdrive':
    case 'google-drive':
      return 'google'
    case 'twitter':
      return 'x'
    case 'meta-unified':
    case 'facebook':
      return 'meta'
    default:
      return value.trim().toLowerCase()
  }
}
