/**
 * Onboarding · local plan-recommendation engine (pure, ported from velion v1
 * PaywallStep). Renders an instant, offline-safe recommendation from the
 * accumulated onboarding signals. Also used server-side as the deterministic
 * fallback in the recommend-plan BFF route when Model Plane is unavailable.
 *
 * No React, no network — safe to import from both client and server.
 */

import {
  formatOnboardingNumber,
  formatOnboardingText,
  ONBOARDING_COPY,
  onboardingPlanName,
  type OnboardingLocale,
} from "./onboarding-i18n";
import type {
  ConnectorPick,
  OnboardingPlanId,
  OnboardingState,
  OrganizationPayload,
  OrganizationSize,
  PlanRecommendation,
  WebsitePayload,
} from "./onboarding-machine";
import type { OnboardingContext } from "./onboarding-api";
import {
  allOnboardingWebsites,
  countWebsiteSources,
  displayOrganizationName,
  resolveBrandThemeColor,
} from "./onboarding-evidence";

type PlanId = OnboardingPlanId;

const MICROSOFT_ALIASES = new Set([
  "teams",
  "sharepoint",
  "onedrive",
  "outlook",
  "m365",
  "microsoft365",
  "microsoft-365",
]);

function normalizeConnectorId(id: string): string {
  return MICROSOFT_ALIASES.has(id) ? "microsoft365" : id;
}

export function normalizedConnectorPicks(connectors: ConnectorPick[]): ConnectorPick[] {
  const seen = new Set<string>();
  const out: ConnectorPick[] = [];
  for (const connector of connectors) {
    const id = normalizeConnectorId(connector.id);
    if (seen.has(id)) continue;
    seen.add(id);
    out.push({ ...connector, id, label: id === "microsoft365" ? "Microsoft 365" : connector.label });
  }
  return out;
}

export function uniqueConnectorCount(connectors: ConnectorPick[]): number {
  return new Set(connectors.map((c) => normalizeConnectorId(c.id))).size;
}

export function countOnboardingSources({
  website,
  additionalWebsites,
  connectors,
}: {
  website?: WebsitePayload;
  additionalWebsites?: WebsitePayload[];
  connectors: ConnectorPick[];
}): number {
  return uniqueConnectorCount(connectors) + countWebsiteSources({ website, additionalWebsites });
}

// Capability rank for compatibility gating. `trial` is Pro-equivalent for the
// 14-day window so connected sources never block the trial path.
export const ONBOARDING_PLAN_RANK: Record<PlanId, number> = {
  trial: 4,
  hobby: 1,
  standard: 3,
  pro: 4,
  enterprise: 5,
};

export function requiredRankForConnectorCount(connectorCount: number): number {
  if (connectorCount >= 2) return ONBOARDING_PLAN_RANK.standard;
  if (connectorCount >= 1) return ONBOARDING_PLAN_RANK.hobby;
  return 0;
}

export function supportsConnectorCount(planId: PlanId, connectorCount: number): boolean {
  return ONBOARDING_PLAN_RANK[planId] >= requiredRankForConnectorCount(connectorCount);
}

/** Build the PII-light context object sent to the Model Plane recommender. */
export function buildOnboardingContext(
  state: OnboardingState,
  locale: OnboardingLocale,
): OnboardingContext {
  const websites = allOnboardingWebsites(state);
  const branding = state.website?.branding;
  const logoUrl = branding?.logoCandidate ?? branding?.favicon ?? branding?.appleTouchIcon;
  return {
    organization: state.organization
      ? {
          name: displayOrganizationName(state.organization.name),
          size: state.organization.size,
          employeeCount: state.organization.employeeCount,
        }
      : undefined,
    website: state.website ? { url: state.website.url, agentBrief: state.website.agentBrief } : undefined,
    websites: websites.map((site) => ({
      url: site.url,
      agentBrief: site.agentBrief,
      crawlEvidence: site.crawlEvidence,
    })),
    brand: {
      logoUrl,
      primaryColor: state.brandTheme?.primaryColor ?? resolveBrandThemeColor(branding),
      palette: branding?.palette,
      themeMode: state.brandTheme?.mode ?? "velion",
      branding: branding
        ? {
            siteName: branding.siteName,
            themeColor: branding.themeColor,
            favicon: branding.favicon,
            logoCandidate: branding.logoCandidate,
          }
        : undefined,
    },
    connectors: normalizedConnectorPicks(state.connectors).map((c) => ({ id: c.id, label: c.label })),
    integrationEvidence: normalizedConnectorPicks(state.connectors).map((connector) => ({
      provider: connector.id,
      label: connector.label,
      status: connector.metadata?.status ?? "pending",
      metadata: connector.metadata,
    })),
    sourceCount: countOnboardingSources({
      website: state.website,
      additionalWebsites: state.additionalWebsites,
      connectors: state.connectors,
    }),
    locale,
  };
}

export function selectLocalPlanId({
  organization,
  website,
  additionalWebsites,
  connectors,
}: {
  organization?: OrganizationPayload;
  website?: WebsitePayload;
  additionalWebsites?: WebsitePayload[];
  connectors: ConnectorPick[];
}): PlanId {
  const sourceCount = countOnboardingSources({ website, additionalWebsites, connectors });
  const hasSignal =
    sourceCount > 0 ||
    Boolean(website?.agentBrief?.trim()) ||
    organization?.employeeCount != null ||
    Boolean(organization?.size);
  if (!hasSignal) return "trial";

  const employeeCount = organization?.employeeCount;
  const connectorCount = uniqueConnectorCount(connectors);
  const advancedIntent = hasAdvancedIntent(website?.agentBrief);
  const expertIntent = hasExpertIntent(website?.agentBrief);
  const customIntent = hasCustomIntent(website?.agentBrief);
  const sizeRank: Record<string, number> = { solo: 1, small: 2, medium: 3, large: 4, enterprise: 5 };
  const sizeScore = organization?.size ? sizeRank[organization.size] ?? 0 : 0;
  const customSized = (employeeCount != null && employeeCount >= 100) || sizeScore >= 4;
  const expertSized = employeeCount != null && employeeCount >= 50 && employeeCount < 100;
  const customComplexity = connectorCount >= 4 || sourceCount >= 5 || customIntent;
  const expertComplexity = connectorCount >= 3 || sourceCount >= 4 || expertIntent;

  if (customSized && customComplexity) return "enterprise";
  if (expertSized && expertComplexity) return "pro";
  if (
    (employeeCount != null && employeeCount >= 11) ||
    sizeScore >= 3 ||
    connectorCount >= 3 ||
    sourceCount >= 4 ||
    (advancedIntent && sourceCount >= 2)
  ) {
    return "standard";
  }
  return "hobby";
}

function hasAdvancedIntent(value?: string): boolean {
  const text = value?.toLowerCase() ?? "";
  return ["automatis", "workflow", "ruting", "routing", "triage", "eskaler", "handoff", "sla", "rapport", "analyse", "flere team", "multi-team", "inbox", "ticket", "sak"].some((n) => text.includes(n));
}

function hasExpertIntent(value?: string): boolean {
  const text = value?.toLowerCase() ?? "";
  return hasAdvancedIntent(value) || ["sla", "rapport", "analyse", "flere team", "multi-team", "multibrand", "sso", "compliance"].some((n) => text.includes(n));
}

function hasCustomIntent(value?: string): boolean {
  const text = value?.toLowerCase() ?? "";
  return ["governance", "sikkerhet", "security", "compliance", "databehandler", "dpa", "sso", "audit", "volum", "enterprise", "onboarding"].some((n) => text.includes(n));
}

interface RecommendationContext {
  goal?: string;
  sourcesText: string;
  subject: string;
  teamText?: string;
  teamQualifier: string;
}

function buildLocalRecommendationContext({
  organization,
  website,
  additionalWebsites,
  connectors,
  locale,
}: {
  organization?: OrganizationPayload;
  website?: WebsitePayload;
  additionalWebsites?: WebsitePayload[];
  connectors: ConnectorPick[];
  locale: OnboardingLocale;
}): RecommendationContext {
  const copy = ONBOARDING_COPY[locale];
  const subject = organization?.name?.trim() || copy.recommendation.subjectFallback;
  const sourceNames = normalizedConnectorPicks(connectors).map((c) => c.label);
  sourceNames.unshift(...allOnboardingWebsites({ website, additionalWebsites }).map((site) => websiteHost(site.url)));
  const sourceCount = countOnboardingSources({ website, additionalWebsites, connectors });
  const sourcesText =
    sourceNames.length > 0
      ? humanList(sourceNames, locale)
      : sourceCount > 0
        ? formatOnboardingText(copy.paywall.sources, { count: sourceCount })
        : copy.paywall.noSources;
  const employeeCount = organization?.employeeCount;
  const teamText =
    employeeCount != null
      ? formatOnboardingText(copy.recommendation.teamQualifier.employees, {
          count: formatOnboardingNumber(employeeCount, locale),
        })
      : organization?.size
        ? organizationSizeLabel(organization.size, locale)
        : undefined;

  return {
    goal: cleanGoal(website?.agentBrief),
    sourcesText,
    subject,
    teamText,
    teamQualifier: teamText ? (locale === "nb" ? `, ${teamText},` : `, ${teamText}`) : "",
  };
}

export function buildLocalSummary({
  organization,
  website,
  additionalWebsites,
  connectors,
  planId,
  locale,
}: {
  organization?: OrganizationPayload;
  website?: WebsitePayload;
  additionalWebsites?: WebsitePayload[];
  connectors: ConnectorPick[];
  planId: PlanId;
  locale: OnboardingLocale;
}): string {
  const context = buildLocalRecommendationContext({ organization, website, additionalWebsites, connectors, locale });
  const plan = nameForId(planId, locale);
  const sourceCount = countOnboardingSources({ website, additionalWebsites, connectors });
  const connectorCount = uniqueConnectorCount(connectors);
  return locale === "nb"
    ? buildNorwegianInsightSummary({
        subject: context.subject,
        teamText: context.teamText,
        goal: context.goal,
        sourcesText: context.sourcesText,
        sourceCount,
        connectorCount,
        planId,
        plan,
      })
    : buildEnglishInsightSummary({
        subject: context.subject,
        teamText: context.teamText,
        goal: context.goal,
        sourcesText: context.sourcesText,
        sourceCount,
        connectorCount,
        planId,
        plan,
    });
}

function buildNorwegianInsightSummary({
  subject,
  teamText,
  goal,
  sourcesText,
  sourceCount,
  connectorCount,
  planId,
  plan,
}: {
  subject: string;
  teamText?: string;
  goal?: string;
  sourcesText: string;
  sourceCount: number;
  connectorCount: number;
  planId: PlanId;
  plan: string;
}): string {
  const subjectText = teamText ? `${subject} (${teamText})` : subject;
  const goalText = goal ? norwegianGoalInsight(goal) : "en første kundeagent med tydelig svargrunnlag";
  const sourceText =
    sourceCount > 1
      ? `${sourcesText} gir agenten både offentlig produktkontekst og interne arbeidsflater å lære fra.`
      : `${sourcesText} gir et kontrollert startpunkt før dere kobler på flere kunnskapsområder.`;
  return `Jeg ville valgt ${plan} for ${subjectText}. Behovet peker mot ${goalText}, og ${sourceText} ${norwegianPlanRationale(planId, plan, connectorCount, sourceCount)}`;
}

function buildEnglishInsightSummary({
  subject,
  teamText,
  goal,
  sourcesText,
  sourceCount,
  connectorCount,
  planId,
  plan,
}: {
  subject: string;
  teamText?: string;
  goal?: string;
  sourcesText: string;
  sourceCount: number;
  connectorCount: number;
  planId: PlanId;
  plan: string;
}): string {
  const subjectText = teamText ? `${subject} (${teamText})` : subject;
  const goalText = goal ? englishGoalInsight(goal) : "a first customer-facing agent with a clear answer base";
  const sourceText =
    sourceCount > 1
      ? `${sourcesText} gives the agent both public product context and internal working knowledge.`
      : `${sourcesText} gives you a controlled starting point before adding more knowledge areas.`;
  return `I would choose ${plan} for ${subjectText}. The need points to ${goalText}, and ${sourceText} ${englishPlanRationale(planId, plan, connectorCount, sourceCount)}`;
}

function norwegianPlanRationale(
  planId: PlanId,
  plan: string,
  connectorCount: number,
  sourceCount: number,
): string {
  switch (planId) {
    case "trial":
      return `Det gjør at dere kan teste hele oppsettet i 14 dager før dere binder dere til en betalt plan.`;
    case "hobby":
      return connectorCount <= 1
        ? `Det er nok til å validere én enkel chatbot uten mer governance enn nødvendig.`
        : `${plan} blir trangt med ${sourceCount} kilder; Advanced er mer realistisk hvis alle kildene skal brukes aktivt.`;
    case "standard":
      return `Det gir rom for flere kunnskapsområder og enkel automasjon uten å dra dere inn i Expert før SSO, SLA eller tyngre rapportering faktisk trengs.`;
    case "pro":
      return `Her handler verdien om kontrollaget rundt kildene: SSO, SLA, rapportering og tydeligere styring når supportarbeidet vokser.`;
    case "enterprise":
      return `Custom bør først inn når volum, governance, sikkerhetskrav eller dedikert onboarding må avtales rundt oppsettet.`;
  }
}

function englishPlanRationale(
  planId: PlanId,
  plan: string,
  connectorCount: number,
  sourceCount: number,
): string {
  switch (planId) {
    case "trial":
      return `That lets you test the full setup for 14 days before committing to a paid plan.`;
    case "hobby":
      return connectorCount <= 1
        ? `That is enough to validate one simple chatbot without adding unnecessary governance.`
        : `${plan} is tight for ${sourceCount} sources; Advanced is more realistic if all sources should be active.`;
    case "standard":
      return `It gives you room for multiple knowledge areas and simple automation without moving into Expert before SSO, SLA, or heavier reporting are actually needed.`;
    case "pro":
      return `The value is the control layer around those sources: SSO, SLA, reporting, and clearer governance as support work grows.`;
    case "enterprise":
      return `Custom should come in when volume, governance, security requirements, or dedicated onboarding need negotiated terms.`;
  }
}

export function buildLocalRecommendation({
  organization,
  website,
  additionalWebsites,
  connectors,
  reasonPrefix,
  locale,
}: {
  organization?: OrganizationPayload;
  website?: WebsitePayload;
  additionalWebsites?: WebsitePayload[];
  connectors: ConnectorPick[];
  reasonPrefix: string;
  locale: OnboardingLocale;
}): PlanRecommendation {
  const copy = ONBOARDING_COPY[locale];
  const planId = selectLocalPlanId({ organization, website, additionalWebsites, connectors });
  const context = buildLocalRecommendationContext({ organization, website, additionalWebsites, connectors, locale });
  const reason = formatOnboardingText(
    context.goal ? copy.recommendation.reasonWithGoal : copy.recommendation.reasonWithoutGoal,
    {
      prefix: reasonPrefix,
      plan: nameForId(planId, locale),
      subject: context.subject,
      goalPhrase: context.goal ? customerGoalPhrase(context.goal, locale) : "",
      sources: context.sourcesText,
    },
  );
  return {
    planId,
    reason,
    summary: buildLocalSummary({ organization, website, additionalWebsites, connectors, planId, locale }),
    ...buildLocalRecommendationProof({ organization, website, additionalWebsites, connectors, planId, locale }),
    generatedAt: new Date().toISOString(),
    source: "local",
  };
}

function buildLocalRecommendationProof({
  organization,
  website,
  additionalWebsites,
  connectors,
  planId,
  locale,
}: {
  organization?: OrganizationPayload;
  website?: WebsitePayload;
  additionalWebsites?: WebsitePayload[];
  connectors: ConnectorPick[];
  planId: PlanId;
  locale: OnboardingLocale;
}): Pick<PlanRecommendation, "proofPoints" | "scopeSignals" | "opportunities" | "expectedOutcomes"> {
  const sourceCount = countOnboardingSources({ website, additionalWebsites, connectors });
  const normalized = normalizedConnectorPicks(connectors);
  const connectorNames = normalized.map((connector) => connector.label);
  const hosts = allOnboardingWebsites({ website, additionalWebsites }).map((site) => websiteHost(site.url));
  const goal = cleanGoal(website?.agentBrief);
  const subject = organization?.name?.trim() || ONBOARDING_COPY[locale].recommendation.subjectFallback;
  const plan = nameForId(planId, locale);
  const proofPoints = compactStrings([
    hosts.length > 0
      ? locale === "nb"
        ? `${humanList(hosts.slice(0, 3), locale)} er allerede brukt som offentlig signal for merkevare, produkter og kundespråk.`
        : `${humanList(hosts.slice(0, 3), locale)} is already being used as a public signal for brand, products and customer language.`
      : null,
    connectorNames.length > 0
      ? locale === "nb"
        ? `${humanList(connectorNames, locale)} viser hvor agenten kan hente intern arbeidskontekst.`
        : `${humanList(connectorNames, locale)} shows where the agent can pull internal working context from.`
      : null,
    organization?.employeeCount != null
      ? locale === "nb"
        ? `${subject} har ${formatOnboardingNumber(organization.employeeCount, locale)} ansatte, så anbefalingen tar høyde for mer enn en enkel testkonto.`
        : `${subject} has ${formatOnboardingNumber(organization.employeeCount, locale)} employees, so the recommendation accounts for more than a simple test account.`
      : null,
  ]);
  const scopeSignals = compactStrings([
    sourceCount > 0
      ? locale === "nb"
        ? `${formatOnboardingNumber(sourceCount, locale)} startkilder kan brukes i første kunnskapsbase.`
        : `${formatOnboardingNumber(sourceCount, locale)} starting sources can feed the first knowledge base.`
      : null,
    goal
      ? locale === "nb"
        ? `Oppgaven peker mot ${norwegianGoalInsight(goal)}.`
        : `The task points to ${englishGoalInsight(goal)}.`
      : null,
    locale === "nb"
      ? `${plan} gir riktig nivå for ${planFitText(planId, locale)}.`
      : `${plan} gives the right level for ${planFitText(planId, locale)}.`,
  ]);
  const opportunities = compactStrings([
    ecommerceIntent(goal)
      ? locale === "nb"
        ? "Første agent kan prioritere produktspørsmål, levering, retur og eskalering fra nettbutikken."
        : "The first agent can prioritize product questions, delivery, returns and webshop escalation."
      : null,
    connectorNames.length > 0
      ? locale === "nb"
        ? "Interne kilder kan brukes til å fange svar som ikke ligger offentlig på nettstedet."
        : "Internal sources can catch answers that are not publicly available on the website."
      : null,
    locale === "nb"
      ? "Dashboardet kan starte med kunnskapshull, mest stilte spørsmål og forslag til neste automasjon."
      : "The dashboard can start with knowledge gaps, top questions and the next automation candidates.",
  ]);

  return {
    proofPoints,
    scopeSignals,
    opportunities,
    expectedOutcomes: buildExpectedOutcomes({ sourceCount, connectorNames, goal, planId, locale }),
  };
}

function buildExpectedOutcomes({
  sourceCount,
  connectorNames,
  goal,
  planId,
  locale,
}: {
  sourceCount: number;
  connectorNames: string[];
  goal?: string;
  planId: PlanId;
  locale: OnboardingLocale;
}): NonNullable<PlanRecommendation["expectedOutcomes"]> {
  const hasMultipleSources = sourceCount >= 2;
  const automations = planId === "standard" || planId === "pro" || planId === "enterprise";
  if (locale === "nb") {
    return [
      {
        label: "Dekning ved start",
        value: sourceCount > 0 ? `${formatOnboardingNumber(sourceCount, locale)} kilder` : "1 agent",
        detail: hasMultipleSources
          ? "Nettsted og tilkoblede systemer blir ett svargrunnlag."
          : "Start smalt og mål hvilke spørsmål som mangler dekning.",
      },
      {
        label: "Første flyter",
        value: automations ? "2-3 forslag" : "1-2 forslag",
        detail: ecommerceIntent(goal)
          ? "Produktspørsmål, ordrestatus og eskalering er naturlig første scope."
          : "Svar, handoff og kunnskapshull er naturlig første scope.",
      },
      {
        label: "Tid til verdi",
        value: connectorNames.length > 0 ? "1-3 dager" : "samme dag",
        detail: connectorNames.length > 0
          ? "Med interne kilder starter vi med verifiserte svar før automasjon."
          : "Med nettstedet alene kan første utkast valideres raskt.",
      },
    ];
  }
  return [
    {
      label: "Launch coverage",
      value: sourceCount > 0 ? `${formatOnboardingNumber(sourceCount, locale)} sources` : "1 agent",
      detail: hasMultipleSources
        ? "Website and connected systems become one answer base."
        : "Start narrow and measure which questions still need coverage.",
    },
    {
      label: "First flows",
      value: automations ? "2-3 ideas" : "1-2 ideas",
      detail: ecommerceIntent(goal)
        ? "Product questions, order status and escalation are the natural first scope."
        : "Answers, handoff and knowledge gaps are the natural first scope.",
    },
    {
      label: "Time to value",
      value: connectorNames.length > 0 ? "1-3 days" : "same day",
      detail: connectorNames.length > 0
        ? "With internal sources, start with verified answers before automation."
        : "With the website alone, the first draft can be validated quickly.",
    },
  ];
}

function ecommerceIntent(goal?: string): boolean {
  const lower = goal?.toLowerCase() ?? "";
  return lower.includes("shopify") || lower.includes("webshop") || lower.includes("nettbutikk");
}

function compactStrings(values: Array<string | null | undefined>): string[] {
  return values.filter((value): value is string => Boolean(value?.trim()));
}

function cleanGoal(value?: string): string | undefined {
  const cleaned = value?.replace(/\s+/g, " ").trim();
  if (!cleaned) return undefined;
  const withoutTrailing = applyGoalCorrections(cleaned.replace(/[.!?]+$/, ""));
  return withoutTrailing.length > 120 ? `${withoutTrailing.slice(0, 117).trim()}...` : withoutTrailing;
}

function applyGoalCorrections(value: string): string {
  return value
    .replace(/\bterneger\b/gi, "trenger")
    .replace(/\bintregert\b/gi, "integrert")
    .replace(/\bintergrert\b/gi, "integrert")
    .replace(/\bsharpoint\b/gi, "SharePoint")
    .replace(/\bshopify\b/gi, "Shopify");
}

function norwegianGoalInsight(goal: string): string {
  const lower = goal.toLowerCase();
  if (lower.includes("chatbot") && (lower.includes("shopify") || lower.includes("webshop"))) {
    return "en chatbot som kan svare med utgangspunkt i nettbutikken og de interne kildene rundt den";
  }
  if (lower.includes("chatbot") && (lower.includes("website") || lower.includes("nettside"))) {
    return "en nettsideagent som kan svare presist uten at teamet må bygge alt som manuelle FAQ-svar";
  }
  return `målet om ${goal}`;
}

function englishGoalInsight(goal: string): string {
  const lower = goal.toLowerCase();
  if (lower.includes("chatbot") && (lower.includes("shopify") || lower.includes("webshop"))) {
    return "a chatbot that can answer from the webshop and the internal sources around it";
  }
  if (lower.includes("chatbot") && (lower.includes("website") || lower.includes("site"))) {
    return "a website agent that can answer accurately without the team maintaining every FAQ manually";
  }
  return `the goal of ${goal}`;
}

function customerGoalPhrase(goal: string, locale: OnboardingLocale): string {
  const lower = goal.toLowerCase();
  if (locale === "en") {
    if (lower.startsWith("a chatbot")) return `to have ${goal}`;
    if (lower.startsWith("chatbot")) return `to have a ${goal}`;
    if (lower.startsWith("to ")) return goal;
    return `the agent to ${goal}`;
  }
  if (lower.startsWith("en chatbot")) return `ha ${goal}`;
  if (lower.startsWith("chatbot")) return `ha en ${goal}`;
  if (lower.startsWith("å ")) return goal;
  return `at agenten skal ${goal}`;
}

function humanList(items: string[], locale: OnboardingLocale): string {
  const unique = Array.from(new Set(items.filter(Boolean)));
  if (unique.length === 0) return "";
  if (unique.length === 1) return unique[0];
  const joiner = locale === "nb" ? "og" : "and";
  if (unique.length === 2) return `${unique[0]} ${joiner} ${unique[1]}`;
  return `${unique.slice(0, -1).join(", ")} ${joiner} ${unique[unique.length - 1]}`;
}

export function planFitText(id: PlanId, locale: OnboardingLocale): string {
  return ONBOARDING_COPY[locale].paywall.planFit[id];
}

export function organizationSizeLabel(size: OrganizationSize | undefined, locale: OnboardingLocale): string {
  if (!size) return ONBOARDING_COPY[locale].recommendation.teamQualifier.fallback;
  return ONBOARDING_COPY[locale].recommendation.teamQualifier[size];
}

export function employeeSignal(employeeCount: number, locale: OnboardingLocale): string {
  return locale === "nb"
    ? `${formatOnboardingNumber(employeeCount, locale)} ansatte`
    : `${formatOnboardingNumber(employeeCount, locale)} employees`;
}

export function websiteHost(url: string): string {
  try {
    return new URL(url).hostname.replace(/^www\./, "");
  } catch {
    return url;
  }
}

export function nameForId(id: PlanId, locale: OnboardingLocale): string {
  return onboardingPlanName(id, locale);
}
