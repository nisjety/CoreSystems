'use client'

/**
 * Step 2 — organization name.
 *
 * Left pane: a single input ("What's the name of your company?") plus
 * an optional company-size select. Submit advances to `website`.
 *
 * Right pane: 3s personalization animation (Slot 2). The static globe
 * mock stands in until the webm lands; the org-name appears in the
 * caption as soon as the user starts typing so the user feels the
 * personalization happen live.
 */

import React, { useState } from 'react'

import { BrregSearch } from '@/components/onboarding/ui/BrregSearch'
import type { BrregEnhet } from '@/lib/services/brreg-service'

import { formatOnboardingText, useOnboardingCopy } from '../i18n'
import type { OnboardingMachine } from '../state/useOnboardingMachine'
import type { OrganizationPayload } from '../state/types'

import {
  LeftPane,
  PrimaryButton,
  RightPane,
  StepDescription,
  StepEyebrow,
  StepTitle,
} from './_shared'

interface CreatedOrganization {
  id: string
  name: string
  slug?: string
}

const SIZES: { value: OrganizationPayload['size']; label: string }[] = [
  { value: 'solo', label: '1' },
  { value: 'small', label: '2–10' },
  { value: 'medium', label: '11–50' },
  { value: 'large', label: '51–250' },
  { value: 'enterprise', label: '250+' },
]

function sizeFromEmployeeCount(
  employeeCount: number | undefined,
): OrganizationPayload['size'] {
  if (employeeCount == null || employeeCount <= 0) return undefined
  if (employeeCount === 1) return 'solo'
  if (employeeCount <= 10) return 'small'
  if (employeeCount <= 50) return 'medium'
  if (employeeCount <= 250) return 'large'
  return 'enterprise'
}

function slugify(value: string): string {
  const slug = value
    .trim()
    .toLowerCase()
    .normalize('NFKD')
    .replace(/[\u0300-\u036f]/g, '')
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')

  return slug || `org-${Date.now()}`
}

async function createOrganization(payload: {
  name: string
  slug: string
  orgNumber?: string
  brregData?: BrregEnhet
  fallbackError: string
}): Promise<CreatedOrganization> {
  const response = await fetch('/api/org/orgs', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    credentials: 'include',
    body: JSON.stringify({
      name: payload.name,
      slug: payload.slug,
      plan: 'free',
      ...(payload.orgNumber ? { org_number: payload.orgNumber } : {}),
      ...(payload.brregData ? { brreg_data: payload.brregData } : {}),
    }),
  })

  const body = (await response.json().catch(() => null)) as
    | CreatedOrganization
    | { error?: string | { message?: string } }
    | null

  if (!response.ok || !body || !('id' in body) || !body.id) {
    const error = body && 'error' in body ? body.error : undefined
    const message =
      typeof error === 'string'
        ? error
        : error?.message || payload.fallbackError
    throw new Error(message)
  }

  return body
}

export function OrganizationStep({ machine }: { machine: OnboardingMachine }) {
  const { copy, formatNumber } = useOnboardingCopy()
  const initial = machine.state.organization
  const [name, setName] = useState(initial?.name ?? '')
  const [orgNumber, setOrgNumber] = useState<string | undefined>(
    initial?.brregOrgNumber,
  )
  const [employeeCount, setEmployeeCount] = useState<number | undefined>(
    initial?.employeeCount,
  )
  const [brregData, setBrregData] = useState<BrregEnhet | undefined>()
  const [size, setSize] = useState<OrganizationPayload['size']>(initial?.size)
  const [submitting, setSubmitting] = useState(false)
  const [error, setError] = useState<string | null>(null)
  // Default to the Brønnøysund-backed lookup that the old onboarding
  // used; the user can opt out with the "Skip verification" link to
  // get a plain text field (e.g. for non-Norwegian orgs).
  const [brregSkipped, setBrregSkipped] = useState(false)

  const handleBrregSelect = (enhet: BrregEnhet) => {
    setName(enhet.navn)
    setOrgNumber(enhet.organisasjonsnummer)
    setEmployeeCount(enhet.antallAnsatte)
    setBrregData(enhet)
    const inferredSize = sizeFromEmployeeCount(enhet.antallAnsatte)
    if (inferredSize) setSize(inferredSize)
  }

  const submit = async (event: React.FormEvent) => {
    event.preventDefault()
    const trimmedName = name.trim()
    if (!trimmedName || submitting) return

    setSubmitting(true)
    setError(null)
    try {
      const slug = initial?.slug || slugify(trimmedName)
      const organization = initial?.id
        ? { id: initial.id, name: trimmedName, slug }
        : await createOrganization({
            name: trimmedName,
            slug,
            orgNumber,
            brregData,
            fallbackError: copy.organization.createError,
          })

      machine.setOrganization({
        id: organization.id,
        name: organization.name || trimmedName,
        slug: organization.slug || slug,
        size,
        brregOrgNumber: orgNumber,
        employeeCount,
      })
      machine.goTo('website')
    } catch (err) {
      setError(err instanceof Error ? err.message : copy.organization.createError)
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <>
      <LeftPane machine={machine}>
        <StepEyebrow>{copy.organization.eyebrow}</StepEyebrow>
        <StepTitle>{copy.organization.title}</StepTitle>
        <StepDescription>{copy.organization.description}</StepDescription>

        <form onSubmit={submit} className="flex flex-col gap-5">
          {brregSkipped ? (
            <label className="block">
              <span className="block font-inter text-[11px] uppercase tracking-[0.16em] text-[#6B6660]">
                {copy.organization.label}
              </span>
              <input
                autoFocus
                required
                type="text"
                value={name}
                onChange={(e) => {
                  setName(e.target.value)
                  // Manual edits invalidate any earlier Brreg match.
                  setOrgNumber(undefined)
                  setEmployeeCount(undefined)
                  setBrregData(undefined)
                }}
                placeholder="Aquatiq AS"
                className="mt-2 w-full rounded-md border border-[#D6D2CB] bg-white px-3 py-2.5 font-inter text-[14px] text-[#1F1B17] placeholder:text-[#A09890] focus:border-[#1F1B17] focus:outline-none"
              />
              <button
                type="button"
                onClick={() => setBrregSkipped(false)}
                className="mt-2 font-inter text-[11px] uppercase tracking-[0.16em] text-[#A09890] hover:text-[#1F1B17]"
              >
                {copy.organization.brregBack}
              </button>
            </label>
          ) : (
            <BrregSearch
              initialQuery={name}
              onSelect={handleBrregSelect}
              onSkip={() => {
                setBrregSkipped(true)
                setBrregData(undefined)
                setOrgNumber(undefined)
                setEmployeeCount(undefined)
              }}
            />
          )}

          <fieldset>
            <legend className="block font-inter text-[11px] uppercase tracking-[0.16em] text-[#6B6660]">
              {copy.organization.sizeLegend}
            </legend>
            <div className="mt-2 flex flex-wrap gap-2">
              {SIZES.map((option) => {
                const active = size === option.value
                return (
                  <button
                    key={option.value}
                    type="button"
                    onClick={() => setSize(option.value)}
                    className={`rounded-full border px-3.5 py-1.5 font-inter text-[12px] transition-colors ${
                      active
                        ? 'border-[#1F1B17] bg-[#1F1B17] text-white'
                        : 'border-[#D6D2CB] bg-white text-[#1F1B17] hover:border-[#A09890]'
                    }`}
                  >
                    {option.label}
                  </button>
                )
              })}
            </div>
            {employeeCount != null && (
              <p className="mt-2 font-inter text-[11px] text-[#6B6660]">
                {formatOnboardingText(copy.organization.employeeHint, {
                  count: formatNumber(employeeCount),
                })}
              </p>
            )}
          </fieldset>

          {error && (
            <p className="font-inter text-[12px] leading-5 text-[#B42318]">
              {error}
            </p>
          )}

          <PrimaryButton type="submit" disabled={!name.trim() || submitting}>
            {submitting ? copy.organization.creating : copy.organization.continue}
          </PrimaryButton>
        </form>
      </LeftPane>

      <RightPane>
        {/* Slot 2 placeholder. Replace with the webm when ready. */}
        <img
          src="/imagens/onboarding/org-personalization-mock.png"
          alt=""
          className="h-full w-full object-cover"
        />
        <div className="pointer-events-none absolute inset-x-6 bottom-6 rounded-xl bg-white/85 px-5 py-3 backdrop-blur-md">
          <p className="font-inter text-[11px] uppercase tracking-[0.16em] text-[#A09890]">
            {copy.organization.personalizing}
          </p>
          <p className="mt-1 font-inter text-[12px] font-medium text-[#1F1B17]">
            {name
              ? employeeCount != null
                ? formatOnboardingText(copy.organization.foundEmployees, {
                    name,
                    count: formatNumber(employeeCount),
                  })
                : formatOnboardingText(copy.organization.fetchingPublicInfo, {
                    name,
                  })
              : copy.organization.enterName}
          </p>
        </div>
      </RightPane>
    </>
  )
}
