'use client'

import { useRef, useState } from 'react'
import Link from 'next/link'
import { useSearchParams } from 'next/navigation'
import {
  ArrowRight,
  Building2,
  Check,
  ChevronRight,
  Chrome,
  ExternalLink,
  Grid2X2,
  LayoutGrid,
  List,
  Loader2,
  MessageSquareText,
} from 'lucide-react'

import { useIntegrationConnect } from '@/components/integrations/useIntegrationConnect'
import { McpServersPanel } from '@/components/integrations/McpServersPanel'
import { useKnowledgeIntegrations } from '@/components/knowledge/hooks/useKnowledgeData'
import { formatIntegrationSourceLabel } from '@/lib/integrations/catalog'
import type { IntegrationProviderSummary } from '@/lib/integrations/types'

type FilterKey = 'all' | 'finance' | 'communication' | 'documents' | 'storage'
type ViewMode = 'grid' | 'list'

const FILTERS: Array<{ key: FilterKey; label: string }> = [
  { key: 'all', label: 'All Integrations' },
  { key: 'communication', label: 'Communications' },
  { key: 'documents', label: 'Documents' },
  { key: 'storage', label: 'Storage' },
  { key: 'finance', label: 'Finance' },
]

function getCategoryCount(providers: IntegrationProviderSummary[], key: FilterKey) {
  if (key === 'all') {
    return providers.length
  }

  return providers.filter((provider) => provider.categories?.includes(key)).length
}

function filterProviders(providers: IntegrationProviderSummary[], key: FilterKey) {
  if (key === 'all') {
    return providers
  }

  return providers.filter((provider) => provider.categories?.includes(key))
}

function normalizeProviderKey(value: string | null) {
  const provider = (value ?? '').trim().toLowerCase()

  switch (provider) {
    case 'gdrive':
    case 'google_drive':
    case 'drive':
      return 'google-drive'
    case 'm365':
    case 'microsoft365':
    case 'microsoft-365':
    case 'teams':
    case 'sharepoint':
    case 'onedrive':
    case 'outlook':
      return 'microsoft'
    default:
      return provider
  }
}

function formatExecutionLabel(value?: string | null) {
  switch (value) {
    case 'first_party':
      return 'Verevon-native'
    case 'connector_runtime':
      return 'Runtime bridge'
    case 'unavailable':
      return 'Not ready'
    default:
      return 'Pending'
  }
}

function ProviderGlyph({ providerKey }: { providerKey: string }) {
  if (providerKey === 'microsoft') {
    return (
      <div className="flex h-11 w-11 items-center justify-center rounded-full bg-[#F1F4FB] text-[#2F5BFF]">
        <Building2 size={20} strokeWidth={2} />
      </div>
    )
  }

  if (providerKey === 'google') {
    return (
      <div className="flex h-11 w-11 items-center justify-center rounded-full bg-[#F7F4EC] text-[#1C1C1C]">
        <Chrome size={20} strokeWidth={2} />
      </div>
    )
  }

  return (
    <div className="flex h-11 w-11 items-center justify-center rounded-full bg-[#F3F5FB] text-[#1C1C1C]">
      <MessageSquareText size={20} strokeWidth={2} />
    </div>
  )
}

function ConnectionButton({
  provider,
  pendingProvider,
  onConnect,
}: {
  provider: IntegrationProviderSummary
  pendingProvider: string | null
  onConnect: (provider: IntegrationProviderSummary) => void
}) {
  if (provider.connected) {
    return (
      <Link
        href="/knowledge/api-integrations"
        className="inline-flex h-9 items-center gap-2 rounded-full bg-[#1F2A44] px-4 text-[14px] font-medium text-white shadow-[0_10px_20px_rgba(31,42,68,0.20)]"
      >
        Connected
        <Check size={13} strokeWidth={2.2} />
      </Link>
    )
  }

  if (provider.signInLinked && !provider.dataAccessReady) {
    return (
      <button
        type="button"
        onClick={() => onConnect(provider)}
        disabled={pendingProvider === provider.key}
        className="inline-flex h-9 items-center gap-2 rounded-full border border-[#E0DDD6] bg-white px-4 text-[14px] font-medium text-[#1C1C1C] shadow-[0_8px_18px_rgba(0,0,0,0.10)] transition hover:border-[#CAC4B8] hover:shadow-[0_12px_22px_rgba(0,0,0,0.12)]"
      >
        {pendingProvider === provider.key ? (
          <>
            <Loader2 size={14} className="animate-spin" />
            Checking
          </>
        ) : (
          'Grant access'
        )}
      </button>
    )
  }

  if (provider.signInLinked && provider.dataAccessReady) {
    return (
      <button
        type="button"
        onClick={() => onConnect(provider)}
        disabled={pendingProvider === provider.key}
        className="inline-flex h-9 items-center gap-2 rounded-full border border-[#D9E2FF] bg-[#F5F8FF] px-4 text-[14px] font-medium text-[#2F5BFF] shadow-[0_8px_18px_rgba(47,91,255,0.10)] transition hover:border-[#B8C9FF]"
      >
        {pendingProvider === provider.key ? (
          <>
            <Loader2 size={14} className="animate-spin" />
            Connecting
          </>
        ) : (
          'Use sign-in'
        )}
      </button>
    )
  }

  return (
    <button
      type="button"
      onClick={() => onConnect(provider)}
      disabled={pendingProvider === provider.key || provider.configured === false}
      className={[
        'inline-flex h-9 items-center gap-2 rounded-full border border-[#E0DDD6] bg-white px-4 text-[14px] font-medium text-[#1C1C1C] shadow-[0_8px_18px_rgba(0,0,0,0.10)] transition',
        provider.configured === false
          ? 'cursor-not-allowed bg-[#F4F1EA] text-[#A28F66]'
          : 'hover:border-[#CAC4B8] hover:shadow-[0_12px_22px_rgba(0,0,0,0.12)]',
      ].join(' ')}
    >
      {pendingProvider === provider.key ? (
        <>
          <Loader2 size={14} className="animate-spin" />
          Connecting
        </>
      ) : provider.configured === false ? (
        'Setup pending'
      ) : (
        <>
          Connect
          <ExternalLink size={13} strokeWidth={2} />
        </>
      )}
    </button>
  )
}

function IntegrationCard({
  provider,
  pendingProvider,
  onConnect,
  onOpenDetails,
  compact = false,
}: {
  provider: IntegrationProviderSummary
  pendingProvider: string | null
  onConnect: (provider: IntegrationProviderSummary) => void
  onOpenDetails: (providerKey: string) => void
  compact?: boolean
}) {
  const sourceText = (provider.connection?.selectedSources.length
    ? provider.connection.selectedSources
    : provider.supportedSources
  )
    .slice(0, 3)
    .map((source) => formatIntegrationSourceLabel(source))
    .join(', ')

  return (
    <article
      className={[
        'bg-white px-8 py-7',
        compact ? 'flex flex-col gap-5 md:flex-row md:items-center md:justify-between' : '',
      ].join(' ')}
    >
      <div className={compact ? 'flex min-w-0 flex-1 items-start gap-5' : ''}>
        <div className={compact ? 'shrink-0' : ''}>
          <ProviderGlyph providerKey={provider.key} />
        </div>

        <div className={compact ? 'min-w-0 flex-1' : 'mt-6'}>
          <h2 className="text-[18px] font-semibold tracking-[-0.03em] text-[#1C1C1C]">
            {provider.label}
          </h2>
          <p className="mt-2 max-w-[30ch] text-[14px] leading-6 text-[#7A7A74]">
            {provider.description}
          </p>
          {provider.signInLinked && !provider.connected ? (
            <p className="mt-3 text-[12px] leading-5 text-[#7D879C]">
              {provider.dataAccessReady
                ? 'This provider is already linked through sign-in and can be activated for this workspace.'
                : 'This provider is linked for sign-in only. Verevon still needs data-access consent before sync can start.'}
            </p>
          ) : null}
          {!compact ? (
            <p className="mt-4 text-[12px] leading-5 text-[#A09A8E]">
              {sourceText}
            </p>
          ) : null}
        </div>
      </div>

      <div className={compact ? 'flex shrink-0 flex-wrap items-center gap-4 md:pl-6' : 'mt-7 flex flex-wrap items-center gap-4'}>
        <ConnectionButton
          provider={provider}
          pendingProvider={pendingProvider}
          onConnect={onConnect}
        />
        <button
          type="button"
          onClick={() => onOpenDetails(provider.key)}
          className="inline-flex items-center gap-1.5 text-[14px] font-medium text-[#2F3442] transition hover:text-black"
        >
          Integrations details
          <ArrowRight size={14} strokeWidth={2} />
        </button>
      </div>
    </article>
  )
}

function LoadingBoard() {
  return (
    <div className="overflow-hidden rounded-[24px] border border-[#DFDDD7] bg-white shadow-[0_26px_50px_rgba(24,20,12,0.08)]">
      <div className="grid gap-px bg-[#E6E2DA] md:grid-cols-2 xl:grid-cols-3">
        {Array.from({ length: 6 }).map((_, index) => (
          <div key={`skeleton-${index}`} className="bg-white px-8 py-7">
            <div className="animate-pulse">
              <div className="h-11 w-11 rounded-full bg-[#F0EEE8]" />
              <div className="mt-6 h-5 w-40 rounded-full bg-[#F3F1EC]" />
              <div className="mt-3 space-y-2">
                <div className="h-3 w-full rounded-full bg-[#F3F1EC]" />
                <div className="h-3 w-4/5 rounded-full bg-[#F3F1EC]" />
              </div>
              <div className="mt-7 flex gap-4">
                <div className="h-9 w-28 rounded-full bg-[#F3F1EC]" />
                <div className="h-6 w-32 rounded-full bg-[#F7F5F0]" />
              </div>
            </div>
          </div>
        ))}
      </div>
    </div>
  )
}

export function IntegrationsSettingsPage() {
  const searchParams = useSearchParams()
  const integrationsQuery = useKnowledgeIntegrations()
  const { connectProvider, error, pendingProvider, setError, statusMessage } = useIntegrationConnect(
    integrationsQuery.data?.orgId,
  )
  const initialProviderKey = normalizeProviderKey(searchParams.get('connect')) || null
  const [activeFilter, setActiveFilter] = useState<FilterKey>('all')
  const [viewMode, setViewMode] = useState<ViewMode>('grid')
  const [selectedProviderKey, setSelectedProviderKey] = useState<string | null>(initialProviderKey)
  const detailsRef = useRef<HTMLDivElement | null>(null)

  const providers = integrationsQuery.data?.providers ?? []
  const filteredProviders = filterProviders(providers, activeFilter)

  const selectedProvider =
    filteredProviders.find((provider) => provider.key === selectedProviderKey) ??
    providers.find((provider) => provider.key === selectedProviderKey) ??
    null

  const openDetails = (providerKey: string) => {
    setSelectedProviderKey(providerKey)
    requestAnimationFrame(() => {
      detailsRef.current?.scrollIntoView({ behavior: 'smooth', block: 'start' })
    })
  }

  return (
    <div className="flex h-full min-h-0 flex-col overflow-y-auto bg-white px-6 py-10 md:px-8">
      <div className="mx-auto w-full max-w-[1440px]">
        <h1 className="mb-10 text-[22px] font-semibold tracking-tight text-[#111111]">
          Integrations
        </h1>

        <div className="flex flex-col gap-5 md:flex-row md:items-center md:justify-between">
          <div className="flex flex-wrap items-center gap-3 md:gap-6">
            {FILTERS.map((filter) => {
              const active = activeFilter === filter.key
              const count = getCategoryCount(providers, filter.key)

              return (
                <button
                  key={filter.key}
                  type="button"
                  onClick={() => setActiveFilter(filter.key)}
                  className={[
                    'inline-flex items-center gap-2 text-[15px] transition',
                    active
                      ? 'rounded-full bg-[#2F5BFF] px-4 py-2 text-white shadow-[0_10px_22px_rgba(47,91,255,0.22)]'
                      : 'text-[#2B2B27] hover:text-black',
                  ].join(' ')}
                >
                  <span className={active ? 'font-medium' : 'font-normal'}>{filter.label}</span>
                  {active ? (
                    <span className="inline-flex h-6 min-w-6 items-center justify-center rounded-full bg-white px-2 text-[12px] font-semibold text-[#2F5BFF]">
                      {count}
                    </span>
                  ) : null}
                </button>
              )
            })}
          </div>

          <div className="inline-flex items-center gap-1 self-start rounded-full border border-[#DFDDD7] bg-white p-1 shadow-[0_8px_16px_rgba(0,0,0,0.05)]">
            <button
              type="button"
              onClick={() => setViewMode('grid')}
              className={[
                'inline-flex h-10 w-10 items-center justify-center rounded-full transition',
                viewMode === 'grid'
                  ? 'bg-[#2F5BFF] text-white'
                  : 'text-[#85817B] hover:bg-[#F2F0EA]',
              ].join(' ')}
              aria-label="Grid view"
            >
              <Grid2X2 size={15} strokeWidth={2} />
            </button>
            <button
              type="button"
              onClick={() => setViewMode('list')}
              className={[
                'inline-flex h-10 w-10 items-center justify-center rounded-full transition',
                viewMode === 'list'
                  ? 'bg-[#2F5BFF] text-white'
                  : 'text-[#85817B] hover:bg-[#F2F0EA]',
              ].join(' ')}
              aria-label="List view"
            >
              <List size={15} strokeWidth={2} />
            </button>
          </div>
        </div>

        <div className="mt-8 space-y-5">
          {statusMessage ? (
            <div className="rounded-[18px] border border-[#D7E0FF] bg-[#F6F8FF] px-5 py-4 text-[14px] leading-6 text-[#41598C]">
              {statusMessage}
            </div>
          ) : null}

          {error ? (
            <button
              type="button"
              onClick={() => setError(null)}
              className="w-full rounded-[18px] border border-[#F0D1CD] bg-[#FFF7F6] px-5 py-4 text-left text-[14px] leading-6 text-[#985554]"
            >
              {error}
            </button>
          ) : null}

          {integrationsQuery.isLoading ? (
            <LoadingBoard />
          ) : integrationsQuery.error ? (
            <div className="rounded-[24px] border border-[#E4E1DA] bg-white px-8 py-10 shadow-[0_26px_50px_rgba(24,20,12,0.08)]">
              <p className="text-[28px] font-semibold tracking-[-0.04em] text-[#1C1C1C]">
                Integration catalog is unavailable
              </p>
              <p className="mt-3 max-w-[56ch] text-[15px] leading-7 text-[#7D7D76]">
                The Verevon integration engine did not respond. Check the ingestion-plane service and reload the page.
              </p>
            </div>
          ) : filteredProviders.length === 0 ? (
            <div className="rounded-[24px] border border-[#E4E1DA] bg-white px-8 py-10 text-center shadow-[0_26px_50px_rgba(24,20,12,0.08)]">
              <p className="text-[24px] font-semibold tracking-[-0.03em] text-[#1C1C1C]">
                No integrations in this category
              </p>
              <p className="mt-3 text-[15px] leading-7 text-[#7D7D76]">
                This filter currently has no available providers in your workspace catalog.
              </p>
            </div>
          ) : (
            <div className="overflow-hidden rounded-[24px] border border-[#DFDDD7] bg-white shadow-[0_34px_60px_rgba(24,20,12,0.10)]">
              <div
                className={[
                  'bg-[#E6E2DA]',
                  viewMode === 'grid' ? 'grid gap-px md:grid-cols-2 xl:grid-cols-3' : 'grid gap-px',
                ].join(' ')}
              >
                {filteredProviders.map((provider) => (
                  <IntegrationCard
                    key={provider.key}
                    provider={provider}
                    pendingProvider={pendingProvider}
                    onConnect={connectProvider}
                    onOpenDetails={openDetails}
                    compact={viewMode === 'list'}
                  />
                ))}
              </div>
            </div>
          )}
        </div>

        <McpServersPanel />

        {selectedProvider ? (
          <section
            ref={detailsRef}
            className="mt-8 rounded-[24px] border border-[#DFDDD7] bg-white px-7 py-7 shadow-[0_26px_50px_rgba(24,20,12,0.08)]"
          >
            <div className="flex flex-col gap-6 lg:flex-row lg:items-start lg:justify-between">
              <div className="max-w-[620px]">
                <div className="flex items-center gap-4">
                  <ProviderGlyph providerKey={selectedProvider.key} />
                  <div>
                    <h2 className="text-[28px] font-semibold tracking-[-0.04em] text-[#1C1C1C]">
                      {selectedProvider.label}
                    </h2>
                    <p className="mt-1 text-[15px] leading-7 text-[#7D7D76]">
                      {selectedProvider.description}
                    </p>
                  </div>
                </div>

                <div className="mt-6 flex flex-wrap gap-2">
                  {(selectedProvider.connection?.selectedSources.length
                    ? selectedProvider.connection.selectedSources
                    : selectedProvider.defaultSources
                  ).map((source) => (
                    <span
                      key={`${selectedProvider.key}-${source}`}
                      className="rounded-full border border-[#E4E1DA] bg-[#F8F6F1] px-3 py-1 text-[12px] font-medium text-[#5E5B55]"
                    >
                      {formatIntegrationSourceLabel(source)}
                    </span>
                  ))}
                </div>
              </div>

              <div className="max-w-[340px] rounded-[18px] border border-[#E4E1DA] bg-[#FAF8F3] px-5 py-5">
                <p className="text-[12px] font-medium uppercase tracking-[0.18em] text-[#979189]">
                  Workspace state
                </p>
                <p className="mt-3 text-[22px] font-semibold tracking-[-0.03em] text-[#1C1C1C]">
                  {selectedProvider.connected
                    ? 'Connected'
                    : selectedProvider.configured === false
                      ? 'Setup pending'
                      : 'Ready to connect'}
                </p>
                <div className="mt-5 flex flex-wrap gap-3">
                  <ConnectionButton
                    provider={selectedProvider}
                    pendingProvider={pendingProvider}
                    onConnect={connectProvider}
                  />
                  <Link
                    href="/knowledge"
                    className="inline-flex h-9 items-center gap-2 rounded-full border border-[#E0DDD6] bg-white px-4 text-[14px] font-medium text-[#1C1C1C]"
                  >
                    Open Knowledge
                    <ExternalLink size={13} strokeWidth={2} />
                  </Link>
                </div>
              </div>
            </div>

            <div className="mt-6 grid gap-4 lg:grid-cols-3">
              {[
                {
                  title: 'Authorization',
                  value: formatExecutionLabel(selectedProvider.authExecution),
                  copy: 'How Verevon currently executes the account authorization boundary.',
                },
                {
                  title: 'Sync orchestration',
                  value: formatExecutionLabel(selectedProvider.syncExecution),
                  copy: 'Where source sync jobs are orchestrated for this provider.',
                },
                {
                  title: 'Sync status',
                  value: selectedProvider.connection?.syncStatus ?? 'pending',
                  copy: selectedProvider.connection?.lastSyncedAt
                    ? `Last sync ${new Date(selectedProvider.connection.lastSyncedAt).toLocaleString('nb-NO')}`
                    : 'No completed sync has been recorded yet.',
                },
              ].map((item) => (
                <article
                  key={item.title}
                  className="rounded-[18px] border border-[#E4E1DA] bg-[#FAF8F3] px-5 py-5"
                >
                  <p className="text-[12px] font-medium uppercase tracking-[0.18em] text-[#979189]">
                    {item.title}
                  </p>
                  <p className="mt-3 text-[20px] font-semibold tracking-[-0.03em] text-[#1C1C1C]">
                    {item.value}
                  </p>
                  <p className="mt-2 text-[14px] leading-6 text-[#7D7D76]">{item.copy}</p>
                </article>
              ))}
            </div>

            <div className="mt-6 flex justify-end">
              <button
                type="button"
                onClick={() => setSelectedProviderKey(null)}
                className="inline-flex items-center gap-1.5 text-[14px] font-medium text-[#3A3A36] transition hover:text-black"
              >
                Close details
                <ChevronRight size={14} className="rotate-90" strokeWidth={2} />
              </button>
            </div>
          </section>
        ) : null}
      </div>
    </div>
  )
}
