/**
 * Chooses the first consent bundle for a user-initiated Settings connection.
 *
 * Meta's product permissions are independently reviewed. Its generic `full`
 * bundle combines unrelated Page, Instagram, WhatsApp, Ads, Catalog, and
 * Threads capabilities, which Meta rejects for a Messenger-only app. LinkedIn
 * has the same constraint for its reviewed/partner-only marketing scopes, so
 * its first connection must only request identity access. Support providers
 * use their inbox bundle so consent is limited to the capability that the
 * Support surface can actually ingest. Start with the narrowly scoped bundle
 * for providers whose full set requires separate product approval;
 * feature-specific consent is requested later.
 */
export function connectBundlesForProvider(providerKey: string): string[] {
  const normalizedProvider = providerKey.trim().toLowerCase()
  if (normalizedProvider === 'meta') return ['messenger']
  if (normalizedProvider === 'linkedin') return ['onboarding']
  if (normalizedProvider === 'slack' || normalizedProvider === 'discord' || normalizedProvider === 'x') {
    return ['inbox']
  }
  return ['full']
}

/** Instagram Inbox is a distinct Meta product and OAuth client. Never add
 * its permissions to the Messenger app's consent URL. */
export function instagramInboxConnectionRequest(): { bundles: string[]; providerKey: 'instagram' } {
  return { providerKey: 'instagram', bundles: ['inbox'] }
}
