'use client'

import Link from 'next/link'
import type { ReactNode } from 'react'
import { Loader2, Link2, CheckCircle2, Github, Globe, ExternalLink } from 'lucide-react'
import { accountService } from '../services/account-service'
import { useQuery } from '@tanstack/react-query'
import type { ProviderAccount } from '@/components/core/profile/types'
import { AccountSection, InlineNotice } from './AccountFormPrimitives'

const PROVIDER_META: Record<
  string,
  { label: string; icon: ReactNode }
> = {
  email:     { label: 'Email / Password', icon: <span className="text-[11px] font-bold text-[#6B7280]">@</span> },
  microsoft: { label: 'Microsoft', icon: <span className="text-[11px] font-bold text-[#00A4EF]">M</span> },
  google:    { label: 'Google', icon: <Globe size={13} className="text-[#4285F4]" /> },
  github:    { label: 'GitHub', icon: <Github size={13} /> },
  vipps:     { label: 'Vipps', icon: <span className="text-[11px] font-bold text-[#FF5B24]">V</span> },
  apple:     { label: 'Apple', icon: <span className="text-[11px] font-bold">A</span> },
  okta:      { label: 'Okta', icon: <span className="text-[11px] font-bold text-[#007DC1]">O</span> },
}

function ProviderRow({ provider }: { provider: ProviderAccount }) {
  const meta = PROVIDER_META[provider.provider] ?? {
    label: provider.provider,
    icon: <Link2 size={13} />,
  }

  const linkedAt = provider.linkedAt
    ? new Date(provider.linkedAt).toLocaleDateString('en-GB', {
        year: 'numeric', month: 'short', day: 'numeric',
      })
    : null

  return (
    <li className="flex flex-col gap-4 rounded-[24px] border border-[#ECE6DA] bg-[linear-gradient(180deg,#FFFFFF_0%,#FBF9F5_100%)] px-5 py-5 shadow-[0_14px_36px_rgba(28,33,45,0.05)] sm:flex-row sm:items-center sm:justify-between">
      <div className="flex items-center gap-3 min-w-0">
        <div className="flex h-11 w-11 shrink-0 items-center justify-center rounded-full border border-[#E4E1DC] bg-white shadow-[0_8px_18px_rgba(28,33,45,0.04)]">
          {meta.icon}
        </div>
        <div className="min-w-0">
          <p className="text-sm font-medium text-black">{meta.label}</p>
          {provider.email && (
            <p className="truncate text-sm text-black/54">{provider.email}</p>
          )}
        </div>
      </div>

      <div className="flex items-center gap-2 shrink-0 rounded-full border border-[#DCEBDC] bg-[#F6FBF6] px-3 py-2">
        <CheckCircle2 size={14} className="text-[#2E7D52]" />
        <span className="text-sm font-medium text-[#4E725B]">
          {linkedAt ? `Connected ${linkedAt}` : 'Connected'}
        </span>
      </div>
    </li>
  )
}

export function LinkedAccountsSection() {
  const { data: providers, isLoading } = useQuery({
    queryKey: ['account', 'linked-providers'],
    queryFn: () => accountService.getLinkedProviders(),
    staleTime: 10 * 60 * 1000,
  })

  return (
    <AccountSection
      id="linked-accounts"
      title="Sign-in methods"
      description=""
    >
      {isLoading ? (
        <div className="flex items-center gap-2 text-sm text-black/54">
          <Loader2 size={14} className="animate-spin" /> Loading…
        </div>
      ) : providers && providers.length > 0 ? (
        <ul className="space-y-3">
          {providers.map((p: ProviderAccount) => (
            <ProviderRow key={p.id} provider={p} />
          ))}
        </ul>
      ) : (
        <InlineNotice>No sign-in providers linked to this account yet.</InlineNotice>
      )}

      <div className="mt-6 flex flex-wrap items-center gap-3">
        <Link
          href="/settings/integrations"
          className="inline-flex items-center justify-center gap-2 rounded-full border border-[#E4E1DC] bg-white px-5 py-3 text-sm font-medium text-black/70 transition hover:border-[#d4d1c7] hover:text-[#171717]"
        >
          Manage integrations
        </Link>
        <div className="flex items-center gap-1.5 text-sm text-black/45">
          <ExternalLink size={12} />
          Connected tools for sync and knowledge routing live in workspace settings.
        </div>
      </div>
    </AccountSection>
  )
}
