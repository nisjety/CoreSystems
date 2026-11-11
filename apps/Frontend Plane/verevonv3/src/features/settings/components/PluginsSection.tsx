import { Loader2, Package, Pin, PinOff, ShieldAlert, Trash2 } from '@/shared/icons'
import { createMemo, createSignal, For, Show } from 'solid-js'
import { createResource } from '@/shared/lib/create-resource-compat'
import {
  createPlugin,
  deletePlugin,
  killPluginDigest,
  liftPluginKill,
  listPlugins,
  listPluginCatalog,
  listPluginKills,
  setPluginCatalogApproval,
  shortPin,
  uninstallPlugin,
  updatePlugin,
  type Plugin,
} from '@/shared/api/plugins-client'
import { hasErrorCode } from '@/shared/api/api-error'
import { useI18n } from '@/shared/i18n'
import { SectionHeader, SettingsButton, SettingsSelect } from '@/features/settings/components/settings-ui'
import { VerevonInput } from '@/shared/ui/verevon/VerevonInput'
import { getSession } from '@/shared/session/session-store'
import { hasWorkspaceAdminAccess } from '@/shared/session/access'
import { PluginInstallDialog, pluginRefusalText } from '@/features/settings/components/PluginInstallDialog'

/**
 * The plugin supply-chain controls (F-t3-059, W4-PL-4): the source pin, the manifest digest, the catalogue approval, the
 * kill state and the install consent. Off (the default), the section is the registry it has always been. Read on each call.
 */
export function pluginLoaderEnabled(): boolean {
  return import.meta.env.VITE_PLUGIN_LOADER === 'true'
}

/**
 * Plugin-pakker — installer og administrer plugin-manifester som kan bidra med
 * verktøy, ferdigheter og hooks til agenten.
 *
 * En plugin er inaktiv til en administrator aktiverer den (trygg utrulling).
 * Bare administratorer kan registrere, aktivere eller fjerne plugin-pakker —
 * de gjelder hele organisasjonen.
 */
export function PluginsSection() {
  const i18n = useI18n()
  const session = getSession()
  const orgId = createMemo(() => session.activeOrg?.id ?? '')
  const isAdmin = createMemo(() => hasWorkspaceAdminAccess(session))
  // With the loader on, the registry is for admins, and a member reads only the org's approved catalogue.
  const loaderOn = pluginLoaderEnabled()
  const catalogueOnly = createMemo(() => loaderOn && !isAdmin())

  const [plugins, { refetch }] = createResource(
    () => (catalogueOnly() ? undefined : orgId() || undefined),
    (id) => listPlugins(id).then((result) => result.plugins),
  )
  const [catalogue, { refetch: refetchCatalogue }] = createResource(
    () => (loaderOn && orgId() ? orgId() : undefined),
    () => listPluginCatalog().then((result) => result.plugins),
  )
  // The org's kill list (W4-PL-4): the digests killed for this org, and the platform's global kills that cover it. Only an
  // org admin reads it, the one role the route answers.
  const [kills, { refetch: refetchKills }] = createResource(
    () => (loaderOn && isAdmin() && orgId() ? orgId() : undefined),
    () => listPluginKills().then((result) => result.kills),
  )

  const [name, setName] = createSignal('')
  const [version, setVersion] = createSignal('')
  const [description, setDescription] = createSignal('')
  const [manifest, setManifest] = createSignal('')
  // The source a package registers from. It is sent only with the loader on, where the server needs all three.
  const [sourceKind, setSourceKind] = createSignal<'git_commit' | 'archive'>('git_commit')
  const [sourceRef, setSourceRef] = createSignal('')
  const [sourcePin, setSourcePin] = createSignal('')

  const [submitting, setSubmitting] = createSignal(false)
  const [busyId, setBusyId] = createSignal<string | null>(null)
  const [formError, setFormError] = createSignal<string | null>(null)
  const [actionError, setActionError] = createSignal<string | null>(null)
  const [notice, setNotice] = createSignal<string | null>(null)
  const [killingId, setKillingId] = createSignal<string | null>(null)
  const [killReason, setKillReason] = createSignal('')
  const [installing, setInstalling] = createSignal<Plugin | null>(null)

  const list = createMemo(() => plugins() ?? [])
  const catalogueEntries = createMemo(() => catalogue() ?? [])
  const listed = createMemo(() => new Set(catalogueEntries().map((entry) => entry.id)))
  const killEntries = createMemo(() => kills() ?? [])
  // Whether a digest is killed for this org, by the org itself or by the platform.
  const isKilled = (digest: string | undefined) =>
    digest !== undefined && killEntries().some((entry) => entry.digest === digest)
  // The org's own kill of a digest: the one an org admin lifts here. A platform kill is shown, and never lifted from here.
  const orgKillOf = (digest: string | undefined) =>
    digest === undefined
      ? undefined
      : killEntries().find((entry) => entry.digest === digest && entry.source === 'org')

  // A refusal that names a killed digest means the org's kill list has changed since it was read.
  const noteRefusal = (err: unknown) => {
    if (hasErrorCode(err, 'plugin_killed')) void refetchKills()
  }

  const sourceLabel = (kind: string | undefined) =>
    kind === 'archive' ? i18n.tr('arkiv', 'archive') : i18n.tr('git-commit', 'git commit')

  // After a change, reload what this page shows: the registry for an admin, the catalogue when the loader is on, and the
  // kill list for an admin with the loader on.
  const reload = async () => {
    if (!catalogueOnly()) await refetch()
    if (loaderOn) await refetchCatalogue()
    if (loaderOn && isAdmin()) await refetchKills()
  }

  const resetForm = () => {
    setName('')
    setVersion('')
    setDescription('')
    setManifest('')
    setSourceKind('git_commit')
    setSourceRef('')
    setSourcePin('')
  }

  const handleCreate = async (event: Event) => {
    event.preventDefault()
    const id = orgId()
    if (!id || submitting()) return

    const trimmedName = name().trim()
    const trimmedVersion = version().trim()
    if (!trimmedName || !trimmedVersion) {
      setFormError('Navn og versjon er påkrevd.')
      return
    }
    if (loaderOn && (sourceRef().trim() === '' || sourcePin().trim() === '')) {
      setFormError(i18n.tr('Kilde-URL og kildepin er påkrevd.', 'The source URL and the source pin are required.'))
      return
    }

    // Parse the optional manifest as JSON before sending so a malformed value is
    // caught here with a clear message rather than server-side.
    let manifestValue: unknown = {}
    const rawManifest = manifest().trim()
    if (rawManifest.length > 0) {
      try {
        manifestValue = JSON.parse(rawManifest)
      } catch {
        setFormError('Manifestet må være gyldig JSON.')
        return
      }
    }

    setSubmitting(true)
    setFormError(null)
    setNotice(null)
    try {
      await createPlugin(id, {
        name: trimmedName,
        version: trimmedVersion,
        description: description().trim() || undefined,
        manifest_json: manifestValue,
        ...(loaderOn
          ? { source_kind: sourceKind(), source_ref: sourceRef().trim(), source_pin: sourcePin().trim() }
          : {}),
      })
      resetForm()
      if (loaderOn) {
        setNotice(
          i18n.tr(
            'Pakken er registrert, men ikke festet. Fest den før den kan legges i katalogen.',
            'The package is registered but not pinned. Pin it before it can be listed in the catalogue.',
          ),
        )
      }
      await reload()
    } catch (err) {
      setFormError(
        pluginRefusalText(err, i18n.tr, { no: 'Kunne ikke registrere plugin-pakken.', en: 'Could not register the plugin package.' }),
      )
    } finally {
      setSubmitting(false)
    }
  }

  const handleToggle = async (plugin: Plugin) => {
    const id = orgId()
    if (!id || busyId()) return
    setBusyId(plugin.id)
    setActionError(null)
    try {
      await updatePlugin(id, plugin.id, { enabled: !plugin.enabled })
      await reload()
    } catch (err) {
      noteRefusal(err)
      setActionError(
        pluginRefusalText(err, i18n.tr, { no: 'Kunne ikke oppdatere plugin-pakken.', en: 'Could not update the plugin package.' }),
      )
    } finally {
      setBusyId(null)
    }
  }

  const handleDelete = async (plugin: Plugin) => {
    const id = orgId()
    if (!id || busyId()) return
    if (
      typeof window !== 'undefined' &&
      !window.confirm(`Fjerne plugin-pakken «${plugin.name}»?`)
    ) {
      return
    }
    setBusyId(plugin.id)
    setActionError(null)
    try {
      await deletePlugin(id, plugin.id)
      await reload()
    } catch (err) {
      setActionError(
        pluginRefusalText(err, i18n.tr, { no: 'Kunne ikke fjerne plugin-pakken.', en: 'Could not remove the plugin package.' }),
      )
    } finally {
      setBusyId(null)
    }
  }

  // Pins or unpins a package. Unpinning also disables it, which the server does, so the person is asked first.
  const handlePin = async (plugin: Plugin) => {
    if (busyId()) return
    if (
      plugin.pinned &&
      plugin.enabled &&
      typeof window !== 'undefined' &&
      !window.confirm(
        i18n.tr(
          `Løsne «${plugin.name}»? Pakken deaktiveres også.`,
          `Unpin “${plugin.name}”? The package is also disabled.`,
        ),
      )
    ) {
      return
    }
    setBusyId(plugin.id)
    setActionError(null)
    setNotice(null)
    try {
      await updatePlugin(orgId(), plugin.id, { pinned: !plugin.pinned })
      await reload()
    } catch (err) {
      noteRefusal(err)
      setActionError(
        pluginRefusalText(err, i18n.tr, { no: 'Kunne ikke endre festingen.', en: 'Could not change the pin.' }),
      )
    } finally {
      setBusyId(null)
    }
  }

  const handleUninstall = async (plugin: Plugin) => {
    if (busyId()) return
    if (
      typeof window !== 'undefined' &&
      !window.confirm(
        i18n.tr(
          `Avinstallere «${plugin.name}»? Ferdighetene og tjenerne pakken installerte, fjernes.`,
          `Uninstall “${plugin.name}”? The skills and servers the package installed are removed.`,
        ),
      )
    ) {
      return
    }
    setBusyId(plugin.id)
    setActionError(null)
    setNotice(null)
    try {
      const result = await uninstallPlugin(plugin.id)
      setNotice(
        i18n.tr(
          `Avinstallert. ${result.children.length} komponenter ble fjernet.`,
          `Uninstalled. ${result.children.length} components were removed.`,
        ),
      )
      await reload()
    } catch (err) {
      setActionError(
        pluginRefusalText(err, i18n.tr, { no: 'Kunne ikke avinstallere pakken.', en: 'Could not uninstall the package.' }),
      )
    } finally {
      setBusyId(null)
    }
  }

  // Lists a pinned package in the org catalogue, or takes it out again.
  const handleCatalogue = async (plugin: Plugin, approve: boolean) => {
    if (busyId()) return
    setBusyId(plugin.id)
    setActionError(null)
    setNotice(null)
    try {
      await setPluginCatalogApproval(plugin.id, approve)
      await reload()
    } catch (err) {
      noteRefusal(err)
      setActionError(
        pluginRefusalText(
          err,
          i18n.tr,
          approve
            ? { no: 'Kunne ikke legge pakken i katalogen.', en: 'Could not list the package in the catalogue.' }
            : { no: 'Kunne ikke fjerne pakken fra katalogen.', en: 'Could not remove the package from the catalogue.' },
        ),
      )
    } finally {
      setBusyId(null)
    }
  }

  const openKill = (plugin: Plugin) => {
    setActionError(null)
    setNotice(null)
    setKillReason('')
    setKillingId(plugin.id)
  }

  // Kills the package's manifest digest for this org. The server uninstalls every installed copy and clears the listing.
  const handleKill = async (plugin: Plugin) => {
    const digest = plugin.manifest_digest
    const reason = killReason().trim()
    if (busyId() || !digest || reason.length === 0) return
    setBusyId(plugin.id)
    setActionError(null)
    setNotice(null)
    try {
      const result = await killPluginDigest(digest, reason)
      setKillingId(null)
      setKillReason('')
      setNotice(
        i18n.tr(
          `Digesten er sperret. ${result.uninstalled_plugins.length} installerte pakker ble avinstallert, og ${result.children_removed} komponenter ble fjernet.`,
          `The digest is killed. ${result.uninstalled_plugins.length} installed packages were uninstalled, and ${result.children_removed} components were removed.`,
        ),
      )
      await reload()
    } catch (err) {
      setActionError(
        pluginRefusalText(err, i18n.tr, { no: 'Kunne ikke sperre digesten.', en: 'Could not kill the digest.' }),
      )
    } finally {
      setBusyId(null)
    }
  }

  // Lifts the org's kill of a package's digest. What the kill removed stays removed: the package is installed and listed again.
  const handleLift = async (plugin: Plugin) => {
    const digest = plugin.manifest_digest
    if (busyId() || !digest) return
    if (
      typeof window !== 'undefined' &&
      !window.confirm(
        i18n.tr(
          `Løfte sperringen av «${plugin.name}»? Digesten kan installeres og listes på nytt. Det sperringen fjernet, kommer ikke tilbake av seg selv.`,
          `Lift the kill on “${plugin.name}”? Its digest can be installed and listed again. What the kill removed does not come back by itself.`,
        ),
      )
    ) {
      return
    }
    setBusyId(plugin.id)
    setActionError(null)
    setNotice(null)
    try {
      await liftPluginKill(digest)
      setNotice(
        i18n.tr(
          'Sperringen er løftet. Pakken må installeres og listes i katalogen på nytt.',
          'The kill is lifted. The package must be installed and listed in the catalogue again.',
        ),
      )
      await reload()
    } catch (err) {
      setActionError(
        pluginRefusalText(err, i18n.tr, { no: 'Kunne ikke løfte sperringen.', en: 'Could not lift the kill.' }),
      )
    } finally {
      setBusyId(null)
    }
  }

  // The read-only catalogue a member sees with the loader on: the packages the org has approved, and nothing else.
  const catalogueView = () => (
    <Show
      when={!catalogue.loading}
      fallback={
        <p class="verevon-settings-subnote" role="status" aria-busy="true">
          {i18n.tr('Laster katalogen…', 'Loading the catalogue…')}
        </p>
      }
    >
      <Show
        when={catalogueEntries().length > 0}
        fallback={
          <div class="verevon-settings-list-card">
            <p class="verevon-settings-empty-row">{i18n.tr('Katalogen er tom.', 'The catalogue is empty.')}</p>
          </div>
        }
      >
        <div class="verevon-settings-list-card">
          <For each={catalogueEntries()}>
            {(entry) => (
              <div class="verevon-settings-integration-row">
                <div>
                  <p>
                    {entry.name} <span class="verevon-settings-subnote">v{entry.version}</span>{' '}
                    <span class="verevon-trust-chip">
                      {entry.installed ? i18n.tr('installert', 'installed') : i18n.tr('ikke installert', 'not installed')}
                    </span>
                  </p>
                  <span>
                    {i18n.tr('risiko', 'risk')} {entry.risk_level} · {entry.components.length}{' '}
                    {i18n.tr('komponenter', 'components')}
                    <Show when={entry.description.trim().length > 0}> · {entry.description}</Show>
                  </span>
                  <span class="verevon-settings-help">
                    {i18n.tr('Kilde', 'Source')}:{' '}
                    <code title={entry.source_ref}>
                      {sourceLabel(entry.source_kind)} {shortPin(entry.source_pin)}
                    </code>
                    {' · '}
                    {i18n.tr('Manifest', 'Manifest')}:{' '}
                    <code title={entry.manifest_digest}>{shortPin(entry.manifest_digest)}</code>
                  </span>
                </div>
              </div>
            )}
          </For>
        </div>
      </Show>
    </Show>
  )

  return (
    <>
      <SectionHeader
        title="Plugin-pakker"
        description={
          catalogueOnly()
            ? i18n.tr(
                'Plugin-pakker som organisasjonen har godkjent i katalogen. Bare administratorer endrer katalogen.',
                'Plugin packages the organization has approved for its catalogue. Only admins change the catalogue.',
              )
            : 'Registrer plugin-manifester som kan bidra med verktøy, ferdigheter og hooks. En plugin er inaktiv til en administrator aktiverer den.'
        }
      />

      <Show when={notice()}>
        {(message) => (
          <p class="verevon-settings-subnote" role="status">
            {message()}
          </p>
        )}
      </Show>

      <Show when={actionError()}>
        {(message) => (
          <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
            {message()}
          </p>
        )}
      </Show>

      <Show when={plugins.error}>
        <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
          Kunne ikke laste plugin-pakker.
        </p>
      </Show>

      <Show when={catalogue.error}>
        <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
          {i18n.tr('Kunne ikke laste plugin-katalogen.', 'Could not load the plugin catalogue.')}
        </p>
      </Show>

      <Show when={kills.error}>
        <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
          {i18n.tr('Kunne ikke laste sperrelisten.', 'Could not load the kill list.')}
        </p>
      </Show>

      <Show when={!catalogueOnly()} fallback={catalogueView()}>
        <Show
          when={!plugins.loading}
          fallback={
            <p class="verevon-settings-subnote" role="status" aria-busy="true">
              Laster plugin-pakker…
            </p>
          }
        >
          <Show
            when={list().length > 0}
            fallback={
              <div class="verevon-settings-list-card">
                <p class="verevon-settings-empty-row">
                  Ingen plugin-pakker er registrert ennå.
                </p>
              </div>
            }
          >
            <div class="verevon-settings-list-card">
              <For each={list()}>
                {(plugin) => (
                  <div class="verevon-settings-integration-row">
                    <div>
                      <p>
                        {plugin.name} <span class="verevon-settings-subnote">v{plugin.version}</span>{' '}
                        <span class="verevon-trust-chip" aria-label={`Status: ${plugin.enabled ? 'aktiv' : 'deaktivert'}`}>
                          {plugin.enabled ? 'aktiv' : 'deaktivert'}
                        </span>
                      </p>
                      <span>
                        {plugin.rollout_state} · risiko {plugin.risk_level}
                        <Show when={plugin.description.trim().length > 0}> · {plugin.description}</Show>
                      </span>
                      <Show when={loaderOn}>
                        <span class="verevon-settings-help">
                          <Show when={plugin.source_pin}>
                            {i18n.tr('Kilde', 'Source')}:{' '}
                            <code title={plugin.source_ref ?? ''}>
                              {sourceLabel(plugin.source_kind)} {shortPin(plugin.source_pin ?? '')}
                            </code>
                            {' · '}
                          </Show>
                          {i18n.tr('Manifest', 'Manifest')}:{' '}
                          <code title={plugin.manifest_digest ?? ''}>
                            {plugin.manifest_digest ? shortPin(plugin.manifest_digest) : i18n.tr('ingen', 'none')}
                          </code>
                        </span>
                        <span class="verevon-settings-help">
                          <span class="verevon-trust-chip">
                            {plugin.installed_at
                              ? i18n.tr('installert', 'installed')
                              : i18n.tr('ikke installert', 'not installed')}
                          </span>{' '}
                          <span class="verevon-trust-chip">
                            {plugin.pinned ? i18n.tr('festet', 'pinned') : i18n.tr('ikke festet', 'not pinned')}
                          </span>{' '}
                          <span class="verevon-trust-chip">
                            {listed().has(plugin.id)
                              ? i18n.tr('i katalogen', 'in the catalogue')
                              : i18n.tr('ikke i katalogen', 'not in the catalogue')}
                          </span>
                          <Show when={isKilled(plugin.manifest_digest)}>
                            {' '}
                            <span class="verevon-trust-chip" role="status">
                              {orgKillOf(plugin.manifest_digest)
                                ? i18n.tr('sperret', 'killed')
                                : i18n.tr('sperret globalt', 'killed globally')}
                            </span>
                          </Show>
                        </span>
                      </Show>
                    </div>
                    <Show when={isAdmin()}>
                      <div>
                        <SettingsButton
                          settingsSize="sm"
                          disabled={busyId() === plugin.id}
                          onClick={() => void handleToggle(plugin)}
                          aria-label={`${plugin.enabled ? 'Deaktiver' : 'Aktiver'} ${plugin.name}`}
                        >
                          <Show
                            when={busyId() === plugin.id}
                            fallback={<>{plugin.enabled ? 'Deaktiver' : 'Aktiver'}</>}
                          >
                            <Loader2 size={14} aria-hidden="true" /> Lagrer…
                          </Show>
                        </SettingsButton>{' '}
                        <SettingsButton
                          settingsSize="sm"
                          danger
                          disabled={busyId() === plugin.id}
                          onClick={() => void handleDelete(plugin)}
                          aria-label={`Fjern ${plugin.name}`}
                        >
                          <Trash2 size={14} aria-hidden="true" /> Fjern
                        </SettingsButton>
                        <Show when={loaderOn}>
                          <div>
                            <SettingsButton
                              settingsSize="sm"
                              disabled={busyId() === plugin.id}
                              onClick={() => void handlePin(plugin)}
                              aria-label={`${plugin.pinned ? i18n.tr('Løsne', 'Unpin') : i18n.tr('Fest', 'Pin')} ${plugin.name}`}
                            >
                              <Show
                                when={plugin.pinned}
                                fallback={
                                  <>
                                    <Pin size={14} aria-hidden="true" /> {i18n.tr('Fest', 'Pin')}
                                  </>
                                }
                              >
                                <PinOff size={14} aria-hidden="true" /> {i18n.tr('Løsne', 'Unpin')}
                              </Show>
                            </SettingsButton>{' '}
                            <Show
                              when={plugin.installed_at}
                              fallback={
                                <SettingsButton
                                  settingsSize="sm"
                                  disabled={busyId() === plugin.id || !plugin.manifest_digest || isKilled(plugin.manifest_digest)}
                                  onClick={() => setInstalling(plugin)}
                                  aria-label={`${i18n.tr('Installer', 'Install')} ${plugin.name}`}
                                >
                                  <Package size={14} aria-hidden="true" /> {i18n.tr('Installer…', 'Install…')}
                                </SettingsButton>
                              }
                            >
                              <SettingsButton
                                settingsSize="sm"
                                disabled={busyId() === plugin.id}
                                onClick={() => void handleUninstall(plugin)}
                                aria-label={`${i18n.tr('Avinstaller', 'Uninstall')} ${plugin.name}`}
                              >
                                {i18n.tr('Avinstaller', 'Uninstall')}
                              </SettingsButton>
                            </Show>{' '}
                            <Show
                              when={listed().has(plugin.id)}
                              fallback={
                                <SettingsButton
                                  settingsSize="sm"
                                  disabled={
                                    busyId() === plugin.id ||
                                    !plugin.pinned ||
                                    !plugin.manifest_digest ||
                                    isKilled(plugin.manifest_digest)
                                  }
                                  onClick={() => void handleCatalogue(plugin, true)}
                                  aria-label={`${i18n.tr('Legg i katalogen', 'List in the catalogue')} ${plugin.name}`}
                                >
                                  {i18n.tr('Legg i katalogen', 'List in the catalogue')}
                                </SettingsButton>
                              }
                            >
                              <SettingsButton
                                settingsSize="sm"
                                disabled={busyId() === plugin.id}
                                onClick={() => void handleCatalogue(plugin, false)}
                                aria-label={`${i18n.tr('Fjern fra katalogen', 'Remove from the catalogue')} ${plugin.name}`}
                              >
                                {i18n.tr('Fjern fra katalogen', 'Remove from the catalogue')}
                              </SettingsButton>
                            </Show>{' '}
                            <Show when={plugin.manifest_digest}>
                              <SettingsButton
                                settingsSize="sm"
                                danger
                                disabled={busyId() === plugin.id}
                                onClick={() => openKill(plugin)}
                                aria-label={`${i18n.tr('Sperr', 'Kill')} ${plugin.name}`}
                              >
                                <ShieldAlert size={14} aria-hidden="true" /> {i18n.tr('Sperr…', 'Kill…')}
                              </SettingsButton>
                            </Show>
                            <Show when={orgKillOf(plugin.manifest_digest)}>
                              {' '}
                              <SettingsButton
                                settingsSize="sm"
                                disabled={busyId() === plugin.id}
                                onClick={() => void handleLift(plugin)}
                                aria-label={`${i18n.tr('Løft sperringen av', 'Lift the kill on')} ${plugin.name}`}
                              >
                                {i18n.tr('Løft sperringen', 'Lift the kill')}
                              </SettingsButton>
                            </Show>
                            <Show when={killingId() === plugin.id}>
                              <div>
                                <label for={`plugin-kill-reason-${plugin.id}`} class="verevon-settings-field">
                                  <span class="verevon-settings-label">{i18n.tr('Begrunnelse', 'Reason')}</span>
                                  <span class="verevon-settings-input-wrap">
                                    <textarea
                                      id={`plugin-kill-reason-${plugin.id}`}
                                      value={killReason()}
                                      rows={2}
                                      maxlength={500}
                                      onInput={(event) => setKillReason(event.currentTarget.value)}
                                      class="verevon-settings-input verevon-settings-textarea"
                                    />
                                  </span>
                                  <span class="verevon-settings-help">
                                    {i18n.tr(
                                      'Sperringen fjerner alle installerte kopier av denne digesten i organisasjonen og tar pakken ut av katalogen. Den kan ikke angres herfra.',
                                      'The kill uninstalls every installed copy of this digest in the organization and takes the package out of the catalogue. It cannot be undone from here.',
                                    )}
                                  </span>
                                </label>
                                <SettingsButton
                                  settingsSize="sm"
                                  danger
                                  disabled={busyId() === plugin.id || killReason().trim().length === 0}
                                  onClick={() => void handleKill(plugin)}
                                >
                                  <ShieldAlert size={14} aria-hidden="true" />{' '}
                                  {i18n.tr('Bekreft sperringen', 'Confirm the kill')}
                                </SettingsButton>{' '}
                                <SettingsButton settingsSize="sm" onClick={() => setKillingId(null)}>
                                  {i18n.tr('Avbryt', 'Cancel')}
                                </SettingsButton>
                              </div>
                            </Show>
                          </div>
                        </Show>
                      </div>
                    </Show>
                  </div>
                )}
              </For>
            </div>
          </Show>
        </Show>
      </Show>

      <Show
        when={isAdmin()}
        fallback={
          <p class="verevon-settings-subnote">
            <Package size={14} aria-hidden="true" /> Bare administratorer kan registrere
            eller endre plugin-pakker.
          </p>
        }
      >
        <form class="verevon-settings-field-grid verevon-settings-field-grid--spaced" onSubmit={handleCreate}>
          <label for="plugin-name" class="verevon-settings-field">
            <span class="verevon-settings-label">Navn</span>
            <span class="verevon-settings-input-wrap">
              <VerevonInput
                id="plugin-name"
                value={name()}
                required
                placeholder="f.eks. github-toolkit"
                onInput={(event) => setName(event.currentTarget.value)}
                class="verevon-settings-input"
              />
            </span>
          </label>

          <label for="plugin-version" class="verevon-settings-field">
            <span class="verevon-settings-label">Versjon</span>
            <span class="verevon-settings-input-wrap">
              <VerevonInput
                id="plugin-version"
                value={version()}
                required
                placeholder="f.eks. 1.0.0"
                onInput={(event) => setVersion(event.currentTarget.value)}
                class="verevon-settings-input"
              />
            </span>
          </label>

          <label for="plugin-description" class="verevon-settings-field">
            <span class="verevon-settings-label">Beskrivelse (valgfritt)</span>
            <span class="verevon-settings-input-wrap">
              <VerevonInput
                id="plugin-description"
                value={description()}
                placeholder="Kort hva pluginen gjør"
                onInput={(event) => setDescription(event.currentTarget.value)}
                class="verevon-settings-input"
              />
            </span>
          </label>

          <label for="plugin-manifest" class="verevon-settings-field">
            <span class="verevon-settings-label">Manifest (JSON, valgfritt)</span>
            <span class="verevon-settings-input-wrap">
              <textarea
                id="plugin-manifest"
                value={manifest()}
                rows={4}
                placeholder={'{\n  "tools": [],\n  "skills": [],\n  "hooks": []\n}'}
                onInput={(event) => setManifest(event.currentTarget.value)}
                class="verevon-settings-input verevon-settings-textarea"
              />
            </span>
            <span class="verevon-settings-help">
              Pakken registreres deaktivert. Aktiver den når den er testet.
            </span>
          </label>

          <Show when={loaderOn}>
            <SettingsSelect
              id="plugin-source-kind"
              label={i18n.tr('Kildetype', 'Source type')}
              value={sourceKind()}
              onChange={(event) => setSourceKind(event.currentTarget.value === 'archive' ? 'archive' : 'git_commit')}
              options={[
                { value: 'git_commit', label: i18n.tr('Git-commit (full SHA)', 'Git commit (full SHA)') },
                { value: 'archive', label: i18n.tr('Arkiv (sha256)', 'Archive (sha256)') },
              ]}
            />

            <label for="plugin-source-ref" class="verevon-settings-field">
              <span class="verevon-settings-label">{i18n.tr('Kilde-URL (https)', 'Source URL (https)')}</span>
              <span class="verevon-settings-input-wrap">
                <VerevonInput
                  id="plugin-source-ref"
                  value={sourceRef()}
                  placeholder="https://"
                  onInput={(event) => setSourceRef(event.currentTarget.value)}
                  class="verevon-settings-input"
                />
              </span>
            </label>

            <label for="plugin-source-pin" class="verevon-settings-field">
              <span class="verevon-settings-label">{i18n.tr('Kildepin', 'Source pin')}</span>
              <span class="verevon-settings-input-wrap">
                <VerevonInput
                  id="plugin-source-pin"
                  value={sourcePin()}
                  placeholder={i18n.tr('40 heksategn, eller sha256 og 64 heksategn', '40 hex characters, or sha256 and 64 hex characters')}
                  onInput={(event) => setSourcePin(event.currentTarget.value)}
                  class="verevon-settings-input"
                />
              </span>
              <span class="verevon-settings-help">
                {i18n.tr(
                  'En full commit-SHA eller en sha256-digest. En gren eller en tagg godtas ikke.',
                  'A full commit SHA or a sha256 digest. A branch or a tag is refused.',
                )}
              </span>
            </label>
          </Show>

          <Show when={formError()}>
            {(message) => (
              <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
                {message()}
              </p>
            )}
          </Show>

          <div>
            <SettingsButton type="submit" variant="primary" settingsSize="sm" disabled={submitting()}>
              <Show
                when={submitting()}
                fallback={<><Package size={14} aria-hidden="true" /> Registrer plugin</>}
              >
                <Loader2 size={14} aria-hidden="true" /> Registrerer…
              </Show>
            </SettingsButton>
          </div>
        </form>
      </Show>

      <Show when={loaderOn && isAdmin()}>
        <p class="verevon-settings-subnote">
          {i18n.tr(
            'Sperringene leses fra sperrelisten til organisasjonen. En global sperring gjelder alle organisasjoner og kan ikke løftes herfra.',
            'Kills are read from the kill list of the organization. A global kill applies to every organization and cannot be lifted here.',
          )}
        </p>
      </Show>

      <p class="verevon-settings-subnote">
        <Package size={14} aria-hidden="true" /> Org-tilhørighet utledes fra den
        verifiserte økten. Plugin-pakker gjelder hele organisasjonen.
      </p>

      <Show when={installing()}>
        {(plugin) => (
          <PluginInstallDialog
            plugin={plugin()}
            onClose={() => setInstalling(null)}
            onChanged={() => void reload()}
            onKilled={() => void refetchKills()}
          />
        )}
      </Show>
    </>
  )
}
