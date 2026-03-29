'use client'

import { useState } from 'react'
import { DashboardTransition } from '@/components/onboarding/page'

export function CompleteStep() {
  const [transitioning, setTransitioning] = useState(false)

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
            { label: 'Invitasjoner sendt', desc: 'Teammedlemmene dine vil motta invitasjoner snart' },
          ].map((item, i) => (
            <div key={i} className="flex items-start gap-4">
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
              'Utforsk dashbordet og tilgjengelige funksjoner',
              'Konfigurer organisasjonsinnstillingene',
              'Inviter flere teammedlemmer etter behov',
              'Se gjennom dokumentasjon og veiledninger',
            ].map((item, i) => (
              <li key={i} className="flex items-start gap-2 font-inter text-[12px] text-[#A09890]">
                <span className="mt-0.5 text-[#D8D2C6]">—</span>
                {item}
              </li>
            ))}
          </ul>
        </div>

        <button
          onClick={() => setTransitioning(true)}
          className="w-full bg-[#111111] py-3.5 font-inter text-[11px] uppercase tracking-widest text-white transition-opacity hover:opacity-80"
        >
          Gå til dashbord
        </button>
      </div>

      {transitioning && <DashboardTransition to="/" />}
    </>
  )
}
