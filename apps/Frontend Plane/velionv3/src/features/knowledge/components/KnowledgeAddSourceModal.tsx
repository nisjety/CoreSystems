import { ArrowUpRight, FileUp, FolderPlus, Globe2, HardDriveUpload } from 'lucide-solid'
import { createSignal, For, onCleanup, onMount, Show, type JSX } from 'solid-js'
import { Button } from '@/shared/ui/Button'
import { VelionInput } from '@/shared/ui/velion/VelionInput'
import { useI18n } from '@/shared/i18n'

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
  onRegisterSharePoint: (input: SharePointSourceForm) => Promise<void>
  onStartWebsiteCrawl: (input: { maxPages?: number; url: string }) => Promise<void>
  onUploadFiles: (files: File[]) => Promise<void>
  providers: readonly ConnectProvider[]
}) {
  const i18n = useI18n()
  let fileInputRef!: HTMLInputElement
  const [selectedFiles, setSelectedFiles] = createSignal<File[]>([])
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
            title={i18n.tr('Registrer SharePoint-stasjon', 'Register SharePoint drive')}
            description={i18n.tr('Lagrer en Finspo-kilde og starter umiddelbart en SharePoint- eller OneDrive-synkronisering.', 'Persists a Finspo source and immediately starts a SharePoint or OneDrive sync.')}
          />

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
