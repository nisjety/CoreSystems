'use client'

import { useCallback, useEffect, useRef, useState } from 'react'
import { useRouter } from 'next/navigation'
import { AccountNav } from './AccountNav'
import { ProfileSection } from './sections/ProfileSection'
import { SecuritySection } from './sections/SecuritySection'
import { LinkedAccountsSection } from './sections/LinkedAccountsSection'
import { NotificationsSection } from './sections/NotificationsSection'
import { DangerZoneSection } from './sections/DangerZoneSection'
import { ACCOUNT_NAV_SECTION_IDS, type AccountNavSectionId } from './types'

interface AccountPageProps {
  initialSection?: AccountNavSectionId
  basePath?: string
}

export function AccountPage({
  initialSection = 'profile',
  basePath = '/profile',
}: AccountPageProps) {
  const router = useRouter()
  const [active, setActive] = useState<AccountNavSectionId>(initialSection)
  const contentRef = useRef<HTMLDivElement>(null)
  const isScrolling = useRef(false)
  const scrollReleaseTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  const buildSectionHref = useCallback(
    (id: AccountNavSectionId) => (id === 'profile' ? basePath : `${basePath}/${id}`),
    [basePath],
  )

  const releaseScrollLock = useCallback(() => {
    if (scrollReleaseTimeoutRef.current) {
      clearTimeout(scrollReleaseTimeoutRef.current)
    }

    scrollReleaseTimeoutRef.current = setTimeout(() => {
      isScrolling.current = false
    }, 800)
  }, [])

  const scrollTo = useCallback((
    id: AccountNavSectionId,
    {
      behavior = 'smooth',
      updateRoute = true,
    }: {
      behavior?: ScrollBehavior
      updateRoute?: boolean
    } = {},
  ) => {
    const section = contentRef.current?.querySelector<HTMLElement>(`#${id}`)
    if (!section) return

    setActive(id)
    isScrolling.current = true

    if (updateRoute) {
      router.push(buildSectionHref(id), { scroll: false })
    }

    section.scrollIntoView({ behavior, block: 'start' })
    releaseScrollLock()
  }, [buildSectionHref, releaseScrollLock, router])

  // Scrollspy — update active nav item as user scrolls
  useEffect(() => {
    const el = contentRef.current
    if (!el) return

    const observer = new IntersectionObserver(
      entries => {
        if (isScrolling.current) return
        for (const entry of entries) {
          if (entry.isIntersecting) {
            setActive(entry.target.id as AccountNavSectionId)
          }
        }
      },
      { root: el, threshold: 0.3 },
    )

    ACCOUNT_NAV_SECTION_IDS.forEach(id => {
      const section = el.querySelector(`#${id}`)
      if (section) observer.observe(section)
    })

    return () => observer.disconnect()
  }, [])

  useEffect(() => {
    const frame = requestAnimationFrame(() => {
      scrollTo(initialSection, { behavior: 'auto', updateRoute: false })
    })

    return () => cancelAnimationFrame(frame)
  }, [initialSection, scrollTo])

  useEffect(() => {
    return () => {
      if (scrollReleaseTimeoutRef.current) {
        clearTimeout(scrollReleaseTimeoutRef.current)
      }
    }
  }, [])

  return (
    <div className="flex h-full min-h-0 overflow-hidden bg-[#F8F7F4]">
      {/* ── Left sticky nav ─────────────────────────────────────────── */}
      <aside className="hidden w-52 shrink-0 border-r border-[#E4E1DC] bg-white md:flex md:flex-col">
        <div className="border-b border-[#E4E1DC] px-5 py-5">
          <h1 className="font-inter text-[13px] font-semibold tracking-[-0.01em] text-[#1C1C1A]">
            Account settings
          </h1>
        </div>
        <div className="flex-1 overflow-y-auto px-3 py-4">
          <AccountNav active={active} onSelect={scrollTo} />
        </div>
      </aside>

      {/* ── Scrollable content ───────────────────────────────────────── */}
      <div
        ref={contentRef}
        className="flex-1 overflow-y-auto"
      >
        {/* Mobile page heading */}
        <div className="border-b border-[#E4E1DC] bg-white px-6 py-5 md:hidden">
          <h1 className="font-inter text-[18px] font-semibold text-[#1C1C1A]">
            Account settings
          </h1>
        </div>

        {/* Desktop page heading */}
        <div className="hidden border-b border-[#E4E1DC] bg-white px-8 py-6 md:block">
          <h1 className="font-inter text-[28px] font-semibold tracking-[-0.03em] text-[#1C1C1A]">
            Account settings
          </h1>
          <p className="mt-0.5 font-inter text-[13px] text-[#9B9691]">
            Manage your profile, security, and notification preferences.
          </p>
        </div>

        <div className="mx-auto max-w-2xl space-y-14 px-6 py-10 md:px-8 md:py-12">
          <ProfileSection />
          <div className="h-px bg-[#E4E1DC]" />
          <SecuritySection />
          <div className="h-px bg-[#E4E1DC]" />
          <LinkedAccountsSection />
          <div className="h-px bg-[#E4E1DC]" />
          <NotificationsSection />
          <div className="h-px bg-[#F5C9C4]" />
          <DangerZoneSection />
        </div>
      </div>
    </div>
  )
}
