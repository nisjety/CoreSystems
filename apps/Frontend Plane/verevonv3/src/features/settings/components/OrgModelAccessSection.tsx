import { BrainCircuit, Loader2, ShieldCheck } from '@/shared/icons'
import { createMemo, createSignal, For, Show } from 'solid-js'
import { createResource } from '@/shared/lib/create-resource-compat'
import {
  getModelEntitlement,
  putModelEntitlement,
  type ModelAllowMode,
  type ModelPolicy,
} from '@/shared/api/model-entitlement-client'
import { EFFORT_LEVELS } from '@/shared/api/chat-client'
import { translateApiError, useI18n } from '@/shared/i18n'
import {
  SectionHeader,
  SettingsButton,
  SettingsSelect,
  SettingsTextarea,
} from '@/features/settings/components/settings-ui'
import { VerevonInput } from '@/shared/ui/verevon/VerevonInput'
import { getSession } from '@/shared/session/session-store'
import { hasWorkspaceAdminAccess } from '@/shared/session/access'

/**
 * Modelltilgang: which models an organisation's members may use, and the highest
 * reasoning effort they may ask for. org-core stores and resolves the policy
 * (ADR-0005), inference-core enforces it; this section only edits the stored set
 * through the gateway.
 *
 * The section is shown when the gateway reports the admin switch on
 * (`{ enabled: true }`), never from a build-time flag: with the switch off
 * nothing enforces a policy, so there is nothing honest to edit.
 *
 * It edits what org-core accepts and nothing more: an organisation default and
 * one override per role. An override replaces the default for that role, and a
 * deny always wins; the copy says so, because the opposite reading (overrides add
 * to the default) is the natural one and would widen access.
 */

/** The layer a policy belongs to: the organisation default or a role override. */
type Layer = { readonly kind: 'org_default' } | { readonly kind: 'role'; readonly role: string }

const ORG_DEFAULT: Layer = { kind: 'org_default' }

/** The limits org-core and the gateway enforce; checked here only to answer before saving. */
const MAX_ENTRIES = 500
const MAX_ENTRY_LENGTH = 128
const MAX_ROLES = 20

const SUGGESTED_ROLES = ['member', 'admin'] as const

function layerKey(layer: Layer): string {
  return layer.kind === 'org_default' ? 'org_default' : `role:${layer.role}`
}

const emptyPolicy = (): ModelPolicy => ({ allow_mode: 'all' })

/** Entries as typed: one per line or comma separated, trimmed, duplicates and blanks dropped. */
export function parseModelEntries(text: string): string[] {
  const seen = new Set<string>()
  for (const part of text.split(/[\n,]/)) {
    const entry = part.trim()
    if (entry) seen.add(entry)
  }
  return [...seen]
}

/** The first thing wrong with a list the admin typed, in Norwegian, or null. */
function entryProblem(entries: readonly string[]): string | null {
  if (entries.length > MAX_ENTRIES) return `En liste kan ha høyst ${MAX_ENTRIES} oppføringer.`
  for (const entry of entries) {
    if (entry.length > MAX_ENTRY_LENGTH) return `«${entry.slice(0, 24)}…» er lengre enn ${MAX_ENTRY_LENGTH} tegn.`
    if (entry.includes('*')) return 'Mønstre med * støttes ikke. Skriv eksakte modell-ID-er, eller provider:<id>.'
  }
  return null
}

export function OrgModelAccessSection() {
  const i18n = useI18n()
  const session = getSession()
  const orgId = createMemo(() => session.activeOrg?.id ?? '')
  const isAdmin = createMemo(() => hasWorkspaceAdminAccess(session))

  const [state, { refetch }] = createResource(
    () => (isAdmin() && orgId() ? orgId() : undefined),
    (id) => getModelEntitlement(id),
  )

  // `undefined` until the admin edits: the stored set is then what is shown.
  const [orgDraft, setOrgDraft] = createSignal<ModelPolicy | null | undefined>(undefined)
  const [roleDrafts, setRoleDrafts] = createSignal<Record<string, ModelPolicy> | undefined>(undefined)
  // The list fields as typed, so a trailing newline survives until it is saved.
  const [texts, setTexts] = createSignal<Record<string, string>>({})
  const [newRole, setNewRole] = createSignal('')
  const [saving, setSaving] = createSignal(false)
  const [saved, setSaved] = createSignal(false)
  const [problem, setProblem] = createSignal<string | null>(null)

  const stored = createMemo(() => {
    const current = state()
    return current && current.enabled ? current : undefined
  })
  const enabled = createMemo(() => stored() !== undefined)

  const orgPolicy = (): ModelPolicy | null => {
    const draft = orgDraft()
    return draft !== undefined ? draft : (stored()?.org_default ?? null)
  }
  const roles = (): Record<string, ModelPolicy> => roleDrafts() ?? stored()?.roles ?? {}

  const policyOf = (layer: Layer): ModelPolicy | null =>
    layer.kind === 'org_default' ? orgPolicy() : (roles()[layer.role] ?? null)

  const setPolicy = (layer: Layer, next: ModelPolicy | null) => {
    setSaved(false)
    if (layer.kind === 'org_default') {
      setOrgDraft(next)
      return
    }
    const copy = { ...roles() }
    if (next) copy[layer.role] = next
    else delete copy[layer.role]
    setRoleDrafts(copy)
  }

  const patch = (layer: Layer, change: Partial<ModelPolicy>) => {
    setPolicy(layer, { ...(policyOf(layer) ?? emptyPolicy()), ...change })
  }

  const listText = (layer: Layer, field: 'allow' | 'deny'): string =>
    texts()[`${layerKey(layer)}:${field}`] ?? (policyOf(layer)?.[field] ?? []).join('\n')

  const typeList = (layer: Layer, field: 'allow' | 'deny', value: string) => {
    setSaved(false)
    setTexts((prev) => ({ ...prev, [`${layerKey(layer)}:${field}`]: value }))
  }

  /** The layer as it would be saved: typed lists parsed, an empty `allow`/`deny` left out. */
  const resolved = (layer: Layer): ModelPolicy | null => {
    const policy = policyOf(layer)
    if (!policy) return null
    const next: ModelPolicy = { ...policy, allow: parseModelEntries(listText(layer, 'allow')), deny: parseModelEntries(listText(layer, 'deny')) }
    if (next.allow?.length === 0) delete next.allow
    if (next.deny?.length === 0) delete next.deny
    return next
  }

  const roleNames = createMemo(() => Object.keys(roles()).sort())
  const layers = createMemo<Layer[]>(() => [ORG_DEFAULT, ...roleNames().map((role): Layer => ({ kind: 'role', role }))])

  const addRole = () => {
    const role = newRole().trim().toLowerCase()
    if (!role || roles()[role]) return
    if (!/^[a-z0-9][a-z0-9_-]{0,63}$/.test(role)) {
      setProblem('Rollenavnet kan bare ha små bokstaver, tall, - og _.')
      return
    }
    if (roleNames().length >= MAX_ROLES) {
      setProblem(`Det kan høyst være ${MAX_ROLES} rolleoverstyringer.`)
      return
    }
    setProblem(null)
    setPolicy({ kind: 'role', role }, emptyPolicy())
    setNewRole('')
  }

  const handleSave = async () => {
    const current = stored()
    const id = orgId()
    if (!current || !id || saving()) return

    const nextRoles: Record<string, ModelPolicy> = {}
    for (const role of roleNames()) {
      const policy = resolved({ kind: 'role', role })
      if (policy) nextRoles[role] = policy
    }
    const nextOrg = resolved(ORG_DEFAULT)
    for (const policy of [nextOrg, ...Object.values(nextRoles)]) {
      const wrong = policy ? entryProblem([...(policy.allow ?? []), ...(policy.deny ?? [])]) : null
      if (wrong) {
        setProblem(wrong)
        return
      }
      if (policy && policy.allow_mode === 'listed' && !(policy.allow?.length)) {
        setProblem('«Bare listede modeller» krever minst én modell i listen. Velg «Alle modeller» for å tillate alle.')
        return
      }
    }

    setSaving(true)
    setProblem(null)
    setSaved(false)
    try {
      await putModelEntitlement(id, {
        expected_revision: current.revision,
        org_default: nextOrg,
        roles: nextRoles,
      })
      setOrgDraft(undefined)
      setRoleDrafts(undefined)
      setTexts({})
      setSaved(true)
      await refetch()
    } catch (err) {
      setProblem(
        translateApiError(err, i18n.tr, {
          no: 'Kunne ikke lagre modelltilgangen.',
          en: 'Could not save the model access.',
        }),
      )
    } finally {
      setSaving(false)
    }
  }

  const layerTitle = (layer: Layer) =>
    layer.kind === 'org_default'
      ? i18n.tr('Organisasjonsstandard', 'Organisation default')
      : i18n.tr(`Rolle: ${layer.role}`, `Role: ${layer.role}`)

  const modeOptions = () => [
    { value: 'all', label: i18n.tr('Alle modeller', 'All models') },
    { value: 'listed', label: i18n.tr('Bare listede modeller', 'Only the listed models') },
  ]

  const effortOptions = () => [
    { value: '', label: i18n.tr('Ingen begrensning', 'No limit') },
    ...EFFORT_LEVELS.map((level) => ({ value: level, label: level })),
  ]

  return (
    <>
      <SectionHeader
        title="Modelltilgang"
        description="Hvilke AI-modeller medlemmene kan bruke, og hvor mye resonnering de kan be om. Model Plane håndhever dette; en modell som ikke er tillatt vises ikke i modellvelgeren."
      />

      <Show
        when={isAdmin()}
        fallback={
          <p class="verevon-settings-subnote">
            <ShieldCheck size={14} aria-hidden="true" /> Bare administratorer kan se eller
            endre modelltilgang.
          </p>
        }
      >
        <Show when={state.error}>
          <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
            Kunne ikke laste modelltilgang.
          </p>
        </Show>

        <Show
          when={!state.loading}
          fallback={
            <p class="verevon-settings-subnote" role="status" aria-busy="true">
              Laster modelltilgang…
            </p>
          }
        >
          <Show
            when={enabled()}
            fallback={
              <Show when={!state.error}>
                <p class="verevon-settings-subnote" role="status">
                  <BrainCircuit size={14} aria-hidden="true" /> Modelltilgang er ikke slått på i
                  denne installasjonen. Alle modeller er tilgjengelige for alle medlemmer.
                </p>
              </Show>
            }
          >
            <p class="verevon-settings-subnote">
              <BrainCircuit size={14} aria-hidden="true" /> Revisjon {stored()?.revision ?? 0}.
              En rolleoverstyring erstatter organisasjonsstandarden for den rollen, og en
              modell på nektelisten er alltid nektet. Eiere og administratorer styres av
              samme regler som andre roller.
            </p>

            <For each={layers()}>
              {(layer) => {
                const key = layerKey(layer)
                return (
                  <fieldset class="verevon-settings-field-grid verevon-settings-field-grid--spaced">
                    <legend class="verevon-settings-label">{layerTitle(layer)}</legend>
                    <Show
                      when={policyOf(layer)}
                      fallback={
                        <span>
                          <SettingsButton
                            type="button"
                            settingsSize="sm"
                            onClick={() => setPolicy(layer, emptyPolicy())}
                          >
                            Opprett organisasjonsstandard
                          </SettingsButton>
                        </span>
                      }
                    >
                      {(policy) => (
                        <>
                          <SettingsSelect
                            id={`model-access-${key}-mode`}
                            label="Tillatte modeller"
                            options={modeOptions()}
                            value={policy().allow_mode}
                            onChange={(event) =>
                              patch(layer, { allow_mode: event.currentTarget.value as ModelAllowMode })
                            }
                          />
                          <Show when={policy().allow_mode === 'listed'}>
                            <SettingsTextarea
                              id={`model-access-${key}-allow`}
                              label="Tillatelseliste (én per linje: modell-ID eller provider:<id>)"
                              value={listText(layer, 'allow')}
                              onInput={(event) => typeList(layer, 'allow', event.currentTarget.value)}
                            />
                          </Show>
                          <SettingsTextarea
                            id={`model-access-${key}-deny`}
                            label="Nektelisten (én per linje, vinner over tillatelser)"
                            value={listText(layer, 'deny')}
                            onInput={(event) => typeList(layer, 'deny', event.currentTarget.value)}
                          />
                          <SettingsSelect
                            id={`model-access-${key}-effort`}
                            label="Høyeste resonneringsinnsats"
                            options={effortOptions()}
                            value={policy().max_effort ?? ''}
                            onChange={(event) => {
                              const value = event.currentTarget.value
                              const next = { ...policy() }
                              if (value) next.max_effort = value
                              else delete next.max_effort
                              setPolicy(layer, next)
                            }}
                          />
                          <span>
                            <SettingsButton
                              type="button"
                              settingsSize="sm"
                              onClick={() => {
                                setTexts((prev) => {
                                  const next = { ...prev }
                                  delete next[`${key}:allow`]
                                  delete next[`${key}:deny`]
                                  return next
                                })
                                setPolicy(layer, null)
                              }}
                            >
                              {layer.kind === 'org_default' ? 'Fjern organisasjonsstandard' : 'Fjern overstyring'}
                            </SettingsButton>
                          </span>
                        </>
                      )}
                    </Show>
                  </fieldset>
                )
              }}
            </For>

            <div class="verevon-settings-field">
              <label for="model-access-new-role" class="verevon-settings-label">
                Ny rolleoverstyring
              </label>
              <span class="verevon-settings-input-wrap">
                <VerevonInput
                  id="model-access-new-role"
                  list="model-access-role-suggestions"
                  value={newRole()}
                  placeholder="member"
                  onInput={(event) => setNewRole(event.currentTarget.value)}
                  class="verevon-settings-input"
                />
                <datalist id="model-access-role-suggestions">
                  <For each={SUGGESTED_ROLES}>{(role) => <option value={role} />}</For>
                </datalist>
              </span>
              <span>
                <SettingsButton type="button" settingsSize="sm" onClick={addRole}>
                  Legg til rolle
                </SettingsButton>
              </span>
            </div>

            <Show when={problem()}>
              {(message) => (
                <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
                  {message()}
                </p>
              )}
            </Show>

            <span>
              <SettingsButton
                type="button"
                variant="primary"
                settingsSize="sm"
                disabled={saving()}
                onClick={() => void handleSave()}
              >
                <Show
                  when={saving()}
                  fallback={
                    <>
                      <BrainCircuit size={14} aria-hidden="true" /> Lagre modelltilgang
                    </>
                  }
                >
                  <Loader2 size={14} aria-hidden="true" /> Lagrer…
                </Show>
              </SettingsButton>
              <Show when={saved() && !saving()}>
                {' '}
                <span class="verevon-trust-chip" role="status">
                  lagret
                </span>
              </Show>
            </span>
          </Show>
        </Show>
      </Show>
    </>
  )
}
