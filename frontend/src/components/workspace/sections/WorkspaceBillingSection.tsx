'use client'

import { Loader2, CreditCard, AlertCircle, CheckCircle2, Clock, Zap, ExternalLink } from 'lucide-react'
import { useCurrentOrganization } from '@/components/core/profile/hooks/useProfile'
import { useOrgBilling, useOrgQuotas } from '@/components/account/hooks/useAccount'
import type { OrgBilling, OrgQuota } from '@/lib/services/org-service'

function PlanBadge({ plan }: { plan: string }) {
  const styles: Record<string, string> = {
    free:       'bg-[#F4F1EB] text-[#4A4A48] border-[#D8D2C6]',
    pro:        'bg-[#EEF2FF] text-[#3730A3] border-[#C7D2FE]',
    enterprise: 'bg-[#FDF4FF] text-[#7E22CE] border-[#E9D5FF]',
  }
  return (
    <span className={`inline-flex items-center rounded-[6px] border px-2.5 py-0.5 font-inter text-[11px] font-semibold uppercase tracking-wide ${styles[plan] ?? styles.free}`}>
      {plan}
    </span>
  )
}

function StatusBadge({ status }: { status: string }) {
  const isGood = status === 'active' || status === 'trialing'
  return (
    <span className={`flex items-center gap-1.5 font-inter text-[12px] ${isGood ? 'text-[#2E7D52]' : 'text-[#C0402A]'}`}>
      {isGood ? <CheckCircle2 size={13} /> : <AlertCircle size={13} />}
      {status.replace(/_/g, ' ')}
    </span>
  )
}

function QuotaBar({ quota }: { quota: OrgQuota }) {
  const pct = quota.quotaLimit > 0 ? Math.min(100, (quota.quotaValue / quota.quotaLimit) * 100) : 0
  const warn = pct >= 80

  return (
    <div className="space-y-1.5">
      <div className="flex items-center justify-between">
        <span className="font-inter text-[12px] capitalize text-[#4A4A48]">
          {quota.quotaKey.replace(/_/g, ' ')}
        </span>
        <span className="font-inter text-[11px] text-[#9B9691]">
          {quota.quotaValue.toLocaleString()} / {quota.quotaLimit.toLocaleString()}
        </span>
      </div>
      <div className="h-1.5 w-full overflow-hidden rounded-full bg-[#E4E1DC]">
        <div
          className={`h-full rounded-full transition-all ${warn ? 'bg-[#D97706]' : 'bg-[#1C1C1A]'}`}
          style={{ width: `${pct}%` }}
        />
      </div>
    </div>
  )
}

export function WorkspaceBillingSection() {
  const { data: org, isLoading: orgLoading } = useCurrentOrganization()
  const { data: billing, isLoading: billingLoading } = useOrgBilling(org?.id)
  const { data: quotas, isLoading: quotasLoading } = useOrgQuotas(org?.id)

  const isLoading = orgLoading || billingLoading || quotasLoading

  const periodEnd = billing?.currentPeriodEnd
    ? new Date(billing.currentPeriodEnd).toLocaleDateString('en-GB', {
        year: 'numeric', month: 'long', day: 'numeric',
      })
    : null

  return (
    <section id="billing" className="scroll-mt-6">
      <h2 className="mb-1.5 font-inter text-[22px] font-semibold tracking-[-0.02em] text-[#1C1C1A]">
        Billing
      </h2>
      <p className="mb-6 font-inter text-[13px] text-[#9B9691]">
        Plan and usage for{' '}
        <strong className="font-medium text-[#1C1C1A]">{org?.name ?? 'your workspace'}</strong>.
      </p>

      {isLoading ? (
        <div className="flex items-center gap-2 font-inter text-[13px] text-[#9B9691]">
          <Loader2 size={14} className="animate-spin" /> Loading…
        </div>
      ) : (
        <div className="space-y-4">
          {/* Plan card */}
          <div className="rounded-[10px] border border-[#E4E1DC] bg-white p-5">
            <div className="mb-4 flex items-start justify-between gap-4">
              <div>
                <div className="mb-2 flex items-center gap-2">
                  <Zap size={14} className="text-[#9B9691]" />
                  <span className="font-inter text-[11px] uppercase tracking-widest text-[#9B9691]">
                    Current plan
                  </span>
                </div>
                <PlanBadge plan={org?.plan ?? 'free'} />
              </div>
              {billing && <StatusBadge status={billing.subscriptionStatus} />}
            </div>

            {periodEnd && (
              <div className="flex items-center gap-1.5 font-inter text-[12px] text-[#9B9691]">
                <Clock size={12} />
                <span>Current period ends {periodEnd}</span>
              </div>
            )}

            {billing?.billingEmail && (
              <p className="mt-1.5 font-inter text-[12px] text-[#9B9691]">
                <CreditCard size={12} className="mr-1.5 inline" />
                Billing email: {billing.billingEmail}
              </p>
            )}

            {/* Upgrade CTA for free plans */}
            {(!org?.plan || org.plan === 'free') && (
              <div className="mt-4 border-t border-[#F4F1EB] pt-4">
                <button className="flex items-center gap-1.5 font-inter text-[12px] font-medium text-[#1C1C1A] underline underline-offset-2 transition-colors hover:text-[#383530]">
                  Upgrade to Pro
                  <ExternalLink size={11} />
                </button>
              </div>
            )}
          </div>

          {/* Usage quotas */}
          {quotas && quotas.length > 0 && (
            <div className="rounded-[10px] border border-[#E4E1DC] bg-white p-5">
              <h3 className="mb-4 font-inter text-[12px] font-medium uppercase tracking-[0.08em] text-[#9B9691]">
                Usage quotas
              </h3>
              <div className="space-y-4">
                {quotas.map((q: OrgQuota) => (
                  <QuotaBar key={q.quotaKey} quota={q} />
                ))}
              </div>
            </div>
          )}

          {!org && (
            <div className="rounded-xl border border-dashed border-[#D8D2C6] px-4 py-6 text-center">
              <p className="font-inter text-[13px] text-[#9B9691]">
                No workspace found.
              </p>
            </div>
          )}
        </div>
      )}
    </section>
  )
}
