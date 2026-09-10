import { For, Show, createEffect, createSignal, untrack } from 'solid-js'
import type { SharePointDrive, SharePointSite, SharePointSourceRegistration } from '@/shared/api/knowledge-client'
import { useI18n } from '@/shared/i18n'
import { Button } from '@/shared/ui/Button'

/** The SharePoint browse + register calls the picker needs. Callers wire them
 * to the Knowledge SharePoint routes; keeping them injected lets each surface
 * own its org id and post-register follow-up, and keeps the picker testable. */
export type LibraryPickerActions = {
  listSites: () => Promise<SharePointSite[]>
  listDrives: (siteId: string) => Promise<SharePointDrive[]>
  register: (connectionId: string, selection: SharePointSourceRegistration) => Promise<void>
}

type SharePointLibraryPickerProps = {
  connectionId: string
  library: LibraryPickerActions
  onDone: () => void
  onSkip: () => void
}

/**
 * Inline "velg bibliotek": the org's SharePoint sites → one site's document
 * libraries → register. Registration goes through the Knowledge SharePoint
 * route (finspo-core source + first sync), which is what makes a Microsoft
 * connection's documents lane succeed — integration-core refuses a generic
 * Microsoft sync with `409 no_sources_registered` until one exists. The first
 * document library of the first site is preselected so the common case is a
 * single click.
 *
 * Shared by the onboarding connect step and Settings → Integrations so both
 * surfaces offer the identical recovery instead of a generic failure.
 */
export function SharePointLibraryPicker(props: SharePointLibraryPickerProps) {
  const i18n = useI18n()
  const [sites, setSites] = createSignal<SharePointSite[]>([])
  const [drives, setDrives] = createSignal<SharePointDrive[]>([])
  const [siteId, setSiteId] = createSignal('')
  const [driveId, setDriveId] = createSignal('')
  const [loadingSites, setLoadingSites] = createSignal(false)
  const [loadingDrives, setLoadingDrives] = createSignal(false)
  const [registering, setRegistering] = createSignal(false)
  const [error, setError] = createSignal<string>()

  const loadDrives = async (nextSiteId: string) => {
    setSiteId(nextSiteId)
    setDrives([])
    setDriveId('')
    if (!nextSiteId) return
    setLoadingDrives(true)
    setError(undefined)
    try {
      const loaded = await props.library.listDrives(nextSiteId)
      setDrives(loaded)
      const preferred = loaded.find((drive) => (drive.drive_type ?? '').toLowerCase() === 'documentlibrary') ?? loaded[0]
      setDriveId(preferred?.id ?? '')
      if (loaded.length === 0) {
        setError(i18n.tr('Dette nettstedet har ingen dokumentbiblioteker.', 'This site has no document libraries.'))
      }
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : i18n.tr('Kunne ikke hente biblioteker.', 'Could not load libraries.'))
    } finally {
      setLoadingDrives(false)
    }
  }

  // Solid v2 has no onMount; a two-phase createEffect with a constant compute
  // runs the effect exactly once after mount.
  createEffect(
    () => undefined,
    () => {
      void (async () => {
        setLoadingSites(true)
        setError(undefined)
        try {
          const loaded = await props.library.listSites()
          setSites(loaded)
          const first = loaded[0]
          if (first) await loadDrives(first.id)
          else setError(i18n.tr('Fant ingen SharePoint-nettsteder på denne kontoen.', 'No SharePoint sites were found on this account.'))
        } catch (reason) {
          setError(reason instanceof Error ? reason.message : i18n.tr('Kunne ikke hente SharePoint-nettsteder.', 'Could not load SharePoint sites.'))
        } finally {
          setLoadingSites(false)
        }
      })()
    },
  )

  const register = async () => {
    const site = untrack(() => sites().find((candidate) => candidate.id === siteId()))
    const drive = untrack(() => drives().find((candidate) => candidate.id === driveId()))
    if (!site || !drive) return
    setRegistering(true)
    setError(undefined)
    try {
      await props.library.register(props.connectionId, {
        kind: 'drive',
        siteId: site.id,
        siteWebUrl: site.web_url,
        driveId: drive.id,
        driveName: drive.name,
        driveType: drive.drive_type,
      })
      props.onDone()
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : i18n.tr('Biblioteket kunne ikke registreres.', 'The library could not be registered.'))
    } finally {
      setRegistering(false)
    }
  }

  const siteName = (site: SharePointSite) => site.display_name || site.name || site.web_url || site.id

  return (
    <div class="onboarding-library-picker" role="group" aria-label={i18n.tr('Velg SharePoint-bibliotek', 'Pick SharePoint library')}>
      <label class="onboarding-library-picker__field">
        <span>{i18n.tr('Nettsted', 'Site')}</span>
        <select
          value={siteId()}
          disabled={loadingSites() || registering()}
          onChange={(event) => void loadDrives(event.currentTarget.value)}
        >
          <Show when={loadingSites()}>
            <option value="">{i18n.tr('Henter nettsteder …', 'Loading sites…')}</option>
          </Show>
          <For each={sites()}>{(site) => <option value={site.id}>{siteName(site)}</option>}</For>
        </select>
      </label>
      <label class="onboarding-library-picker__field">
        <span>{i18n.tr('Bibliotek', 'Library')}</span>
        <select
          value={driveId()}
          disabled={loadingDrives() || registering() || drives().length === 0}
          onChange={(event) => setDriveId(event.currentTarget.value)}
        >
          <Show when={loadingDrives()}>
            <option value="">{i18n.tr('Henter biblioteker …', 'Loading libraries…')}</option>
          </Show>
          <For each={drives()}>{(drive) => <option value={drive.id}>{drive.name}</option>}</For>
        </select>
      </label>
      <Show when={error()}>
        {(message) => <p class="onboarding-library-picker__error" role="alert">{message()}</p>}
      </Show>
      <div class="onboarding-library-picker__actions">
        <Button size="sm" variant="primary" disabled={!siteId() || !driveId() || registering()} onClick={() => void register()}>
          {registering() ? i18n.tr('Registrerer …', 'Registering…') : i18n.tr('Legg til bibliotek', 'Add library')}
        </Button>
        <button type="button" class="onboarding-connected__refresh" disabled={registering()} onClick={props.onSkip}>
          {i18n.tr('Velg senere under Kunnskap', 'Choose later under Knowledge')}
        </button>
      </div>
    </div>
  )
}
