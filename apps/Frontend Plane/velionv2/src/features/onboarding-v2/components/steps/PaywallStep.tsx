"use client";

/**
 * Step 6 — recommended paywall.
 *
 * Recommendation pipeline: an instant local-engine result renders immediately
 * (never blocks), then the Model Plane recommendation refines it. The user is
 * never locked to the suggestion.
 *
 * Plan model: the org starts on a 14-day Pro trial (set at creation). The
 * paywall lets the user pick; if they pick nothing the trial reverts to Free
 * (enforced by billing-core). Lower tiers are still selectable; if the user
 * chooses one, onboarding keeps only the integrations that plan supports.
 *
 * Full-screen view keeps v1's pricing-board shape: compact plan cards, a single
 * contextual recommendation panel, and no blocking model wait.
 */

import { useEffect, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Check, Star } from "lucide-react";
import { m } from "framer-motion";

import { cn } from "@/lib/utils";
import {
  formatOnboardingText,
  type OnboardingLocale,
  useOnboardingCopy,
} from "../../lib/onboarding-i18n";
import type {
  BrandingSignals,
  ConnectorPick,
  OnboardingMachine,
  OnboardingPlanId,
  OrganizationPayload,
  PlanRecommendation,
  WebsitePayload,
} from "../../lib/onboarding-machine";
import {
  buildLocalSummary,
  buildLocalRecommendation,
  buildOnboardingContext,
  countOnboardingSources,
  employeeSignal,
  nameForId,
  organizationSizeLabel,
  supportsConnectorCount,
  uniqueConnectorCount,
  websiteHost,
} from "../../lib/onboarding-recommendation";
import { displayOrganizationName } from "../../lib/onboarding-evidence";
import { disconnectConnections, recommendPlan } from "../../lib/onboarding-api";
import {
  isPaidPlan,
  organizationExists,
  setOrganizationPlan,
  startCheckout,
  OnboardingServiceError,
} from "../../lib/onboarding-service";
import {
  LeftPane,
  OnboardingTopActions,
  PrimaryButton,
  RightPane,
  StepDescription,
  StepEyebrow,
  StepTitle,
} from "../onboarding-shared";

type PlanId = OnboardingPlanId;

interface Plan {
  id: PlanId;
  name: string;
  monthlyPrice: string;
  yearlyPrice?: string;
  cadence: string;
  description: string;
  features: string[];
  badge?: string;
}

interface RecommendationProofGroups {
  proofPoints: string[];
  scopeSignals: string[];
  opportunities: string[];
  expectedOutcomes: Array<{ label: string; value: string; detail?: string }>;
}

const PLAN_PRICE_META: Record<OnboardingLocale, Record<PlanId, { monthlyPrice: string; yearlyPrice?: string }>> = {
  nb: {
    trial: { monthlyPrice: "0 kr" },
    hobby: { monthlyPrice: "299 kr", yearlyPrice: "239 kr" },
    standard: { monthlyPrice: "999 kr", yearlyPrice: "849 kr" },
    pro: { monthlyPrice: "1 499 kr", yearlyPrice: "1 099 kr" },
    enterprise: { monthlyPrice: "Tilpasset" },
  },
  en: {
    trial: { monthlyPrice: "$0" },
    hobby: { monthlyPrice: "$25", yearlyPrice: "$20" },
    standard: { monthlyPrice: "$99", yearlyPrice: "$85" },
    pro: { monthlyPrice: "$149", yearlyPrice: "$110" },
    enterprise: { monthlyPrice: "Contact sales" },
  },
};

const PLAN_IDS: PlanId[] = ["trial", "hobby", "standard", "pro", "enterprise"];
const DISCONNECT_WAIT_MS = 1_800;

function plansForCopy(
  copy: ReturnType<typeof useOnboardingCopy>["copy"]["paywall"]["plans"],
  locale: OnboardingLocale,
): Plan[] {
  const prices = PLAN_PRICE_META[locale];
  return PLAN_IDS.map((id) => ({
    id,
    ...prices[id],
    name: copy[id].name,
    cadence: copy[id].cadence,
    description: copy[id].description,
    features: [...copy[id].features],
    badge: "badge" in copy[id] ? (copy[id] as { badge?: string }).badge : undefined,
  }));
}

export function PaywallStep({ machine, fullScreen = false }: { machine: OnboardingMachine; fullScreen?: boolean }) {
  const { locale, copy } = useOnboardingCopy();

  const [savingPlan, setSavingPlan] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [userPickedPlan, setUserPickedPlan] = useState<PlanId | null>(null);
  const [billingCycle, setBillingCycle] = useState<"monthly" | "yearly">("monthly");

  const hasSignal =
    Boolean(machine.state.website?.url) ||
    Boolean(machine.state.website?.agentBrief?.trim()) ||
    machine.state.connectors.length > 0 ||
    Boolean(machine.state.organization?.size) ||
    machine.state.organization?.employeeCount != null;

  const onboardingContext = useMemo(
    () => buildOnboardingContext(machine.state, locale),
    [machine.state, locale],
  );
  const contextKey = useMemo(() => recommendationContextKey(onboardingContext), [onboardingContext]);

  // Instant, offline-safe local recommendation for the current onboarding
  // signals. Do not trust a stored recommendation after the user goes back and
  // changes integrations.
  const localRecommendation = useMemo<PlanRecommendation>(
    () =>
      hasSignal
        ? buildLocalRecommendation({
            organization: machine.state.organization,
            website: machine.state.website,
            additionalWebsites: machine.state.additionalWebsites,
            connectors: machine.state.connectors,
            reasonPrefix: copy.paywall.reasonPrefix,
            locale,
          })
        : {
            planId: "trial",
            reason: copy.paywall.fallbackTrialReason,
            summary: copy.paywall.fallbackTrialSummary,
            generatedAt: new Date().toISOString(),
          },
    [
      copy.paywall.fallbackTrialReason,
      copy.paywall.fallbackTrialSummary,
      copy.paywall.reasonPrefix,
      hasSignal,
      locale,
      machine.state.additionalWebsites,
      machine.state.connectors,
      machine.state.organization,
      machine.state.website,
    ],
  );

  // Authoritative refinement from Model Plane — React Query owns the
  // loading/dedup/cancellation that V1 hand-rolled in an effect.
  const recommendQuery = useQuery({
    queryKey: ["onboarding", "recommend-plan", locale, contextKey],
    queryFn: ({ signal }) => recommendPlan(onboardingContext, signal),
    enabled: hasSignal,
    staleTime: Infinity,
    retry: 1,
  });

  const recommendation = recommendQuery.data ?? localRecommendation;
  const loading = recommendQuery.isFetching;
  const recommendedPlanId = recommendation.planId;
  const selected = userPickedPlan ?? recommendedPlanId;
  const connectorAdjustment = connectorAdjustmentForPlan(selected, machine.state.connectors, locale);
  const summary =
    recommendQuery.isFetching && !recommendQuery.data
      ? copy.paywall.modelAnalyzing
      : cleanRecommendationSummary(recommendation.summary) ??
        buildLocalSummary({
          organization: machine.state.organization,
          website: machine.state.website,
          additionalWebsites: machine.state.additionalWebsites,
          connectors: machine.state.connectors,
          planId: recommendedPlanId,
          locale,
        });
  const recommendationProof = useMemo(() => recommendationProofGroups(recommendation), [recommendation]);
  // Persist the active recommendation to the wizard machine (external setter,
  // re-runs only when the authoritative recommendation identity changes).
  useEffect(() => {
    machine.setRecommendation(recommendation);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [recommendation]);

  const handleSelect = (planId: PlanId) => {
    setUserPickedPlan(planId);
    setSaveError(null);
  };

  const handleContinue = async (planId: PlanId) => {
    if (savingPlan) return;
    const organization = machine.state.organization;
    if (!organization?.id) {
      setSaveError(missingOrgError(locale));
      return;
    }
    const adjustment = connectorAdjustmentForPlan(planId, machine.state.connectors, locale);
    setSavingPlan(true);
    setSaveError(null);
    const disconnectStatus = await disconnectRemovedConnectors(adjustment.removed);
    for (const connector of adjustment.removed) {
      machine.removeConnector(connector.id);
    }
    const onboardingMeta = {
      selected_plan_id: planId,
      selected_plan_name: nameForId(planId, locale),
      billing_cycle: billingCycle,
      recommendation,
      source_count: countOnboardingSources({
        website: machine.state.website,
        additionalWebsites: machine.state.additionalWebsites,
        connectors: adjustment.kept,
      }),
      disconnected_connector_ids: adjustment.removed.map((connector) => connector.id),
      connection_adjustment_status: disconnectStatus,
    };
    try {
      const exists = await organizationExists(organization.id);
      if (!exists) {
        machine.invalidateOrganization();
        return;
      }

      if (isPaidPlan(planId)) {
        const { url } = await startCheckout(organization.id, planId, {
          successUrl: `${window.location.origin}/onboarding?checkout=success`,
          cancelUrl: `${window.location.origin}/onboarding?checkout=cancel`,
        });
        if (url) {
          window.location.href = url;
          return;
        }
        await setOrganizationPlan(organization.id, planId, onboardingMeta);
      } else {
        await setOrganizationPlan(organization.id, planId, onboardingMeta);
      }
      machine.setOrganization({ ...organization, plan: planId });
      machine.goTo("assembly");
    } catch (err) {
      if (err instanceof OnboardingServiceError && err.status === 404) {
        machine.invalidateOrganization();
        return;
      }
      setSaveError(err instanceof OnboardingServiceError ? err.message : planSaveError(locale));
    } finally {
      setSavingPlan(false);
    }
  };

  const plans = plansForCopy(copy.paywall.plans, locale);

  if (fullScreen) {
    return (
      <FullScreenPaywall
        machine={machine}
        locale={locale}
        copy={copy.paywall}
        plans={plans}
        loading={loading}
        selected={selected}
        recommendedPlanId={recommendedPlanId}
        summary={summary}
        savingPlan={savingPlan}
        saveError={saveError}
        billingCycle={billingCycle}
        onBillingCycle={setBillingCycle}
        downgradeNotice={connectorAdjustment.notice}
        onSelect={handleSelect}
        onContinue={handleContinue}
      />
    );
  }

  return (
    <>
      <LeftPane machine={machine}>
        <StepEyebrow>{copy.paywall.eyebrow}</StepEyebrow>
        <StepTitle>{copy.paywall.title}</StepTitle>
        <StepDescription>{loading ? copy.paywall.loadingReason : recommendation?.reason ?? copy.paywall.defaultReason}</StepDescription>

        <div className="rounded-2xl border border-[#E7E5E4] bg-[#FAFAFA] p-4">
          <p className="font-inter text-[10px] font-semibold uppercase tracking-[0.16em] text-[#777169]">{copy.paywall.why}</p>
          <p className="mt-2 font-inter text-[13px] leading-5 text-[#292524]">{summary}</p>
          <RecommendationProof proof={recommendationProof} locale={locale} compact />
          <SignalStrip machine={machine} locale={locale} signalAgentTask={copy.paywall.signalAgentTask} sourcesTemplate={copy.paywall.sources} />
        </div>

        {(saveError || connectorAdjustment.notice) && (
          <p className={cn("font-inter text-[12px] leading-5", saveError ? "text-[#B42318]" : "text-[#6B6660]")}>
            {saveError ?? connectorAdjustment.notice}
          </p>
        )}

        <PrimaryButton onClick={() => void handleContinue(selected)} disabled={savingPlan}>
          {savingPlan ? copy.paywall.continueToSetup : formatOnboardingText(copy.paywall.choose, { plan: nameForId(selected, locale) })}
        </PrimaryButton>
      </LeftPane>

      <RightPane>
        <div className="relative size-full overflow-y-auto bg-[#F5F5F5] p-5 text-[#0C0A09]">
          <div className="mb-4 flex items-start justify-between gap-4">
            <div>
              <p className="font-inter text-[10px] font-semibold uppercase tracking-[0.16em] text-[#777169]">{copy.paywall.plansTitle}</p>
              <p className="mt-1 text-[clamp(24px,2.2vw,34px)] font-normal leading-[1.05] text-[#0C0A09]" style={{ fontFamily: "var(--font-geist-sans), var(--font-inter), Arial, sans-serif" }}>
                {copy.paywall.plansHeading}
              </p>
            </div>
            <span className="rounded-full bg-[#F0EFED] px-3 py-1 font-inter text-[10px] font-semibold uppercase tracking-[0.12em] text-[#292524]">
              {loading ? copy.paywall.analyzing : formatOnboardingText(copy.paywall.recommended, { plan: nameForId(recommendedPlanId, locale) })}
            </span>
          </div>
          <PlanGrid plans={plans} copy={copy.paywall} selected={selected} recommendedPlanId={recommendedPlanId} onSelect={handleSelect} billingCycle={billingCycle} disabled={savingPlan} />
        </div>
        <PaywallRingStyle />
      </RightPane>
    </>
  );
}

function FullScreenPaywall({
  machine,
  locale,
  copy,
  plans,
  loading,
  selected,
  recommendedPlanId,
  summary,
  savingPlan,
  saveError,
  billingCycle,
  onBillingCycle,
  downgradeNotice,
  onSelect,
  onContinue,
}: {
  machine: OnboardingMachine;
  locale: OnboardingLocale;
  copy: ReturnType<typeof useOnboardingCopy>["copy"]["paywall"];
  plans: Plan[];
  loading: boolean;
  selected: PlanId;
  recommendedPlanId: PlanId;
  summary: string;
  savingPlan: boolean;
  saveError: string | null;
  billingCycle: "monthly" | "yearly";
  onBillingCycle: (value: "monthly" | "yearly") => void;
  downgradeNotice: string | null;
  onSelect: (planId: PlanId) => void;
  onContinue: (planId: PlanId) => Promise<void>;
}) {
  const choosePlan = (planId: PlanId) => {
    onSelect(planId);
    void onContinue(planId);
  };

  return (
    <div className="relative isolate min-h-[100dvh] w-[100dvw] overflow-x-hidden overflow-y-auto bg-[#F7F7F6] text-[#0C0A09]">
      <main className="relative z-10 mx-auto flex min-h-[100dvh] w-full max-w-[1320px] flex-col justify-start px-4 py-7 sm:px-6 lg:px-8 xl:py-9">
        <OnboardingTopActions machine={machine} fullScreen />

        <div className="mb-6 flex flex-col gap-5 md:flex-row md:items-end md:justify-between">
          <div>
            <PaywallMatchTitle machine={machine} locale={locale} fallbackTitle={copy.fullTitle} />
            <p className="mt-3 font-inter text-[15px] font-medium text-[#777169]">{copy.fullSubtitle}</p>
          </div>
          <BillingToggle value={billingCycle} onChange={onBillingCycle} monthlyLabel={copy.monthly} yearlyLabel={copy.yearly} badgeLabel={copy.trialBadge} />
        </div>

        <PlanGrid plans={plans} copy={copy} selected={selected} recommendedPlanId={recommendedPlanId} onSelect={onSelect} onChoose={choosePlan} billingCycle={billingCycle} disabled={savingPlan} fullScreen />

        <div className="mt-5 rounded-[16px] bg-white/75 px-5 py-4 text-center shadow-[inset_0_0_0_1px_rgba(231,229,228,0.86)]">
          <p className="font-inter text-[15px] font-semibold text-[#191716]">
            {loading ? copy.modelAnalyzing : formatOnboardingText(copy.recommendedShort, { plan: nameForId(recommendedPlanId, locale) })}
          </p>
          <p className="mx-auto mt-2 max-w-[760px] font-inter text-[14px] leading-6 text-[#777169]">{summary}</p>
          <div className="mt-3 flex justify-center">
            <SignalStrip machine={machine} locale={locale} signalAgentTask={copy.signalAgentTask} sourcesTemplate={copy.sources} />
          </div>
        </div>

        {(saveError || downgradeNotice) && (
          <p className={cn("mt-4 text-center font-inter text-[13px] leading-5", saveError ? "text-[#B42318]" : "text-[#6B6660]")}>
            {saveError ?? downgradeNotice}
          </p>
        )}

        <div className="mt-5 flex items-center justify-center gap-2">
          <button type="button" onClick={() => choosePlan("trial")} disabled={savingPlan} className="h-10 rounded-[10px] border border-[#E7E5E4] bg-white px-5 font-inter text-[14px] font-semibold text-[#191716] transition-colors hover:border-[#D6D3D1] disabled:cursor-not-allowed disabled:opacity-60">
            {copy.skipToSetup}
          </button>
          <button type="button" onClick={() => choosePlan(selected)} disabled={savingPlan} className="h-10 rounded-[10px] border border-[#191716] bg-[#191716] px-5 font-inter text-[14px] font-semibold text-white transition-opacity hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-50">
            {copy.continueToSetup}
          </button>
        </div>
      </main>
      <PaywallRingStyle />
    </div>
  );
}

function PaywallMatchTitle({
  machine,
  locale,
  fallbackTitle,
}: {
  machine: OnboardingMachine;
  locale: OnboardingLocale;
  fallbackTitle: string;
}) {
  const orgName = machine.state.organization?.name?.trim();
  const logoUrl = brandLogoUrl(machine.state.website?.branding);
  const title = orgName ? organizationMatchTitle(orgName, locale) : fallbackTitle;

  return (
    <div className="flex max-w-[980px] items-center gap-4">
      {logoUrl && (
        <span className="grid size-14 shrink-0 place-items-center overflow-hidden rounded-[14px] border border-[#E7E5E4] bg-white shadow-sm sm:size-16">
          {/* eslint-disable-next-line @next/next/no-img-element */}
          <img
            src={logoUrl}
            alt=""
            className="max-h-12 max-w-12 object-contain sm:max-h-14 sm:max-w-14"
            onError={(event) => {
              event.currentTarget.style.display = "none";
            }}
          />
        </span>
      )}
      <h1
        className="min-w-0 text-[clamp(36px,4.8vw,64px)] font-normal leading-[1.02] tracking-normal text-[#191716]"
        style={{ fontFamily: "var(--font-geist-sans), var(--font-inter), Arial, sans-serif" }}
      >
        {title}
      </h1>
    </div>
  );
}

function BillingToggle({
  value,
  onChange,
  monthlyLabel,
  yearlyLabel,
  badgeLabel,
}: {
  value: "monthly" | "yearly";
  onChange: (value: "monthly" | "yearly") => void;
  monthlyLabel: string;
  yearlyLabel: string;
  badgeLabel: string;
}) {
  const yearly = value === "yearly";
  return (
    <div className="flex flex-wrap items-center justify-end gap-2 font-inter text-[14px] font-medium text-[#191716]">
      <button type="button" onClick={() => onChange("monthly")} className={yearly ? "text-[#777169]" : "text-[#191716]"}>
        {monthlyLabel}
      </button>
      <button
        type="button"
        role="switch"
        aria-checked={yearly}
        aria-label={`${monthlyLabel} / ${yearlyLabel}`}
        onClick={() => onChange(yearly ? "monthly" : "yearly")}
        className="relative h-6 w-11 shrink-0 rounded-full bg-[#E5E5E7] transition-colors data-[checked=true]:bg-[#D8F8E7]"
        data-checked={yearly}
      >
        <span className={cn("absolute left-1 top-1 size-4 rounded-full bg-white shadow-sm transition-transform", yearly ? "translate-x-5" : "translate-x-0")} />
      </button>
      <button type="button" onClick={() => onChange("yearly")} className={yearly ? "text-[#191716]" : "text-[#777169]"}>
        {yearlyLabel}
      </button>
      <span className="ml-1 rounded-full bg-[#DDFBEA] px-3 py-1 font-inter text-[12px] font-semibold text-[#1F5135]">{badgeLabel}</span>
    </div>
  );
}

function PlanGrid({
  plans,
  copy,
  selected,
  recommendedPlanId,
  onSelect,
  onChoose,
  billingCycle = "monthly",
  disabled = false,
  fullScreen = false,
}: {
  plans: Plan[];
  copy: { selected: string; choosePlan: string; recommended: string; trialRail: { currentTrial: string } };
  selected: PlanId;
  recommendedPlanId: PlanId;
  onSelect: (planId: PlanId) => void;
  onChoose?: (planId: PlanId) => void;
  billingCycle?: "monthly" | "yearly";
  disabled?: boolean;
  fullScreen?: boolean;
}) {
  return (
    <div className={fullScreen ? "grid grid-cols-1 items-stretch gap-3 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-5" : "grid grid-cols-2 gap-3"}>
      {plans.map((plan) => (
        <PlanCard
          key={plan.id}
          plan={plan}
          copy={copy}
          selected={selected === plan.id}
          recommended={recommendedPlanId === plan.id}
          isTrial={plan.id === "trial"}
          onSelect={() => onSelect(plan.id)}
          onChoose={onChoose ? () => onChoose(plan.id) : undefined}
          billingCycle={billingCycle}
          disabled={disabled}
          fullScreen={fullScreen}
        />
      ))}
    </div>
  );
}

function PlanCard({
  plan,
  copy,
  selected,
  recommended,
  isTrial,
  onSelect,
  onChoose,
  billingCycle = "monthly",
  disabled = false,
  fullScreen = false,
}: {
  plan: Plan;
  copy: { selected: string; choosePlan: string; recommended: string; trialRail: { currentTrial: string } };
  selected: boolean;
  recommended: boolean;
  isTrial: boolean;
  onSelect: () => void;
  onChoose?: () => void;
  billingCycle?: "monthly" | "yearly";
  disabled?: boolean;
  fullScreen?: boolean;
}) {
  const price = billingCycle === "yearly" && plan.yearlyPrice ? plan.yearlyPrice : plan.monthlyPrice;
  return (
    <div className={cn("relative h-full p-[2px]", fullScreen ? "rounded-[20px]" : "rounded-[24px]", selected ? "" : "bg-transparent")}>
      {selected && (
        <m.div
          layoutId="selected-paywall-card-ring"
          className={cn("absolute inset-0 shadow-[0_10px_24px_rgba(12,10,9,0.10)]", fullScreen ? "rounded-[20px]" : "rounded-[24px]")}
          style={{
            background: "linear-gradient(120deg, #A7E5D3, #F4C5A8, #C8B8E0, #A8C8E8, #A7E5D3)",
            backgroundSize: "260% 260%",
            animation: "verevon-paywall-ring 8s ease-in-out infinite",
          }}
          transition={{ type: "spring", stiffness: 420, damping: 38, mass: 0.8 }}
        />
      )}
      <article
        onClick={() => {
          if (!disabled) onSelect();
        }}
        className={cn(
          "relative z-10 flex h-full cursor-pointer flex-col border bg-white text-left transition duration-200",
          fullScreen ? "min-h-[350px] rounded-[18px] p-4" : "min-h-[184px] rounded-2xl p-4",
          selected ? "border-transparent shadow-[0_12px_28px_rgba(12,10,9,0.08)]" : "border-[#E7E5E4] hover:border-[#D6D3D1]",
        )}
        aria-current={selected ? "true" : undefined}
      >
        <div className="flex min-h-[38px] items-start justify-between gap-2">
          <h2 className={cn("font-inter font-semibold leading-tight text-[#191716]", fullScreen ? "text-[18px]" : "text-[20px]")}>{plan.name}</h2>
          <div className="flex flex-wrap justify-end gap-1.5">
            {isTrial && (
              <span className="rounded-full bg-[#DDFBEA] px-2 py-1 font-inter text-[10px] font-semibold text-[#1F5135]">{copy.trialRail.currentTrial}</span>
            )}
            {recommended && (
              <span className="inline-flex items-center gap-1 rounded-full bg-[#F0EFED] px-2 py-1 font-inter text-[10px] font-semibold text-[#191716]">
                <Star className="size-3 fill-[#191716] text-[#191716]" />
                {formatOnboardingText(copy.recommended, { plan: "" }).trim() || copy.recommended}
              </span>
            )}
            {plan.badge && !isTrial && (
              <span className="rounded-full bg-[#DDFBEA] px-2 py-1 font-inter text-[10px] font-semibold text-[#1F5135]">{plan.badge}</span>
            )}
          </div>
        </div>

        <div className={cn("flex min-h-[38px] items-end gap-2", fullScreen ? "mt-4" : "mt-5")}>
          <span className={cn("whitespace-nowrap font-inter font-bold leading-none tracking-normal text-[#191716]", fullScreen ? "text-[29px]" : "text-[32px]")}>{price}</span>
          <span className={cn("pb-1 font-inter font-medium text-[#191716]", fullScreen ? "text-[14px]" : "text-[16px]")}>{plan.cadence}</span>
        </div>

        <p className={cn("font-inter font-medium text-[#777169]", fullScreen ? "mt-4 min-h-[54px] text-[13px] leading-[1.45]" : "mt-5 min-h-[60px] text-[14px] leading-5")}>{plan.description}</p>

        <ul className={cn(fullScreen ? "mt-4 space-y-2.5" : "mt-5 min-h-[132px] space-y-3")}>
          {plan.features.map((feature) => (
            <li key={feature} className={cn("flex items-start font-inter font-semibold text-[#292524]", fullScreen ? "gap-2.5 text-[12.5px] leading-[1.35]" : "gap-3 text-[14px] leading-5")}>
              <span className={cn("mt-0.5 flex shrink-0 items-center justify-center rounded-full bg-[#191716]", fullScreen ? "size-4" : "size-5")} aria-hidden="true">
                <Check className="size-3 text-white" strokeWidth={3} />
              </span>
              {feature}
            </li>
          ))}
        </ul>

        <button
          type="button"
          disabled={disabled}
          onClick={(event) => {
            event.stopPropagation();
            if (disabled) return;
            if (onChoose) onChoose();
            else onSelect();
          }}
          className={cn("mt-auto rounded-[10px] border border-[#E7E5E4] bg-white px-4 font-inter font-semibold text-[#191716] transition-colors hover:border-[#D6D3D1] disabled:cursor-not-allowed disabled:opacity-60", fullScreen ? "h-9 text-[13px]" : "h-10 text-[14px]")}
        >
          {selected ? copy.selected : copy.choosePlan}
        </button>
      </article>
    </div>
  );
}

function SignalStrip({
  machine,
  locale,
  signalAgentTask,
  sourcesTemplate,
}: {
  machine: OnboardingMachine;
  locale: OnboardingLocale;
  signalAgentTask: string;
  sourcesTemplate: string;
}) {
  const organization: OrganizationPayload | undefined = machine.state.organization;
  const website: WebsitePayload | undefined = machine.state.website;
  const connectors: ConnectorPick[] = machine.state.connectors;
  const sourceCount = countOnboardingSources({
    website,
    additionalWebsites: machine.state.additionalWebsites,
    connectors,
  });
  const signals = [
    organization?.employeeCount != null
      ? employeeSignal(organization.employeeCount, locale)
      : organization?.size
        ? organizationSizeLabel(organization.size, locale)
        : null,
    sourceCount > 0 ? formatOnboardingText(sourcesTemplate, { count: sourceCount }) : null,
    website?.url ? websiteHost(website.url) : null,
    website?.agentBrief?.trim() ? signalAgentTask : null,
  ].filter((s): s is string => Boolean(s));

  if (signals.length === 0) return null;
  return (
    <div className="mt-3 flex flex-wrap gap-1.5">
      {signals.map((signal) => (
        <span key={signal} className="rounded-full bg-[#F0EFED] px-2 py-1 font-inter text-[10px] font-medium text-[#4E4E4E]">
          {signal}
        </span>
      ))}
    </div>
  );
}

function RecommendationProof({
  proof,
  locale,
  compact = false,
}: {
  proof: RecommendationProofGroups;
  locale: OnboardingLocale;
  compact?: boolean;
}) {
  const sections = [
    {
      label: locale === "nb" ? "Omfang" : "Scope",
      items: proof.scopeSignals,
    },
    {
      label: locale === "nb" ? "Det vi fant" : "What we found",
      items: proof.proofPoints,
    },
    {
      label: locale === "nb" ? "Første forbedringer" : "First improvements",
      items: proof.opportunities,
    },
  ].filter((section) => section.items.length > 0);

  if (sections.length === 0 && proof.expectedOutcomes.length === 0) return null;

  return (
    <div className="mt-4 border-t border-[#E7E5E4] pt-3 text-left">
      <p className="font-inter text-[10px] font-semibold uppercase tracking-[0.16em] text-[#777169]">
        {locale === "nb" ? "Verevon ser allerede" : "Verevon already sees"}
      </p>
      {sections.length > 0 && (
        <div className={cn("mt-3 grid gap-3", compact ? "grid-cols-1" : "md:grid-cols-3")}>
          {sections.map((section) => (
            <div key={section.label}>
              <p className="font-inter text-[11px] font-semibold text-[#191716]">{section.label}</p>
              <ul className="mt-2 space-y-1.5">
                {section.items.slice(0, compact ? 2 : 3).map((item) => (
                  <li key={item} className="flex gap-2 font-inter text-[12px] leading-5 text-[#6B6660]">
                    <span className="mt-[8px] size-1.5 shrink-0 rounded-full bg-[#191716]" aria-hidden="true" />
                    <span>{item}</span>
                  </li>
                ))}
              </ul>
            </div>
          ))}
        </div>
      )}
      {proof.expectedOutcomes.length > 0 && (
        <div className={cn("mt-3 grid gap-3", compact ? "grid-cols-1" : "sm:grid-cols-3")}>
          {proof.expectedOutcomes.slice(0, 3).map((outcome) => (
            <div key={`${outcome.label}-${outcome.value}`} className="border-l border-[#D6D3D1] pl-3">
              <p className="font-inter text-[17px] font-semibold leading-tight text-[#191716]">{outcome.value}</p>
              <p className="mt-1 font-inter text-[11px] font-semibold text-[#4E4A45]">{outcome.label}</p>
              {outcome.detail && (
                <p className="mt-1 font-inter text-[11px] leading-4 text-[#777169]">{outcome.detail}</p>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function recommendationProofGroups(recommendation: PlanRecommendation): RecommendationProofGroups {
  return {
    proofPoints: cleanProofList(recommendation.proofPoints),
    scopeSignals: cleanProofList(recommendation.scopeSignals),
    opportunities: cleanProofList(recommendation.opportunities),
    expectedOutcomes: cleanExpectedOutcomes(recommendation.expectedOutcomes),
  };
}

function cleanProofList(values?: string[]): string[] {
  return (values ?? [])
    .map((value) => value.replace(/\s+/g, " ").trim())
    .filter((value, index, array) => value.length >= 6 && array.indexOf(value) === index)
    .slice(0, 4);
}

function cleanExpectedOutcomes(values?: PlanRecommendation["expectedOutcomes"]): RecommendationProofGroups["expectedOutcomes"] {
  return (values ?? [])
    .map((value) => ({
      label: value.label?.replace(/\s+/g, " ").trim(),
      value: value.value?.replace(/\s+/g, " ").trim(),
      detail: value.detail?.replace(/\s+/g, " ").trim(),
    }))
    .filter((value): value is { label: string; value: string; detail: string | undefined } => Boolean(value.label && value.value))
    .slice(0, 3);
}

function PaywallRingStyle() {
  return (
    <style>{`
      @keyframes verevon-paywall-ring {
        0% { background-position: 0% 50%; }
        50% { background-position: 100% 50%; }
        100% { background-position: 0% 50%; }
      }
    `}</style>
  );
}

async function disconnectRemovedConnectors(
  removed: ConnectorPick[],
): Promise<"not_required" | "completed" | "partial" | "requested" | "failed"> {
  if (removed.length === 0) return "not_required";

  const request = disconnectConnections(removed.map((connector) => connector.id))
    .then((result) => (result.failed && result.failed.length > 0 ? "partial" : "completed"))
    .catch(() => "failed" as const);
  const timeout = new Promise<"requested">((resolve) => {
    window.setTimeout(() => resolve("requested"), DISCONNECT_WAIT_MS);
  });

  return Promise.race([request, timeout]);
}

function cleanRecommendationSummary(value?: string): string | null {
  const text = value?.trim();
  if (!text || text.length < 36) return null;
  const lower = text.toLowerCase();
  const genericFragments = [
    "select the plan",
    "choose the plan",
    "you can change plan",
    "velg planen",
    "du kan endre plan",
  ];
  if (text.length < 120 && genericFragments.some((fragment) => lower.includes(fragment))) {
    return null;
  }
  return text;
}

interface ConnectorAdjustment {
  kept: ConnectorPick[];
  removed: ConnectorPick[];
  notice: string | null;
}

function connectorAdjustmentForPlan(
  planId: PlanId,
  connectors: ConnectorPick[],
  locale: OnboardingLocale,
): ConnectorAdjustment {
  if (supportsConnectorCount(planId, uniqueConnectorCount(connectors))) {
    return { kept: connectors, removed: [], notice: null };
  }
  const limit = connectorLimitForPlan(planId);
  if (limit === null) return { kept: connectors, removed: [], notice: null };
  const kept = connectors.slice(0, limit);
  const removed = connectors.slice(limit);
  return {
    kept,
    removed,
    notice: removed.length > 0 ? connectorAdjustmentNotice(planId, kept, removed, locale) : null,
  };
}

function connectorLimitForPlan(planId: PlanId): number | null {
  switch (planId) {
    case "hobby":
      return 1;
    default:
      return null;
  }
}

function connectorAdjustmentNotice(
  planId: PlanId,
  kept: ConnectorPick[],
  removed: ConnectorPick[],
  locale: OnboardingLocale,
): string {
  const plan = nameForId(planId, locale);
  const keptText = connectorListText(kept, locale);
  const removedText = connectorListText(removed, locale);
  return locale === "nb"
    ? `${plan} inkluderer én aktiv integrasjon i onboarding. Fortsetter du, beholder vi ${keptText} og kobler fra ${removedText}; de kan legges til igjen senere.`
    : `${plan} includes one active onboarding integration. If you continue, we will keep ${keptText} and disconnect ${removedText}; you can add them again later.`;
}

function connectorListText(connectors: ConnectorPick[], locale: OnboardingLocale): string {
  const names = connectors.map((connector) => connector.label || connector.id);
  if (names.length === 0) return locale === "nb" ? "ingen" : "none";
  if (names.length === 1) return names[0];
  const joiner = locale === "nb" ? "og" : "and";
  if (names.length === 2) return `${names[0]} ${joiner} ${names[1]}`;
  return `${names.slice(0, -1).join(", ")} ${joiner} ${names[names.length - 1]}`;
}

function recommendationContextKey(context: ReturnType<typeof buildOnboardingContext>): string {
  return JSON.stringify({
    organization: context.organization,
    website: context.website,
    connectors: context.connectors,
    sourceCount: context.sourceCount,
    locale: context.locale,
  });
}

function organizationMatchTitle(orgName: string, locale: OnboardingLocale): string {
  const displayName = displayOrganizationName(orgName);
  if (locale === "en") {
    return displayName.endsWith("s") ? `${displayName}' best fit` : `${displayName}'s best fit`;
  }
  return /[sxz]$/i.test(displayName) ? `${displayName}’ beste match` : `${displayName}s beste match`;
}

function brandLogoUrl(branding: BrandingSignals | undefined): string | null {
  const raw = branding?.logoCandidate ?? branding?.favicon ?? branding?.appleTouchIcon;
  if (!raw) return null;
  try {
    const url = new URL(raw);
    return url.protocol === "http:" || url.protocol === "https:" ? url.toString() : null;
  } catch {
    return null;
  }
}

function missingOrgError(locale: OnboardingLocale): string {
  return locale === "nb"
    ? "Organisasjonen mangler i onboarding-økten. Gå tilbake og opprett den først."
    : "The organization is missing from this onboarding session. Go back and create it first.";
}

function planSaveError(locale: OnboardingLocale): string {
  return locale === "nb"
    ? "Kunne ikke lagre planen. Prøv igjen."
    : "Could not save the plan. Try again.";
}
