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
            Social commerce
          </span>
          <h1>Ad metrics &amp; catalogs</h1>
          <p>
            Per-provider ad and analytics snapshots plus read-only Meta Commerce catalogs for the
            connected social accounts in this organization. Values are served by social-core; nothing
            is fabricated.
          </p>
        </div>
        <A href="/settings/integrations">Manage integrations</A>
      </section>

      <Show when={!workspace()}>
        <p class="velion-social-ops-state">
          <Clock3 size={16} />
          Loading organization-scoped commerce workspace...
        </p>
      </Show>

      <Show when={workspace() && !workspace()!.orgId}>
        <p class="velion-social-ops-state velion-social-ops-state--warning">
          <AlertCircle size={16} />
          No organization scope was resolved, so no metrics or catalogs could be loaded.
        </p>
      </Show>

      <Show when={workspace()?.orgId}>
        <section class="velion-social-ops-metrics" aria-label="Ad metrics">
          <Show
            when={metrics().length}
            fallback={
              <p class="velion-social-ops-state">
                <AlertCircle size={16} />
                <Show
                  when={workspace()?.metricsAvailable}
                  fallback="Metrics are unavailable because the social gateway or social-core is unreachable."
                >
                  No metric snapshots recorded yet. Metrics populate after social-core runs a snapshot
                  for connected ad/analytics accounts.
                </Show>
              </p>
            }
          >
            <For each={metrics()}>
              {(metric) => (
                <MetricCard
                  label={`${providerLabel(metric.providerKey)} · ${metric.metricName}`}
                  value={formatMetricValue(metric.metricValue)}
                  delta={formatSnapshotDate(metric.snapshotDate)}
                />
              )}
            </For>
          </Show>
        </section>
      </Show>

      <Show when={workspace()?.orgId}>
        <section class="velion-social-ops-grid" aria-label="Commerce catalogs">
          <Show
            when={catalogs().length}
            fallback={
              <article class="velion-social-ops-card">
                <div>
                  <ShoppingBag size={16} />
                  <span>Meta Commerce</span>
                </div>
                <h2>No commerce catalogs</h2>
                <p>
                  <Show
                    when={workspace()?.catalogsAvailable}
                    fallback="Catalogs are unavailable because the social gateway or social-core is unreachable."
                  >
                    Connect a Meta (Facebook/Instagram) account with the social.catalog.manage
                    capability to read its commerce catalogs here.
                  </Show>
                </p>
                <footer class="velion-social-ops-card__footer">
                  <A href="/settings/integrations">Connect accounts</A>
                </footer>
              </article>
            }
          >
            <For each={catalogs()}>
              {(catalog) => {
                const catalogId = catalogField(catalog, 'id')
                const accountId = catalogField(catalog, 'account_id')
                const label = catalogField(catalog, 'name') || catalogId || 'Catalog'
                const providerKey = catalogField(catalog, 'provider_key')
                const productCount = catalogField(catalog, 'product_count')
                const isSelected = createMemo(() => selected()?.catalogId === catalogId)
                return (
                  <article class="velion-social-ops-card">
                    <div>
                      <ShoppingBag size={16} />
                      <span>{providerLabel(providerKey)}</span>
                    </div>
                    <h2>{label}</h2>
                    <p>
                      {productCount ? `${productCount} products.` : 'Catalog connected.'}
                      {catalogId ? ` Catalog ${catalogId}.` : ''}
                    </p>
                    <footer class="velion-social-ops-card__footer">
                      <button
                        type="button"
                        disabled={!catalogId || !accountId}
                        onClick={() => setSelected({ catalogId, accountId, label })}
                      >
                        {isSelected() ? 'Viewing products' : 'View products'}
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
          <section class="velion-social-ops-grid" aria-label="Catalog products">
            <Show when={!products.loading} fallback={
              <p class="velion-social-ops-state">
                <Clock3 size={16} />
                Loading products for {choice().label}...
              </p>
            }>
              <Show
                when={products()?.items.length}
                fallback={
                  <p class="velion-social-ops-state">
                    <PackageSearch size={16} />
                    <Show
                      when={products()?.ok}
                      fallback="Products are unavailable for this catalog right now."
                    >
                      No products found in {choice().label}.
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
                      <h2>{catalogField(product, 'name') || catalogField(product, 'id') || 'Product'}</h2>
                      <p>
                        {[
                          catalogField(product, 'retailer_id') && `SKU ${catalogField(product, 'retailer_id')}`,
                          catalogField(product, 'price'),
                          catalogField(product, 'availability'),
                        ]
                          .filter(Boolean)
                          .join(' · ') || 'Product record.'}
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

function providerLabel(providerKey: string): string {
  return platformLabels[providerKey as SocialProviderKey] ?? providerKey ?? 'Provider'
}

function formatMetricValue(value: number): string {
  if (!Number.isFinite(value)) return '0'
  return new Intl.NumberFormat('en-GB', { maximumFractionDigits: 2 }).format(value)
}

function formatSnapshotDate(iso: string): string {
  if (!iso) return 'no snapshot date'
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
