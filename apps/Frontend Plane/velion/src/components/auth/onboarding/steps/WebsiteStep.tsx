'use client'

/**
 * Step 3 — website seed (Chatbase pattern).
 *
 * Left pane: URL field + "describe what you want the agent to do"
 * textarea. Submit kicks off a Quarry crawl (via the existing
 * `/api/ingestion/crawl` proxy) and advances when the first batch of
 * snippets returns.
 *
 * Right pane: live snippet-drop folder. Hybrid of the Taskello card +
 * the 4-Files folder mock — snippets fall from the top into the
 * folder as Quarry indexes them. While the real `/api/onboarding/
 * crawl-preview` route lands the panel cycles through a small set of
 * placeholder cards so the animation still reads.
 */

import React, { useEffect, useMemo, useState } from 'react'

import type { OnboardingMachine } from '../state/useOnboardingMachine'

import {
  LeftPane,
  PrimaryButton,
  RightPane,
  SkipLink,
  StepDescription,
  StepEyebrow,
  StepTitle,
} from './_shared'

interface SnippetPreview {
  id: string
  title: string
  excerpt: string
}

// While the real crawl preview API lands, cycle through these so the
// animation has motion. The shape matches what the real endpoint will
// return so swapping it in later is mechanical.
const PLACEHOLDER_SNIPPETS: SnippetPreview[] = [
  { id: 'p1', title: '/about', excerpt: 'Vi bygger en plattform for kunde­service.' },
  { id: 'p2', title: '/pricing', excerpt: 'Tre planer — Starter, Growth, Enterprise.' },
  { id: 'p3', title: '/docs/api', excerpt: 'POST /v1/chat — strømmet svar via SSE.' },
  { id: 'p4', title: '/blog/launch', excerpt: 'Lansert i Q4 — fokus på Norden.' },
  { id: 'p5', title: '/contact', excerpt: 'support@example.no · +47 22 00 00 00' },
  { id: 'p6', title: '/team', excerpt: '12 ingeniører, 4 designere, 2 i salg.' },
  { id: 'p7', title: '/changelog', excerpt: 'v0.9 — knowledge graph rules out.' },
  { id: 'p8', title: '/legal/dpa', excerpt: 'GDPR-compliant data processing addendum.' },
]

export function WebsiteStep({ machine }: { machine: OnboardingMachine }) {
  const initial = machine.state.website
  const [url, setUrl] = useState(initial?.url ?? '')
  const [brief, setBrief] = useState(initial?.agentBrief ?? '')
  const [submitted, setSubmitted] = useState(false)
  const [snippets, setSnippets] = useState<SnippetPreview[]>([])

  // Drip-feed placeholder snippets into the folder once the user
  // submits. The real wire-up replaces this `setInterval` with an SSE
  // subscription on /api/onboarding/crawl-preview.
  useEffect(() => {
    if (!submitted) return
    let index = 0
    const id = window.setInterval(() => {
      const next = PLACEHOLDER_SNIPPETS[index]
      if (!next) {
        window.clearInterval(id)
        // Advance to connect step once the folder is "full enough".
        window.setTimeout(() => machine.goTo('connect'), 1_200)
        return
      }
      setSnippets((prev) => [...prev, next])
      index += 1
    }, 600)
    return () => window.clearInterval(id)
  }, [submitted, machine])

  const submit = (event: React.FormEvent) => {
    event.preventDefault()
    if (!url.trim()) return
    machine.setWebsite({
      url: url.trim(),
      agentBrief: brief.trim(),
    })
    setSubmitted(true)
  }

  const fillPct = useMemo(() => {
    const target = PLACEHOLDER_SNIPPETS.length
    return Math.min(100, Math.round((snippets.length / target) * 100))
  }, [snippets.length])

  return (
    <>
      <LeftPane>
        <StepEyebrow>Steg 3 av 6</StepEyebrow>
        <StepTitle>Vis Velion nettsiden din.</StepTitle>
        <StepDescription>
          Vi henter innholdet og bruker det som kunnskapsbase for første
          agent. Du kan legge til flere kilder etterpå.
        </StepDescription>

        <form onSubmit={submit} className="flex flex-col gap-5">
          <label className="block">
            <span className="block font-inter text-[11px] uppercase tracking-[0.16em] text-[#6B6660]">
              Nettside
            </span>
            <div className="mt-2 flex items-stretch rounded-md border border-[#D6D2CB] bg-white">
              <span className="flex items-center px-3 font-inter text-[12px] text-[#A09890]">
                https://
              </span>
              <input
                autoFocus
                required
                type="text"
                disabled={submitted}
                value={url}
                onChange={(e) => setUrl(e.target.value)}
                placeholder="aquatiq.com"
                className="w-full rounded-r-md bg-transparent py-2.5 pr-3 font-inter text-[14px] text-[#1F1B17] placeholder:text-[#A09890] focus:outline-none disabled:opacity-60"
              />
            </div>
          </label>

          <label className="block">
            <span className="block font-inter text-[11px] uppercase tracking-[0.16em] text-[#6B6660]">
              Hva skal agenten gjøre?
            </span>
            <textarea
              rows={3}
              disabled={submitted}
              value={brief}
              onChange={(e) => setBrief(e.target.value)}
              placeholder="Du er kundestøtteagenten for Aquatiq. Svar på kundespørsmål om vannrensing, abonnement og bestillinger."
              className="mt-2 w-full resize-none rounded-md border border-[#D6D2CB] bg-white px-3 py-2.5 font-inter text-[13px] text-[#1F1B17] placeholder:text-[#A09890] focus:border-[#1F1B17] focus:outline-none disabled:opacity-60"
            />
          </label>

          {!submitted ? (
            <div className="flex items-center gap-4">
              <PrimaryButton type="submit" disabled={!url.trim()}>
                Start henting
              </PrimaryButton>
              <SkipLink onClick={() => machine.goTo('connect')}>
                Hopp over
              </SkipLink>
            </div>
          ) : (
            <p className="font-inter text-[12px] text-[#6B6660]">
              Henter innhold fra <strong>{url}</strong> …
            </p>
          )}
        </form>
      </LeftPane>

      <RightPane>
        <SnippetDropFolder snippets={snippets} fillPct={fillPct} />
      </RightPane>
    </>
  )
}

function SnippetDropFolder({
  snippets,
  fillPct,
}: {
  snippets: SnippetPreview[]
  fillPct: number
}) {
  return (
    <div className="relative flex h-full w-full items-center justify-center px-10">
      {/* Falling snippets layer */}
      <div className="pointer-events-none absolute inset-x-0 top-0 bottom-1/2 overflow-hidden">
        {snippets.slice(-3).map((snippet, i) => (
          <div
            key={snippet.id}
            className="absolute left-1/2 -translate-x-1/2 rounded-md border border-[#E5DFD3] bg-white px-3 py-2 shadow-sm"
            style={{
              top: `${10 + i * 30}%`,
              opacity: 1 - i * 0.3,
              animation: 'snippet-drop 0.7s ease-out forwards',
            }}
          >
            <p className="font-inter text-[10px] uppercase tracking-[0.12em] text-[#A09890]">
              {snippet.title}
            </p>
            <p className="mt-0.5 max-w-[14ch] truncate font-inter text-[11px] text-[#1F1B17]">
              {snippet.excerpt}
            </p>
          </div>
        ))}
      </div>

      {/* Folder card */}
      <div className="relative w-[18rem] rounded-2xl border border-[#E5DFD3] bg-white shadow-[0_8px_24px_rgba(31,27,23,0.08)]">
        {/* Folder tab */}
        <div className="absolute -top-3 left-6 h-3 w-20 rounded-t-md border border-b-0 border-[#E5DFD3] bg-white" />
        <div className="px-5 pt-6 pb-4">
          <p className="font-inter text-[11px] uppercase tracking-[0.16em] text-[#A09890]">
            Nettsidekunnskap
          </p>
          <p
            className="mt-2 text-[18px] text-[#1F1B17]"
            style={{ fontFamily: 'var(--font-cormorant-garamond), Georgia, serif' }}
          >
            {snippets.length} snutter samlet
          </p>
          <p className="mt-1 font-inter text-[11px] text-[#6B6660]">
            Quarry henter strukturert tekst og bilder fra siden din.
          </p>
        </div>
        <div className="border-t border-[#F1ECDF] px-5 py-3">
          <div className="flex items-center justify-between">
            <span className="font-inter text-[10px] uppercase tracking-[0.16em] text-[#A09890]">
              Fremdrift
            </span>
            <span className="font-inter text-[11px] tabular-nums text-[#1F1B17]">
              {fillPct}%
            </span>
          </div>
          <div className="mt-1.5 h-1.5 w-full overflow-hidden rounded-full bg-[#F1ECDF]">
            <div
              className="h-full rounded-full bg-[#1F1B17] transition-[width] duration-500"
              style={{ width: `${fillPct}%` }}
            />
          </div>
        </div>
      </div>

      <style>{`
        @keyframes snippet-drop {
          from { transform: translateX(-50%) translateY(-20%); opacity: 0; }
          to   { transform: translateX(-50%) translateY(80%);  opacity: 1; }
        }
      `}</style>
    </div>
  )
}
