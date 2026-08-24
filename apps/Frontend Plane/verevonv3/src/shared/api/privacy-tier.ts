// ── Privacy tiers (Venice-style programmatic privacy tiering) ────────────────
// Pinned wire contract (plan "Start: Firecrawl-parity finishers (Quarry-v2) +
// Venice privacy tiers", mirroring `PROVIDER_AND_PRIVACY_STRATEGY.md` §4):
//
//   unspecified < global < eu_resident < zdr_contractual < sovereign
//
// The backend derives each catalog model's tier from its provider Residency +
// supports_zdr attestation, exposes `privacy_tier` + `residency` per model on
// `/v1/models`, and accepts `min_privacy_tier` on the invoke path. Enforcement
// is server-side (ineligible providers are skipped, an empty remainder fails
// CLOSED with a typed error naming the tier — never a silent downgrade); this
// module only carries the values honestly to the UI.
//
// Honesty rule (mirrors the stated/inferred memory-provenance rule): an
// affirmative claim is rendered ONLY when the backend actually asserted one.
// Unknown/garbage values degrade to `undefined` — neutral presentation, no
// badge, no fabricated residency claim.

/** Wire values exactly as `/v1/models` emits them (snake_case). */
export const PRIVACY_TIERS = [
  'unspecified',
  'global',
  'eu_resident',
  'zdr_contractual',
  'sovereign',
] as const

export type PrivacyTier = (typeof PRIVACY_TIERS)[number]

/**
 * Coerce an untrusted wire value into a {@link PrivacyTier}.
 *
 * Accepts the snake_case wire strings, their camelCase aliases (defensive,
 * matching the rest of the API layer), or the raw proto ordinal. Anything else
 * — including `'unspecified'` spelled wrong or an unknown future tier — is
 * `undefined`: the UI then renders nothing rather than guessing a claim.
 */
export function normalizePrivacyTier(value: unknown): PrivacyTier | undefined {
  if (typeof value === 'number') {
    // Proto ordinal (UNSPECIFIED=0 … SOVEREIGN=4), weakest→strongest.
    return PRIVACY_TIERS[value] as PrivacyTier | undefined
  }
  if (typeof value !== 'string') return undefined
  const key = value.trim().toLowerCase()
  if ((PRIVACY_TIERS as readonly string[]).includes(key)) return key as PrivacyTier
  // camelCase aliases, defensively (the rest of the API layer accepts both
  // spellings; the wire itself is snake_case).
  if (key === 'euresident') return 'eu_resident'
  if (key === 'zdrcontractual') return 'zdr_contractual'
  return undefined
}

/**
 * Tiers whose value is a POSITIVE claim the backend actually attested
 * (`Residency >= Eu`, plus a ZDR contract for `zdr_contractual`).
 *
 * Only these earn the affirmative tint. `global` states a fact plainly;
 * `unspecified`/unknown state nothing at all.
 */
export function isClaimedPrivacyTier(tier: PrivacyTier | undefined | null): boolean {
  return tier === 'eu_resident' || tier === 'zdr_contractual' || tier === 'sovereign'
}

/**
 * Whether a tier expresses a REAL constraint worth serializing or rendering.
 * `'unspecified'` (and absence) mean "no constraint" — today's behavior — so
 * it must never ride the wire as `min_privacy_tier` nor light up a badge;
 * truthy-string checks alone would wrongly serialize it.
 */
export function isSelectablePrivacyTier(
  tier: PrivacyTier | undefined | null,
): tier is Exclude<PrivacyTier, 'unspecified'> {
  return tier !== undefined && tier !== null && tier !== 'unspecified'
}

/**
 * Short badge label for the picker's tier chip. Norwegian-first via `i18n.tr`,
 * matching the rest of the composer copy.
 */
export function privacyTierBadgeLabel(
  i18n: { tr(no: string, en: string): string },
  tier: PrivacyTier,
): string {
  switch (tier) {
    case 'sovereign':
      return i18n.tr('Soveren', 'Sovereign')
    case 'eu_resident':
      return i18n.tr('EU', 'EU')
    case 'zdr_contractual':
      return i18n.tr('ZDR', 'ZDR')
    case 'global':
      return i18n.tr('Global', 'Global')
    default:
      return ''
  }
}

/**
 * Tooltip for a tier badge: what the tier claims, in one honest sentence.
 */
export function privacyTierBadgeTitle(
  i18n: { tr(no: string, en: string): string },
  tier: PrivacyTier,
): string {
  switch (tier) {
    case 'sovereign':
      return i18n.tr(
        'Behandles kun i Norge – aldri utenfor norsk jurisdiksjon',
        'Processed only in Norway – never outside Norwegian jurisdiction',
      )
    case 'eu_resident':
      return i18n.tr(
        'Beholdes innenfor EU/EØS-jurisdiksjon',
        'Kept within EU/EEA jurisdiction',
      )
    case 'zdr_contractual':
      return i18n.tr(
        'Null datalagring – ingenting lagres etter svaret',
        'Zero data retention – nothing is stored after the answer',
      )
    case 'global':
      return i18n.tr('Ingen regionsbegrensning', 'No region restriction')
    default:
      return ''
  }
}

/**
 * The honest smaller-catalog notice shown when the user selects a SOVEREIGN
 * model. Sovereign pinning narrows the fleet to Norway-resident providers, so
 * fewer models are available — the response says so rather than silently
 * downgrading quality. Callers gate on `tier === 'sovereign'`: no other tier
 * narrows the catalog, so no other selection earns a notice.
 */
export function sovereignCatalogNotice(
  i18n: { tr(no: string, en: string): string },
): string {
  return i18n.tr(
    'Sikreste nivå: færre modeller er tilgjengelige – behandling kun i Norge.',
    'Strongest tier: fewer models are available – processing stays in Norway.',
  )
}
