import { ArrowUpRight, FileUp, FolderPlus, Globe2, HardDriveUpload, RefreshCw } from 'lucide-solid'
import { createSignal, For, onCleanup, onMount, Show, type JSX } from 'solid-js'
import { Button } from '@/shared/ui/Button'
import { VelionInput } from '@/shared/ui/velion/VelionInput'
import { useI18n } from '@/shared/i18n'
import type { SharePointDrive, SharePointSite } from '@/shared/api/knowledge-client'

type ConnectProvider = {
  detail: string
  id: string
  label: string
  sources: readonly string[]
}

type SharePointSourceForm = {
  driveId: string
  driveName: string
  driveType: string
  siteId: string
  siteWebUrl: string
  tenantId: string
}

type WebsiteCrawlForm = {
  maxPages: string
  url: string
}

export function KnowledgeAddSourceModal(props: {
  busy: boolean
  onClose: () => void
  onConnectProvider: (provider: ConnectProvider) => Promise<void> | void
  onListSharePointSites: () => Promise<SharePointSite[]>
  onListSharePointDrives: (siteId: string) => Promise<SharePointDrive[]>
  onRegisterSharePoint: (input: SharePointSourceForm) => Promise<void>
  onStartWebsiteCrawl: (input: { maxPages?: number; url: string }) => Promise<void>
  onUploadFiles: (files: File[]) => Promise<void>
  providers: readonly ConnectProvider[]
}) {
  const i18n = useI18n()
  let fileInputRef!: HTMLInputElement
  const [selectedFiles, setSelectedFiles] = createSignal<File[]>([])

  // SharePoint picker state: load the org's sites, pick one, load its document
  // libraries, pick one → register. Falls back to manual id entry via a toggle.
  const [spSites, setSpSites] = createSignal<SharePointSite[]>([])
  const [spDrives, setSpDrives] = createSignal<SharePointDrive[]>([])
  const [spSelectedSite, setSpSelectedSite] = createSignal<SharePointSite | null>(null)
  const [spSelectedDrive, setSpSelectedDrive] = createSignal<SharePointDrive | null>(null)
  const [spLoadingSites, setSpLoadingSites] = createSignal(false)
  const [spLoadingDrives, setSpLoadingDrives] = createSignal(false)
  const [spError, setSpError] = createSignal<string | null>(null)
  const [spManual, setSpManual] = createSignal(false)

  const loadSpSites = async () => {
    setSpError(null)
    setSpLoadingSites(true)
    try {
      const sites = await props.onListSharePointSites()
      setSpSites(sites)
      if (sites.length === 0) {
        setSpError(i18n.tr('Ingen SharePoint-nettsteder funnet. Koble til Microsoft 365 først.', 'No SharePoint sites found. Connect Microsoft 365 first.'))
      }
    } catch (error) {
      setSpError(error instanceof Error ? error.message : i18n.tr('Kunne ikke laste nettsteder.', 'Could not load sites.'))
    } finally {
      setSpLoadingSites(false)
    }
  }

  const selectSpSite = async (site: SharePointSite | null) => {
    setSpSelectedSite(site)
    setSpSelectedDrive(null)
    setSpDrives([])
    if (!site) return
    setSpError(null)
    setSpLoadingDrives(true)
    try {
      const drives = await props.onListSharePointDrives(site.id)
      setSpDrives(drives)
      if (drives.length === 0) {
        setSpError(i18n.tr('Ingen dokumentbiblioteker på dette nettstedet.', 'No document libraries on this site.'))
      } else if (drives.length === 1) {
        setSpSelectedDrive(drives[0] ?? null)
      }
    } catch (error) {
      setSpError(error instanceof Error ? error.message : i18n.tr('Kunne ikke laste biblioteker.', 'Could not load libraries.'))
    } finally {
      setSpLoadingDrives(false)
    }
  }

  const submitSpPicker = async () => {
    const site = spSelectedSite()
    const drive = spSelectedDrive()
    if (!site || !drive) return
    await props.onRegisterSharePoint({
      siteId: site.id,
      siteWebUrl: site.web_url ?? '',
      driveId: drive.id,
      driveName: drive.name || site.display_name || site.name,
      driveType: drive.drive_type || 'documentLibrary',
      tenantId: '',
    })
  }

  const [sharePoint, setSharePoint] = createSignal<SharePointSourceForm>({
    driveId: '',
    driveName: '',
    driveType: 'documentLibrary',
    siteId: '',
    siteWebUrl: '',
    tenantId: '',
  })
  const [websiteCrawl, setWebsiteCrawl] = createSignal<WebsiteCrawlForm>({
    url: '',
    maxPages: '12',
  })

  onMount(() => {
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') props.onClose()
    }
    window.addEventListener('keydown', closeOnEscape)
    onCleanup(() => window.removeEventListener('keydown', closeOnEscape))
  })

  async function submitUpload() {
    if (selectedFiles().length === 0) return
    await props.onUploadFiles(selectedFiles())
    setSelectedFiles([])
    if (fileInputRef) fileInputRef.value = ''
  }

  async function submitWebsiteCrawl() {
    const target = websiteCrawl().url.trim()
    if (!target) return
    const parsedMaxPages = Number.parseInt(websiteCrawl().maxPages, 10)
    await props.onStartWebsiteCrawl({
      url: target,
      maxPages: Number.isFinite(parsedMaxPages) ? parsedMaxPages : undefined,
    })
    setWebsiteCrawl({ url: '', maxPages: '12' })
  }

  return (
    <div class="knowledge-modal" role="dialog" aria-modal="true" aria-label={i18n.tr('Legg til kunnskapskilde', 'Add knowledge source')}>
      <button class="knowledge-modal__scrim" type="button" aria-label={i18n.tr('Lukk legg til kilde', 'Close add source')} onClick={() => props.onClose()} />
      <div class="knowledge-modal__panel">
        <header class="knowledge-modal__header">
          <div>
            <h2>{i18n.tr('Legg til kilde', 'Add source')}</h2>
            <p>
              {i18n.tr(
                'Last opp filer via imports-core, åpne nye autorisasjonsflyter for integrasjoner, eller registrer en SharePoint-stasjon for Finspo-synkronisering.',
                'Upload files through imports-core, open new integration auth flows, or register a SharePoint drive for Finspo sync.',
              )}
            </p>
          </div>
          <button class="knowledge-modal__close" type="button" onClick={() => props.onClose()} aria-label={i18n.tr('Lukk legg til kilde', 'Close add source')}>
            {i18n.tr('Esc', 'Esc')}
          </button>
        </header>

        <div class="knowledge-modal__top-grid">
          <section class="knowledge-modal-card">
            <ModalCardHeading
              icon={<FileUp class="size-5" />}
              title={i18n.tr('Last opp filer', 'Upload files')}
              description={i18n.tr('Oppretter en imports-core-jobb og sender filene inn i Data Plane v2.', 'Creates an imports-core job and pushes the files into Data Plane v2.')}
            />

            <input
              ref={fileInputRef}
              type="file"
              multiple
              class="knowledge-file-input"
              onChange={(event) => setSelectedFiles(Array.from(event.currentTarget.files ?? []))}
            />

            <div class="knowledge-file-chip-row">
              <Show
                when={selectedFiles().length > 0}
                fallback={<span class="knowledge-muted-copy">{i18n.tr('Ingen filer valgt ennå.', 'No files selected yet.')}</span>}
              >
                <For each={selectedFiles()}>
                  {(file) => <span class="knowledge-file-chip">{file.name}</span>}
                </For>
              </Show>
            </div>

            <Button
              variant="primary"
              size="sm"
              class="knowledge-modal-action"
              disabled={props.busy || selectedFiles().length === 0}
              onClick={() => void submitUpload()}
            >
              <HardDriveUpload class="size-4" />
              {i18n.tr('Importer valgte filer', 'Import selected files')}
            </Button>
          </section>

          <section class="knowledge-modal-card">
            <ModalCardHeading
              icon={<ArrowUpRight class="size-5" />}
              title={i18n.tr('Koble til et arbeidsområde', 'Connect a workspace')}
              description={i18n.tr('Starter en direkte integration-core OAuth-økt i et nytt vindu.', 'Starts a live integration-core OAuth session in a new window.')}
            />

            <div class="knowledge-provider-grid">
              <For each={props.providers}>
                {(provider) => (
                  <button
                    type="button"
                    disabled={props.busy}
                    onClick={() => void props.onConnectProvider(provider)}
                    class="knowledge-provider-button"
                  >
                    <span>
                      <strong>{provider.label}</strong>
                      <small>{provider.detail}</small>
                    </span>
                    <ArrowUpRight class="size-4" />
                  </button>
                )}
              </For>
            </div>
          </section>
        </div>

        <section class="knowledge-modal-card">
          <ModalCardHeading
            icon={<Globe2 class="size-5" />}
            title={i18n.tr('Gjennomsøk et nettsted', 'Crawl a website')}
            description={i18n.tr('Starter en Quarry-gjennomsøking slik at nettsider kan flyte inn i Kunnskap via innhentingsstakken.', 'Starts a Quarry crawl so website pages can flow into Knowledge through the ingestion stack.')}
          />

          <div class="knowledge-modal-form-grid knowledge-modal-form-grid--url">
            <Field
              label={i18n.tr('Nettadresse', 'Website URL')}
              value={websiteCrawl().url}
              onChange={(value) => setWebsiteCrawl((current) => ({ ...current, url: value }))}
              placeholder="https://docs.velion.ai"
            />
            <Field
              label={i18n.tr('Maks antall sider', 'Max pages')}
              value={websiteCrawl().maxPages}
              onChange={(value) => setWebsiteCrawl((current) => ({ ...current, maxPages: value }))}
              placeholder="12"
            />
          </div>

          <Button
            variant="primary"
            size="sm"
            class="knowledge-modal-action"
            disabled={props.busy || !websiteCrawl().url.trim()}
            onClick={() => void submitWebsiteCrawl()}
          >
            <Globe2 class="size-4" />
            {i18n.tr('Start gjennomsøking', 'Start crawl')}
          </Button>
        </section>

        <section class="knowledge-modal-card">
          <ModalCardHeading
            icon={<FolderPlus class="size-5" />}
            title={i18n.tr('Legg til SharePoint-bibliotek', 'Add SharePoint library')}
            description={i18n.tr('Velg et nettsted og et dokumentbibliotek fra Microsoft 365 — så synkroniserer Velion det inn i Kunnskap.', 'Pick a site and a document library from Microsoft 365 — Velion syncs it into Knowledge.')}
          />

          <Show
            when={!spManual()}
            fallback={
              <div class="knowledge-modal-form-grid">
                <Field
                  label={i18n.tr('Nettsted-ID', 'Site ID')}
                  value={sharePoint().siteId}
                  onChange={(value) => setSharePoint((current) => ({ ...current, siteId: value }))}
                  placeholder="contoso.sharepoint.com,site-id,web-id"
                />
                <Field
                  label={i18n.tr('Stasjon-ID', 'Drive ID')}
                  value={sharePoint().driveId}
                  onChange={(value) => setSharePoint((current) => ({ ...current, driveId: value }))}
                  placeholder="b!drive-id"
                />
                <Field
                  label={i18n.tr('Stasjonsnavn', 'Drive name')}
                  value={sharePoint().driveName}
                  onChange={(value) => setSharePoint((current) => ({ ...current, driveName: value }))}
                  placeholder={i18n.tr('Support-kunnskap', 'Support knowledge')}
                />
                <Field
                  label={i18n.tr('Stasjonstype', 'Drive type')}
                  value={sharePoint().driveType}
                  onChange={(value) => setSharePoint((current) => ({ ...current, driveType: value }))}
                  placeholder="documentLibrary"
                />
                <Field
                  label={i18n.tr('Nettsted-URL', 'Site URL')}
                  value={sharePoint().siteWebUrl}
                  onChange={(value) => setSharePoint((current) => ({ ...current, siteWebUrl: value }))}
                  placeholder="https://contoso.sharepoint.com/sites/Support"
                />
                <Field
                  label={i18n.tr('Leier-ID', 'Tenant ID')}
                  value={sharePoint().tenantId}
                  onChange={(value) => setSharePoint((current) => ({ ...current, tenantId: value }))}
                  placeholder={i18n.tr('Valgfritt', 'Optional')}
                />
              </div>
            }
          >
            <div class="knowledge-sp-picker">
              <Show
                when={spSites().length > 0}
                fallback={
                  <Button
                    variant="secondary"
                    size="sm"
                    disabled={props.busy || spLoadingSites()}
                    onClick={() => void loadSpSites()}
                  >
                    <RefreshCw class={`size-4 ${spLoadingSites() ? 'knowledge-sp-spin' : ''}`} />
                    {spLoadingSites()
                      ? i18n.tr('Laster nettsteder ...', 'Loading sites ...')
                      : i18n.tr('Last inn SharePoint-nettsteder', 'Load SharePoint sites')}
                  </Button>
                }
              >
                <label class="velion-settings-field">
                  <span>{i18n.tr('Nettsted', 'Site')}</span>
                  <select
                    value={spSelectedSite()?.id ?? ''}
                    disabled={props.busy || spLoadingDrives()}
                    onChange={(event) =>
                      void selectSpSite(spSites().find((site) => site.id === event.currentTarget.value) ?? null)
                    }
                  >
                    <option value="">{i18n.tr('Velg et nettsted …', 'Select a site …')}</option>
                    <For each={spSites()}>
                      {(site) => <option value={site.id}>{site.display_name || site.name}</option>}
                    </For>
                  </select>
                </label>

                <Show when={spSelectedSite()}>
                  <label class="velion-settings-field">
                    <span>{i18n.tr('Dokumentbibliotek', 'Document library')}</span>
                    <select
                      value={spSelectedDrive()?.id ?? ''}
                      disabled={props.busy || spLoadingDrives() || spDrives().length === 0}
                      onChange={(event) =>
                        setSpSelectedDrive(spDrives().find((drive) => drive.id === event.currentTarget.value) ?? null)
                      }
                    >
                      <option value="">
                        {spLoadingDrives()
                          ? i18n.tr('Laster biblioteker …', 'Loading libraries …')
                          : i18n.tr('Velg et bibliotek …', 'Select a library …')}
                      </option>
                      <For each={spDrives()}>
                        {(drive) => <option value={drive.id}>{drive.name}</option>}
                      </For>
                    </select>
                  </label>
                </Show>
              </Show>

              <Show when={spError()}>
                {(message) => (
                  <p class="velion-settings-status-message velion-settings-status-message--error" role="alert">
                    {message()}
                  </p>
                )}
              </Show>
            </div>
          </Show>

          <div class="knowledge-modal-action-row">
            <Show
              when={!spManual()}
              fallback={
                <Button
                  variant="primary"
                  size="sm"
                  class="knowledge-modal-action"
                  disabled={props.busy || !sharePoint().siteId.trim() || !sharePoint().driveId.trim()}
                  onClick={() => void props.onRegisterSharePoint(sharePoint())}
                >
                  <FolderPlus class="size-4" />
                  {i18n.tr('Registrer og synkroniser', 'Register and sync')}
                </Button>
              }
            >
              <Button
                variant="primary"
                size="sm"
                class="knowledge-modal-action"
                disabled={props.busy || !spSelectedSite() || !spSelectedDrive()}
                onClick={() => void submitSpPicker()}
              >
                <FolderPlus class="size-4" />
                {i18n.tr('Registrer og synkroniser', 'Register and sync')}
              </Button>
            </Show>

            <button
              type="button"
              class="knowledge-sp-manual-toggle"
              onClick={() => setSpManual((value) => !value)}
            >
              {spManual()
                ? i18n.tr('← Bruk nettsted-velger', '← Use site picker')
                : i18n.tr('Skriv inn ID-er manuelt', 'Enter IDs manually')}
            </button>
          </div>
        </section>
      </div>
    </div>
  )
}

function ModalCardHeading(props: {
  description: string
  icon: JSX.Element
  title: string
}) {
  return (
    <div class="knowledge-modal-card__heading">
      <span>{props.icon}</span>
      <div>
        <h3>{props.title}</h3>
        <p>{props.description}</p>
      </div>
    </div>
  )
}

function Field(props: {
  label: string
  onChange: (value: string) => void
  placeholder: string
  value: string
}) {
  return (
    <label class="knowledge-field">
      <span>{props.label}</span>
      <VelionInput
        value={props.value}
        placeholder={props.placeholder}
        onInput={(event) => props.onChange(event.currentTarget.value)}
      />
    </label>
  )
}
