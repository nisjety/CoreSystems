import { Loader2, Package, Trash2 } from 'lucide-solid'
import { createMemo, createResource, createSignal, For, Show } from 'solid-js'
import {
  createPlugin,
  deletePlugin,
  listPlugins,
  updatePlugin,
  type Plugin,
} from '@/shared/api/plugins-client'
import { ApiError } from '@/shared/api/http'
import { SectionHeader, SettingsButton } from '@/features/settings/components/settings-ui'
import { VelionInput } from '@/shared/ui/velion/VelionInput'
import { getSession } from '@/shared/session/session-store'
import { hasWorkspaceAdminAccess } from '@/shared/session/access'

/**
 * Plugin-pakker — installer og administrer plugin-manifester som kan bidra med
 * verktøy, ferdigheter og hooks til agenten.
 *
 * En plugin er inaktiv til en administrator aktiverer den (trygg utrulling).
 * Bare administratorer kan registrere, aktivere eller fjerne plugin-pakker —
 * de gjelder hele organisasjonen.
 */
export function PluginsSection() {
  const session = getSession()
  const orgId = createMemo(() => session.activeOrg?.id ?? '')
  const isAdmin = createMemo(() => hasWorkspaceAdminAccess(session))

  const [plugins, { refetch }] = createResource(
    () => orgId() || undefined,
    (id) => listPlugins(id).then((result) => result.plugins),
  )

  const [name, setName] = createSignal('')
  const [version, setVersion] = createSignal('')
  const [description, setDescription] = createSignal('')
  const [manifest, setManifest] = createSignal('')

  const [submitting, setSubmitting] = createSignal(false)
  const [busyId, setBusyId] = createSignal<string | null>(null)
  const [formError, setFormError] = createSignal<string | null>(null)
  const [actionError, setActionError] = createSignal<string | null>(null)

  const list = createMemo(() => plugins() ?? [])

  const resetForm = () => {
    setName('')
    setVersion('')
    setDescription('')
    setManifest('')
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
    try {
      await createPlugin(id, {
        name: trimmedName,
        version: trimmedVersion,
        description: description().trim() || undefined,
        manifest_json: manifestValue,
      })
      resetForm()
      await refetch()
    } catch (err) {
      setFormError(
        err instanceof ApiError ? err.message : 'Kunne ikke registrere plugin-pakken.',
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
      await refetch()
    } catch (err) {
      setActionError(
        err instanceof ApiError ? err.message : 'Kunne ikke oppdatere plugin-pakken.',
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
      await refetch()
    } catch (err) {
      setActionError(
        err instanceof ApiError ? err.message : 'Kunne ikke fjerne plugin-pakken.',
      )
    } finally {
      setBusyId(null)
    }
  }

  return (
    <>
      <SectionHeader
        title="Plugin-pakker"
        description="Registrer plugin-manifester som kan bidra med verktøy, ferdigheter og hooks. En plugin er inaktiv til en administrator aktiverer den."
      />

      <Show when={actionError()}>
        {(message) => (
          <p class="velion-settings-status-message velion-settings-status-message--error" role="alert">
            {message()}
          </p>
        )}
      </Show>

      <Show when={plugins.error}>
        <p class="velion-settings-status-message velion-settings-status-message--error" role="alert">
          Kunne ikke laste plugin-pakker.
        </p>
      </Show>

      <Show
        when={!plugins.loading}
        fallback={
          <p class="velion-settings-subnote" role="status" aria-busy="true">
            Laster plugin-pakker…
          </p>
        }
      >
        <Show
          when={list().length > 0}
          fallback={
            <div class="velion-settings-list-card">
              <p class="velion-settings-empty-row">
                Ingen plugin-pakker er registrert ennå.
              </p>
            </div>
          }
        >
          <div class="velion-settings-list-card">
            <For each={list()}>
              {(plugin) => (
                <div class="velion-settings-integration-row">
                  <div>
                    <p>
                      {plugin.name} <span class="velion-settings-subnote">v{plugin.version}</span>{' '}
                      <span class="velion-trust-chip" aria-label={`Status: ${plugin.enabled ? 'aktiv' : 'deaktivert'}`}>
                        {plugin.enabled ? 'aktiv' : 'deaktivert'}
                      </span>
                    </p>
                    <span>
                      {plugin.rollout_state} · risiko {plugin.risk_level}
                      <Show when={plugin.description.trim().length > 0}> · {plugin.description}</Show>
                    </span>
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
                    </div>
                  </Show>
                </div>
              )}
            </For>
          </div>
        </Show>
      </Show>

      <Show
        when={isAdmin()}
        fallback={
          <p class="velion-settings-subnote">
            <Package size={14} aria-hidden="true" /> Bare administratorer kan registrere
            eller endre plugin-pakker.
          </p>
        }
      >
        <form class="velion-settings-field-grid velion-settings-field-grid--spaced" onSubmit={handleCreate}>
          <label for="plugin-name" class="velion-settings-field">
            <span class="velion-settings-label">Navn</span>
            <span class="velion-settings-input-wrap">
              <VelionInput
                id="plugin-name"
                value={name()}
                required
                placeholder="f.eks. github-toolkit"
                onInput={(event) => setName(event.currentTarget.value)}
                class="velion-settings-input"
              />
            </span>
          </label>

          <label for="plugin-version" class="velion-settings-field">
            <span class="velion-settings-label">Versjon</span>
            <span class="velion-settings-input-wrap">
              <VelionInput
                id="plugin-version"
                value={version()}
                required
                placeholder="f.eks. 1.0.0"
                onInput={(event) => setVersion(event.currentTarget.value)}
                class="velion-settings-input"
              />
            </span>
          </label>

          <label for="plugin-description" class="velion-settings-field">
            <span class="velion-settings-label">Beskrivelse (valgfritt)</span>
            <span class="velion-settings-input-wrap">
              <VelionInput
                id="plugin-description"
                value={description()}
                placeholder="Kort hva pluginen gjør"
                onInput={(event) => setDescription(event.currentTarget.value)}
                class="velion-settings-input"
              />
            </span>
          </label>

          <label for="plugin-manifest" class="velion-settings-field">
            <span class="velion-settings-label">Manifest (JSON, valgfritt)</span>
            <span class="velion-settings-input-wrap">
              <textarea
                id="plugin-manifest"
                value={manifest()}
                rows={4}
                placeholder={'{\n  "tools": [],\n  "skills": [],\n  "hooks": []\n}'}
                onInput={(event) => setManifest(event.currentTarget.value)}
                class="velion-settings-input velion-settings-textarea"
              />
            </span>
            <span class="velion-settings-help">
              Pakken registreres deaktivert. Aktiver den når den er testet.
            </span>
          </label>

          <Show when={formError()}>
            {(message) => (
              <p class="velion-settings-status-message velion-settings-status-message--error" role="alert">
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

      <p class="velion-settings-subnote">
        <Package size={14} aria-hidden="true" /> Org-tilhørighet utledes fra den
        verifiserte økten. Plugin-pakker gjelder hele organisasjonen.
      </p>
    </>
  )
}
