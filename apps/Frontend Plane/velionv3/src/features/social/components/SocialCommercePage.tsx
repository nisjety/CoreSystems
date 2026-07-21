import { A } from '@solidjs/router'
import { AlertCircle, BarChart3, Clock3, PackageSearch, ShoppingBag } from 'lucide-solid'
import { createMemo, createResource, createSignal, For, Show } from 'solid-js'
import { loadSocialContext, platformLabels } from '@/features/social/lib/social-workspace'
import {
  listSocialCatalogProducts,
  listSocialCatalogs,
  listSocialMetrics,
  type SocialCatalog,
  type SocialCatalogProduct,
  type SocialMetric,
  type SocialProviderKey,
} from '@/shared/api/social-client'
import { MetricCard } from '@/shared/ui/MetricCard'
import { useI18n } from '@/shared/i18n'

type TrFn = (noText: string, enText: string) => string

type CommerceWorkspace = {
  orgId: string
  orgLabel: string
  metrics: SocialMetric[]
  metricsAvailable: boolean
  catalogs: SocialCatalog[]
  catalogsAvailable: boolean
}

type SelectedCatalog = {
  catalogId: string
  accountId: string
  label: string
}

async function loadCommerceWorkspace(): Promise<CommerceWorkspace> {
  const context = await loadSocialContext()
  if (!context.orgId.trim()) {
    return {
      orgId: '',
      orgLabel: context.orgLabel,
      metrics: [],
      metricsAvailable: false,
      catalogs: [],
      catalogsAvailable: false,
    }
  }

  const [metrics, catalogs] = await Promise.all([
    listSocialMetrics(context.orgId)
      .then((result) => ({ ok: true, items: result.metrics }))
      .catch(() => ({ ok: false, items: [] as SocialMetric[] })),
    listSocialCatalogs(context.orgId)
      .then((result) => ({ ok: true, items: result.catalogs }))
      .catch(() => ({ ok: false, items: [] as SocialCatalog[] })),
  ])

  return {
    orgId: context.orgId,
    orgLabel: context.orgLabel,
    metrics: metrics.items,
    metricsAvailable: metrics.ok,
    catalogs: catalogs.items,
    catalogsAvailable: catalogs.ok,
  }
}

export default function SocialCommercePage() {
  const i18n = useI18n()
  const [workspace] = createResource(loadCommerceWorkspace)
  const [selected, setSelected] = createSignal<SelectedCatalog | null>(null)

  const [products] = createResource(selected, async (choice) => {
    const orgId = workspace()?.orgId ?? ''
    if (!orgId || !choice.accountId) return { ok: false, items: [] as SocialCatalogProduct[] }
    return listSocialCatalogProducts(orgId, choice.catalogId, choice.accountId)
      .then((result) => ({ ok: true, items: result.products }))
      .catch(() => ({ ok: false, items: [] as SocialCatalogProduct[] }))
  })

  const metrics = createMemo(() => workspace()?.metrics ?? [])
  const catalogs = createMemo(() => workspace()?.catalogs ?? [])

  return (
    <div class="velion-social-ops-page">
      <section class="velion-social-ops-hero">
        <div>
          <span class="velion-social-kicker">
            <BarChart3 size={14} />
            {i18n.tr('Sosial handel', 'Social commerce')}
          </span>
          <h1>{i18n.tr('Annonsestatistikk og kataloger', 'Ad metrics & catalogs')}</h1>
          <p>
            {i18n.tr(
              'Annonse- og analysebilder per leverandør, samt skrivebeskyttede Meta Commerce-kataloger for de tilkoblede sosiale kontoene i denne organisasjonen. Verdiene leveres av social-core; ingenting er fabrikkert.',
              'Per-provider ad and analytics snapshots plus read-only Meta Commerce catalogs for the connected social accounts in this organization. Values are served by social-core; nothing is fabricated.',
            )}
          </p>
        </div>
        <A href="/settings/integrations">{i18n.tr('Administrer integrasjoner', 'Manage integrations')}</A>
      </section>

      <Show when={!workspace()}>
        <p class="velion-social-ops-state">
          <Clock3 size={16} />
          {i18n.tr('Laster organisasjonsscopet handelsarbeidsområde …', 'Loading organization-scoped commerce workspace...')}
        </p>
      </Show>

      <Show when={workspace() && !workspace()!.orgId}>
        <p class="velion-social-ops-state velion-social-ops-state--warning">
          <AlertCircle size={16} />
          {i18n.tr('Ingen organisasjonsscope ble løst, så ingen statistikk eller kataloger kunne lastes.', 'No organization scope was resolved, so no metrics or catalogs could be loaded.')}
        </p>
      </Show>

      <Show when={workspace()?.orgId}>
        <section class="velion-social-ops-metrics" aria-label={i18n.tr('Annonsestatistikk', 'Ad metrics')}>
          <Show
            when={metrics().length}
            fallback={
              <p class="velion-social-ops-state">
                <AlertCircle size={16} />
                <Show
                  when={workspace()?.metricsAvailable}
                  fallback={i18n.tr('Statistikk er utilgjengelig fordi den sosiale gatewayen eller social-core ikke kan nås.', 'Metrics are unavailable because the social gateway or social-core is unreachable.')}
                >
                  {i18n.tr(
                    'Ingen statistikkbilder registrert ennå. Statistikk fylles ut etter at social-core kjører et øyeblikksbilde for tilkoblede annonse-/analysekontoer.',
                    'No metric snapshots recorded yet. Metrics populate after social-core runs a snapshot for connected ad/analytics accounts.',
                  )}
                </Show>
              </p>
            }
          >
            <For each={metrics()}>
              {(metric) => (
                <MetricCard
                  label={`${providerLabel(metric.providerKey, i18n.tr)} · ${metric.metricName}`}
                  value={formatMetricValue(metric.metricValue)}
                  delta={formatSnapshotDate(metric.snapshotDate, i18n.tr)}
                />
              )}
            </For>
          </Show>
        </section>
      </Show>

      <Show when={workspace()?.orgId}>
        <section class="velion-social-ops-grid" aria-label={i18n.tr('Handelskataloger', 'Commerce catalogs')}>
          <Show
            when={catalogs().length}
            fallback={
              <article class="velion-social-ops-card">
                <div>
                  <ShoppingBag size={16} />
                  <span>Meta Commerce</span>
                </div>
                <h2>{i18n.tr('Ingen handelskataloger', 'No commerce catalogs')}</h2>
                <p>
                  <Show
                    when={workspace()?.catalogsAvailable}
                    fallback={i18n.tr('Kataloger er utilgjengelige fordi den sosiale gatewayen eller social-core ikke kan nås.', 'Catalogs are unavailable because the social gateway or social-core is unreachable.')}
                  >
                    {i18n.tr(
                      'Koble til en Meta-konto (Facebook/Instagram) med social.catalog.manage-funksjonen for å lese handelskatalogene dens her.',
                      'Connect a Meta (Facebook/Instagram) account with the social.catalog.manage capability to read its commerce catalogs here.',
                    )}
                  </Show>
                </p>
                <footer class="velion-social-ops-card__footer">
                  <A href="/settings/integrations">{i18n.tr('Koble til kontoer', 'Connect accounts')}</A>
                </footer>
              </article>
            }
          >
            <For each={catalogs()}>
              {(catalog) => {
                const catalogId = catalogField(catalog, 'id')
                const accountId = catalogField(catalog, 'account_id')
                const label = catalogField(catalog, 'name') || catalogId || i18n.tr('Katalog', 'Catalog')
                const providerKey = catalogField(catalog, 'provider_key')
                const productCount = catalogField(catalog, 'product_count')
                const isSelected = createMemo(() => selected()?.catalogId === catalogId)
                return (
                  <article class="velion-social-ops-card">
                    <div>
                      <ShoppingBag size={16} />
                      <span>{providerLabel(providerKey, i18n.tr)}</span>
                    </div>
                    <h2>{label}</h2>
                    <p>
                      {productCount ? i18n.tr(`${productCount} produkter.`, `${productCount} products.`) : i18n.tr('Katalog tilkoblet.', 'Catalog connected.')}
                      {catalogId ? i18n.tr(` Katalog ${catalogId}.`, ` Catalog ${catalogId}.`) : ''}
                    </p>
                    <footer class="velion-social-ops-card__footer">
                      <button
                        type="button"
                        disabled={!catalogId || !accountId}
                        onClick={() => setSelected({ catalogId, accountId, label })}
                      >
                        {isSelected() ? i18n.tr('Viser produkter', 'Viewing products') : i18n.tr('Vis produkter', 'View products')}
                      </button>
                    </footer>
                  </article>
                )
              }}
            </For>
          </Show>
        </section>
      </Show>

      <Show when={selected()}>
        {(choice) => (
          <section class="velion-social-ops-grid" aria-label={i18n.tr('Katalogprodukter', 'Catalog products')}>
            <Show when={!products.loading} fallback={
              <p class="velion-social-ops-state">
                <Clock3 size={16} />
                {i18n.tr(`Laster produkter for ${choice().label} …`, `Loading products for ${choice().label}...`)}
              </p>
            }>
              <Show
                when={products()?.items.length}
                fallback={
                  <p class="velion-social-ops-state">
                    <PackageSearch size={16} />
                    <Show
                      when={products()?.ok}
                      fallback={i18n.tr('Produkter er utilgjengelige for denne katalogen akkurat nå.', 'Products are unavailable for this catalog right now.')}
                    >
                      {i18n.tr(`Ingen produkter funnet i ${choice().label}.`, `No products found in ${choice().label}.`)}
                    </Show>
                  </p>
                }
              >
                <For each={products()?.items ?? []}>
                  {(product) => (
                    <article class="velion-social-ops-card">
                      <div>
                        <PackageSearch size={16} />
                        <span>{choice().label}</span>
                      </div>
                      <h2>{catalogField(product, 'name') || catalogField(product, 'id') || i18n.tr('Produkt', 'Product')}</h2>
                      <p>
                        {[
                          catalogField(product, 'retailer_id') && `SKU ${catalogField(product, 'retailer_id')}`,
                          catalogField(product, 'price'),
                          catalogField(product, 'availability'),
                        ]
                          .filter(Boolean)
                          .join(' · ') || i18n.tr('Produktoppføring.', 'Product record.')}
                      </p>
                    </article>
                  )}
                </For>
              </Show>
            </Show>
          </section>
        )}
      </Show>
    </div>
  )
}

function providerLabel(providerKey: string, tr: TrFn): string {
  return platformLabels[providerKey as SocialProviderKey] ?? providerKey ?? tr('Leverandør', 'Provider')
}

function formatMetricValue(value: number): string {
  if (!Number.isFinite(value)) return '0'
  return new Intl.NumberFormat('en-GB', { maximumFractionDigits: 2 }).format(value)
}

function formatSnapshotDate(iso: string, tr: TrFn): string {
  if (!iso) return tr('ingen øyeblikksbildedato', 'no snapshot date')
  const parsed = new Date(iso)
  if (Number.isNaN(parsed.getTime())) return iso
  return parsed.toLocaleDateString('en-GB', { day: 'numeric', month: 'short', year: 'numeric' })
}

// Reads a string-ish field from an opaque provider record (Meta catalog/product
// objects are snake_case passthroughs). Numbers are coerced to strings.
function catalogField(record: Record<string, unknown>, key: string): string {
  const value = record[key]
  if (typeof value === 'string') return value.trim()
  if (typeof value === 'number' && Number.isFinite(value)) return String(value)
  return ''
}
