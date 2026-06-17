'use client'

import { useState } from 'react'
import { useRouter } from 'next/navigation'
import { DashboardTransition } from '@/components/onboarding/page'
import { onboardingService } from '@/components/onboarding/services/onboarding-service'

/**
 * Phase A · A2 — final onboarding step.
 *
 * Confirms the four wizard outputs (profile, organisation, website seed,
 * team invites) and directs the user to the natural "first day"
 * destinations:
 *
 *   1. **Create your first agent** (`/agents/create`) — the primary
 *      next action. Without an agent, the user has nothing to embed and
 *      nothing to test the website-seed retrieval against.
 *   2. **Open the dashboard** (`/dashboard`) — secondary action, for
 *      users who want to poke around before committing to an agent.
 *
 * The component is intentionally just navigation glue — the agent itself
 * gets configured on `/agents/create`. This keeps the wizard short and
 * defers the (more involved) agent-shape decisions to a dedicated screen.
 */
export function CompleteStep() {
  const router = useRouter()
  const [transitioning, setTransitioning] = useState(false)
  const [destination, setDestination] = useState<'/agents/create' | '/dashboard'>(
    '/agents/create',
  )

  const handleNavigate = (to: '/agents/create' | '/dashboard') => {
    setDestination(to)
    setTransitioning(true)
  }

  return (
    <>
      <div className="space-y-8">
        <div className="flex justify-center">
          <div className="flex h-14 w-14 items-center justify-center rounded-full bg-[#111111]">
            <svg className="h-7 w-7 text-white" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M5 13l4 4L19 7" />
            </svg>
          </div>
        </div>

        <div className="space-y-4">
          {[
            { label: 'Profil fullført', desc: 'Profilinformasjonen din er lagret' },
            { label: 'Organisasjon opprettet', desc: 'Organisasjonen din er klar for samarbeid' },
            { label: 'Nettsted indeksert', desc: 'Quarry har startet innhentingen av kunnskapen din' },
            { label: 'Invitasjoner sendt', desc: 'Teammedlemmene dine vil motta invitasjoner snart' },
          ].map((item, i) => (
            <div key={item.label} className="flex items-start gap-4">
              <div className="flex h-6 w-6 shrink-0 items-center justify-center rounded-full bg-[#111111] font-inter text-[11px] text-white">
                {i + 1}
              </div>
              <div>
                <h4
                  className="text-[15px] text-[#2B2B2B]"
                  style={{ fontFamily: 'var(--font-cormorant-garamond), Georgia, serif' }}
                >
                  {item.label}
                </h4>
                <p className="font-inter text-[12px] text-[#A09890]">{item.desc}</p>
              </div>
            </div>
          ))}
        </div>

        <div className="border-t border-[#D8D2C6] pt-5 space-y-3">
          <h4
            className="text-[15px] text-[#2B2B2B]"
            style={{ fontFamily: 'var(--font-cormorant-garamond), Georgia, serif' }}
          >
            Hva er neste steg?
          </h4>
          <ul className="space-y-2">
            {[
              'Opprett din første agent og koble den til kunnskapen din',
              'Test agenten i chatten før du legger den ut',
              'Inviter flere teammedlemmer ved behov',
              'Konfigurer organisasjonsinnstillinger og tillatelser',
            ].map((item) => (
              <li key={item} className="flex items-start gap-2 font-inter text-[12px] text-[#A09890]">
                <span className="mt-0.5 text-[#D8D2C6]">—</span>
                {item}
              </li>
            ))}
          </ul>
        </div>

        <button
          type="button"
          data-testid="onboarding-complete-back"
          onClick={async () => { await onboardingService.saveCurrentStep('team'); router.push('/onboarding/team') }}
          className="mb-1 text-xs font-bold tracking-tight text-[#111111] transition-colors hover:text-[#FF2E63]"
        >
          ← TILBAKE
        </button>
        <button
          data-testid="onboarding-complete-create-agent"
          onClick={() => handleNavigate('/agents/create')}
          className="w-full bg-[#111111] py-3.5 font-inter text-[11px] uppercase tracking-widest text-white transition-opacity hover:opacity-80"
        >
          Opprett første agent
        </button>
        <button
          type="button"
          data-testid="onboarding-complete-dashboard"
          onClick={() => handleNavigate('/dashboard')}
          className="w-full border border-[#D8D2C6] bg-transparent py-3 font-inter text-[11px] uppercase tracking-widest text-[#2B2B2B] transition-colors hover:bg-[#F4EFE5]"
        >
          Hopp over — gå til dashbord
        </button>
      </div>

      {transitioning && <DashboardTransition to={destination} />}
    </>
  )
}
