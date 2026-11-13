import { Loader2, ShieldCheck } from '@/shared/icons'
import { createMemo, createSignal, For, Show } from 'solid-js'
import { createResource } from '@/shared/lib/create-resource-compat'
import {
  SPACE_ENTITLEMENT_KEYS,
  SPACE_POLICY_REVISION_CONFLICT,
  getSpacePolicy,
  isSpacePolicyAdminDisabled,
  listSpacePolicyEvents,
  newSpacePolicyIdempotencyKey,
  setSpacePolicyEntitlements,
  type SpacePolicyEntitlementView,
  type SpacePolicyEvent,
  type SpacePolicyPresetView,
  type SpacePolicyView,
} from '@/shared/api/space-policy-client'
import { getOrganizationAISettings } from '@/shared/api/organization-client'
import { ApiError } from '@/shared/api/http'
import { translateApiError, useI18n } from '@/shared/i18n'
import { SpaceConfirmButton } from '@/features/spaces/components/SpaceConfirmButton'
import { SectionHeader, SettingsButton } from '@/features/settings/components/settings-ui'
import { VerevonSwitch } from '@/shared/ui/verevon/VerevonSwitch'
import { getSession } from '@/shared/session/session-store'
import { hasWorkspaceAdminAccess } from '@/shared/session/access'

/**
 * Space-policy console (WS03-S6): which Space capabilities the organisation allows.
 *
 * Control Plane (user-core) owns the policy row, the owner/admin check, the write switch and
 * the audit trail. This section is presentation: `hasWorkspaceAdminAccess` only decides what to
 * draw; the gateway and Control decide what is allowed. A 404 `space_policy_admin_disabled`
 * (the gateway flag) hides the console. Nothing here widens anything on its own: a ZDR-blocked
 * bit cannot be switched on, and an edit that turns something off arms in place first.
 */

type Tr = (no: string, en: string) => string

interface EntitlementCopy {
  readonly group: 'conversation' | 'knowledge' | 'agents'
  readonly label: (tr: Tr) => string
  readonly help: (tr: Tr) => string
}

const ENTITLEMENT_COPY: Record<string, EntitlementCopy> = {
  thread_create: {
    group: 'conversation',
    label: (tr) => tr('Skrive i rom og starte samtaler', 'Write in rooms and start conversations'),
    help: (tr) => tr('Medlemmer kan starte samtaler og skrive i rom.', 'Members can start conversations and post in rooms.'),
  },
  thread_read: {
    group: 'conversation',
    label: (tr) => tr('Lese hele romsamtalen', 'Read the whole room conversation'),
    help: (tr) => tr('Rommets samtale kan leses av dem som har tilgang til rommet.', 'The room conversation can be read by people with access to the room.'),
  },
  retrieval_read: {
    group: 'knowledge',
    label: (tr) => tr('Bruke kunnskap i rom', 'Use knowledge in rooms'),
    help: (tr) => tr('Svar i rom kan bygge på organisasjonens kunnskap.', 'Answers in rooms can draw on the organization knowledge.'),
  },
  import_write: {
    group: 'knowledge',
    label: (tr) => tr('Importere filer og kilder', 'Import files and sources'),
    help: (tr) => tr('Filer og kilder kan hentes inn i et rom.', 'Files and sources can be brought into a room.'),
  },
  agent_action: {
    group: 'agents',
    label: (tr) => tr('La agenter utføre handlinger', 'Let agents perform actions'),
    help: (tr) => tr('Agenter i rom kan utføre handlinger, ikke bare svare.', 'Agents in rooms can take actions, not only answer.'),
  },
  schedule_fire: {
    group: 'agents',
    label: (tr) => tr('Planlagte kjøringer og overvåking', 'Scheduled runs and watches'),
    help: (tr) => tr('Rom kan kjøre planlagte oppgaver og overvåke endringer.', 'Rooms can run scheduled tasks and watch for changes.'),
  },
  sandbox_capability: {
    group: 'agents',
    label: (tr) => tr('Agent-datamaskin (sandkasse)', 'Agent computer (sandbox)'),
    help: (tr) => tr('Agenter kan bruke en isolert datamaskin i et rom.', 'Agents can use an isolated computer in a room.'),
  },
  process_registry: {
    group: 'agents',
    label: (tr) => tr('Bakgrunnsprosesser', 'Background processes'),
    help: (tr) => tr('Agenter kan la prosesser gå i bakgrunnen i et rom.', 'Agents can leave processes running in the background in a room.'),
  },
}

const GROUPS: ReadonlyArray<{ id: EntitlementCopy['group']; title: (tr: Tr) => string }> = [
  { id: 'conversation', title: (tr) => tr('Samtale', 'Conversation') },
  { id: 'knowledge', title: (tr) => tr('Kunnskap og filer', 'Knowledge and files') },
  { id: 'agents', title: (tr) => tr('Agenter', 'Agents') },
]

const PRESET_COPY: Record<string, { label: (tr: Tr) => string; help: (tr: Tr) => string }> = {
  locked: {
    label: (tr) => tr('Låst', 'Locked'),
    help: (tr) => tr('Ingenting er tillatt i rom.', 'Nothing is allowed in rooms.'),
  },
  conversation: {
    label: (tr) => tr('Samtale', 'Conversation'),
    help: (tr) => tr('Skrive og lese i rom.', 'Write and read in rooms.'),
  },
  conversation_knowledge: {
    label: (tr) => tr('Samtale og kunnskap', 'Conversation and knowledge'),
    help: (tr) => tr('Samtale, pluss kunnskap i rom.', 'Conversation, plus knowledge in rooms.'),
  },
  agent_work: {
    label: (tr) => tr('Agentarbeid', 'Agent work'),
    help: (tr) => tr('Dessuten import, handlinger og planlagte kjøringer.', 'Also imports, actions and scheduled runs.'),
  },
  agent_computer: {
    label: (tr) => tr('Agent-datamaskin', 'Agent computer'),
    help: (tr) => tr('Alt over, pluss sandkasse og bakgrunnsprosesser.', 'All of the above, plus sandbox and background processes.'),
  },
}

const EVENT_KIND_COPY: Record<string, (tr: Tr) => string> = {
  entitlements_set: (tr) => tr('Rettigheter endret', 'Entitlements changed'),
  preset_applied: (tr) => tr('Forhåndsvalg brukt', 'Preset applied'),
  operator_created: (tr) => tr('Opprettet av Verevon-drift', 'Created by Verevon operations'),
  operator_updated: (tr) => tr('Oppdatert av Verevon-drift', 'Updated by Verevon operations'),
}

function formatWhen(value?: string | null): string {
  if (!value) return '—'
  try {
    return new Date(value).toLocaleString('nb-NO', { dateStyle: 'short', timeStyle: 'short' })
  } catch {
    return value
  }
}

function labelFor(key: string, tr: Tr): string {
  return ENTITLEMENT_COPY[key]?.label(tr) ?? key
}

function presetLabel(id: string, tr: Tr): string {
  return PRESET_COPY[id]?.label(tr) ?? id
}

function actorLabel(event: SpacePolicyEvent, tr: Tr): string {
  if (event.source === 'operator') return tr('Verevon-drift', 'Verevon operations')
  return event.actor.name || event.actor.id
}

/** Refusals the console can word itself; anything else goes through the shared translator. */
function describeWriteError(err: unknown, tr: Tr): string {
  if (err instanceof ApiError) {
    switch (err.code) {
      case SPACE_POLICY_REVISION_CONFLICT:
        return tr(
          'Innstillingen ble endret av noen andre. Den nyeste versjonen er lastet; sjekk utkastet og lagre på nytt.',
          'The setting was changed by someone else. The latest version is loaded; check your draft and save again.',
        )
      case 'space_policy_zdr_conflict': {
        const keys = !Array.isArray(err.details) && Array.isArray(err.details?.keys)
          ? (err.details?.keys as unknown[]).filter((k): k is string => typeof k === 'string')
          : []
        const named = keys.map((key) => labelFor(key, tr)).join(', ')
        return tr(
          `Kan ikke slås på mens nulloppbevaring for rom er aktiv${named ? `: ${named}` : ''}.`,
          `Cannot be switched on while zero data retention for rooms is on${named ? `: ${named}` : ''}.`,
        )
      }
      case 'space_policy_admin_writes_disabled':
        return tr(
          'Endringer er ikke slått på for denne installasjonen ennå. Du kan se innstillingene, men ikke endre dem.',
          'Changes are not switched on for this installation yet. You can view the settings but not change them.',
        )
      case 'space_policy_idempotency_conflict':
        return tr(
          'Denne forespørselen er allerede brukt med et annet innhold. Gjør endringen på nytt.',
          'This request was already used with different content. Make the change again.',
        )
      case 'space_policy_not_provisioned':
        return tr(
          'Organisasjonen har ikke fått satt opp rom-policy ennå. Verevon-drift gjør dette.',
          'The organization has no room policy set up yet. Verevon operations does this.',
        )
      case 'space_policy_membership_authority_unavailable':
      case 'space_policy_unavailable':
        return tr(
          'Tjenesten er midlertidig utilgjengelig. Prøv igjen om litt.',
          'The service is temporarily unavailable. Try again shortly.',
        )
      default:
        break
    }
  }
  return translateApiError(err, tr, {
    no: 'Kunne ikke lagre endringen.',
    en: 'Could not save the change.',
  })
}

export function OrgSpacePolicySection() {
  const i18n = useI18n()
  const tr: Tr = (no, en) => i18n.tr(no, en)
  const session = getSession()
  const orgId = createMemo(() => session.activeOrg?.id ?? '')
  const isAdmin = createMemo(() => hasWorkspaceAdminAccess(session))
  const askedOrg = () => (isAdmin() && orgId() ? orgId() : undefined)

  const [policy, { refetch: refetchPolicy }] = createResource(askedOrg, (id) => getSpacePolicy(id))
  const [events, { refetch: refetchEvents }] = createResource(askedOrg, (id) => listSpacePolicyEvents(id))
  // The org's interactive ZDR intent, shown beside the Space attestation. A failed read shows
  // "could not check" and never a guess.
  const [orgZdr] = createResource(askedOrg, (id) => getOrganizationAISettings(id).then((s) => s.zdr))

  const [draft, setDraft] = createSignal<Record<string, boolean> | null>(null)
  const [draftPresetId, setDraftPresetId] = createSignal<string | null>(null)
  const [saving, setSaving] = createSignal(false)
  const [saveError, setSaveError] = createSignal<string | null>(null)
  const [savedNote, setSavedNote] = createSignal<string | null>(null)
  // One key per distinct edit, reused on a retry of the same edit. Any edit, and a revision
  // conflict (the request body changes), starts a new one.
  let draftKey: string | null = null

  const view = (): SpacePolicyView | undefined => (policy.error ? undefined : policy())
  const disabled = () => isSpacePolicyAdminDisabled(policy.error)
  const writable = () => view()?.state === 'provisioned' && view()?.capabilities.write === true

  const stored = createMemo<Record<string, boolean>>(() => {
    const out: Record<string, boolean> = {}
    for (const e of view()?.entitlements ?? []) out[e.key] = e.enabled
    return out
  })
  const effectiveDraft = (): Record<string, boolean> => draft() ?? stored()
  const entitlementByKey = createMemo(() => {
    const map: Record<string, SpacePolicyEntitlementView> = {}
    for (const e of view()?.entitlements ?? []) map[e.key] = e
    return map
  })
  // Render the catalogue the server sent, in the server's order; the known keys give the copy.
  const keys = (): string[] => {
    const sent = (view()?.entitlements ?? []).map((e) => e.key)
    return sent.length > 0 ? sent : [...SPACE_ENTITLEMENT_KEYS]
  }
  const dirty = createMemo(() => {
    const d = draft()
    if (!d) return false
    const s = stored()
    return Object.keys(s).some((key) => (d[key] ?? false) !== s[key])
  })
  const turnsSomethingOff = createMemo(() => {
    const d = effectiveDraft()
    const s = stored()
    return Object.keys(s).some((key) => s[key] && d[key] === false)
  })
  const turnedOffNames = () => {
    const d = effectiveDraft()
    const s = stored()
    return Object.keys(s)
      .filter((key) => s[key] && d[key] === false)
      .map((key) => labelFor(key, tr))
      .join(', ')
  }

  const beginEdit = () => {
    draftKey = null
    setSaveError(null)
    setSavedNote(null)
  }

  const toggle = (key: string, value: boolean) => {
    beginEdit()
    setDraftPresetId(null)
    setDraft({ ...effectiveDraft(), [key]: value })
  }

  const pickPreset = (preset: SpacePolicyPresetView) => {
    if (!writable() || !preset.available) return
    beginEdit()
    const next: Record<string, boolean> = {}
    for (const key of keys()) next[key] = preset.entitlements[key] === true
    setDraft(next)
    setDraftPresetId(preset.id)
  }

  const discard = () => {
    beginEdit()
    setDraft(null)
    setDraftPresetId(null)
  }

  const save = async () => {
    const id = orgId()
    const current = view()
    const d = draft()
    if (!id || !current || !d || saving() || !writable()) return
    draftKey ??= newSpacePolicyIdempotencyKey()
    setSaving(true)
    setSaveError(null)
    setSavedNote(null)
    try {
      const presetId = draftPresetId()
      const result = await setSpacePolicyEntitlements(id, {
        idempotencyKey: draftKey,
        expectedEntitlementRevision: current.revisions.entitlement,
        change: presetId ? { preset_id: presetId } : { entitlements: { ...stored(), ...d } },
      })
      setDraft(null)
      setDraftPresetId(null)
      draftKey = null
      setSavedNote(
        result.changed
          ? tr(
              `Lagret. ${result.spaces_fenced} aktive rom ble oppdatert.`,
              `Saved. ${result.spaces_fenced} active rooms were updated.`,
            )
          : tr('Ingen endring: innstillingene var allerede slik.', 'No change: the settings were already like this.'),
      )
      await Promise.all([refetchPolicy(), refetchEvents()])
    } catch (err) {
      setSaveError(describeWriteError(err, tr))
      if (err instanceof ApiError && err.code === SPACE_POLICY_REVISION_CONFLICT) {
        // Reload so the next save carries the current revision; the draft stays visible.
        draftKey = null
        await refetchPolicy()
      }
    } finally {
      setSaving(false)
    }
  }

  return (
    <>
      <SectionHeader
        title={tr('Rom-policy', 'Room policy')}
        description={tr(
          'Hva organisasjonen tillater i rom: samtale, kunnskap og agenter. Gjelder alle rom i organisasjonen.',
          'What the organization allows in rooms: conversation, knowledge and agents. Applies to every room in the organization.',
        )}
      />

      <Show
        when={isAdmin()}
        fallback={
          <p class="verevon-settings-subnote">
            <ShieldCheck size={14} aria-hidden="true" />{' '}
            {tr('Bare administratorer kan se eller endre rom-policy.', 'Only administrators can view or change room policy.')}
          </p>
        }
      >
        <Show when={disabled()}>
          <p class="verevon-settings-subnote" role="status">
            <ShieldCheck size={14} aria-hidden="true" />{' '}
            {tr('Rom-policy er ikke slått på for denne installasjonen.', 'Room policy is not switched on for this installation.')}
          </p>
        </Show>

        <Show when={policy.error && !disabled()}>
          <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
            {translateApiError(policy.error, tr, {
              no: 'Kunne ikke laste rom-policy.',
              en: 'Could not load room policy.',
            })}
          </p>
        </Show>

        <Show
          when={!policy.loading}
          fallback={
            <p class="verevon-settings-subnote" role="status" aria-busy="true">
              {tr('Laster rom-policy…', 'Loading room policy…')}
            </p>
          }
        >
          <Show when={view()}>
            {(current) => (
              <>
                <Show
                  when={current().state === 'provisioned'}
                  fallback={
                    <p class="verevon-settings-subnote" role="status" data-testid="space-policy-not-provisioned">
                      <ShieldCheck size={14} aria-hidden="true" />{' '}
                      {tr(
                        'Rom er avslått som standard for denne organisasjonen. Personvernerklæringen settes av Verevon-drift; frem til den er satt kan ingenting slås på her.',
                        'Rooms are denied by default for this organization. The privacy attestation is set by Verevon operations; until it is set nothing can be switched on here.',
                      )}
                    </p>
                  }
                >
                  {/* Read-only attestation, beside the org's interactive ZDR state. */}
                  <div class="verevon-settings-status-grid">
                    <div class="verevon-settings-status-card">
                      <span class="verevon-settings-label">{tr('Personvernerklæring (satt av Verevon-drift)', 'Privacy attestation (set by Verevon operations)')}</span>
                      <strong>
                        {current().attestation?.zero_data_retention
                          ? tr('Nulloppbevaring for rom: på', 'Zero data retention for rooms: on')
                          : tr('Nulloppbevaring for rom: av', 'Zero data retention for rooms: off')}
                      </strong>
                      <span class="verevon-settings-help">
                        {current().attestation?.retention_class} · {current().attestation?.residency} ·{' '}
                        {current().attestation?.privacy_class}
                      </span>
                    </div>
                    <div class="verevon-settings-status-card">
                      <span class="verevon-settings-label">{tr('Organisasjonens nulloppbevaring', 'Organization zero data retention')}</span>
                      <strong>
                        <Show
                          when={!orgZdr.error && orgZdr() !== undefined}
                          fallback={tr('Kunne ikke sjekke', 'Could not check')}
                        >
                          {orgZdr() ? tr('På', 'On') : tr('Av', 'Off')}
                        </Show>
                      </strong>
                      <Show
                        when={
                          !orgZdr.error
                          && orgZdr() !== undefined
                          && orgZdr() !== (current().attestation?.zero_data_retention ?? false)
                        }
                      >
                        <span class="verevon-settings-help" role="status">
                          {tr(
                            'Avviker fra rommenes personvernerklæring. Rommene følger erklæringen.',
                            'Differs from the rooms privacy attestation. Rooms follow the attestation.',
                          )}
                        </span>
                      </Show>
                    </div>
                  </div>

                  <Show when={!current().capabilities.write}>
                    <p class="verevon-settings-subnote" role="status" data-testid="space-policy-read-only">
                      <ShieldCheck size={14} aria-hidden="true" />{' '}
                      {tr(
                        'Endringer er ikke slått på for denne installasjonen. Du kan se innstillingene, men ikke endre dem.',
                        'Changes are not switched on for this installation. You can view the settings but not change them.',
                      )}
                    </p>
                  </Show>

                  <h3 class="verevon-settings-label">{tr('Forhåndsvalg', 'Presets')}</h3>
                  <div class="verevon-settings-field-grid" data-testid="space-policy-presets">
                    <For each={current().presets}>
                      {(preset) => (
                        <div class="verevon-settings-status-card">
                          <strong>{presetLabel(preset.id, tr)}</strong>
                          <Show when={current().matches_preset === preset.id}>
                            {' '}
                            <span class="verevon-trust-chip">{tr('gjeldende', 'current')}</span>
                          </Show>
                          <span class="verevon-settings-help">
                            {PRESET_COPY[preset.id]?.help(tr) ?? ''}
                          </span>
                          <Show when={!preset.available}>
                            <span class="verevon-settings-help" role="status">
                              {tr(
                                'Ikke tilgjengelig mens nulloppbevaring for rom er på.',
                                'Not available while zero data retention for rooms is on.',
                              )}
                            </span>
                          </Show>
                          <SettingsButton
                            settingsSize="sm"
                            disabled={!writable() || !preset.available || saving()}
                            aria-pressed={draftPresetId() === preset.id ? 'true' : 'false'}
                            onClick={() => pickPreset(preset)}
                          >
                            {draftPresetId() === preset.id ? tr('Valgt', 'Selected') : tr('Velg', 'Choose')}
                          </SettingsButton>
                        </div>
                      )}
                    </For>
                  </div>

                  <For each={GROUPS}>
                    {(group) => (
                      <fieldset class="verevon-settings-field-grid verevon-settings-field-grid--spaced">
                        <legend class="verevon-settings-label">{group.title(tr)}</legend>
                        <For each={keys().filter((key) => (ENTITLEMENT_COPY[key]?.group ?? 'agents') === group.id)}>
                          {(key) => {
                            const row = () => entitlementByKey()[key]
                            const on = () => effectiveDraft()[key] === true
                            const blockedOn = () => row()?.zdr_blocked === true && !on()
                            return (
                              <div class="verevon-settings-field">
                                <span class="verevon-settings-label">{labelFor(key, tr)}</span>
                                <span class="verevon-settings-help">{ENTITLEMENT_COPY[key]?.help(tr) ?? ''}</span>
                                <VerevonSwitch
                                  label={labelFor(key, tr)}
                                  checked={on()}
                                  disabled={!writable() || saving() || blockedOn()}
                                  onChange={(value) => toggle(key, value)}
                                />
                                <Show when={row()?.zdr_blocked}>
                                  <span class="verevon-settings-help" role="status">
                                    {tr(
                                      'Kan ikke slås på mens nulloppbevaring for rom er på.',
                                      'Cannot be switched on while zero data retention for rooms is on.',
                                    )}
                                  </span>
                                </Show>
                              </div>
                            )
                          }}
                        </For>
                      </fieldset>
                    )}
                  </For>

                  <p class="verevon-settings-subnote" data-testid="space-policy-impact">
                    {tr(
                      `Gjelder ${current().active_spaces} aktive rom. Nye forespørsler endres med en gang; godkjenninger som allerede er utstedt varer i opptil to minutter.`,
                      `Applies to ${current().active_spaces} active rooms. New requests change at once; approvals already issued last up to two minutes.`,
                    )}
                  </p>

                  <Show when={saveError()}>
                    {(message) => (
                      <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
                        {message()}
                      </p>
                    )}
                  </Show>
                  <Show when={savedNote()}>
                    {(message) => (
                      <p class="verevon-settings-status-message" role="status">
                        {message()}
                      </p>
                    )}
                  </Show>

                  <Show when={writable() && dirty()}>
                    <div>
                      <Show
                        when={turnsSomethingOff()}
                        fallback={
                          <SettingsButton variant="primary" settingsSize="sm" disabled={saving()} onClick={() => void save()}>
                            <Show when={saving()} fallback={tr('Lagre endringer', 'Save changes')}>
                              <Loader2 size={14} aria-hidden="true" /> {tr('Lagrer…', 'Saving…')}
                            </Show>
                          </SettingsButton>
                        }
                      >
                        <SpaceConfirmButton
                          class="verevon-settings-button verevon-settings-button--primary verevon-settings-button--sm"
                          label={tr('Lagre endringer', 'Save changes')}
                          confirmLabel={tr('Bekreft at du slår av rettigheter', 'Confirm turning entitlements off')}
                          consequence={tr(
                            `Slår av: ${turnedOffNames()}. Rom mister dette med en gang for nye forespørsler.`,
                            `Turns off: ${turnedOffNames()}. Rooms lose this at once for new requests.`,
                          )}
                          disabled={saving()}
                          onConfirm={() => void save()}
                        />
                      </Show>{' '}
                      <SettingsButton settingsSize="sm" disabled={saving()} onClick={discard}>
                        {tr('Forkast', 'Discard')}
                      </SettingsButton>
                    </div>
                  </Show>

                  <h3 class="verevon-settings-label">{tr('Historikk', 'History')}</h3>
                  <Show
                    when={!events.error && (events() ?? []).length > 0}
                    fallback={
                      <p class="verevon-settings-subnote" data-testid="space-policy-history-empty">
                        {events.error
                          ? tr('Kunne ikke laste historikken.', 'Could not load the history.')
                          : tr('Ingen endringer er registrert ennå.', 'No changes have been recorded yet.')}
                      </p>
                    }
                  >
                    <ul class="verevon-settings-field-grid" data-testid="space-policy-history">
                      <For each={events() ?? []}>
                        {(event) => (
                          <li class="verevon-settings-help">
                            <strong>{(EVENT_KIND_COPY[event.change_kind] ?? (() => event.change_kind))(tr)}</strong>
                            <Show when={event.preset_id}>{' '}({presetLabel(event.preset_id ?? '', tr)})</Show>
                            {' · '}
                            {actorLabel(event, tr)} · {formatWhen(event.at)}
                            <Show when={event.changed.length > 0}>
                              {' · '}
                              {event.changed
                                .map((c) => `${labelFor(c.key, tr)}: ${c.to ? tr('på', 'on') : tr('av', 'off')}`)
                                .join(', ')}
                            </Show>
                          </li>
                        )}
                      </For>
                    </ul>
                  </Show>
                </Show>
              </>
            )}
          </Show>
        </Show>
      </Show>
    </>
  )
}
