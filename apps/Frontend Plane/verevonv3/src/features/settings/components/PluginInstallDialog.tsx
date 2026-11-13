import { Loader2, ShieldCheck, X } from '@/shared/icons'
import { createMemo, createSignal, For, Show } from 'solid-js'
import { Portal } from '@solidjs/web'
import { createResource } from '@/shared/lib/create-resource-compat'
import {
  getPluginComponents,
  installPlugin,
  shortPin,
  type PluginComponent,
  type PluginInstallResult,
} from '@/shared/api/plugins-client'
import { hasErrorCode } from '@/shared/api/api-error'
import { translateApiError, useI18n } from '@/shared/i18n'
import { SettingsButton } from '@/features/settings/components/settings-ui'

type Tr = (noText: string, enText: string) => string

/** What a component kind is called in the install report, in the singular. An unknown kind shows its own key. */
function componentKindLabel(kind: string, tr: Tr): string {
  switch (kind) {
    case 'skill':
      return tr('Ferdighet', 'Skill')
    case 'mcp_server':
      return tr('MCP-tjener', 'MCP server')
    case 'command':
      return tr('Kommando', 'Command')
    case 'hook':
      return tr('Krok', 'Hook')
    case 'bin':
      return tr('Kjørbar fil', 'Binary')
    case 'script':
      return tr('Skript', 'Script')
    case 'stdio_server':
      return tr('Lokal stdio-tjener', 'Local stdio server')
    default:
      return kind
  }
}

/** The same kind in the plural, for the consent line that says how many an install creates. */
function componentKindPlural(kind: string, tr: Tr): string {
  switch (kind) {
    case 'skill':
      return tr('ferdigheter', 'skills')
    case 'mcp_server':
      return tr('MCP-tjenere', 'MCP servers')
    case 'command':
      return tr('kommandoer', 'commands')
    default:
      return componentKindLabel(kind, tr).toLowerCase()
  }
}

function statusLabel(status: string, tr: Tr): string {
  switch (status) {
    case 'pending':
      return tr('Installeres', 'Will be installed')
    case 'installed':
      return tr('Installert', 'Installed')
    case 'refused':
      return tr('Ikke installert', 'Not installed')
    case 'removed':
      return tr('Fjernet', 'Removed')
    default:
      return status
  }
}

/**
 * The typed refusals of the plugin routes, in words a person can act on. Any other refusal gets translateApiError's
 * copy for its status class, or the caller's own sentence. The server's raw message is never shown.
 */
const PLUGIN_REFUSAL_COPY: ReadonlyArray<readonly [string, { no: string; en: string }]> = [
  [
    'plugin_killed',
    {
      no: 'Pakken er på sperrelisten og kan ikke festes, aktiveres eller installeres.',
      en: 'The package is on the kill list and cannot be pinned, enabled or installed.',
    },
  ],
  [
    'plugin_source_required',
    {
      no: 'Pakken må ha en registrert kilde før den kan festes eller brukes.',
      en: 'The package needs a registered source before it can be pinned or used.',
    },
  ],
  [
    'plugin_source_invalid',
    {
      no: 'Kilden må være en https-adresse, og pinnen en full commit-SHA eller en sha256-digest.',
      en: 'The source must be an https address, and the pin a full commit SHA or a sha256 digest.',
    },
  ],
  [
    'plugin_manifest_invalid',
    { no: 'Manifestet er ugyldig.', en: 'The manifest is not valid.' },
  ],
  [
    'plugin_already_installed',
    { no: 'Pakken er allerede installert.', en: 'The package is already installed.' },
  ],
  [
    'plugin_not_installed',
    {
      no: 'Pakken må være installert og festet før den kan aktiveres.',
      en: 'The package must be installed and pinned before it can be enabled.',
    },
  ],
  [
    'plugin_changed',
    {
      no: 'Pakken endret seg mens du arbeidet med den. Prøv igjen.',
      en: 'The package changed while you were working on it. Try again.',
    },
  ],
  [
    'plugin_install_under_zdr_refused',
    {
      no: 'Pakker kan ikke installeres under null datalagring (ZDR), fordi de lagrer ferdigheter og tjenere.',
      en: 'Packages cannot be installed under zero data retention (ZDR), because they store skills and servers.',
    },
  ],
  [
    'plugin_write_under_zdr_refused',
    {
      no: 'Katalogen og sperrelisten er varig tilstand, og kan ikke endres under null datalagring (ZDR).',
      en: 'The catalogue and the kill list are durable state, and cannot change under zero data retention (ZDR).',
    },
  ],
  [
    // The server counts the reason in bytes, so a long text with æøå or emoji can pass the field and still be refused.
    'plugin_kill_reason_invalid',
    {
      no: 'Begrunnelsen må være gyldig tekst på høyst 500 byte.',
      en: 'The reason must be valid text of at most 500 bytes.',
    },
  ],
]

/** The sentence a failed plugin request shows: a typed refusal's own words, otherwise translateApiError's copy. */
export function pluginRefusalText(err: unknown, tr: Tr, fallback: { no: string; en: string }): string {
  const refusal = PLUGIN_REFUSAL_COPY.find(([code]) => hasErrorCode(err, code))
  return refusal ? tr(refusal[1].no, refusal[1].en) : translateApiError(err, tr, fallback)
}

/**
 * The install consent of one plugin (W4-PL-4). It reads the install report, lists every component with its kind,
 * name and digest, and marks what is refused. The install needs an explicit tick for each kind an install would create,
 * and it sends back exactly the digests the report listed as consentable. The server's answer is shown after it.
 */
export function PluginInstallDialog(props: {
  plugin: { id: string; name: string; version: string }
  onClose: () => void
  /** Called after an install the server accepted, so the page behind the dialog reloads what it shows. */
  onChanged: () => void
  /** Called when the server refuses the install because the manifest digest is on the kill list. */
  onKilled?: () => void
}) {
  const i18n = useI18n()
  const [report, { refetch }] = createResource(
    () => props.plugin.id,
    (id) => getPluginComponents(id),
  )
  // Whether the admin has ticked the consent for each component kind an install would create.
  const [consented, setConsented] = createSignal<Record<string, boolean>>({})
  const [busy, setBusy] = createSignal(false)
  const [error, setError] = createSignal<string | null>(null)
  // The server's answer to the install, once it has one. Until then the dialog shows the report it read.
  const [outcome, setOutcome] = createSignal<PluginInstallResult | null>(null)

  const components = createMemo<PluginComponent[]>(() => outcome()?.components ?? report()?.components ?? [])
  const installable = createMemo(() => components().filter((component) => component.status === 'pending'))
  const consentKinds = createMemo(() => Array.from(new Set(installable().map((component) => component.kind))))
  const installedNow = createMemo(() => outcome()?.installed ?? report()?.installed ?? false)
  const allConsented = createMemo(
    () => consentKinds().length > 0 && consentKinds().every((kind) => consented()[kind] === true),
  )
  const canInstall = createMemo(
    () =>
      outcome() === null &&
      !installedNow() &&
      !busy() &&
      allConsented() &&
      (report()?.consent_digests.length ?? 0) > 0,
  )

  const kindInstallable = (kind: string) => installable().filter((component) => component.kind === kind)

  const setKindConsent = (kind: string, checked: boolean) => {
    setConsented({ ...consented(), [kind]: checked })
  }

  const close = () => {
    if (!busy()) props.onClose()
  }

  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key === 'Escape') {
      event.stopPropagation()
      close()
    }
  }

  const install = async () => {
    const current = report()
    if (!current || !canInstall()) return
    setBusy(true)
    setError(null)
    try {
      const result = await installPlugin(props.plugin.id, current.consent_digests)
      setOutcome(result)
      props.onChanged()
    } catch (err) {
      if (hasErrorCode(err, 'plugin_consent_mismatch', 'plugin_changed')) {
        // The list the admin consented to is no longer the one the server holds: read it again, then consent again.
        setConsented({})
        setError(
          i18n.tr(
            'Komponentlisten har endret seg siden du leste den. Les den på nytt og samtykk igjen.',
            'The component list changed since you read it. Read it again and consent again.',
          ),
        )
        void refetch()
      } else {
        if (hasErrorCode(err, 'plugin_killed')) props.onKilled?.()
        setError(
          pluginRefusalText(err, i18n.tr, { no: 'Kunne ikke installere pakken.', en: 'Could not install the package.' }),
        )
      }
    } finally {
      setBusy(false)
    }
  }

  return (
    <Portal>
      <div
        class="verevon-schedule-dialog"
        role="presentation"
        onClick={(event) => {
          if (event.target === event.currentTarget) close()
        }}
      >
        <section
          class="verevon-schedule-dialog__dialog"
          role="dialog"
          aria-modal="true"
          aria-labelledby="plugin-install-title"
          onKeyDown={onKeyDown}
        >
          <header>
            <h3 id="plugin-install-title">
              <ShieldCheck size={16} aria-hidden="true" /> {i18n.tr('Installer', 'Install')}: {props.plugin.name}{' '}
              <span class="verevon-settings-subnote">v{props.plugin.version}</span>
            </h3>
            <button
              type="button"
              ref={(element) => queueMicrotask(() => element.focus())}
              onClick={close}
              aria-label={i18n.tr('Lukk', 'Close')}
            >
              <X size={16} aria-hidden="true" />
            </button>
          </header>

          <p class="verevon-schedule-dialog__note">
            {i18n.tr(
              'Krok, kjørbare filer, skript og lokale stdio-tjenere kjøres aldri og installeres ikke. De vises bare som ikke installert.',
              'Hooks, binaries, scripts and local stdio servers are never run or installed. They are shown only as not installed.',
            )}
          </p>

          <Show
            when={!report.loading}
            fallback={
              <p class="verevon-schedule-dialog__note" role="status" aria-busy="true">
                {i18n.tr('Laster installasjonsrapporten…', 'Loading the install report…')}
              </p>
            }
          >
            <Show when={report.error}>
              <p class="verevon-schedule-dialog__error" role="alert">
                {translateApiError(report.error, i18n.tr, {
                  no: 'Kunne ikke laste installasjonsrapporten.',
                  en: 'Could not load the install report.',
                })}
              </p>
            </Show>

            <ol class="verevon-settings-list-card" aria-label={i18n.tr('Komponenter', 'Components')}>
              <For each={components()}>
                {(component) => (
                  <li class="verevon-settings-integration-row">
                    <div>
                      <p>
                        <span class="verevon-trust-chip">{componentKindLabel(component.kind, i18n.tr)}</span>{' '}
                        {component.name}
                      </p>
                      <span class="verevon-settings-help">
                        <code title={component.digest}>{shortPin(component.digest)}</code>
                        {' · '}
                        {statusLabel(component.status, i18n.tr)}
                        <Show when={component.status === 'refused' && component.report}>
                          {' · '}
                          {component.report}
                        </Show>
                      </span>
                    </div>
                  </li>
                )}
              </For>
            </ol>
          </Show>

          <Show when={outcome() === null && !installedNow() && consentKinds().length > 0}>
            <div class="verevon-settings-field-grid verevon-settings-field-grid--spaced">
              <For each={consentKinds()}>
                {(kind) => (
                  <label class="verevon-settings-field">
                    <span class="verevon-settings-label">
                      <input
                        type="checkbox"
                        checked={consented()[kind] === true}
                        disabled={busy()}
                        onChange={(event) => setKindConsent(kind, event.currentTarget.checked)}
                      />{' '}
                      {i18n.tr('Jeg samtykker til å installere', 'I consent to installing')}{' '}
                      {kindInstallable(kind).length} {componentKindPlural(kind, i18n.tr)}
                    </span>
                    <span class="verevon-settings-help">
                      {kindInstallable(kind).map((component) => component.name).join(', ')}
                    </span>
                  </label>
                )}
              </For>
            </div>
          </Show>

          <Show when={outcome() === null && installedNow()}>
            <p class="verevon-schedule-dialog__note" role="status">
              {i18n.tr(
                'Pakken er allerede installert. Avinstaller den før du installerer den på nytt.',
                'The package is already installed. Uninstall it before installing it again.',
              )}
            </p>
          </Show>

          <Show when={outcome() === null && !installedNow() && report() !== undefined && installable().length === 0}>
            <p class="verevon-schedule-dialog__note">
              {i18n.tr(
                'Ingen av komponentene kan installeres, så det er ingenting å samtykke til.',
                'None of the components can be installed, so there is nothing to consent to.',
              )}
            </p>
          </Show>

          <Show when={error()}>
            {(message) => (
              <p class="verevon-schedule-dialog__error" role="alert">
                {message()}
              </p>
            )}
          </Show>

          <Show when={outcome()}>
            {(result) => (
              <p class="verevon-schedule-dialog__note" role="status">
                {i18n.tr(
                  `Installert. ${result().children.length} komponenter ble opprettet. Pakken tas i bruk først når den er festet og aktivert i listen.`,
                  `Installed. ${result().children.length} components were created. The package is used only once it is pinned and enabled in the list.`,
                )}
              </p>
            )}
          </Show>

          <footer>
            <Show when={outcome() === null && !installedNow()}>
              <SettingsButton
                type="button"
                variant="primary"
                settingsSize="sm"
                disabled={!canInstall()}
                onClick={() => void install()}
              >
                <Show
                  when={busy()}
                  fallback={
                    <>
                      <ShieldCheck size={14} aria-hidden="true" />{' '}
                      {i18n.tr('Installer med dette samtykket', 'Install with this consent')}
                    </>
                  }
                >
                  <Loader2 size={14} aria-hidden="true" /> {i18n.tr('Installerer…', 'Installing…')}
                </Show>
              </SettingsButton>
            </Show>
            <SettingsButton type="button" settingsSize="sm" onClick={close}>
              {i18n.tr('Lukk', 'Close')}
            </SettingsButton>
          </footer>
        </section>
      </div>
    </Portal>
  )
}
