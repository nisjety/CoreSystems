'use client'

import { useCallback, useEffect, useRef, useState } from 'react'
import { useRouter } from 'next/navigation'
import { WorkspaceNav } from './WorkspaceNav'
import { GeneralSection } from './sections/GeneralSection'
import { MembersSection } from './sections/MembersSection'
import { WorkspaceBillingSection } from './sections/WorkspaceBillingSection'
import { WORKSPACE_SECTION_IDS, type WorkspaceSectionId } from './types'

interface WorkspacePageProps {
  initialSection?: WorkspaceSectionId
  basePath?: string
}

export function WorkspacePage({
  initialSection = 'general',
  basePath = '/workspace',
}: WorkspacePageProps) {
  const router = useRouter()
  const [active, setActive] = useState<WorkspaceSectionId>(initialSection)
  const contentRef = useRef<HTMLDivElement>(null)
  const isScrolling = useRef(false)
  const scrollReleaseTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  const buildSectionHref = useCallback(
    (id: WorkspaceSectionId) => (id === 'general' ? basePath : `${basePath}/${id}`),
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
    id: WorkspaceSectionId,
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

  useEffect(() => {
    const el = contentRef.current
    if (!el) return

    const observer = new IntersectionObserver(
      entries => {
        if (isScrolling.current) return
        for (const entry of entries) {
          if (entry.isIntersecting) {
            setActive(entry.target.id as WorkspaceSectionId)
          }
        }
      },
      { root: el, threshold: 0.3 },
    )

    WORKSPACE_SECTION_IDS.forEach(id => {
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
            Workspace settings
          </h1>
        </div>
        <div className="flex-1 overflow-y-auto px-3 py-4">
          <WorkspaceNav active={active} onSelect={scrollTo} />
        </div>
      </aside>

      {/* ── Scrollable content ───────────────────────────────────────── */}
      <div ref={contentRef} className="flex-1 overflow-y-auto">
        {/* Mobile heading */}
        <div className="border-b border-[#E4E1DC] bg-white px-6 py-5 md:hidden">
          <h1 className="font-inter text-[18px] font-semibold text-[#1C1C1A]">
            Workspace settings
          </h1>
        </div>

        {/* Desktop heading */}
        <div className="hidden border-b border-[#E4E1DC] bg-white px-8 py-6 md:block">
          <h1 className="font-inter text-[28px] font-semibold tracking-[-0.03em] text-[#1C1C1A]">
            Workspace settings
          </h1>
          <p className="mt-1 font-inter text-[13px] text-[#9B9691]">
            Manage your workspace name, members, and billing.
          </p>
        </div>

        <div className="px-6 py-8 md:px-8">
          <div className="mx-auto max-w-[640px] space-y-12">
            <GeneralSection />
            <hr className="border-[#E4E1DC]" />
            <MembersSection />
            <hr className="border-[#E4E1DC]" />
            <WorkspaceBillingSection />
          </div>
        </div>
      </div>
    </div>
  )
}
