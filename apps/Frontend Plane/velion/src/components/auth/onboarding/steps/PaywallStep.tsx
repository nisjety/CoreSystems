'use client'

/**
 * Step 6 — Model Plane recommended paywall.
 *
 * The recommendation is recalculated from the current onboarding
 * choices every time this step mounts: Brreg employee count, selected
 * size, website and connector picks are sent to the Model Plane-backed
 * route. The full-screen view follows the ElevenLabs pricing modal:
 * warm off-white canvas, editorial white cards, a light featured
 * shell for the recommended plan, pill badges and a thin animated
 * pastel ring.
 */

import React, { useEffect, useMemo, useState } from 'react'
import { Check, Star } from 'lucide-react'
import { m } from 'framer-motion'

import {
  formatOnboardingNumber,
  formatOnboardingText,
  ONBOARDING_COPY,
  onboardingPlanName,
  type OnboardingLocale,
  useOnboardingCopy,
} from '../i18n'
import type { OnboardingMachine } from '../state/useOnboardingMachine'
import type {
  ConnectorPick,
  OrganizationPayload,
  PlanRecommendation,
  WebsitePayload,
} from '../state/types'

import {
  LeftPane,
  OnboardingTopActions,
  PrimaryButton,
  RightPane,
  StepDescription,
  StepEyebrow,
  StepTitle,
} from './_shared'

type PlanId = PlanRecommendation['planId']

interface PaywallStepProps {
  machine: OnboardingMachine
  fullScreen?: boolean
}

interface Plan {
  id: PlanId
  name: string
  monthlyPrice: string
  yearlyPrice?: string
  cadence: string
  oldPrice?: string
  description: string
  features: string[]
  badge?: string
}

type PaywallPlanCopy = Record<
  PlanId,
  {
    name: string
    description: string
    features: readonly string[]
    cadence: string
    badge?: string
  }
>

interface PaywallCopy {
  plans: PaywallPlanCopy
  fullTitle: string
  fullSubtitle: string
  monthly: string
  yearly: string
  trialBadge: string
  modelAnalyzing: string
  recommendedShort: string
  skipToSetup: string
  continueToSetup: string
  selected: string
  choosePlan: string
  recommended: string
  signalAgentTask: string
  sources: string
}

const PLAN_PRICE_META: Record<
  OnboardingLocale,
  Record<PlanId, Pick<Plan, 'id' | 'monthlyPrice' | 'yearlyPrice'>>
> = {
  nb: {
    trial: { id: 'trial', monthlyPrice: '0 kr' },
    hobby: { id: 'hobby', monthlyPrice: '299 kr', yearlyPrice: '239 kr' },
    standard: { id: 'standard', monthlyPrice: '999 kr', yearlyPrice: '849 kr' },
    pro: { id: 'pro', monthlyPrice: '1 499 kr', yearlyPrice: '1 099 kr' },
    enterprise: { id: 'enterprise', monthlyPrice: 'Tilpasset' },
  },
  en: {
    trial: { id: 'trial', monthlyPrice: '$0' },
    hobby: { id: 'hobby', monthlyPrice: '$25', yearlyPrice: '$20' },
    standard: { id: 'standard', monthlyPrice: '$99', yearlyPrice: '$85' },
    pro: { id: 'pro', monthlyPrice: '$149', yearlyPrice: '$110' },
    enterprise: { id: 'enterprise', monthlyPrice: 'Contact sales' },
  },
}

function plansForCopy(copy: PaywallPlanCopy, locale: OnboardingLocale): Plan[] {
  const prices = PLAN_PRICE_META[locale]
  return (['trial', 'hobby', 'standard', 'pro', 'enterprise'] as const).map(
    (id) => ({
      ...prices[id],
      name: copy[id].name,
      cadence: copy[id].cadence,
      description: copy[id].description,
      features: [...copy[id].features],
      badge: copy[id].badge,
    }),
  )
}

const MICROSOFT_CONNECTOR_ALIASES = new Set([
  'teams',
  'sharepoint',
  'onedrive',
  'outlook',
  'm365',
  'microsoft365',
  'microsoft-365',
])

function selectedPlanReason(planId: PlanId, locale: OnboardingLocale): string {
  const planName = nameForId(planId, locale)
  return locale === 'nb'
    ? `${planName} ble valgt under onboarding.`
    : `${planName} was selected during onboarding.`
}

function planSaveFallbackError(locale: OnboardingLocale): string {
  return locale === 'nb'
    ? 'Kunne ikke lagre planen for organisasjonen. Prøv igjen.'
    : 'Could not save the plan for this organization. Try again.'
}

function missingOrganizationError(locale: OnboardingLocale): string {
  return locale === 'nb'
    ? 'Organisasjonen mangler i onboarding-økten. Gå tilbake og opprett organisasjonen først.'
    : 'The organization is missing from this onboarding session. Go back and create it first.'
}

function buildSelectedPlanRecommendation({
  current,
  planId,
  summary,
  locale,
}: {
  current: PlanRecommendation | null
  planId: PlanId
  summary: string
  locale: OnboardingLocale
}): PlanRecommendation {
  return {
    planId,
    reason:
      current?.planId === planId
        ? current.reason
        : selectedPlanReason(planId, locale),
    summary,
    generatedAt: new Date().toISOString(),
  }
}

async function parsePlanSaveError(
  response: Response,
  fallback: string,
): Promise<string> {
  const body = (await response.json().catch(() => null)) as
    | { error?: string | { message?: string }; message?: string }
    | null

  if (typeof body?.error === 'string') return body.error
  if (
    body?.error &&
    typeof body.error === 'object' &&
    typeof body.error.message === 'string'
  ) {
    return body.error.message
  }
  if (body?.message) return body.message
  return fallback
}

async function persistSelectedPlan({
  organization,
  website,
  connectors,
  planId,
  recommendation,
  locale,
}: {
  organization: OrganizationPayload
  website?: WebsitePayload
  connectors: ConnectorPick[]
  planId: PlanId
  recommendation: PlanRecommendation
  locale: OnboardingLocale
}) {
  if (!organization.id) {
    throw new Error(missingOrganizationError(locale))
  }

  const selectedAt = new Date().toISOString()
  const selectedPlanName = nameForId(planId, locale)
  const onboardingPlan = {
    selected_plan_id: planId,
    selected_plan_name: selectedPlanName,
    selected_at: selectedAt,
    locale,
    recommendation,
    source_count: countOnboardingSources({ website, connectors }),
  }

  const response = await fetch(
    `/api/org/orgs/${encodeURIComponent(organization.id)}/plan`,
    {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      credentials: 'include',
      body: JSON.stringify({
        plan: planId,
        reason: recommendation.reason,
        onboarding: onboardingPlan,
      }),
    },
  )

  if (!response.ok) {
    throw new Error(
      await parsePlanSaveError(response, planSaveFallbackError(locale)),
    )
  }

  await fetch(
    `/api/org/internal/orgs/${encodeURIComponent(
      organization.id,
    )}/onboarding/state`,
    {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      credentials: 'include',
      body: JSON.stringify({
        status: 'PLAN_SELECTED',
        steps: {
          plan: onboardingPlan,
          organization: {
            id: organization.id,
            name: organization.name,
            slug: organization.slug,
          },
        },
      }),
    },
  ).catch(() => {
    // The org plan update above is the source of truth. The onboarding-state
    // mirror is best-effort so a transient internal write does not block setup.
  })
}

export function PaywallStep({ machine, fullScreen = false }: PaywallStepProps) {
  const { locale, copy } = useOnboardingCopy()
  const initialRecommendation = machine.state.recommendation ?? null
  const [recommendation, setRecommendation] =
    useState<PlanRecommendation | null>(initialRecommendation)
  const [loading, setLoading] = useState(false)
  const [savingPlan, setSavingPlan] = useState(false)
  const [saveError, setSaveError] = useState<string | null>(null)
  const [selected, setSelected] = useState<PlanId>(
    initialRecommendation?.planId ?? 'trial',
  )

  const localSummary = useMemo(
    () =>
      buildLocalSummary({
        organization: machine.state.organization,
        website: machine.state.website,
        connectors: machine.state.connectors,
        planId: recommendation?.planId ?? selected,
        locale,
      }),
    [
      machine.state.organization,
      machine.state.website,
      machine.state.connectors,
      recommendation?.planId,
      selected,
      locale,
    ],
  )

  // Recompute from current user choices instead of trusting a stale
  // localStorage recommendation from a previous visit to this step.
  useEffect(() => {
    const hasSignal =
      Boolean(machine.state.website?.url) ||
      Boolean(machine.state.website?.agentBrief?.trim()) ||
      machine.state.connectors.length > 0 ||
      Boolean(machine.state.organization?.size) ||
      machine.state.organization?.employeeCount != null

    if (!hasSignal) {
      const trial: PlanRecommendation = {
        planId: 'trial',
        reason: copy.paywall.fallbackTrialReason,
        summary: copy.paywall.fallbackTrialSummary,
        generatedAt: new Date().toISOString(),
      }
      setRecommendation(trial)
      machine.setRecommendation(trial)
      setSelected('trial')
      return
    }

    const localRecommendation = buildLocalRecommendation({
      organization: machine.state.organization,
      website: machine.state.website,
      connectors: machine.state.connectors,
      reasonPrefix: copy.paywall.reasonPrefix,
      locale,
    })
    setRecommendation(localRecommendation)
    machine.setRecommendation(localRecommendation)
    setSelected(localRecommendation.planId)

    let cancelled = false
    setLoading(true)
    fetch('/api/onboarding/recommend-plan', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({
        organization: machine.state.organization,
        website: machine.state.website,
        connectors: normalizedConnectorPicks(machine.state.connectors),
        locale,
        sourceCount: countOnboardingSources({
          website: machine.state.website,
          connectors: machine.state.connectors,
        }),
      }),
    })
      .then((res) => {
        if (!res.ok) throw new Error(`recommend-plan ${res.status}`)
        return res.json()
      })
      .then((body: { recommendation?: PlanRecommendation }) => {
        if (cancelled) return
        if (body.recommendation) {
          setRecommendation(body.recommendation)
          machine.setRecommendation(body.recommendation)
          setSelected(body.recommendation.planId)
        }
      })
      .catch(() => {
        const fallback = buildLocalRecommendation({
          organization: machine.state.organization,
          website: machine.state.website,
          connectors: machine.state.connectors,
          reasonPrefix: copy.paywall.reasonPrefix,
          locale,
        })
        if (!cancelled) {
          setRecommendation(fallback)
          machine.setRecommendation(fallback)
          setSelected(fallback.planId)
        }
      })
      .finally(() => {
        if (!cancelled) setLoading(false)
      })

    return () => {
      cancelled = true
    }
    // Run once per mount with the current state snapshot.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [locale])

  const recommendedPlanId = recommendation?.planId ?? selected
  const summary = recommendation?.summary ?? localSummary
  const recommendationReason = loading
    ? copy.paywall.loadingReason
    : recommendation?.reason ??
      copy.paywall.defaultReason

  const handleSelect = (planId: PlanId) => {
    setSelected(planId)
    setSaveError(null)
  }

  const handleContinue = async (planId: PlanId) => {
    if (savingPlan) return

    const organization = machine.state.organization
    if (!organization?.id) {
      setSaveError(missingOrganizationError(locale))
      return
    }

    const selectedSummary =
      recommendation?.planId === planId && recommendation.summary
        ? recommendation.summary
        : buildLocalSummary({
            organization,
            website: machine.state.website,
            connectors: machine.state.connectors,
            planId,
            locale,
          })
    const selectedRecommendation = buildSelectedPlanRecommendation({
      current: recommendation,
      planId,
      summary: selectedSummary,
      locale,
    })

    setSavingPlan(true)
    setSaveError(null)

    try {
      await persistSelectedPlan({
        organization,
        website: machine.state.website,
        connectors: machine.state.connectors,
        planId,
        recommendation: selectedRecommendation,
        locale,
      })
      setSelected(planId)
      setRecommendation(selectedRecommendation)
      machine.setRecommendation(selectedRecommendation)
      machine.setOrganization({
        ...organization,
        plan: planId,
      })
      machine.goTo('assembly')
    } catch (err) {
      setSaveError(
        err instanceof Error ? err.message : planSaveFallbackError(locale),
      )
    } finally {
      setSavingPlan(false)
    }
  }

  if (fullScreen) {
    return (
      <FullScreenPaywall
        machine={machine}
        locale={locale}
        copy={copy.paywall}
        loading={loading}
        selected={selected}
        recommendedPlanId={recommendedPlanId}
        summary={summary}
        savingPlan={savingPlan}
        saveError={saveError}
        onSelect={handleSelect}
        onContinue={handleContinue}
      />
    )
  }

  return (
    <>
      <LeftPane machine={machine}>
        <StepEyebrow>{copy.paywall.eyebrow}</StepEyebrow>
        <StepTitle>{copy.paywall.title}</StepTitle>
        <StepDescription>{recommendationReason}</StepDescription>

        <div className="rounded-2xl border border-[#E7E5E4] bg-[#FAFAFA] p-4">
          <p className="font-inter text-[10px] font-semibold uppercase tracking-[0.16em] text-[#777169]">
            {copy.paywall.why}
          </p>
          <p className="mt-2 font-inter text-[13px] leading-5 text-[#292524]">
            {summary}
          </p>
          <SignalStrip
            organization={machine.state.organization}
            website={machine.state.website}
            connectors={machine.state.connectors}
            locale={locale}
            signalAgentTask={copy.paywall.signalAgentTask}
            sourcesTemplate={copy.paywall.sources}
          />
        </div>

        {saveError && (
          <p className="font-inter text-[12px] leading-5 text-[#B42318]">
            {saveError}
          </p>
        )}

        <PrimaryButton
          onClick={() => void handleContinue(selected)}
          disabled={savingPlan}
        >
          {savingPlan
            ? copy.paywall.continueToSetup
            : formatOnboardingText(copy.paywall.choose, {
                plan: nameForId(selected, locale),
              })}
        </PrimaryButton>
      </LeftPane>

      <RightPane>
        <div className="relative h-full w-full overflow-y-auto bg-[#F5F5F5] p-5 text-[#0C0A09]">
          <div className="mb-4 flex items-start justify-between gap-4">
            <div>
              <p className="font-inter text-[10px] font-semibold uppercase tracking-[0.16em] text-[#777169]">
                {copy.paywall.plansTitle}
              </p>
              <p
                className="mt-1 text-[clamp(24px,2.2vw,34px)] font-normal leading-[1.05] tracking-normal text-[#0C0A09]"
                style={{
                  fontFamily:
                    'var(--font-geist-sans), Arial, sans-serif',
                }}
              >
                {copy.paywall.plansHeading}
              </p>
            </div>
            <span className="rounded-full bg-[#F0EFED] px-3 py-1 font-inter text-[10px] font-semibold uppercase tracking-[0.12em] text-[#292524]">
              {loading
                ? copy.paywall.analyzing
                : formatOnboardingText(copy.paywall.recommended, {
                    plan: nameForId(recommendedPlanId, locale),
                  })}
            </span>
          </div>

          <PlanGrid
            plans={plansForCopy(copy.paywall.plans, locale)}
            copy={copy.paywall}
            selected={selected}
            recommendedPlanId={recommendedPlanId}
            onSelect={handleSelect}
            disabled={savingPlan}
          />
        </div>
        <style>{`
          @keyframes verevon-elevenlabs-ring {
            0% { background-position: 0 0, 0% 50%; }
            50% { background-position: 0 0, 100% 50%; }
            100% { background-position: 0 0, 0% 50%; }
          }
        `}</style>
      </RightPane>
    </>
  )
}

function FullScreenPaywall({
  machine,
  locale,
  copy,
  loading,
  selected,
  recommendedPlanId,
  summary,
  savingPlan,
  saveError,
  onSelect,
  onContinue,
}: {
  machine: OnboardingMachine
  locale: OnboardingLocale
  copy: PaywallCopy
  loading: boolean
  selected: PlanId
  recommendedPlanId: PlanId
  summary: string
  savingPlan: boolean
  saveError: string | null
  onSelect: (planId: PlanId) => void
  onContinue: (planId: PlanId) => Promise<void>
}) {
  const [billingCycle, setBillingCycle] = useState<'monthly' | 'yearly'>(
    'monthly',
  )

  const choosePlan = (planId: PlanId) => {
    onSelect(planId)
    void onContinue(planId)
  }

  return (
    <div className="relative isolate min-h-[100dvh] w-[100dvw] overflow-y-auto bg-[#F7F7F6] text-[#0C0A09]">
      <div
        aria-hidden="true"
        className="pointer-events-none fixed inset-0 z-0 overflow-hidden opacity-[0.045]"
      >
        <div
          className="absolute inset-[-200%] h-[400%] w-[400%]"
          style={{
            backgroundImage:
              'url("https://framerusercontent.com/images/rR6HYXBrMmX4cRpXfXUOvpvpB0.png")',
            animation: 'noise-pan 60s ease-in-out infinite',
          }}
        />
      </div>

      <main className="relative z-10 mx-auto flex min-h-[100dvh] w-full max-w-[1260px] flex-col justify-center px-4 py-10 sm:px-6 lg:px-8">
        <OnboardingTopActions machine={machine} fullScreen />

        <div className="mb-8 flex flex-col gap-5 md:flex-row md:items-end md:justify-between">
          <div>
            <h1
              className="text-[clamp(40px,5vw,72px)] font-normal leading-[1.02] tracking-normal text-[#191716]"
              style={{ fontFamily: 'var(--font-geist-sans), Arial, sans-serif' }}
            >
              {copy.fullTitle}
            </h1>
            <p className="mt-3 font-inter text-[15px] font-medium text-[#777169]">
              {copy.fullSubtitle}
            </p>
          </div>
          <BillingToggle
            value={billingCycle}
            onChange={setBillingCycle}
            monthlyLabel={copy.monthly}
            yearlyLabel={copy.yearly}
            badgeLabel={copy.trialBadge}
          />
        </div>

        <PlanGrid
          plans={plansForCopy(copy.plans, locale)}
          copy={copy}
          selected={selected}
          recommendedPlanId={recommendedPlanId}
          onSelect={onSelect}
          onChoose={choosePlan}
          billingCycle={billingCycle}
          disabled={savingPlan}
          fullScreen
        />

        <div className="mt-6 rounded-[18px] bg-white/70 px-5 py-5 text-center shadow-[inset_0_0_0_1px_rgba(231,229,228,0.8)]">
          <p className="font-inter text-[15px] font-semibold text-[#191716]">
            {loading
              ? copy.modelAnalyzing
              : formatOnboardingText(copy.recommendedShort, {
                  plan: nameForId(recommendedPlanId, locale),
                })}
          </p>
          <p className="mx-auto mt-2 max-w-[760px] font-inter text-[14px] leading-6 text-[#777169]">
            {summary}
          </p>
          <div className="mt-3 flex justify-center">
            <SignalStrip
              organization={machine.state.organization}
              website={machine.state.website}
              connectors={machine.state.connectors}
              locale={locale}
              signalAgentTask={copy.signalAgentTask}
              sourcesTemplate={copy.sources}
            />
          </div>
        </div>

        {saveError && (
          <p className="mt-4 text-center font-inter text-[13px] leading-5 text-[#B42318]">
            {saveError}
          </p>
        )}

        <div className="mt-7 flex items-center justify-center gap-2">
          <button
            type="button"
            onClick={() => choosePlan('trial')}
            disabled={savingPlan}
            className="h-10 rounded-[10px] border border-[#E7E5E4] bg-white px-5 font-inter text-[14px] font-semibold text-[#191716] transition-colors hover:border-[#D6D3D1] disabled:cursor-not-allowed disabled:opacity-60"
          >
            {copy.skipToSetup}
          </button>
          <button
            type="button"
            onClick={() => choosePlan(selected)}
            disabled={savingPlan}
            className="h-10 rounded-[10px] border border-[#E7E5E4] bg-white px-5 font-inter text-[14px] font-semibold text-[#191716] transition-colors hover:border-[#D6D3D1] disabled:cursor-not-allowed disabled:opacity-60"
          >
            {copy.continueToSetup}
          </button>
        </div>
      </main>
      <style>{`
        @keyframes verevon-elevenlabs-ring {
          0% { background-position: 0% 50%; }
          50% { background-position: 100% 50%; }
          100% { background-position: 0% 50%; }
        }
      `}</style>
    </div>
  )
}

function BillingToggle({
  value,
  onChange,
  monthlyLabel,
  yearlyLabel,
  badgeLabel,
}: {
  value: 'monthly' | 'yearly'
  onChange: (value: 'monthly' | 'yearly') => void
  monthlyLabel: string
  yearlyLabel: string
  badgeLabel: string
}) {
  const yearly = value === 'yearly'

  return (
    <div className="flex flex-wrap items-center justify-end gap-2 font-inter text-[14px] font-medium text-[#191716]">
      <button
        type="button"
        onClick={() => onChange('monthly')}
        className={yearly ? 'text-[#777169]' : 'text-[#191716]'}
      >
        {monthlyLabel}
      </button>
      <button
        type="button"
        role="switch"
        aria-checked={yearly}
        onClick={() => onChange(yearly ? 'monthly' : 'yearly')}
        className="relative h-6 w-11 shrink-0 rounded-full bg-[#E5E5E7] transition-colors data-[checked=true]:bg-[#D8F8E7]"
        data-checked={yearly}
      >
        <span
          className={`absolute left-1 top-1 h-4 w-4 rounded-full bg-white shadow-sm transition-transform ${
            yearly ? 'translate-x-5' : 'translate-x-0'
          }`}
        />
      </button>
      <button
        type="button"
        onClick={() => onChange('yearly')}
        className={yearly ? 'text-[#191716]' : 'text-[#777169]'}
      >
        {yearlyLabel}
      </button>
      <span className="ml-1 rounded-full bg-[#DDFBEA] px-3 py-1 font-inter text-[12px] font-semibold text-[#1F5135]">
        {badgeLabel}
      </span>
    </div>
  )
}

function PlanGrid({
  plans,
  copy,
  selected,
  recommendedPlanId,
  onSelect,
  onChoose,
  billingCycle = 'monthly',
  disabled = false,
  fullScreen = false,
}: {
  plans: Plan[]
  copy: Pick<PaywallCopy, 'selected' | 'choosePlan' | 'recommended'>
  selected: PlanId
  recommendedPlanId: PlanId
  onSelect: (planId: PlanId) => void
  onChoose?: (planId: PlanId) => void
  billingCycle?: 'monthly' | 'yearly'
  disabled?: boolean
  fullScreen?: boolean
}) {
  return (
    <div
      className={
        fullScreen
          ? 'grid grid-cols-1 items-stretch gap-4 sm:grid-cols-2 lg:grid-cols-5'
          : 'grid grid-cols-2 gap-3'
      }
    >
      {plans.map((plan) => (
        <PlanCard
          key={plan.id}
          plan={plan}
          copy={copy}
          selected={selected === plan.id}
          recommended={recommendedPlanId === plan.id}
          onSelect={() => onSelect(plan.id)}
          onChoose={onChoose ? () => onChoose(plan.id) : undefined}
          billingCycle={billingCycle}
          disabled={disabled}
          fullScreen={fullScreen}
        />
      ))}
    </div>
  )
}

function PlanCard({
  plan,
  copy,
  selected,
  recommended,
  onSelect,
  onChoose,
  billingCycle = 'monthly',
  disabled = false,
  fullScreen = false,
}: {
  plan: Plan
  copy: Pick<PaywallCopy, 'selected' | 'choosePlan' | 'recommended'>
  selected: boolean
  recommended: boolean
  onSelect: () => void
  onChoose?: () => void
  billingCycle?: 'monthly' | 'yearly'
  disabled?: boolean
  fullScreen?: boolean
}) {
  const price =
    billingCycle === 'yearly' && plan.yearlyPrice
      ? plan.yearlyPrice
      : plan.monthlyPrice

  return (
    <div
      className={`relative h-full rounded-[24px] p-[2px] ${
        selected ? '' : 'bg-transparent'
      }`}
    >
      {selected && (
        <m.div
          layoutId="selected-paywall-card-ring"
          className="absolute inset-0 rounded-[24px] shadow-[0_10px_24px_rgba(12,10,9,0.10)]"
          style={{
            background:
              'linear-gradient(120deg, #A7E5D3, #F4C5A8, #C8B8E0, #A8C8E8, #A7E5D3)',
            backgroundSize: '260% 260%',
            animation: 'verevon-elevenlabs-ring 8s ease-in-out infinite',
          }}
          transition={{
            type: 'spring',
            stiffness: 420,
            damping: 38,
            mass: 0.8,
          }}
        />
      )}

    <article
      onClick={() => {
        if (!disabled) onSelect()
      }}
        className={`relative z-10 flex h-full cursor-pointer flex-col border bg-white text-left transition duration-200 ${
        fullScreen
            ? 'min-h-[392px] rounded-[22px] p-5'
          : 'min-h-[184px] rounded-2xl p-4'
      } ${
        selected
            ? 'border-transparent shadow-[0_12px_28px_rgba(12,10,9,0.08)]'
          : 'border-[#E7E5E4] hover:border-[#D6D3D1]'
      }`}
      aria-current={selected ? 'true' : undefined}
    >
      <div className="flex min-h-[42px] items-start justify-between gap-2">
        <h2 className="font-inter text-[20px] font-semibold leading-tight text-[#191716]">
          {plan.name}
        </h2>
        <div className="flex flex-wrap justify-end gap-1.5">
          {recommended && (
            <span className="inline-flex items-center gap-1 rounded-full bg-[#F0EFED] px-2.5 py-1 font-inter text-[11px] font-semibold text-[#191716]">
              <Star className="h-3 w-3 fill-[#191716] text-[#191716]" />
              {formatOnboardingText(copy.recommended, { plan: '' }).trim() ||
                copy.recommended}
            </span>
          )}
          {plan.badge && (
            <span className="rounded-full bg-[#DDFBEA] px-3 py-1 font-inter text-[12px] font-semibold text-[#1F5135]">
              {plan.badge}
            </span>
          )}
        </div>
      </div>

      <div className="mt-5 flex min-h-[40px] items-end gap-2">
        {plan.oldPrice && (
          <span className="font-inter text-[18px] font-bold leading-none text-[#777169] line-through">
            {plan.oldPrice}
          </span>
        )}
        <span className="font-inter text-[32px] font-bold leading-none tracking-normal text-[#191716]">
          {price}
        </span>
        <span className="pb-1 font-inter text-[16px] font-medium text-[#191716]">
          {plan.cadence}
        </span>
      </div>

      <p className="mt-5 min-h-[60px] font-inter text-[14px] font-medium leading-5 text-[#777169]">
        {plan.description}
      </p>

      <ul className="mt-5 min-h-[132px] space-y-3">
        {plan.features.map((feature) => (
          <li
            key={feature}
            className="flex items-start gap-3 font-inter text-[14px] font-semibold leading-5 text-[#292524]"
          >
            <span
              className="mt-0.5 flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-[#191716]"
              aria-hidden="true"
            >
              <Check className="h-3 w-3 text-white" strokeWidth={3} />
            </span>
            {feature}
          </li>
        ))}
      </ul>

      <button
        type="button"
        disabled={disabled}
        onClick={(event) => {
          event.stopPropagation()
          if (disabled) return
          if (onChoose) onChoose()
          else onSelect()
        }}
        className="mt-auto h-10 rounded-[10px] border border-[#E7E5E4] bg-white px-4 font-inter text-[14px] font-semibold text-[#191716] transition-colors hover:border-[#D6D3D1] disabled:cursor-not-allowed disabled:opacity-60"
      >
        {selected ? copy.selected : copy.choosePlan}
      </button>
    </article>
    </div>
  )
}

function SignalStrip({
  organization,
  website,
  connectors,
  locale,
  signalAgentTask,
  sourcesTemplate,
}: {
  organization?: OrganizationPayload
  website?: WebsitePayload
  connectors: ConnectorPick[]
  locale: OnboardingLocale
  signalAgentTask: string
  sourcesTemplate: string
}) {
  const sourceCount = countOnboardingSources({ website, connectors })
  const signals = [
    organization?.employeeCount != null
      ? employeeSignal(organization.employeeCount, locale)
      : organization?.size
        ? organizationSizeLabel(organization.size, locale)
        : null,
    sourceCount > 0
      ? formatOnboardingText(sourcesTemplate, { count: sourceCount })
      : null,
    website?.url ? websiteHost(website.url) : null,
    website?.agentBrief?.trim() ? signalAgentTask : null,
  ].filter((signal): signal is string => Boolean(signal))

  if (signals.length === 0) return null

  return (
    <div className="mt-3 flex flex-wrap gap-1.5">
      {signals.map((signal) => (
        <span
          key={signal}
          className="rounded-full bg-[#F0EFED] px-2 py-1 font-inter text-[10px] font-medium text-[#4E4E4E]"
        >
          {signal}
        </span>
      ))}
    </div>
  )
}

function buildLocalSummary({
  organization,
  website,
  connectors,
  planId,
  locale,
}: {
  organization?: OrganizationPayload
  website?: WebsitePayload
  connectors: ConnectorPick[]
  planId: PlanId
  locale: OnboardingLocale
}): string {
  const copy = ONBOARDING_COPY[locale]
  const context = buildLocalRecommendationContext({
    organization,
    website,
    connectors,
    locale,
  })
  const plan = nameForId(planId, locale)
  return formatOnboardingText(
    context.goal
      ? copy.recommendation.summaryWithGoal
      : copy.recommendation.summaryWithoutGoal,
    {
      subject: context.subject,
      teamQualifier: context.teamQualifier,
      plan,
      goal: context.goal ?? '',
      sources: context.sourcesText,
      fit: planFitText(planId, locale),
    },
  )
}

function buildLocalRecommendation({
  organization,
  website,
  connectors,
  reasonPrefix,
  locale,
}: {
  organization?: OrganizationPayload
  website?: WebsitePayload
  connectors: ConnectorPick[]
  reasonPrefix: string
  locale: OnboardingLocale
}): PlanRecommendation {
  const copy = ONBOARDING_COPY[locale]
  const planId = selectLocalPlanId({ organization, website, connectors })
  const context = buildLocalRecommendationContext({
    organization,
    website,
    connectors,
    locale,
  })
  const reason = formatOnboardingText(
    context.goal
      ? copy.recommendation.reasonWithGoal
      : copy.recommendation.reasonWithoutGoal,
    {
      prefix: reasonPrefix,
      plan: nameForId(planId, locale),
      subject: context.subject,
      goalPhrase: context.goal ? customerGoalPhrase(context.goal, locale) : '',
      sources: context.sourcesText,
    },
  )
  return {
    planId,
    reason,
    summary: buildLocalSummary({
      organization,
      website,
      connectors,
      planId,
      locale,
    }),
    generatedAt: new Date().toISOString(),
  }
}

function selectLocalPlanId({
  organization,
  website,
  connectors,
}: {
  organization?: OrganizationPayload
  website?: WebsitePayload
  connectors: ConnectorPick[]
}): PlanId {
  const sourceCount = countOnboardingSources({ website, connectors })
  const hasSignal =
    sourceCount > 0 ||
    Boolean(website?.agentBrief?.trim()) ||
    organization?.employeeCount != null ||
    Boolean(organization?.size)
  if (!hasSignal) return 'trial'

  const employeeCount = organization?.employeeCount
  const connectorCount = uniqueConnectorCount(connectors)
  const advancedIntent = hasAdvancedIntent(website?.agentBrief)
  const expertIntent = hasExpertIntent(website?.agentBrief)
  const customIntent = hasCustomIntent(website?.agentBrief)
  const sizeRank: Record<string, number> = {
    solo: 1,
    small: 2,
    medium: 3,
    large: 4,
    enterprise: 5,
  }
  const sizeScore = organization?.size ? sizeRank[organization.size] ?? 0 : 0
  const customSized =
    (employeeCount != null && employeeCount >= 100) ||
    sizeScore >= 4
  const expertSized =
    employeeCount != null && employeeCount >= 50 && employeeCount < 100
  const customComplexity =
    connectorCount >= 4 ||
    sourceCount >= 5 ||
    customIntent
  const expertComplexity =
    connectorCount >= 3 ||
    sourceCount >= 4 ||
    expertIntent

  if (customSized && customComplexity) {
    return 'enterprise'
  }
  if (expertSized && expertComplexity) {
    return 'pro'
  }
  if (
    (employeeCount != null && employeeCount >= 11) ||
    sizeScore >= 3 ||
    connectorCount >= 3 ||
    sourceCount >= 4 ||
    (advancedIntent && sourceCount >= 2)
  ) {
    return 'standard'
  }
  return 'hobby'
}

function hasAdvancedIntent(value?: string): boolean {
  const text = value?.toLowerCase() ?? ''
  return [
    'automatis',
    'workflow',
    'ruting',
    'routing',
    'triage',
    'eskaler',
    'handoff',
    'sla',
    'rapport',
    'analyse',
    'flere team',
    'multi-team',
    'inbox',
    'ticket',
    'sak',
  ].some((needle) => text.includes(needle))
}

function hasExpertIntent(value?: string): boolean {
  const text = value?.toLowerCase() ?? ''
  return hasAdvancedIntent(value) || [
    'sla',
    'rapport',
    'analyse',
    'flere team',
    'multi-team',
    'multibrand',
    'sso',
    'compliance',
  ].some((needle) => text.includes(needle))
}

function hasCustomIntent(value?: string): boolean {
  const text = value?.toLowerCase() ?? ''
  return [
    'governance',
    'sikkerhet',
    'security',
    'compliance',
    'databehandler',
    'dpa',
    'sso',
    'audit',
    'volum',
    'enterprise',
    'onboarding',
  ].some((needle) => text.includes(needle))
}

function countOnboardingSources({
  website,
  connectors,
}: {
  website?: WebsitePayload
  connectors: ConnectorPick[]
}): number {
  return uniqueConnectorCount(connectors) + (website?.url ? 1 : 0)
}

function buildLocalRecommendationContext({
  organization,
  website,
  connectors,
  locale,
}: {
  organization?: OrganizationPayload
  website?: WebsitePayload
  connectors: ConnectorPick[]
  locale: OnboardingLocale
}): {
  goal?: string
  sourcesText: string
  subject: string
  teamText?: string
  teamQualifier: string
} {
  const copy = ONBOARDING_COPY[locale]
  const subject = organization?.name?.trim() || copy.recommendation.subjectFallback
  const sourceNames = normalizedConnectorPicks(connectors).map((c) => c.label)
  if (website?.url) sourceNames.unshift(websiteHost(website.url))
  const sourceCount = countOnboardingSources({ website, connectors })
  const sourcesText =
    sourceNames.length > 0
      ? humanList(sourceNames, locale)
      : sourceCount > 0
        ? formatOnboardingText(copy.paywall.sources, { count: sourceCount })
        : copy.paywall.noSources
  const employeeCount = organization?.employeeCount
  const teamText =
    employeeCount != null
      ? formatOnboardingText(copy.recommendation.teamQualifier.employees, {
          count: formatOnboardingNumber(employeeCount, locale),
        })
      : organization?.size
        ? organizationSizeLabel(organization.size, locale)
        : undefined

  return {
    goal: cleanGoal(website?.agentBrief),
    sourcesText,
    subject,
    teamText,
    teamQualifier: teamText
      ? locale === 'nb'
        ? `, ${teamText},`
        : `, ${teamText}`
      : '',
  }
}

function cleanGoal(value?: string): string | undefined {
  const cleaned = value?.replace(/\s+/g, ' ').trim()
  if (!cleaned) return undefined
  const withoutTrailing = cleaned.replace(/[.!?]+$/, '')
  return withoutTrailing.length > 120
    ? `${withoutTrailing.slice(0, 117).trim()}...`
    : withoutTrailing
}

function customerGoalPhrase(goal: string, locale: OnboardingLocale): string {
  const lower = goal.toLowerCase()
  if (locale === 'en') {
    if (lower.startsWith('a chatbot')) return `to have ${goal}`
    if (lower.startsWith('chatbot')) return `to have a ${goal}`
    if (lower.startsWith('to ')) return goal
    return `the agent to ${goal}`
  }
  if (lower.startsWith('en chatbot')) return `ha ${goal}`
  if (lower.startsWith('chatbot')) return `ha en ${goal}`
  if (lower.startsWith('å ')) return goal
  return `at agenten skal ${goal}`
}

function humanList(items: string[], locale: OnboardingLocale): string {
  const unique = Array.from(new Set(items.filter(Boolean)))
  if (unique.length === 0) return ''
  if (unique.length === 1) return unique[0]
  const joiner = locale === 'nb' ? 'og' : 'and'
  if (unique.length === 2) return `${unique[0]} ${joiner} ${unique[1]}`
  return `${unique.slice(0, -1).join(', ')} ${joiner} ${unique[unique.length - 1]}`
}

function planFitText(id: PlanId, locale: OnboardingLocale): string {
  return ONBOARDING_COPY[locale].paywall.planFit[id]
}

function organizationSizeLabel(
  size: OrganizationPayload['size'],
  locale: OnboardingLocale,
): string {
  if (!size) return ONBOARDING_COPY[locale].recommendation.teamQualifier.fallback
  return ONBOARDING_COPY[locale].recommendation.teamQualifier[size]
}

function employeeSignal(employeeCount: number, locale: OnboardingLocale): string {
  return locale === 'nb'
    ? `${formatOnboardingNumber(employeeCount, locale)} ansatte`
    : `${formatOnboardingNumber(employeeCount, locale)} employees`
}

function normalizedConnectorPicks(connectors: ConnectorPick[]): ConnectorPick[] {
  const seen = new Set<string>()
  const out: ConnectorPick[] = []
  for (const connector of connectors) {
    const id = normalizeConnectorId(connector.id)
    if (seen.has(id)) continue
    seen.add(id)
    out.push({
      ...connector,
      id,
      label: id === 'microsoft365' ? 'Microsoft 365' : connector.label,
    })
  }
  return out
}

function uniqueConnectorCount(connectors: ConnectorPick[]): number {
  return new Set(connectors.map((connector) => normalizeConnectorId(connector.id))).size
}

function normalizeConnectorId(id: string): string {
  return MICROSOFT_CONNECTOR_ALIASES.has(id) ? 'microsoft365' : id
}

function websiteHost(url: string): string {
  try {
    return new URL(url).hostname.replace(/^www\./, '')
  } catch {
    return 'Nettsted'
  }
}

function nameForId(id: PlanId, locale: OnboardingLocale): string {
  return onboardingPlanName(id, locale)
}
