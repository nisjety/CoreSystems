/**
 * Phase A · A3 — `/settings/api-keys` page (stub UI).
 *
 * Surfaces the future API-key management surface so the menu wires up
 * even before auth-core's `/api/auth/api-keys` endpoint lands. The page
 * deliberately renders the empty state + the architectural note rather
 * than a fake list — users see exactly where the feature lives once
 * the backend ships.
 *
 * Follow-up (tracked in `docs/phase-a-implementation-plan.md` A3.4):
 *   - auth-core: `POST/GET/DELETE /api/auth/api-keys` (Better Auth has
 *     a plugin for this; the wire-up is the work).
 *   - verevon: replace this stub with a list/create/revoke UI.
 */

'use client'

import { type JSX } from 'react'

export default function ApiKeysPage(): JSX.Element {
  return (
    <div className="bg-[#F4EFE5] min-h-full px-8 py-10">
      <div className="mx-auto max-w-3xl space-y-6">
        <header>
          <h1
            className="text-[28px] text-[#2B2B2B]"
            style={{ fontFamily: 'var(--font-cormorant-garamond), Georgia, serif' }}
          >
            API-nøkler
          </h1>
          <p className="mt-2 font-inter text-[13px] text-[#A09890]">
            Generer servernøkler for å kalle dine agenter, kunnskap og verktøy
            programatisk.
          </p>
        </header>

        <section className="rounded-lg border border-[#E9EBF2] bg-white px-6 py-8 text-center">
          <h2 className="font-inter text-[14px] font-semibold text-[#2B2B2B]">
            Kommer snart
          </h2>
          <p className="mt-2 font-inter text-[12px] text-[#A09890]">
            Vi avslutter integrasjonen med auth-core (Better Auth) sin
            API-nøkkelplugin. Når den er klar kan du opprette nøkler herfra,
            begrense dem til spesifikke agenter eller datakilder, og rotere
            dem på sekunder.
          </p>
          <button
            type="button"
            disabled
            className="mt-5 cursor-not-allowed rounded-md bg-[#111111] px-4 py-2 font-inter text-[11px] uppercase tracking-widest text-white opacity-50"
          >
            Opprett nøkkel
          </button>
        </section>

        <section className="rounded-lg border border-[#E9EBF2] bg-white px-6 py-5 font-inter text-[12px] text-[#A09890]">
          <h3 className="font-inter text-[12px] font-semibold text-[#2B2B2B]">
            Hva venter du på?
          </h3>
          <ul className="mt-2 space-y-1.5">
            <li>— Backend: <code>POST /api/auth/api-keys</code> i auth-core.</li>
            <li>— UI: list / opprett / roter / tilbakekall, scoped per nøkkel.</li>
            <li>— Audit: hver nøkkel-bruk logges automatisk i Revisjonslogg.</li>
          </ul>
        </section>
      </div>
    </div>
  )
}
