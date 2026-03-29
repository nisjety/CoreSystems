'use client'

import { Loader2, Link2, CheckCircle2, Github, Globe, ExternalLink } from 'lucide-react'
import { accountService } from '../services/account-service'
import { useQuery } from '@tanstack/react-query'
import type { ProviderAccount } from '@/components/core/profile/types'

const PROVIDER_META: Record<
  string,
  { label: string; color: string; icon: React.ReactNode }
> = {
  email:     { label: 'Email / Password', color: '#6B7280', icon: <span className="text-[11px] font-bold text-[#6B7280]">@</span> },
  microsoft: { label: 'Microsoft',        color: '#00A4EF', icon: <span className="text-[11px] font-bold text-[#00A4EF]">M</span> },
  google:    { label: 'Google',           color: '#4285F4', icon: <Globe size={13} className="text-[#4285F4]" /> },
  github:    { label: 'GitHub',           color: '#24292F', icon: <Github size={13} /> },
  vipps:     { label: 'Vipps',            color: '#FF5B24', icon: <span className="text-[11px] font-bold text-[#FF5B24]">V</span> },
  apple:     { label: 'Apple',            color: '#000000', icon: <span className="text-[11px] font-bold">A</span> },
  okta:      { label: 'Okta',             color: '#007DC1', icon: <span className="text-[11px] font-bold text-[#007DC1]">O</span> },
}

function ProviderRow({ provider }: { provider: ProviderAccount }) {
  const meta = PROVIDER_META[provider.provider] ?? {
    label: provider.provider,
    color: '#9B9691',
    icon: <Link2 size={13} />,
  }

  const linkedAt = provider.linkedAt
    ? new Date(provider.linkedAt).toLocaleDateString('en-GB', {
        year: 'numeric', month: 'short', day: 'numeric',
      })
    : null

  return (
    <li className="flex items-center justify-between gap-3 rounded-xl border border-[#E4E1DC] bg-white px-4 py-3.5">
      {/* Provider icon + name */}
      <div className="flex items-center gap-3 min-w-0">
        <div className="flex h-7 w-7 shrink-0 items-center justify-center rounded-full border border-[#E4E1DC] bg-[#F8F7F4]">
          {meta.icon}
        </div>
        <div className="min-w-0">
          <p className="font-inter text-[13px] font-medium text-[#1C1C1A]">{meta.label}</p>
          {provider.email && (
            <p className="font-inter text-[11px] text-[#9B9691] truncate">{provider.email}</p>
          )}
        </div>
      </div>

      {/* Status + date */}
      <div className="flex items-center gap-2 shrink-0">
        <CheckCircle2 size={13} className="text-[#2E7D52]" />
        <span className="font-inter text-[11px] text-[#9B9691]">
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
    <section id="linked-accounts" className="scroll-mt-6">
      <h2 className="mb-1.5 font-inter text-[22px] font-semibold tracking-[-0.02em] text-[#1C1C1A]">
        Linked accounts
      </h2>
      <p className="mb-6 font-inter text-[13px] text-[#9B9691]">
        OAuth providers currently connected to your account.
      </p>

      {isLoading ? (
        <div className="flex items-center gap-2 text-[#9B9691] font-inter text-[13px]">
          <Loader2 size={14} className="animate-spin" /> Loading…
        </div>
      ) : providers && providers.length > 0 ? (
        <ul className="space-y-2">
          {providers.map((p: ProviderAccount) => (
            <ProviderRow key={p.id} provider={p} />
          ))}
        </ul>
      ) : (
        <div className="rounded-xl border border-dashed border-[#D8D2C6] px-4 py-6 text-center">
          <p className="font-inter text-[13px] text-[#9B9691]">
            No linked accounts detected.
          </p>
        </div>
      )}

      <div className="mt-4 flex items-center gap-1.5">
        <ExternalLink size={12} className="text-[#9B9691]" />
        <p className="font-inter text-[11px] text-[#9B9691]">
          Manage OAuth connections from the sign-in page.
        </p>
      </div>
    </section>
  )
}
