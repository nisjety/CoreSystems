import { Loader2, Lock, ShieldCheck } from '@/shared/icons'
import { createMemo, createSignal, For, Show } from 'solid-js'
import { createResource } from '@/shared/lib/create-resource-compat'
import { requestJson } from '@/shared/api/http'
import { ApiError } from '@/shared/api/api-error'
import { translateApiError, useI18n } from '@/shared/i18n'
import { SectionHeader, SettingsButton } from '@/features/settings/components/settings-ui'
import { VerevonInput } from '@/shared/ui/verevon/VerevonInput'
import { getSession } from '@/shared/session/session-store'

/**
 * Oppbevaring og rettslig hold (EP-9 C1).
 *
 * org-core owns the retention window and the legal holds; session-core's sweep
 * reads them and deletes personal threads past the window, never a held one. An
 * owner or admin sets the window; an owner or a compliance member places and
 * releases a hold. The writes go through the actions `org.set_retention`,
 * `org.place_legal_hold` and `org.release_legal_hold` (all high risk), whose
 * schemas mirror the bounds checked here.
 *
 * While org-core's ORG_RETENTION_POLICY_ENABLED is off the gateway answers 404
 * `not_found`, and the section says the feature is not on instead of showing
 * controls that would all fail.
 */

interface RetentionPolicy {
  organization_id: string
  thread_days: number | null
  revision: number
  updated_by: string
  updated_at: string | null
}

interface LegalHold {
  id: string
  scope: 'org' | 'thread'
  target_ref: string
  hold_ref: string
  created_by: string
  created_at: string
  released_by: string | null
  released_at: string | null
}

const MIN_DAYS = 30
const MAX_DAYS = 3650
const MAX_HOLD_REF = 200

const orgPath = (orgId: string) => `/api/v1/orgs/${encodeURIComponent(orgId)}`

function roleIn(role: string | null | undefined, allowed: readonly string[]): boolean {
  return (role ?? '')
    .split(',')
    .map((value) => value.trim().toLowerCase())
    .some((value) => allowed.includes(value))
}

function notEnabled(error: unknown): boolean {
  return error instanceof ApiError && error.status === 404 && error.code === 'not_found'
}

function formatWhen(value?: string | null): string {
  if (!value) return '—'
  try {
    return new Date(value).toLocaleString('nb-NO', { dateStyle: 'short', timeStyle: 'short' })
  } catch {
    return value
  }
}

export function OrgRetentionSection() {
  const i18n = useI18n()
  const session = getSession()
  const orgId = createMemo(() => session.activeOrg?.id ?? '')
  const canSetWindow = createMemo(() => roleIn(session.activeOrg?.role, ['owner', 'admin']))
  const canReadHolds = createMemo(() => roleIn(session.activeOrg?.role, ['owner', 'admin', 'compliance']))
  const canWriteHolds = createMemo(() => roleIn(session.activeOrg?.role, ['owner', 'compliance']))

  const [policy, { refetch: refetchPolicy }] = createResource(
    () => (canSetWindow() && orgId() ? orgId() : undefined),
    (id) => requestJson<RetentionPolicy>(`${orgPath(id)}/retention`),
  )
  const [holds, { refetch: refetchHolds }] = createResource(
    () => (canReadHolds() && orgId() ? orgId() : undefined),
    (id) => requestJson<{ holds: LegalHold[] }>(`${orgPath(id)}/legal-holds`).then((list) => list.holds),
  )
  const disabled = createMemo(() => notEnabled(policy.error) || notEnabled(holds.error))

  const [keep, setKeep] = createSignal<boolean | null>(null)
  const [daysDraft, setDaysDraft] = createSignal<string | null>(null)
  const [windowError, setWindowError] = createSignal<string | null>(null)
  const [savingWindow, setSavingWindow] = createSignal(false)

  const keepAll = () => keep() ?? (policy()?.thread_days ?? null) === null
  const daysValue = () => daysDraft() ?? String(policy()?.thread_days ?? 365)

  const saveWindow = async () => {
    const current = policy()
    if (!current || savingWindow()) return
    let threadDays: number | null = null
    if (!keepAll()) {
      const parsed = Number(daysValue().trim())
      if (!Number.isInteger(parsed) || parsed < MIN_DAYS || parsed > MAX_DAYS) {
        setWindowError(`Oppgi et helt antall dager mellom ${MIN_DAYS} og ${MAX_DAYS}.`)
        return
      }
      threadDays = parsed
    }
    setSavingWindow(true)
    setWindowError(null)
    try {
      await requestJson<RetentionPolicy>(`${orgPath(orgId())}/retention`, {
        method: 'PUT',
        body: JSON.stringify({ expected_revision: current.revision, thread_days: threadDays }),
      })
      setKeep(null)
      setDaysDraft(null)
      await refetchPolicy()
    } catch (err) {
      setWindowError(
        err instanceof ApiError && err.code === 'revision_conflict'
          ? 'Innstillingen ble endret av noen andre. Last inn på nytt og prøv igjen.'
          : translateApiError(err, i18n.tr, { no: 'Kunne ikke lagre oppbevaringen.', en: 'Could not save the retention window.' }),
      )
    } finally {
      setSavingWindow(false)
    }
  }

  const [scope, setScope] = createSignal<'org' | 'thread'>('thread')
  const [threadId, setThreadId] = createSignal('')
  const [holdRef, setHoldRef] = createSignal('')
  const [holdError, setHoldError] = createSignal<string | null>(null)
  const [busyHold, setBusyHold] = createSignal<string | null>(null)

  const placeHold = async () => {
    if (busyHold()) return
    const reference = holdRef().trim()
    if (!reference || reference.length > MAX_HOLD_REF) {
      setHoldError(`Oppgi en saksreferanse på høyst ${MAX_HOLD_REF} tegn.`)
      return
    }
    if (scope() === 'thread' && !/^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/.test(threadId().trim())) {
      setHoldError('Oppgi ID-en til tråden som skal holdes.')
      return
    }
    setBusyHold('new')
    setHoldError(null)
    try {
      const body = scope() === 'org'
        ? { scope: 'org', hold_ref: reference }
        : { scope: 'thread', thread_id: threadId().trim(), hold_ref: reference }
      await requestJson(`${orgPath(orgId())}/legal-holds`, { method: 'POST', body: JSON.stringify(body) })
      setThreadId('')
      setHoldRef('')
      await refetchHolds()
    } catch (err) {
      setHoldError(
        err instanceof ApiError && err.code === 'legal_hold_nothing_retained'
          ? 'Denne tråden lagres ikke (null datalagring), så det finnes ingenting å holde.'
          : translateApiError(err, i18n.tr, { no: 'Kunne ikke plassere holdet.', en: 'Could not place the hold.' }),
      )
    } finally {
      setBusyHold(null)
    }
  }

  const releaseHold = async (hold: LegalHold) => {
    if (busyHold()) return
    setBusyHold(hold.id)
    setHoldError(null)
    try {
      await requestJson(`${orgPath(orgId())}/legal-holds/${encodeURIComponent(hold.id)}/release`, { method: 'POST' })
      await refetchHolds()
    } catch (err) {
      setHoldError(translateApiError(err, i18n.tr, { no: 'Kunne ikke oppheve holdet.', en: 'Could not release the hold.' }))
    } finally {
      setBusyHold(null)
    }
  }

  return (
    <>
      <SectionHeader
        title="Oppbevaring og rettslig hold"
        description="Hvor lenge personlige tråder beholdes etter siste aktivitet, og hva som holdes tilbake fra sletting."
      />

      <Show
        when={canReadHolds()}
        fallback={
          <p class="verevon-settings-subnote">
            <ShieldCheck size={14} aria-hidden="true" /> Bare eiere, administratorer og
            compliance-ansvarlige kan se oppbevaring og rettslig hold.
          </p>
        }
      >
        <Show
          when={!disabled()}
          fallback={
            <p class="verevon-settings-subnote" role="status">
              Oppbevaringsregler er ikke slått på for denne organisasjonen ennå.
            </p>
          }
        >
          <Show when={canSetWindow()}>
            <Show
              when={!policy.loading}
              fallback={<p class="verevon-settings-subnote" role="status" aria-busy="true">Laster oppbevaring…</p>}
            >
              <Show when={policy.error && !disabled()}>
                <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
                  Kunne ikke laste oppbevaringen.
                </p>
              </Show>
              <Show when={policy()}>
                {(current) => (
                  <div class="verevon-settings-field-grid verevon-settings-field-grid--spaced">
                    <label class="verevon-settings-field">
                      <span class="verevon-settings-label">
                        <input
                          type="checkbox"
                          checked={keepAll()}
                          onChange={(event) => setKeep(event.currentTarget.checked)}
                        />{' '}
                        Behold tråder uten tidsgrense
                      </span>
                      <span class="verevon-settings-help">Standard. Ingenting slettes på grunn av alder.</span>
                    </label>
                    <Show when={!keepAll()}>
                      <label for="retention-thread-days" class="verevon-settings-field">
                        <span class="verevon-settings-label">Dager etter siste aktivitet</span>
                        <span class="verevon-settings-input-wrap">
                          <VerevonInput
                            id="retention-thread-days"
                            value={daysValue()}
                            inputmode="numeric"
                            onInput={(event) => setDaysDraft(event.currentTarget.value)}
                            class="verevon-settings-input"
                          />
                        </span>
                        <span class="verevon-settings-help">
                          Mellom {MIN_DAYS} og {MAX_DAYS} dager. Tråder som holdes, slettes aldri.
                        </span>
                      </label>
                    </Show>
                    <Show when={windowError()}>
                      {(message) => (
                        <span class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
                          {message()}
                        </span>
                      )}
                    </Show>
                    <span>
                      <SettingsButton
                        type="button"
                        variant="primary"
                        settingsSize="sm"
                        disabled={savingWindow()}
                        onClick={() => void saveWindow()}
                      >
                        <Show when={savingWindow()} fallback={<>Lagre oppbevaring</>}>
                          <Loader2 size={14} aria-hidden="true" /> Lagrer…
                        </Show>
                      </SettingsButton>{' '}
                      <span class="verevon-settings-help">
                        Versjon {current().revision} · sist endret {formatWhen(current().updated_at)}
                      </span>
                    </span>
                  </div>
                )}
              </Show>
            </Show>
          </Show>

          <h3 class="verevon-settings-label">Rettslig hold</h3>
          <Show when={holds.error && !disabled()}>
            <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
              Kunne ikke laste rettslige hold.
            </p>
          </Show>
          <Show when={(holds() ?? []).length > 0} fallback={<p class="verevon-settings-subnote">Ingen hold.</p>}>
            <ul class="verevon-settings-list">
              <For each={holds() ?? []}>
                {(hold) => (
                  <li>
                    <Lock size={14} aria-hidden="true" />{' '}
                    {hold.scope === 'org' ? 'Hele organisasjonen' : <>Tråd <code>{hold.target_ref}</code></>}
                    {' '}· {hold.hold_ref} · plassert {formatWhen(hold.created_at)}
                    <Show
                      when={hold.released_at === null}
                      fallback={<> · opphevet {formatWhen(hold.released_at)}</>}
                    >
                      <Show when={canWriteHolds()}>
                        {' '}
                        <SettingsButton
                          type="button"
                          settingsSize="sm"
                          disabled={busyHold() !== null}
                          onClick={() => void releaseHold(hold)}
                        >
                          Opphev
                        </SettingsButton>
                      </Show>
                    </Show>
                  </li>
                )}
              </For>
            </ul>
          </Show>

          <Show when={canWriteHolds()}>
            <div class="verevon-settings-field-grid verevon-settings-field-grid--spaced">
              <label class="verevon-settings-field">
                <span class="verevon-settings-label">Omfang</span>
                <span class="verevon-settings-input-wrap">
                  <select
                    aria-label="Omfang for holdet"
                    value={scope()}
                    onChange={(event) => setScope(event.currentTarget.value as 'org' | 'thread')}
                    class="verevon-settings-input verevon-settings-select"
                  >
                    <option value="thread">Én tråd</option>
                    <option value="org">Hele organisasjonen</option>
                  </select>
                </span>
              </label>
              <Show when={scope() === 'thread'}>
                <label for="legal-hold-thread" class="verevon-settings-field">
                  <span class="verevon-settings-label">Tråd-ID</span>
                  <span class="verevon-settings-input-wrap">
                    <VerevonInput
                      id="legal-hold-thread"
                      value={threadId()}
                      onInput={(event) => setThreadId(event.currentTarget.value)}
                      class="verevon-settings-input"
                    />
                  </span>
                </label>
              </Show>
              <label for="legal-hold-ref" class="verevon-settings-field">
                <span class="verevon-settings-label">Saksreferanse</span>
                <span class="verevon-settings-input-wrap">
                  <VerevonInput
                    id="legal-hold-ref"
                    value={holdRef()}
                    maxlength={MAX_HOLD_REF}
                    onInput={(event) => setHoldRef(event.currentTarget.value)}
                    class="verevon-settings-input"
                  />
                </span>
                <span class="verevon-settings-help">Et saksnummer eller en referanse til saken, aldri innhold.</span>
              </label>
              <Show when={holdError()}>
                {(message) => (
                  <span class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
                    {message()}
                  </span>
                )}
              </Show>
              <span>
                <SettingsButton
                  type="button"
                  variant="primary"
                  settingsSize="sm"
                  disabled={busyHold() !== null}
                  onClick={() => void placeHold()}
                >
                  <Show when={busyHold() === 'new'} fallback={<>Plasser hold</>}>
                    <Loader2 size={14} aria-hidden="true" /> Plasserer…
                  </Show>
                </SettingsButton>
              </span>
            </div>
          </Show>
        </Show>
      </Show>
    </>
  )
}
