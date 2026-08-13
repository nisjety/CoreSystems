import { Coins, Loader2, ShieldCheck } from 'lucide-solid'
import { createMemo, createResource, createSignal, For, Show } from 'solid-js'
import {
  listOrgQuotas,
  microsToUsd,
  setOrgQuota,
  usdToMicros,
  QUOTA_KEY_MAX_COST_PER_RUN_USD_MICROS,
  QUOTA_KEY_MAX_TOKENS_PER_RUN,
  type OrgQuota,
  type QuotaResetPeriod,
} from '@/shared/api/org-quota-client'
import { translateApiError, useI18n } from '@/shared/i18n'
import { SectionHeader, SettingsButton } from '@/features/settings/components/settings-ui'
import { VerevonInput } from '@/shared/ui/verevon/VerevonInput'
import { getSession } from '@/shared/session/session-store'
import { hasWorkspaceAdminAccess } from '@/shared/session/access'

/**
 * Forbrukstak — per-run cost and token ceilings owned by Control Plane
 * (org-core `org_quotas`) and enforced by model-gateway's `org_quota.rs` in
 * cost-core's budget check.
 *
 * Deliberately a separate section from Fakturering. The billing screen shows
 * PLAN quotas (seats, api_calls, storage) sourced from billing-core; these are
 * operational spend guards with a different owner, a different enforcement
 * point, and — unlike plan quotas — they are settable. Merging them would
 * suggest changing one affects the other.
 *
 * The enforcement side of this has been live for a while; what was missing was
 * any way for an admin to set a ceiling, so the API had no caller.
 */

/** A settable ceiling and how to present it. */
interface QuotaField {
  readonly key: string
  readonly label: string
  readonly help: string
  /** Cost is stored in micro-dollars; tokens are stored as-is. */
  readonly unit: 'usd' | 'count'
  readonly placeholder: string
}

const QUOTA_FIELDS: readonly QuotaField[] = [
  {
    key: QUOTA_KEY_MAX_COST_PER_RUN_USD_MICROS,
    label: 'Maks kostnad per kjøring (USD)',
    help: 'Øvre grense for hva én kjøring får koste. Lagres i mikro-dollar; skriv beløpet i dollar, for eksempel 2,50.',
    unit: 'usd',
    placeholder: '2.50',
  },
  {
    key: QUOTA_KEY_MAX_TOKENS_PER_RUN,
    label: 'Maks tokens per kjøring',
    help: 'Øvre grense for antall tokens én kjøring får bruke.',
    unit: 'count',
    placeholder: '100000',
  },
]

const RESET_PERIODS: readonly { value: QuotaResetPeriod; label: string }[] = [
  { value: 'none', label: 'Ingen tilbakestilling' },
  { value: 'daily', label: 'Daglig' },
  { value: 'monthly', label: 'Månedlig' },
]

/** Present a stored limit in the field's own unit. */
function displayLimit(quota: OrgQuota | undefined, field: QuotaField): string {
  if (!quota) return ''
  if (field.unit === 'usd') {
    const usd = microsToUsd(quota.limit)
    // Trim a trailing .00 so a whole-dollar ceiling reads as "2", not "2.00",
    // and still round-trips through usdToMicros unchanged.
    return Number.isInteger(usd) ? String(usd) : usd.toFixed(2)
  }
  return String(quota.limit)
}

function formatWhen(value?: string | null): string {
  if (!value) return '—'
  try {
    return new Date(value).toLocaleString('nb-NO', { dateStyle: 'short', timeStyle: 'short' })
  } catch {
    return value
  }
}

export function OrgQuotasSection() {
  const i18n = useI18n()
  const session = getSession()
  const orgId = createMemo(() => session.activeOrg?.id ?? '')
  const isAdmin = createMemo(() => hasWorkspaceAdminAccess(session))

  const [quotas, { refetch }] = createResource(
    () => (isAdmin() && orgId() ? orgId() : undefined),
    (id) => listOrgQuotas(id).then((result) => result.quotas),
  )

  const [drafts, setDrafts] = createSignal<Record<string, string>>({})
  const [periods, setPeriods] = createSignal<Record<string, QuotaResetPeriod>>({})
  const [savingKey, setSavingKey] = createSignal<string | null>(null)
  const [fieldError, setFieldError] = createSignal<Record<string, string>>({})
  const [savedKey, setSavedKey] = createSignal<string | null>(null)

  const byKey = createMemo(() => {
    const map: Record<string, OrgQuota> = {}
    for (const quota of quotas() ?? []) map[quota.key] = quota
    return map
  })

  const currentValue = (field: QuotaField): string => {
    const draft = drafts()[field.key]
    if (draft !== undefined) return draft
    return displayLimit(byKey()[field.key], field)
  }

  const currentPeriod = (field: QuotaField): QuotaResetPeriod => {
    const draft = periods()[field.key]
    if (draft) return draft
    const stored = byKey()[field.key]?.reset_period
    if (stored === 'daily' || stored === 'monthly' || stored === 'none') return stored
    return 'none'
  }

  const setError = (key: string, message: string | null) => {
    setFieldError((prev) => {
      const next = { ...prev }
      if (message) next[key] = message
      else delete next[key]
      return next
    })
  }

  const handleSave = async (field: QuotaField) => {
    const id = orgId()
    if (!id || savingKey()) return

    const raw = currentValue(field).trim()
    if (!raw) {
      setError(field.key, 'Oppgi et tak. 0 betyr ingen tildeling.')
      return
    }
    // Accept a comma as the decimal separator: this UI is Norwegian and a
    // Norwegian keyboard produces "2,50" for a cost ceiling.
    const normalized = raw.replace(',', '.')
    const parsed = Number(normalized)
    if (!Number.isFinite(parsed) || parsed < 0) {
      setError(field.key, 'Taket må være et tall som er 0 eller høyere.')
      return
    }
    if (field.unit === 'count' && !Number.isInteger(parsed)) {
      setError(field.key, 'Antall tokens må være et heltall.')
      return
    }

    const limit = field.unit === 'usd' ? usdToMicros(parsed) : parsed
    setSavingKey(field.key)
    setError(field.key, null)
    setSavedKey(null)
    try {
      await setOrgQuota(id, field.key, { limit, reset_period: currentPeriod(field) })
      // Drop the draft so the field falls back to the persisted value.
      setDrafts((prev) => {
        const next = { ...prev }
        delete next[field.key]
        return next
      })
      setSavedKey(field.key)
      await refetch()
    } catch (err) {
      setError(
        field.key,
        translateApiError(err, i18n.tr, {
          no: 'Kunne ikke lagre taket.',
          en: 'Could not save the limit.',
        }),
      )
    } finally {
      setSavingKey(null)
    }
  }

  return (
    <>
      <SectionHeader
        title="Forbrukstak"
        description="Tak for kostnad og tokens per kjøring. Model Plane leser disse og håndhever dem i budsjettsjekken."
      />

      <Show
        when={isAdmin()}
        fallback={
          <p class="verevon-settings-subnote">
            <ShieldCheck size={14} aria-hidden="true" /> Bare administratorer kan se eller
            endre forbrukstak.
          </p>
        }
      >
        <Show when={quotas.error}>
          <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
            Kunne ikke laste forbrukstak.
          </p>
        </Show>

        <Show
          when={!quotas.loading}
          fallback={
            <p class="verevon-settings-subnote" role="status" aria-busy="true">
              Laster forbrukstak…
            </p>
          }
        >
          <div class="verevon-settings-field-grid verevon-settings-field-grid--spaced">
            <For each={QUOTA_FIELDS}>
              {(field) => {
                const stored = () => byKey()[field.key]
                return (
                  <label for={`quota-${field.key}`} class="verevon-settings-field">
                    <span class="verevon-settings-label">{field.label}</span>
                    <span class="verevon-settings-input-wrap">
                      <VerevonInput
                        id={`quota-${field.key}`}
                        value={currentValue(field)}
                        placeholder={field.placeholder}
                        inputmode="decimal"
                        onInput={(event) =>
                          setDrafts((prev) => ({ ...prev, [field.key]: event.currentTarget.value }))
                        }
                        class="verevon-settings-input"
                      />
                    </span>
                    <span class="verevon-settings-help">{field.help}</span>

                    <span class="verevon-settings-input-wrap">
                      <select
                        aria-label={`Tilbakestilling for ${field.label}`}
                        value={currentPeriod(field)}
                        onChange={(event) =>
                          setPeriods((prev) => ({
                            ...prev,
                            [field.key]: event.currentTarget.value as QuotaResetPeriod,
                          }))
                        }
                        class="verevon-settings-input verevon-settings-select"
                      >
                        <For each={RESET_PERIODS}>
                          {(period) => <option value={period.value}>{period.label}</option>}
                        </For>
                      </select>
                    </span>

                    <Show when={stored()}>
                      {(quota) => (
                        <span class="verevon-settings-help">
                          Brukt så langt: <code>{quota().value}</code>
                          {' '}· sist endret: {formatWhen(quota().updated_at)}
                          <Show when={quota().last_reset_at}>
                            {' '}· sist tilbakestilt: {formatWhen(quota().last_reset_at)}
                          </Show>
                        </span>
                      )}
                    </Show>

                    <Show when={fieldError()[field.key]}>
                      {(message) => (
                        <span
                          class="verevon-settings-status-message verevon-settings-status-message--error"
                          role="alert"
                        >
                          {message()}
                        </span>
                      )}
                    </Show>

                    <span>
                      <SettingsButton
                        type="button"
                        variant="primary"
                        settingsSize="sm"
                        disabled={savingKey() === field.key}
                        onClick={() => void handleSave(field)}
                      >
                        <Show
                          when={savingKey() === field.key}
                          fallback={
                            <>
                              <Coins size={14} aria-hidden="true" />{' '}
                              {stored() ? 'Oppdater tak' : 'Sett tak'}
                            </>
                          }
                        >
                          <Loader2 size={14} aria-hidden="true" /> Lagrer…
                        </Show>
                      </SettingsButton>
                      <Show when={savedKey() === field.key && savingKey() !== field.key}>
                        {' '}
                        <span class="verevon-trust-chip" role="status">
                          lagret
                        </span>
                      </Show>
                    </span>
                  </label>
                )
              }}
            </For>
          </div>
        </Show>

        <p class="verevon-settings-subnote">
          <Coins size={14} aria-hidden="true" /> Bare taket er innstillbart. Forbruket
          måles av tjenesten som bruker kvoten, så å heve et tak gir ingen ny tildeling
          av allerede brukt forbruk. Dette er ikke plankvotene under Fakturering.
        </p>
      </Show>
    </>
  )
}
