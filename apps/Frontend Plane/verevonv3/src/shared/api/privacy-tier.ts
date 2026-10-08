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
 * Encode the UI's readable tier name for Model Gateway's protobuf-backed
 * request contract. `/v1/models` may expose either names or ordinals, but the
 * invoke boundary accepts the numeric `PrivacyTier` value only.
 */
export function privacyTierWireValue(
  tier: PrivacyTier | undefined | null,
): number | undefined {
  if (!isSelectablePrivacyTier(tier)) return undefined
  const ordinal = PRIVACY_TIERS.indexOf(tier)
  return ordinal > 0 ? ordinal : undefined
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
        'Erklært: behandles kun i Norge, på norskdrevet infrastruktur, med registrert null datalagring (ikke uavhengig verifisert)',
        'Declared: processed only in Norway on Norwegian-operated infrastructure, with a recorded zero-data-retention attestation (not independently verified)',
      )
    case 'eu_resident':
      return i18n.tr(
        'Leverandørens region er satt til EU/EØS (erklært, ikke uavhengig verifisert)',
        'Provider region is set to EU/EEA (declared, not independently verified)',
      )
    case 'zdr_contractual':
      return i18n.tr(
        'Leverandøren har en registrert ZDR-godkjenning (vår egen registrering, ikke uavhengig verifisert)',
        'The provider has a recorded zero-data-retention attestation (our own record, not independently verified)',
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
    'Sikreste nivå: færre modeller er tilgjengelige – erklært behandling kun i Norge.',
    'Strongest tier: fewer models are available – declared processing stays in Norway.',
  )
}

// ── Where an answer was processed ───────────────────────────────────────────
// The gateway stamps the serving provider, the residency that provider declares
// and the strongest privacy tier it can substantiate onto each answer's `usage`
// event (`provider_used`, `residency`, `privacy_tier`). The honesty rule above
// applies in full: a claim is rendered ONLY from a value the server asserted,
// never from a model name, and a gateway that sends none (an older one, or a
// turn no provider served) renders nothing at all.

/** Residency labels exactly as the server stamps them (`global`, `eu`, `norway`). */
export const ANSWER_RESIDENCIES = ['global', 'eu', 'norway'] as const

export type AnswerResidency = (typeof ANSWER_RESIDENCIES)[number]

/** What the server stamped on one answer. Absent fields are unknown, never defaulted. */
export type AnswerServedBy = {
  /** Registry id of the serving provider, as stamped. */
  providerUsed?: string
  residency?: AnswerResidency
  /** Never `'unspecified'`: that is "no constraint", and a stamp says something. */
  privacyTier?: Exclude<PrivacyTier, 'unspecified'>
}

/** Coerce an untrusted wire value into a stamped residency, or `undefined`. */
export function normalizeAnswerResidency(value: unknown): AnswerResidency | undefined {
  if (typeof value !== 'string') return undefined
  const key = value.trim().toLowerCase()
  return (ANSWER_RESIDENCIES as readonly string[]).includes(key)
    ? (key as AnswerResidency)
    : undefined
}

/**
 * Coerce the three untrusted `usage` fields into what the server stamped, or
 * `undefined` when none of them is usable (the common case on an older
 * gateway). A provider id is kept only when it has the shape of a registry id,
 * so nothing free-form can reach the page through it.
 */
export function normalizeAnswerServedBy(wire: {
  provider_used?: unknown
  residency?: unknown
  privacy_tier?: unknown
}): AnswerServedBy | undefined {
  const providerRaw = typeof wire.provider_used === 'string' ? wire.provider_used.trim() : ''
  const providerUsed = /^[a-z0-9][a-z0-9._-]{0,63}$/i.test(providerRaw) ? providerRaw : undefined
  const residency = normalizeAnswerResidency(wire.residency)
  const tier = normalizePrivacyTier(wire.privacy_tier)
  const privacyTier = tier === undefined || tier === 'unspecified' ? undefined : tier
  if (providerUsed === undefined && residency === undefined && privacyTier === undefined) {
    return undefined
  }
  return {
    ...(providerUsed === undefined ? {} : { providerUsed }),
    ...(residency === undefined ? {} : { residency }),
    ...(privacyTier === undefined ? {} : { privacyTier }),
  }
}

/**
 * A name for the serving provider where the id is one we know how to name.
 * The ChatGPT-subscription route and anything unrecognised get none: the line
 * then says where the answer was processed without naming a vendor.
 */
function answerProviderLabel(providerUsed: string | undefined): string | undefined {
  const id = providerUsed?.trim().toLowerCase()
  if (!id) return undefined
  if (id === 'azure' || id.startsWith('azure-')) return 'Azure'
  if (id === 'openai') return 'OpenAI'
  if (id === 'anthropic') return 'Anthropic'
  return undefined
}

export type AnswerProcessingNote = {
  /** The one small line, for example `Behandlet: EU (Azure), ZDR`. */
  text: string
  /** The hover explanation, saying what the line does and does not promise. */
  title: string
}

/**
 * The one small line under an answer saying where it was processed, from the
 * server's stamp alone: `Behandlet: EU (Azure), ZDR` for an EU deployment under
 * a retention contract, and `Behandlet: global, ingen EU- eller ZDR-garanti`
 * where nothing stronger is promised, as on the ChatGPT-subscription route.
 *
 * Returns `undefined`, so nothing is rendered, when the stamp carries no
 * residency and no retention claim. A tier fills in the region only where it
 * says the same thing (`eu_resident`, `sovereign`, `global`); where the stamped
 * residency and the tier disagree, the weaker reading wins. `ZDR` is the
 * server's attestation, not "nothing is stored", and the hover text says so.
 */
export function answerProcessingNote(
  i18n: { tr(no: string, en: string): string },
  servedBy: AnswerServedBy | undefined | null,
): AnswerProcessingNote | undefined {
  if (!servedBy) return undefined
  const tier = servedBy.privacyTier
  const zdr = tier === 'zdr_contractual' || tier === 'sovereign'
  const residency: AnswerResidency | undefined = servedBy.residency
    ?? (tier === 'eu_resident' ? 'eu' : tier === 'sovereign' ? 'norway' : tier === 'global' ? 'global' : undefined)
  if (residency === undefined && !zdr) return undefined

  const region = residency === 'eu'
    ? 'EU'
    : residency === 'norway'
      ? i18n.tr('Norge', 'Norway')
      : residency === 'global'
        ? i18n.tr('global', 'global')
        : undefined
  const provider = region === undefined ? undefined : answerProviderLabel(servedBy.providerUsed)
  const parts: string[] = []
  if (region !== undefined) parts.push(provider ? `${region} (${provider})` : region)
  if (residency === 'global') {
    parts.push(zdr
      ? i18n.tr('ingen EU-garanti', 'no EU guarantee')
      : i18n.tr('ingen EU- eller ZDR-garanti', 'no EU or ZDR guarantee'))
  }
  if (zdr) parts.push('ZDR')

  const unguaranteed = residency === 'global' && !zdr
  return {
    text: `${i18n.tr('Behandlet', 'Processed')}: ${parts.join(', ')}`,
    title: unguaranteed
      ? i18n.tr(
          'Dette svaret kan være behandlet utenfor EU/EØS, og null datalagring er ikke lovet.',
          'This answer may have been processed outside the EU/EEA, and zero data retention is not promised.',
        )
      : i18n.tr(
          'Behandlingssted og datalagring slik oppsettet erklærer dem. Ikke uavhengig verifisert.',
          'Processing location and data retention as the deployment declares them. Not independently verified.',
        ),
  }
}
