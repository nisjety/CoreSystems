import { Download, Loader2, ShieldCheck, Trash2 } from '@/shared/icons'
import { createMemo, createSignal, For, Show } from 'solid-js'
import { createResource } from '@/shared/lib/create-resource-compat'
import {
  createSiemDestination,
  deleteSiemDestination,
  exportComplianceEvents,
  listSiemDestinations,
  updateSiemDestination,
  verifyComplianceChain,
  type ChainVerifyReport,
  type ChainVerifyStatus,
  type CreateSiemDestination,
  type SiemDestination,
  type SiemDestinationKind,
} from '@/shared/api/audit-compliance-client'
import { translateApiError, useI18n } from '@/shared/i18n'
import { SectionHeader, SettingsButton } from '@/features/settings/components/settings-ui'
import { VerevonInput } from '@/shared/ui/verevon/VerevonInput'
import { getSession } from '@/shared/session/session-store'

/**
 * Revisjonslogg: integritetskontroll, eksport og SIEM-mottakere (WS06).
 *
 * Owner/admin only, matching the gateway and audit-core: the gateway refuses
 * everyone else, so this section is simply not offered to them. Platform admins
 * and permission grants are deliberately not enough here, the tenant role is the
 * authority. No action in this section is model-eligible; an agent never reads or
 * administers the audit log.
 *
 * The verdict is shown as audit-core states it. `partial`, `key_unavailable` and
 * `verified_with_legacy` are not green: a verified chain does not claim the log
 * is complete, and rows written before the chain was switched on sit outside it.
 */

const VERDICT_TEXT: Record<ChainVerifyStatus, { tone: 'ok' | 'warn' | 'error'; text: string }> = {
  verified: { tone: 'ok', text: 'Kjeden er verifisert mot signert forankring.' },
  verified_with_legacy: {
    tone: 'warn',
    text: 'Kjeden er verifisert, men eldre rader fra før kjeden ble slått på ligger utenfor og er ikke dekket.',
  },
  broken: { tone: 'error', text: 'Kjeden er brutt. Se første ødelagte posisjon.' },
  partial: { tone: 'warn', text: 'Ingen feil i radene som ble lest, men kontrollen dekket ikke hele kjeden eller mangler forankring.' },
  key_unavailable: { tone: 'warn', text: 'Signaturnøkkelen er ikke tilgjengelig, så forankringen kunne ikke kontrolleres.' },
}

const KIND_LABEL: Record<SiemDestinationKind, string> = {
  ndjson_webhook: 'NDJSON-webhook',
  splunk_hec: 'Splunk HEC',
}

const EXPORT_PAGE_LIMIT = 1000

function downloadNdjson(lines: string[]): void {
  const blob = new Blob([lines.join('\n'), '\n'], { type: 'application/x-ndjson' })
  const url = URL.createObjectURL(blob)
  const link = document.createElement('a')
  link.href = url
  link.download = `audit-log-${new Date().toISOString().slice(0, 10)}.ndjson`
  link.click()
  URL.revokeObjectURL(url)
}

export function AuditComplianceSection() {
  const i18n = useI18n()
  const session = getSession()
  const isAdmin = createMemo(() => {
    const role = (session.activeOrg?.role ?? '').toLowerCase()
    return session.status === 'authenticated' && (role === 'owner' || role === 'admin')
  })

  const [report, setReport] = createSignal<ChainVerifyReport | null>(null)
  const [busy, setBusy] = createSignal<'verify' | 'export' | 'destination' | null>(null)
  const [message, setMessage] = createSignal<string | null>(null)
  const [error, setError] = createSignal<string | null>(null)

  const [kind, setKind] = createSignal<SiemDestinationKind>('ndjson_webhook')
  const [url, setUrl] = createSignal('')
  const [secret, setSecret] = createSignal('')

  const [destinations, { refetch }] = createResource(
    () => (isAdmin() ? session.activeOrg?.id : undefined),
    () => listSiemDestinations(),
  )

  const run = async (kindOfWork: 'verify' | 'export' | 'destination', work: () => Promise<void>) => {
    if (busy()) return
    setBusy(kindOfWork)
    setError(null)
    setMessage(null)
    try {
      await work()
    } catch (err) {
      setError(
        translateApiError(err, i18n.tr, {
          no: 'Handlingen mislyktes.',
          en: 'The action failed.',
        }),
      )
    } finally {
      setBusy(null)
    }
  }

  const handleVerify = () =>
    run('verify', async () => {
      setReport(await verifyComplianceChain())
    })

  const handleExport = () =>
    run('export', async () => {
      const lines: string[] = []
      for await (const page of exportComplianceEvents({ limit: EXPORT_PAGE_LIMIT })) {
        for (const event of page.events) lines.push(JSON.stringify(event))
      }
      downloadNdjson(lines)
      setMessage(`Eksporterte ${lines.length} rader. Radene bærer hash og forrige hash, så de kan kontrolleres frakoblet.`)
    })

  const handleCreate = (input: CreateSiemDestination) =>
    run('destination', async () => {
      await createSiemDestination(input)
      setUrl('')
      setSecret('')
      await refetch()
    })

  const handleToggle = (destination: SiemDestination) =>
    run('destination', async () => {
      await updateSiemDestination(destination.id, {
        expectedRevision: destination.revision,
        enabled: !destination.enabled,
      })
      await refetch()
    })

  const handleDelete = (destination: SiemDestination) =>
    run('destination', async () => {
      await deleteSiemDestination(destination.id)
      await refetch()
    })

  return (
    <>
      <SectionHeader
        title="Revisjonslogg"
        description="Kontroller at revisjonsloggen ikke er endret, eksporter den, og send kopier til et eksternt SIEM-system."
      />

      <Show
        when={isAdmin()}
        fallback={
          <p class="verevon-settings-subnote">
            <ShieldCheck size={14} aria-hidden="true" /> Bare eiere og administratorer kan se
            integritetskontroll, eksport og SIEM-mottakere.
          </p>
        }
      >
        <Show when={error()}>
          {(text) => (
            <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
              {text()}
            </p>
          )}
        </Show>
        <Show when={message()}>
          {(text) => (
            <p class="verevon-settings-status-message" role="status">
              {text()}
            </p>
          )}
        </Show>

        <div class="verevon-settings-field-grid verevon-settings-field-grid--spaced">
          <span>
            <SettingsButton
              type="button"
              variant="primary"
              settingsSize="sm"
              disabled={busy() !== null}
              onClick={() => void handleVerify()}
            >
              <Show when={busy() === 'verify'} fallback={<><ShieldCheck size={14} aria-hidden="true" /> Verifiser kjeden</>}>
                <Loader2 size={14} aria-hidden="true" /> Verifiserer…
              </Show>
            </SettingsButton>{' '}
            <SettingsButton
              type="button"
              settingsSize="sm"
              disabled={busy() !== null}
              onClick={() => void handleExport()}
            >
              <Show when={busy() === 'export'} fallback={<><Download size={14} aria-hidden="true" /> Eksporter logg</>}>
                <Loader2 size={14} aria-hidden="true" /> Eksporterer…
              </Show>
            </SettingsButton>
          </span>

          <Show when={report()}>
            {(current) => (
              <div role="status" data-testid="audit-verify-report">
                <p
                  class={
                    VERDICT_TEXT[current().status].tone === 'error'
                      ? 'verevon-settings-status-message verevon-settings-status-message--error'
                      : 'verevon-settings-status-message'
                  }
                >
                  <strong>{current().status}</strong>: {VERDICT_TEXT[current().status].text}
                </p>
                <span class="verevon-settings-help">
                  Rader kontrollert: <code>{current().rows_verified}</code>
                  <Show when={current().first_broken_seq}>
                    {' '}· første ødelagte posisjon: <code>{current().first_broken_seq}</code>
                  </Show>
                  <Show when={current().reason}>
                    {' '}· årsak: <code>{current().reason}</code>
                  </Show>
                  <Show when={!current().complete}>
                    {' '}· kontrollen nådde radgrensen og dekket ikke hele kjeden
                  </Show>
                  <Show when={current().legacy_unchained_rows > 0}>
                    {' '}· rader utenfor kjeden: <code>{current().legacy_unchained_rows}</code>
                  </Show>
                </span>
              </div>
            )}
          </Show>
        </div>

        <SectionHeader
          title="SIEM-mottakere"
          description="Hver mottaker får loggradene i rekkefølge, med hash-kjeden. Hemmeligheten kan bare skrives, aldri leses tilbake."
        />

        <Show when={destinations.error}>
          <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
            Kunne ikke laste SIEM-mottakere. Funksjonen er kanskje ikke slått på for denne organisasjonen.
          </p>
        </Show>

        <ul class="verevon-settings-field-grid">
          <For each={destinations() ?? []}>
            {(destination) => (
              <li class="verevon-settings-field">
                <span class="verevon-settings-label">
                  {KIND_LABEL[destination.kind] ?? destination.kind}: <code>{destination.url}</code>
                </span>
                <span class="verevon-settings-help">
                  {destination.enabled ? 'Aktiv' : 'Pauset'}
                  {destination.secret_set ? ' · hemmelighet er satt' : ' · ingen hemmelighet'}
                </span>
                <span>
                  <SettingsButton
                    type="button"
                    settingsSize="xs"
                    disabled={busy() !== null}
                    onClick={() => void handleToggle(destination)}
                  >
                    {destination.enabled ? 'Pause' : 'Aktiver'}
                  </SettingsButton>{' '}
                  <SettingsButton
                    type="button"
                    danger
                    settingsSize="xs"
                    disabled={busy() !== null}
                    aria-label={`Slett mottaker ${destination.url}`}
                    onClick={() => void handleDelete(destination)}
                  >
                    <Trash2 size={14} aria-hidden="true" /> Slett
                  </SettingsButton>
                </span>
              </li>
            )}
          </For>
        </ul>

        <div class="verevon-settings-field-grid verevon-settings-field-grid--spaced">
          <label for="siem-kind" class="verevon-settings-field">
            <span class="verevon-settings-label">Type</span>
            <select
              id="siem-kind"
              value={kind()}
              onChange={(event) => setKind(event.currentTarget.value as SiemDestinationKind)}
              class="verevon-settings-input verevon-settings-select"
            >
              <For each={Object.entries(KIND_LABEL)}>
                {([value, label]) => <option value={value}>{label}</option>}
              </For>
            </select>
          </label>
          <label for="siem-url" class="verevon-settings-field">
            <span class="verevon-settings-label">URL (https)</span>
            <VerevonInput
              id="siem-url"
              value={url()}
              placeholder="https://siem.example.com/ingest"
              onInput={(event) => setUrl(event.currentTarget.value)}
              class="verevon-settings-input"
            />
          </label>
          <label for="siem-secret" class="verevon-settings-field">
            <span class="verevon-settings-label">Hemmelighet eller token</span>
            <VerevonInput
              id="siem-secret"
              type="password"
              autocomplete="off"
              value={secret()}
              onInput={(event) => setSecret(event.currentTarget.value)}
              class="verevon-settings-input"
            />
          </label>
          <span>
            <SettingsButton
              type="button"
              variant="primary"
              settingsSize="sm"
              disabled={busy() !== null || !url().trim() || !secret()}
              onClick={() => void handleCreate({ kind: kind(), url: url().trim(), secret: secret() })}
            >
              Legg til mottaker
            </SettingsButton>
          </span>
        </div>
      </Show>
    </>
  )
}
