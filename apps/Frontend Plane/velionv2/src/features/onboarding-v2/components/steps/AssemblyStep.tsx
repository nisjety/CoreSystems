"use client";

/**
 * Step 7 — "assembling your dashboard" finale. Ticks through the setup items,
 * shows the personalized proof collected during onboarding, and waits for the
 * user before marking onboarding complete and opening the dashboard.
 */

import { useEffect, useMemo, useState } from "react";
import { useRouter } from "next/navigation";
import type { Route } from "next";
import { Sparkles } from "lucide-react";

import {
  allOnboardingWebsites,
  displayOrganizationName,
  resolveBrandThemeColor,
} from "../../lib/onboarding-evidence";
import { useOnboardingCopy } from "../../lib/onboarding-i18n";
import type { OnboardingMachine, OnboardingState, PlanRecommendation, WebsitePayload } from "../../lib/onboarding-machine";
import {
  buildLocalRecommendation,
  countOnboardingSources,
  nameForId,
  websiteHost,
} from "../../lib/onboarding-recommendation";
import { completeOnboarding } from "../../lib/onboarding-service";
import {
  LeftPane,
  PrimaryButton,
  RightPane,
  StepDescription,
  StepEyebrow,
  StepTitle,
} from "../onboarding-shared";

const ASSEMBLY_TICK_MS = 260;

export function AssemblyStep({ machine }: { machine: OnboardingMachine }) {
  const { locale, copy } = useOnboardingCopy();
  const router = useRouter();
  const [progress, setProgress] = useState<{ key: string; completed: number }>({ key: "", completed: 0 });
  const [finishing, setFinishing] = useState(false);
  const recommendation = useMemo<PlanRecommendation>(
    () =>
      machine.state.recommendation ??
      buildLocalRecommendation({
        organization: machine.state.organization,
        website: machine.state.website,
        additionalWebsites: machine.state.additionalWebsites,
        connectors: machine.state.connectors,
        reasonPrefix: copy.paywall.reasonPrefix,
        locale,
      }),
    [
      copy.paywall.reasonPrefix,
      locale,
      machine.state.additionalWebsites,
      machine.state.connectors,
      machine.state.organization,
      machine.state.recommendation,
      machine.state.website,
    ],
  );
  const ticks = useMemo(
    () => personalizedAssemblyTicks(machine, recommendation, copy.assembly.ticks, locale),
    [copy.assembly.ticks, locale, machine, recommendation],
  );
  const tickKey = ticks.join("\u001f");
  const completed = progress.key === tickKey ? progress.completed : 0;
  const outlook = useMemo(
    () => buildAssemblyOutlook(machine, recommendation, locale),
    [locale, machine, recommendation],
  );
  const proof = useMemo(
    () => buildAssemblyProof(machine, recommendation, locale),
    [locale, machine, recommendation],
  );
  const completionMetadata = useMemo(
    () => buildOnboardingCompletionMetadata(machine.state, recommendation),
    [machine.state, recommendation],
  );
  const orgId = machine.state.organization?.id;
  const plan = machine.state.organization?.plan;
  const resetMachine = machine.reset;

  useEffect(() => {
    const timers: number[] = [];
    Array.from({ length: ticks.length }).forEach((_, index) => {
      timers.push(
        window.setTimeout(
          () => setProgress({ key: tickKey, completed: index + 1 }),
          ASSEMBLY_TICK_MS * (index + 1),
        ),
      );
    });
    return () => timers.forEach((timer) => window.clearTimeout(timer));
  }, [tickKey, ticks.length]);

  const openDashboard = async () => {
    if (finishing) return;
    setFinishing(true);
    await completeOnboarding({
      plan,
      orgId,
      source: "wizard-v2",
      metadata: completionMetadata,
    }).catch(() => undefined);
    resetMachine();
    router.push("/dashboard" as Route);
    router.refresh();
  };

  return (
    <>
      <LeftPane machine={machine}>
        <StepEyebrow>{copy.assembly.eyebrow}</StepEyebrow>
        <StepTitle>
          <span>{outlook.title}</span>
        </StepTitle>
        <StepDescription>{outlook.description}</StepDescription>
        <ul className="flex flex-col gap-2">
          {ticks.map((label, index) => {
            const done = index < completed;
            const active = index === completed;
            return (
              <li key={label} className="flex items-center gap-3 font-inter text-[13px]">
                <span
                  className={
                    done
                      ? "flex size-5 items-center justify-center rounded-full border border-[#1F1B17] bg-[#1F1B17] text-white"
                      : active
                        ? "flex size-5 items-center justify-center rounded-full border border-[#1F1B17] bg-white text-[#1F1B17]"
                        : "flex size-5 items-center justify-center rounded-full border border-[#D6D2CB] bg-white text-transparent"
                  }
                >
                  {done ? "✓" : active ? "·" : ""}
                </span>
                <span className={done ? "text-[#1F1B17]" : "text-[#A09890]"}>{label}</span>
              </li>
            );
          })}
        </ul>
        <PrimaryButton onClick={() => void openDashboard()} disabled={finishing}>
          {finishing
            ? locale === "nb"
              ? "Åpner dashboard"
              : "Opening dashboard"
            : locale === "nb"
              ? "Fortsett til dashboard"
              : "Open dashboard"}
        </PrimaryButton>
      </LeftPane>

      <RightPane>
        <div className="size-full overflow-y-auto bg-[#FCFCFD] p-6">
          <div className="min-h-full rounded-[24px] border border-[#E6E6E8] bg-white p-5 shadow-[0_18px_38px_rgba(20,21,24,0.08)]">
            <div className="flex items-center gap-3 border-b border-[#ECECF1] pb-4">
              <Sparkles className="size-5 text-[#5E6AD2]" />
              <span className="text-sm font-semibold text-[#26282f]">{outlook.dashboardTitle}</span>
            </div>
            <div className="mt-5">
              <p className="font-inter text-[10px] font-semibold uppercase tracking-[0.16em] text-[#7d828a]">
                {locale === "nb" ? "Første lanseringsbilde" : "First launch outlook"}
              </p>
              <h2 className="mt-2 text-[28px] font-semibold leading-tight text-[#191716]">
                {outlook.headline}
              </h2>
              <p className="mt-3 max-w-[560px] font-inter text-[13px] leading-5 text-[#6B6660]">
                {outlook.summary}
              </p>
            </div>
            <AssemblyProofPanel proof={proof} locale={locale} />
            <div className="mt-6 flex justify-end border-t border-[#ECECF1] pt-4">
              <button
                type="button"
                onClick={() => void openDashboard()}
                disabled={finishing}
                className="h-10 rounded-[10px] border border-[#191716] bg-[#191716] px-5 font-inter text-[13px] font-semibold text-white transition-opacity hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-50"
              >
                {finishing
                  ? locale === "nb"
                    ? "Åpner dashboard"
                    : "Opening dashboard"
                  : locale === "nb"
                    ? "Fortsett"
                    : "Continue"}
              </button>
            </div>
          </div>
        </div>
      </RightPane>
    </>
  );
}

interface AssemblyOutlook {
  title: string;
  description: string;
  dashboardTitle: string;
  headline: string;
  summary: string;
  proofPoints: string[];
  opportunities: string[];
  expectedOutcomes: Array<{ label: string; value: string; detail?: string }>;
}

interface AssemblyProof {
  sections: Array<{ label: string; items: string[] }>;
  proofGroups: Array<{ label: string; items: string[] }>;
  expectedOutcomes: Array<{ label: string; value: string; detail?: string }>;
}

function buildAssemblyProof(
  machine: OnboardingMachine,
  recommendation: PlanRecommendation,
  locale: "nb" | "en",
): AssemblyProof {
  const state = machine.state;
  const websites = allOnboardingWebsites(state);
  const orgName = displayOrganizationName(state.organization?.name);
  const brandColor = resolveBrandThemeColor(state.website?.branding);
  const connectorNames = state.connectors.map((connector) => connector.label).filter(Boolean);
  const proofPoints = cleanTextList(recommendation.proofPoints);
  const scopeSignals = cleanTextList(recommendation.scopeSignals);
  const opportunities = cleanTextList(recommendation.opportunities);
  const expectedOutcomes = cleanOutcomeList(recommendation.expectedOutcomes);
  const pageCount = websites.reduce((sum, site) => sum + (site.crawlEvidence?.pages ?? 0), 0);
  const elementCount = websites.reduce((sum, site) => sum + (site.crawlEvidence?.elements ?? 0), 0);
  const graphSignals = proofPoints.filter((item) => /graf|graph|node|relasjon|relationship/i.test(item));
  const proofOfConcept = recommendation.proofOfConcept;
  const sections = [
    {
      label: locale === "nb" ? "Identitet" : "Identity",
      items: proofOfConcept?.companyIdentity ?? [
        orgName || (locale === "nb" ? "Organisasjon valgt" : "Organization selected"),
        websites[0]?.url ? websiteHost(websites[0].url) : null,
        state.organization?.employeeCount != null
          ? locale === "nb"
            ? `${state.organization.employeeCount} ansatte`
            : `${state.organization.employeeCount} employees`
          : null,
        state.brandTheme?.mode === "brand"
          ? locale === "nb"
            ? `Merketema aktivt (${brandColor})`
            : `Brand theme active (${brandColor})`
          : null,
      ],
    },
    {
      label: locale === "nb" ? "Lært så langt" : "Learned so far",
      items: proofOfConcept?.learnedSignals ?? [
        websites.length > 0
          ? locale === "nb"
            ? `${websites.length} nettside${websites.length === 1 ? "" : "r"}`
            : `${websites.length} website${websites.length === 1 ? "" : "s"}`
          : null,
        pageCount > 0 ? (locale === "nb" ? `${pageCount} crawlede sider` : `${pageCount} crawled pages`) : null,
        elementCount > 0 ? (locale === "nb" ? `${elementCount} elementer` : `${elementCount} elements`) : null,
        connectorNames.length > 0
          ? locale === "nb"
            ? `${connectorNames.join(", ")} er koblet til`
            : `${connectorNames.join(", ")} connected`
          : null,
        graphSignals[0] ?? null,
      ],
    },
    {
      label: locale === "nb" ? "Trolig første spørsmål" : "Likely first intents",
      items: proofOfConcept?.likelyIntents ?? likelyIntentSignals(state.website?.agentBrief, websites, locale),
    },
    {
      label: locale === "nb" ? "Neste Verevon-jobber" : "Next Verevon jobs",
      items: proofOfConcept?.nextActions ?? [
        locale === "nb" ? "Bygg første chatbot-utkast" : "Build the first chatbot draft",
        connectorNames.length > 0
          ? locale === "nb"
            ? "Bruk kilder til svarutkast med menneskelig kontroll"
            : "Use sources for reviewed reply drafts"
          : null,
        locale === "nb" ? "Finn kunnskapshull før lansering" : "Find knowledge gaps before launch",
      ],
    },
    {
      label: locale === "nb" ? "Første effekt" : "First impact",
      items: proofOfConcept?.operationalImpact?.map((item) =>
        item.detail ? `${item.label}: ${item.value} · ${item.detail}` : `${item.label}: ${item.value}`,
      ) ?? expectedOutcomes.map((item) =>
        item.detail ? `${item.label}: ${item.value} · ${item.detail}` : `${item.label}: ${item.value}`,
      ),
    },
    {
      label: locale === "nb" ? "Planlogikk" : "Plan fit",
      items: proofOfConcept?.recommendationFit ?? scopeSignals,
    },
  ].map((section) => ({
    ...section,
    items: section.items.filter((item): item is string => Boolean(item)),
  }));

  return {
    sections,
    proofGroups: [
      { label: locale === "nb" ? "Omfang" : "Scope", items: scopeSignals },
      { label: locale === "nb" ? "Det vi fant" : "What we found", items: proofPoints },
      { label: locale === "nb" ? "Første forbedringer" : "First improvements", items: opportunities },
    ].filter((group) => group.items.length > 0),
    expectedOutcomes,
  };
}

function AssemblyProofPanel({
  proof,
  locale,
}: {
  proof: AssemblyProof;
  locale: "nb" | "en";
}) {
  return (
    <div className="mt-5">
      <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
        {proof.sections.map((section) => (
          <div key={section.label} className="min-h-[120px] rounded-xl border border-[#E7E5E4] bg-white/70 p-3">
            <p className="font-inter text-[10px] font-semibold uppercase tracking-[0.14em] text-[#777169]">{section.label}</p>
            <ul className="mt-2 space-y-1.5">
              {section.items.slice(0, 4).map((item) => (
                <li key={item} className="font-inter text-[12px] leading-5 text-[#4E4A45]">
                  {item}
                </li>
              ))}
            </ul>
          </div>
        ))}
      </div>

      {(proof.proofGroups.length > 0 || proof.expectedOutcomes.length > 0) && (
        <div className="mt-5 border-t border-[#E7E5E4] pt-4">
          <p className="font-inter text-[10px] font-semibold uppercase tracking-[0.16em] text-[#777169]">
            {locale === "nb" ? "Verevon ser allerede" : "Verevon already sees"}
          </p>
          {proof.proofGroups.length > 0 && (
            <div className="mt-3 grid gap-4 md:grid-cols-3">
              {proof.proofGroups.map((group) => (
                <AssemblySignalList key={group.label} title={group.label} items={group.items} />
              ))}
            </div>
          )}
          {proof.expectedOutcomes.length > 0 && (
            <div className="mt-5 grid gap-4 sm:grid-cols-3">
              {proof.expectedOutcomes.map((outcome) => (
                <div key={`${outcome.label}-${outcome.value}`} className="border-l border-[#D6D3D1] pl-3">
                  <p className="font-inter text-[22px] font-semibold leading-none text-[#191716]">{outcome.value}</p>
                  <p className="mt-2 font-inter text-[11px] font-semibold text-[#26282f]">{outcome.label}</p>
                  {outcome.detail && <p className="mt-1 font-inter text-[11px] leading-4 text-[#7d828a]">{outcome.detail}</p>}
                </div>
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function buildAssemblyOutlook(
  machine: OnboardingMachine,
  recommendation: PlanRecommendation,
  locale: "nb" | "en",
): AssemblyOutlook {
  const orgName = machine.state.organization?.name?.trim() || (locale === "nb" ? "arbeidsplassen" : "your workspace");
  const planName = nameForId(machine.state.organization?.plan ?? recommendation.planId, locale);
  const sourceCount = countOnboardingSources({
    website: machine.state.website,
    additionalWebsites: machine.state.additionalWebsites,
    connectors: machine.state.connectors,
  });
  const host = machine.state.website?.url ? websiteHost(machine.state.website.url) : undefined;
  const connectorNames = machine.state.connectors.map((connector) => connector.label).filter(Boolean);
  const sourceText =
    sourceCount > 0
      ? locale === "nb"
        ? `${sourceCount} kilder`
        : `${sourceCount} sources`
      : locale === "nb"
        ? "første agent"
        : "first agent";
  const fallbackExpected = fallbackExpectedOutcomes(sourceCount, connectorNames.length, locale);
  const expectedOutcomes = cleanOutcomeList(recommendation.expectedOutcomes).length > 0
    ? cleanOutcomeList(recommendation.expectedOutcomes)
    : fallbackExpected;
  const proofPoints = cleanTextList(recommendation.proofPoints).length > 0
    ? cleanTextList(recommendation.proofPoints)
    : cleanTextList([
        host ? (locale === "nb" ? `${host} er inne som første offentlig kunnskapsflate.` : `${host} is in as the first public knowledge surface.`) : "",
        connectorNames.length > 0
          ? locale === "nb"
            ? `${connectorNames.join(", ")} er klare som interne signaler.`
            : `${connectorNames.join(", ")} are ready as internal signals.`
          : "",
      ]);
  const opportunities = cleanTextList(recommendation.opportunities).length > 0
    ? cleanTextList(recommendation.opportunities)
    : cleanTextList([
        locale === "nb"
          ? "Start med de vanligste kundespørsmålene og mål kunnskapshullene."
          : "Start with the most common customer questions and measure knowledge gaps.",
        locale === "nb"
          ? "Legg på ruting når svarene er kvalitetssikret."
          : "Add routing once answers are verified.",
      ]);

  return locale === "nb"
    ? {
        title: `Verevon klargjøres for ${orgName}`,
        description: `Vi setter opp ${planName} med ${sourceText}, lager første agentkart og åpner dashboardet med konkrete neste steg.`,
        dashboardTitle: `${orgName} dashboard`,
        headline: `Første agent for ${orgName}`,
        summary: `${recommendation.summary ?? "Verevon bygger et første bilde av scope, kilder og forbedringsmuligheter."}`,
        proofPoints,
        opportunities,
        expectedOutcomes,
      }
    : {
        title: `Preparing Verevon for ${orgName}`,
        description: `We are setting up ${planName} with ${sourceText}, building the first agent map and opening the dashboard with concrete next steps.`,
        dashboardTitle: `${orgName} dashboard`,
        headline: `First agent for ${orgName}`,
        summary: `${recommendation.summary ?? "Verevon is building the first view of scope, sources and improvement opportunities."}`,
        proofPoints,
        opportunities,
        expectedOutcomes,
      };
}

function buildOnboardingCompletionMetadata(state: OnboardingState, recommendation: PlanRecommendation) {
  return {
    selected_theme: state.brandTheme
      ? {
          mode: state.brandTheme.mode,
          primary_color: state.brandTheme.primaryColor,
          save_status: state.brandTheme.saveStatus,
        }
      : undefined,
    branding: state.website?.branding
      ? {
          site_name: state.website.branding.siteName,
          theme_color: state.website.branding.themeColor,
          favicon: state.website.branding.favicon,
          logo_candidate: state.website.branding.logoCandidate,
          apple_touch_icon: state.website.branding.appleTouchIcon,
          palette: state.website.branding.palette?.slice(0, 8),
        }
      : undefined,
    websites: allOnboardingWebsites(state).map((site) => ({
      url: site.url,
      status: site.crawlEvidence?.status,
      pages: site.crawlEvidence?.pages ?? 0,
      elements: site.crawlEvidence?.elements ?? 0,
      content_types: site.crawlEvidence?.contentTypes?.slice(0, 8) ?? [],
      seed_status: site.crawlEvidence?.seedStatus,
    })),
    connectors: state.connectors.map((connector) => ({
      id: connector.id,
      label: connector.label,
      metadata_status: connector.metadata?.status,
      sensitivity: connector.metadata?.sensitivity,
    })),
    recommendation: {
      plan_id: recommendation.planId,
      source: recommendation.source,
      generated_at: recommendation.generatedAt,
    },
  };
}

function personalizedAssemblyTicks(
  machine: OnboardingMachine,
  recommendation: PlanRecommendation,
  fallbackTicks: readonly string[],
  locale: "nb" | "en",
): string[] {
  const host = machine.state.website?.url ? websiteHost(machine.state.website.url) : undefined;
  const connectorNames = machine.state.connectors.map((connector) => connector.label).filter(Boolean);
  const planName = nameForId(machine.state.organization?.plan ?? recommendation.planId, locale);
  return [
    locale === "nb" ? `Setter opp ${planName}-arbeidsplassen` : `Setting up the ${planName} workspace`,
    host
      ? locale === "nb"
        ? `Bygger kunnskapskart fra ${host}`
        : `Building the knowledge map from ${host}`
      : fallbackTicks[1],
    connectorNames.length > 0
      ? locale === "nb"
        ? `Kobler ${connectorNames.slice(0, 2).join(" og ")} inn i agentgrunnlaget`
        : `Connecting ${connectorNames.slice(0, 2).join(" and ")} to the agent base`
      : fallbackTicks[2],
    locale === "nb" ? "Lager første forbedringsforslag" : "Drafting the first improvement ideas",
    locale === "nb" ? "Åpner dashboard med lanseringsestimat" : "Opening the dashboard with launch estimates",
  ].filter(Boolean);
}

function AssemblySignalList({ title, items }: { title: string; items: string[] }) {
  return (
    <div>
      <p className="font-inter text-[11px] font-semibold text-[#26282f]">{title}</p>
      <ul className="mt-2 space-y-2">
        {items.slice(0, 3).map((item) => (
          <li key={item} className="flex gap-2 font-inter text-[12px] leading-5 text-[#6B6660]">
            <span className="mt-[8px] size-1.5 shrink-0 rounded-full bg-[#191716]" aria-hidden="true" />
            <span>{item}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}

function likelyIntentSignals(
  brief: string | undefined,
  websites: WebsitePayload[],
  locale: "nb" | "en",
): string[] {
  const text = `${brief ?? ""} ${websites.flatMap((site) => site.crawlEvidence?.snippets.map((snippet) => `${snippet.title} ${snippet.excerpt ?? ""}`) ?? []).join(" ")}`.toLowerCase();
  const nb = locale === "nb";
  const signals = [
    /shopify|webshop|nettbutikk|product|produkt/.test(text) ? (nb ? "Produkt- og nettbutikkspørsmål" : "Product and webshop questions") : null,
    /return|refund|shipping|delivery|retur|frakt|levering/.test(text) ? (nb ? "Retur, frakt og levering" : "Returns, shipping and delivery") : null,
    /price|pricing|demo|quote|pris|tilbud/.test(text) ? (nb ? "Pris, tilbud og kjøpshjelp" : "Pricing, quotes and buying help") : null,
    /support|contact|help|kundeservice|kontakt|hjelp/.test(text) ? (nb ? "Kontakt og support" : "Contact and support") : null,
  ].filter((item): item is string => Boolean(item));
  return signals.length > 0 ? signals : [nb ? "Vanlige spørsmål fra nettsiden" : "Common website questions"];
}

function fallbackExpectedOutcomes(
  sourceCount: number,
  connectorCount: number,
  locale: "nb" | "en",
): NonNullable<PlanRecommendation["expectedOutcomes"]> {
  if (locale === "nb") {
    return [
      {
        label: "Dekning ved start",
        value: sourceCount > 0 ? `${sourceCount} kilder` : "1 agent",
        detail: "Første svargrunnlag blir klart for kvalitetssikring.",
      },
      {
        label: "Første flyter",
        value: connectorCount > 0 ? "2-3 forslag" : "1-2 forslag",
        detail: "Svar, handoff og kunnskapshull blir prioritert først.",
      },
      {
        label: "Tid til verdi",
        value: connectorCount > 0 ? "1-3 dager" : "samme dag",
        detail: "Estimat for første kvalitetssikrede agentutkast.",
      },
    ];
  }
  return [
    {
      label: "Launch coverage",
      value: sourceCount > 0 ? `${sourceCount} sources` : "1 agent",
      detail: "The first answer base becomes ready for review.",
    },
    {
      label: "First flows",
      value: connectorCount > 0 ? "2-3 ideas" : "1-2 ideas",
      detail: "Answers, handoff and knowledge gaps are prioritized first.",
    },
    {
      label: "Time to value",
      value: connectorCount > 0 ? "1-3 days" : "same day",
      detail: "Estimate for the first verified agent draft.",
    },
  ];
}

function cleanTextList(values?: string[]): string[] {
  return (values ?? [])
    .map((value) => value.replace(/\s+/g, " ").trim())
    .filter((value, index, array) => value.length > 0 && array.indexOf(value) === index)
    .slice(0, 4);
}

function cleanOutcomeList(values?: PlanRecommendation["expectedOutcomes"]): NonNullable<PlanRecommendation["expectedOutcomes"]> {
  return (values ?? [])
    .map((value) => ({
      label: value.label?.replace(/\s+/g, " ").trim(),
      value: value.value?.replace(/\s+/g, " ").trim(),
      detail: value.detail?.replace(/\s+/g, " ").trim(),
    }))
    .filter((value): value is { label: string; value: string; detail: string | undefined } => Boolean(value.label && value.value))
    .slice(0, 3);
}
