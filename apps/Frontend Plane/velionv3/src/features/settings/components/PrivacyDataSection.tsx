import { Download, Loader2, ShieldAlert, Trash2 } from 'lucide-solid'
import { createSignal, For, onMount, Show } from 'solid-js'
import { signIn, signOut } from '@/shared/api/auth-client'
import { getPreferences, updatePreferences, type CrawlIngestMode } from '@/shared/api/settings-client'
import {
  CONTROL_PLANE_DSAR_DISCLOSURE,
  eraseMyAccount,
  exportMyData,
  type DsarExport,
} from '@/shared/api/privacy-client'
import { SectionHeader, SettingsButton } from '@/features/settings/components/settings-ui'
import { getSession } from '@/shared/session/session-store'
import { useI18n } from '@/shared/i18n'

function errorMessage(error: unknown, i18n: ReturnType<typeof useI18n>): string {
  return error instanceof Error ? error.message : i18n.tr('Uventet feil', 'Unexpected error')
}

/**
 * Privacy & data (GDPR Art. 15 / 17) self-service.
 *
 * Export renders the Control-Plane data plus user-core's verbatim scope notes.
 * Erase is gated by a typed-email confirmation AND a step-up password re-auth;
 * the user is signed out ONLY after a confirmed 2xx erase — any failure leaves
 * them signed in with a "contact support" message and their account intact.
 */
export function PrivacyDataSection() {
  const i18n = useI18n()
  const session = getSession()
  const accountEmail = () => session.user?.email ?? ''

  // ── Export (Art. 15) ────────────────────────────────────────────────────────
  const [exportData, setExportData] = createSignal<DsarExport | null>(null)
  const [exporting, setExporting] = createSignal(false)
  const [exportError, setExportError] = createSignal<string | null>(null)

  async function runExport() {
    setExporting(true)
    setExportError(null)
    try {
      setExportData(await exportMyData())
    } catch (error) {
      setExportError(errorMessage(error, i18n))
    } finally {
      setExporting(false)
    }
  }

  function downloadExport() {
    const data = exportData()
    if (!data) return
    const blob = new Blob([JSON.stringify(data, null, 2)], { type: 'application/json' })
    const url = URL.createObjectURL(blob)
    const anchor = document.createElement('a')
    anchor.href = url
    anchor.download = `velion-data-export-${data.subject_id}.json`
    anchor.click()
    URL.revokeObjectURL(url)
  }

  // ── Knowledge ingestion (Phase 6 selective ingest) ─────────────────────────
  // Unset defaults to 'auto' to match the crawl composer (KnowledgeComposer):
  // an explicit "add website to Knowledge" crawl persists by default, so the
  // Settings toggle must show 'auto' as the active mode until the user changes
  // it — otherwise the displayed default ('never') would contradict the actual
  // behavior. Persisted pages are owner=user / private until shared.
  const [ingestMode, setIngestMode] = createSignal<CrawlIngestMode>('auto')
  const [savingMode, setSavingMode] = createSignal(false)
  const [modeError, setModeError] = createSignal<string | null>(null)

  onMount(() => {
    void getPreferences()
      .then((prefs) => setIngestMode(prefs.crawlIngestMode ?? 'auto'))
      .catch(() => undefined)
  })

  async function changeIngestMode(mode: CrawlIngestMode) {
    const previous = ingestMode()
    if (mode === previous) return
    setIngestMode(mode)
    setSavingMode(true)
    setModeError(null)
    try {
      await updatePreferences({ crawlIngestMode: mode })
    } catch (error) {
      setIngestMode(previous) // revert optimistic change on failure
      setModeError(errorMessage(error, i18n))
    } finally {
      setSavingMode(false)
    }
  }

  // ── Erase (Art. 17) ─────────────────────────────────────────────────────────
  const [showErase, setShowErase] = createSignal(false)
  const [typedEmail, setTypedEmail] = createSignal('')
  const [password, setPassword] = createSignal('')
  const [erasing, setErasing] = createSignal(false)
  const [eraseError, setEraseError] = createSignal<string | null>(null)

  const emailMatches = () =>
    accountEmail().length > 0 && typedEmail().trim().toLowerCase() === accountEmail().toLowerCase()
  const canErase = () => emailMatches() && password().length > 0 && !erasing()

  function cancelErase() {
    setShowErase(false)
    setEraseError(null)
    setPassword('')
    setTypedEmail('')
  }

  async function runErase(event: Event) {
    event.preventDefault()
    if (!canErase()) return
    setErasing(true)
    setEraseError(null)

    // Step-up re-auth: re-verify the password before the irreversible call.
    try {
      await signIn({ email: accountEmail(), password: password() })
    } catch {
      setEraseError(i18n.tr('Re-autentisering mislyktes. Kontroller passordet ditt og prøv igjen.', 'Re-authentication failed. Check your password and try again.'))
      setErasing(false)
      return
    }

    // Irreversible erase. Sign out + redirect ONLY on a confirmed 2xx.
    try {
      await eraseMyAccount()
    } catch {
      setEraseError(i18n.tr('Slettingen ble ikke fullført — kontoen din er uendret. Ta kontakt med support.', 'Erasure did not complete — your account is unchanged. Please contact support.'))
      setErasing(false)
      return
    }

    await signOut().catch(() => undefined)
    window.location.href = '/login?erased=1'
  }

  return (
    <section id="privacy-data" class="velion-settings-section">
      <SectionHeader
        title={i18n.tr('Personvern og data', 'Privacy & data')}
        description={i18n.tr(
          'Eksporter eller slett permanent dataene dette arbeidsområdets Control Plane har om kontoen din.',
          "Export or permanently erase the data this workspace's Control Plane holds about your account.",
        )}
      />

      <div class="velion-privacy-disclosure">
        <p class="velion-privacy-disclosure__title">{i18n.tr('Hva dette omfatter', 'What this covers')}</p>
        <ul>
          <For each={CONTROL_PLANE_DSAR_DISCLOSURE}>{(note) => <li>{note}</li>}</For>
        </ul>
      </div>

      <div class="velion-privacy-block">
        <div class="velion-privacy-block__head">
          <h3>{i18n.tr('Eksporter dataene mine', 'Export my data')}</h3>
          <SettingsButton settingsSize="sm" onClick={() => void runExport()} disabled={exporting()}>
            <Show
              when={exporting()}
              fallback={<><Download size={14} aria-hidden="true" /> {i18n.tr('Generer eksport', 'Generate export')}</>}
            >
              <Loader2 size={14} class="velion-trust-spin" aria-hidden="true" /> {i18n.tr('Genererer…', 'Generating…')}
            </Show>
          </SettingsButton>
        </div>
        <Show when={exportError()}>
          {(message) => (
            <p class="velion-settings-status-message velion-settings-status-message--error" role="alert">
              {message()}
            </p>
          )}
        </Show>
        <Show when={exportData()}>
          {(data) => (
            <div class="velion-privacy-export">
              <p class="velion-privacy-export__summary">
                {i18n.tr('Generert', 'Generated')} {new Date(data().generated_at).toLocaleString()} ·{' '}
                {data().org_memberships.length} {i18n.tr('org.-medlemskap', 'org membership(s)')} · {data().api_keys.length} {i18n.tr('API-nøkler', 'API key(s)')}
              </p>
              <ul class="velion-privacy-export__notes">
                <For each={data().notes}>{(note) => <li>{note}</li>}</For>
              </ul>
              <SettingsButton settingsSize="sm" onClick={downloadExport}>
                <Download size={14} aria-hidden="true" /> {i18n.tr('Last ned JSON', 'Download JSON')}
              </SettingsButton>
            </div>
          )}
        </Show>
      </div>

      <div class="velion-privacy-block">
        <div class="velion-privacy-block__head">
          <h3>{i18n.tr('Kunnskapsinnhenting', 'Knowledge ingestion')}</h3>
          <Show when={savingMode()}>
            <Loader2 size={14} class="velion-trust-spin" aria-hidden="true" />
          </Show>
        </div>
        <p class="velion-privacy-danger__note">
          {i18n.tr(
            'Styrer om sider du krabber eller skraper lagres i kunnskapsbasen (synlig for hele organisasjonen din). Nettlesing lagrer aldri automatisk.',
            'Controls whether pages you crawl or scrape are saved to the knowledge base (visible to your whole organization). Browsing never saves automatically.',
          )}
        </p>
        <label class="velion-settings-field">
          <span>{i18n.tr('Innhentingsmodus for krabbing', 'Crawl ingestion mode')}</span>
          <select
            value={ingestMode()}
            disabled={savingMode()}
            onChange={(event) => void changeIngestMode(event.currentTarget.value as CrawlIngestMode)}
          >
            <option value="auto">{i18n.tr('Lagre alltid til kunnskapsbasen min (standard)', 'Always save to my knowledge base (default)')}</option>
            <option value="prompt">{i18n.tr('Spør meg etter hver krabbing', 'Ask me after each crawl')}</option>
            <option value="never">{i18n.tr('Lagre aldri — kun nettlesing', 'Never save — browse only')}</option>
          </select>
        </label>
        <Show when={modeError()}>
          {(message) => (
            <p class="velion-settings-status-message velion-settings-status-message--error" role="alert">
              {message()}
            </p>
          )}
        </Show>
      </div>

      <div class="velion-privacy-danger">
        <div class="velion-privacy-block__head">
          <h3>
            <ShieldAlert size={15} aria-hidden="true" /> {i18n.tr('Slett kontoen min', 'Erase my account')}
          </h3>
          <Show when={!showErase()}>
            <SettingsButton settingsSize="sm" danger onClick={() => setShowErase(true)}>
              <Trash2 size={14} aria-hidden="true" /> {i18n.tr('Slett…', 'Erase…')}
            </SettingsButton>
          </Show>
        </div>
        <p class="velion-privacy-danger__note">
          {i18n.tr(
            'Dette sletter permanent Control-Plane-kontodataene dine og kan ikke angres. Data i Model- og Data-plane fjernes gjennom den separate slettingsprosessen nevnt ovenfor.',
            'This permanently erases your Control-Plane account data and cannot be undone. Model- and Data-plane data is removed via the separate erasure fan-out noted above.',
          )}
        </p>
        <Show when={showErase()}>
          <form class="velion-privacy-erase-form" onSubmit={runErase}>
            <label>
              {i18n.tr('Skriv inn e-posten din', 'Type your email')} (<strong>{accountEmail()}</strong>) {i18n.tr('for å bekrefte', 'to confirm')}
              <input
                type="email"
                autocomplete="off"
                value={typedEmail()}
                onInput={(event) => setTypedEmail(event.currentTarget.value)}
                placeholder={accountEmail()}
              />
            </label>
            <label>
              {i18n.tr('Bekreft passordet ditt', 'Confirm your password')}
              <input
                type="password"
                autocomplete="current-password"
                value={password()}
                onInput={(event) => setPassword(event.currentTarget.value)}
              />
            </label>
            <Show when={eraseError()}>
              {(message) => (
                <p class="velion-settings-status-message velion-settings-status-message--error" role="alert">
                  {message()}
                </p>
              )}
            </Show>
            <div class="velion-privacy-erase-actions">
              <SettingsButton type="button" settingsSize="sm" onClick={cancelErase} disabled={erasing()}>
                {i18n.tr('Avbryt', 'Cancel')}
              </SettingsButton>
              <SettingsButton type="submit" settingsSize="sm" danger disabled={!canErase()}>
                <Show when={erasing()} fallback={<>{i18n.tr('Slett kontoen min permanent', 'Permanently erase my account')}</>}>
                  <Loader2 size={14} class="velion-trust-spin" aria-hidden="true" /> {i18n.tr('Sletter…', 'Erasing…')}
                </Show>
              </SettingsButton>
            </div>
          </form>
        </Show>
      </div>
    </section>
  )
}
