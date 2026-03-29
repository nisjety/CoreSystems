'use client'

import Link from 'next/link'
import { ArrowRight } from 'lucide-react'
import type { DashboardStats } from '@/app/api/dashboard/stats/route'

interface Props {
  stats: DashboardStats | null
  userName?: string
}

export function TeamActivityCard({ stats, userName }: Props) {
  const members = stats?.memberCount
  const hasTeam = members != null && members > 1

  return (
    <Link
      href="/team"
      className="group flex flex-col border border-[#D8D2C6] bg-white transition-colors hover:bg-[#F4F1EB]"
    >
      {/* Header */}
      <div className="flex items-center justify-between border-b border-[#D8D2C6] px-5 py-3">
        <span className="font-inter text-[10px] uppercase tracking-widest text-[#A09890]">
          Team
        </span>
        <span className="h-1.5 w-1.5 rounded-full bg-[#111111]" />
      </div>

      {/* Body */}
      <div className="flex flex-1 flex-col justify-between px-5 py-5">
        {hasTeam ? (
          <div>
            <p
              className="text-[42px] font-normal leading-none text-[#2B2B2B]"
              style={{ fontFamily: 'var(--font-cormorant-garamond), Georgia, serif' }}
            >
              {members!.toLocaleString('nb-NO')}
            </p>
            <p className="mt-1.5 font-inter text-[12px] text-[#A09890]">
              {members === 1 ? 'teammedlem' : 'teammedlemmer'}
            </p>
          </div>
        ) : (
          <div>
            <p
              className="text-[22px] font-normal leading-snug text-[#2B2B2B]"
              style={{ fontFamily: 'var(--font-cormorant-garamond), Georgia, serif' }}
            >
              {userName ? `Bare du foreløpig` : 'Kun deg foreløpig'}
            </p>
            <p className="mt-1.5 font-inter text-[12px] text-[#A09890]">
              Inviter kollegaer til teamet
            </p>
          </div>
        )}
      </div>

      {/* Footer */}
      <div className="flex items-center justify-between border-t border-[#D8D2C6] px-5 py-3">
        <span className="font-inter text-[10px] uppercase tracking-widest text-[#A09890]">
          Inviter
        </span>
        <ArrowRight
          size={13}
          className="text-[#C8C1B3] transition-transform group-hover:translate-x-1 group-hover:text-[#2B2B2B]"
        />
      </div>
    </Link>
  )
}
