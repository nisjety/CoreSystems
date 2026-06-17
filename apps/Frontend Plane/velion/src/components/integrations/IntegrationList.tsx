'use client'

import {
  Building2,
  CheckCircle2,
  Chrome,
  ExternalLink,
  Loader2,
  MessageSquareText,
  PlugZap,
  RefreshCw,
} from 'lucide-react'
import Link from 'next/link'

import { formatIntegrationSourceLabel } from '@/lib/integrations/catalog'
import type { IntegrationProviderSummary } from '@/lib/integrations/types'

type IntegrationListMode = 'catalog' | 'connected'

interface IntegrationListProps {
  providers: IntegrationProviderSummary[]
  mode?: IntegrationListMode
  onConnect?: (provider: IntegrationProviderSummary) => void
  pendingProvider?: string | null
  actionHref?: string
}

function ProviderIcon({ providerKey }: { providerKey: string }) {
  if (providerKey === 'microsoft') {
    return <Building2 size={15} strokeWidth={1.6} className="text-[#0078D4]" />
  }

  if (providerKey === 'google') {
    return <Chrome size={15} strokeWidth={1.6} className="text-[#EA4335]" />
  }

  if (providerKey === 'slack') {
    return <MessageSquareText size={15} strokeWidth={1.6} className="text-[#611F69]" />
  }

  return <PlugZap size={15} strokeWidth={1.6} className="text-[#7A7F8A]" />
}

function ConnectionStatus({ provider }: { provider: IntegrationProviderSummary }) {
  if (!provider.connection) {
    return (
      <span className="inline-flex items-center gap-1.5 rounded-full border border-[#E5E7EE] bg-[#FAFBFD] px-3 py-1 text-[11px] font-semibold uppercase tracking-[0.16em] text-[#8D93A1]">
        Ready
      </span>
    )
  }

  if (provider.connection.syncStatus && provider.connection.syncStatus !== 'idle') {
    return (
      <span className="inline-flex items-center gap-1.5 rounded-full border border-[#E7E0CF] bg-[#FCF6E8] px-3 py-1 text-[11px] font-semibold uppercase tracking-[0.16em] text-[#A27B31]">
        <RefreshCw size={11} strokeWidth={1.6} />
        {provider.connection.syncStatus}
      </span>
    )
  }

  return (
    <span className="inline-flex items-center gap-1.5 rounded-full border border-[#D8EAD9] bg-[#F3FBF5] px-3 py-1 text-[11px] font-semibold uppercase tracking-[0.16em] text-[#3F8A5A]">
      <CheckCircle2 size={11} strokeWidth={1.6} />
      Connected
    </span>
  )
}

export function IntegrationList({
  providers,
  mode = 'catalog',
  onConnect,
  pendingProvider,
  actionHref = '/settings/integrations',
}: IntegrationListProps) {
  const items = mode === 'connected'
    ? providers.filter((provider) => provider.connected)
    : providers

  if (items.length === 0) {
    return (
      <div className="rounded-[24px] border border-dashed border-[#D8DCE5] bg-[#FAFBFD] px-6 py-12 text-center">
        <p className="text-[22px] font-semibold tracking-[-0.03em] text-[#2F3138]">
          No workspace services connected yet
        </p>
        <p className="mt-2 text-[14px] leading-7 text-[#707480]">
          Connect Microsoft 365, Google Workspace, or Slack to route operational context into Velion.
        </p>
        <Link
          href={actionHref}
          className="mt-5 inline-flex items-center gap-2 rounded-full border border-[#E6E8EF] bg-white px-4 py-2 text-[12px] font-semibold uppercase tracking-[0.16em] text-[#525866] transition-colors hover:border-[#2F3138] hover:text-[#2F3138]"
        >
          Open catalog
          <ExternalLink size={12} strokeWidth={1.8} />
        </Link>
      </div>
    )
  }

  return (
    <div className="grid gap-4 md:grid-cols-2">
      {items.map((provider) => {
        const isConnecting = pendingProvider === provider.key
        const connection = provider.connection

        return (
          <article
            key={provider.key}
            className="rounded-[24px] border border-[#E6E8EF] bg-white p-5 shadow-[0_14px_36px_rgba(32,36,48,0.06)]"
          >
            <div className="flex items-start justify-between gap-4">
              <div className="flex items-start gap-3">
                <div className="flex h-11 w-11 shrink-0 items-center justify-center rounded-2xl bg-[#F5F7FB]">
                  <ProviderIcon providerKey={provider.key} />
                </div>
                <div>
                  <p className="text-[18px] font-semibold tracking-[-0.03em] text-[#2F3138]">
                    {provider.label}
                  </p>
                  <p className="mt-1 text-[13px] leading-6 text-[#6D7280]">
                    {provider.description}
                  </p>
                </div>
              </div>

              <ConnectionStatus provider={provider} />
            </div>

            <div className="mt-4 flex flex-wrap gap-2">
              {(connection?.selectedSources.length
                ? connection.selectedSources
                : provider.supportedSources
              ).map((source) => (
                <span
                  key={`${provider.key}-${source}`}
                  className="rounded-full border border-[#E6E8EF] bg-[#FAFBFD] px-3 py-1 text-[11px] font-medium text-[#6D7280]"
                >
                  {formatIntegrationSourceLabel(source)}
                </span>
              ))}
            </div>

            {connection ? (
              <div className="mt-4 space-y-1 text-[12px] leading-6 text-[#7B808C]">
                <p>
                  Status: <span className="font-medium text-[#3E4450]">{connection.status}</span>
                </p>
                <p>
                  Last sync:{' '}
                  <span className="font-medium text-[#3E4450]">
                    {connection.lastSyncedAt
                      ? new Date(connection.lastSyncedAt).toLocaleString('nb-NO')
                      : 'Not synced yet'}
                  </span>
                </p>
                {connection.syncError ? (
                  <p className="text-[#B25555]">{connection.syncError}</p>
                ) : null}
              </div>
            ) : null}

            {mode === 'catalog' ? (
              <div className="mt-5 flex items-center justify-end">
                {connection ? (
                  <Link
                    href="/knowledge/api-integrations"
                    className="inline-flex items-center gap-2 rounded-full border border-[#E6E8EF] bg-[#FAFBFD] px-4 py-2 text-[12px] font-semibold uppercase tracking-[0.16em] text-[#5D6370] transition-colors hover:border-[#2F3138] hover:text-[#2F3138]"
                  >
                    View in knowledge
                    <ExternalLink size={12} strokeWidth={1.8} />
                  </Link>
                ) : (
                  <button
                    type="button"
                    onClick={() => onConnect?.(provider)}
                    disabled={isConnecting}
                    className="inline-flex items-center gap-2 rounded-full border border-[#2F3138] bg-[#2F3138] px-4 py-2 text-[12px] font-semibold uppercase tracking-[0.16em] text-white transition-colors hover:bg-[#1F2128] disabled:cursor-wait disabled:border-[#A0A6B2] disabled:bg-[#A0A6B2]"
                  >
                    {isConnecting ? (
                      <>
                        <Loader2 size={12} strokeWidth={1.8} className="animate-spin" />
                        Connecting
                      </>
                    ) : (
                      'Connect'
                    )}
                  </button>
                )}
              </div>
            ) : null}
          </article>
        )
      })}
    </div>
  )
}
