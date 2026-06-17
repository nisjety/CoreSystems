/**
 * Phase 1 onboarding · plan recommendation.
 *
 * Asks Model Plane (via `mintPlaneToken({ audience: 'model-plane' })`)
 * to pick one of `trial | hobby | standard | pro | enterprise` based
 * on what the user supplied in the wizard. The UI labels these as
 * Gratis, Essential, Advanced, Expert and Custom.
 *
 * The route degrades open: if Model Plane is unavailable we still use
 * the collected onboarding signals deterministically. `trial` is only
 * returned when the user has not supplied organization, website, agent
 * brief, or source signals.
 */

import { NextRequest, NextResponse } from 'next/server'

import {
  mintPlaneToken,
  PlaneTokenError,
} from '@/lib/auth/plane-token'

interface OrganizationPayload {
  name?: string
  size?: 'solo' | 'small' | 'medium' | 'large' | 'enterprise'
  employeeCount?: number
}

interface WebsitePayload {
  url?: string
  agentBrief?: string
}

interface ConnectorPick {
  id: string
  label: string
}

interface RequestBody {
  organization?: OrganizationPayload
  website?: WebsitePayload
  connectors?: ConnectorPick[]
  sourceCount?: number
  locale?: OnboardingLocale
}

interface Recommendation {
  planId: 'hobby' | 'standard' | 'pro' | 'enterprise' | 'trial'
  reason: string
  summary: string
  generatedAt: string
}

type OnboardingLocale = 'nb' | 'en'

const COPY = {
  nb: {
    noSources: 'Du har ikke koblet til kilder ennå.',
    noSignals: 'Du har ikke gitt oss nok signaler ennå.',
    trialSummary:
      'Gratis lar deg teste i 14 dager uten kort. Betalte planer bruker fast månedspris pluss usage per henvendelse løst av AI.',
    system:
      'Du er en intern rådgiver i Velion. Du svarer KUN med JSON som matcher response_format. Bruk norsk i feltene "reason" og "summary". Gjør teksten personlig: nevn organisasjonen hvis kjent, hva brukeren vil at agenten skal gjøre, nettstedet og de faktiske kildene. Ikke finn på data.',
    prompt: {
      organization: 'Organisasjon',
      size: 'Størrelse',
      employees: 'Ansatte fra Enhetsregisteret',
      website: 'Nettside/kunnskapsbase',
      goal: 'Det brukeren vil at agenten skal gjøre',
      connectors: 'Koblede kilder',
      sourceCount: 'Totalt antall kilder',
      choose: 'Velg den planen som passer best for dette teamet. Mulige verdier:',
      trial: '- trial = Gratis (14 dagers gratis test, ingen kort)',
      hobby: '- hobby = Essential (299 kr/mnd eller 239 kr/mnd årlig + 4 kr per henvendelse løst av AI)',
      standard: '- standard = Advanced (999 kr/mnd eller 849 kr/mnd årlig + 3,50 kr per henvendelse løst av AI)',
      pro: '- pro = Expert (1 499 kr/mnd eller 1 099 kr/mnd årlig + 2,90 kr per henvendelse løst av AI)',
      enterprise: '- enterprise = Custom (kontakt salg, volumpris, onboarding og governance)',
      trialRule: 'Velg trial bare når det ikke finnes organisasjonsstørrelse, ansatte, nettsted, agentoppgave eller kilder.',
      essentialRule: 'Essential er normalt riktig for 1-10 ansatte, enkel chatbot, website og opptil 2 integrasjoner.',
      advancedRule: 'Advanced passer for 11-50 ansatte, 3+ integrasjoner, 4+ totale kilder, eller tydelig automasjon/workflow/ruting/eskalering.',
      expertRule: 'Expert kan bare anbefales for ca. 50-100 ansatte og må også ha tydelig kompleksitet: 3+ integrasjoner, 4+ kilder, SLA/rapportering eller flere team.',
      customRule: 'Custom kan bare anbefales for 100+ ansatte eller large/enterprise org, og bør ha 4+ integrasjoner, 5+ kilder eller tydelig governance/volumbehov.',
      reasonRule: 'reason skal være én personlig setning som forklarer hvorfor planen passer for denne konkrete brukeren.',
      summaryRule: 'summary skal være 1-2 korte setninger som refererer til faktiske signaler: agentoppgave, nettsted, kilder og ansatte/størrelse når kjent.',
    },
    recommendation: {
      subjectFallback: 'teamet ditt',
      sourcesFallback: 'ingen tilkoblede kilder ennå',
      sources: '{{count}} kilder',
      teamEmployees: '{{count}} ansatte fra Brønnøysund',
      teamSize: {
        solo: 'som en solo-organisasjon',
        small: 'som et lite team',
        medium: 'som et voksende team',
        large: 'som en større organisasjon',
        enterprise: 'som en enterprise-organisasjon',
      },
      reasonWithGoal: '{{plan}} er riktig valg for {{subject}}: dere vil {{goalPhrase}}, og vi har {{sources}} som svargrunnlag.',
      reasonWithoutGoal: '{{plan}} er riktig valg for {{subject}} fordi dere har {{sources}} som kunnskapsgrunnlag.',
      summaryWithGoal: 'For {{subject}}{{teamQualifier}} gir {{plan}} mest mening. Dere beskrev behovet som "{{goal}}", og Velion kan bygge svarene på {{sources}}. {{plan}} gir {{fit}}.',
      summaryWithoutGoal: 'For {{subject}}{{teamQualifier}} gir {{plan}} mest mening. Velion kan bygge svarene på {{sources}}. {{plan}} gir {{fit}}.',
      planFit: {
        trial: 'en trygg 14 dagers start uten kort',
        hobby: 'nok kapasitet til å validere agenten på de første kundespørsmålene',
        standard: 'automatisering og flere kilder, uten at dere trenger Custom før volumet blir større',
        pro: 'mer styring, rapportering og kapasitet når agenten skal håndtere flere team og flere kilder',
        enterprise: 'tilpasset volum, onboarding og governance for et større oppsett',
      },
    },
  },
  en: {
    noSources: 'You have not connected sources yet.',
    noSignals: 'You have not given us enough signals yet.',
    trialSummary:
      'Free lets you test for 14 days without a card. Paid plans use a fixed monthly price plus usage per conversation resolved by AI.',
    system:
      'You are an internal advisor at Velion. Respond ONLY with JSON matching response_format. Use English in "reason" and "summary". Make it personal: mention the organization if known, what the user wants the agent to do, the website, and the actual sources. Do not invent data.',
    prompt: {
      organization: 'Organization',
      size: 'Size',
      employees: 'Employees from the Norwegian business registry',
      website: 'Website/knowledge base',
      goal: 'What the user wants the agent to do',
      connectors: 'Connected sources',
      sourceCount: 'Total source count',
      choose: 'Choose the plan that best fits this team. Allowed values:',
      trial: '- trial = Free (14 day free test, no card)',
      hobby: '- hobby = Essential ($25/mo or $20/mo yearly + $0.39 per conversation resolved by AI)',
      standard: '- standard = Advanced ($99/mo or $85/mo yearly + $0.35 per conversation resolved by AI)',
      pro: '- pro = Expert ($149/mo or $110/mo yearly + $0.29 per conversation resolved by AI)',
      enterprise: '- enterprise = Custom (contact sales, volume pricing, onboarding and governance)',
      trialRule: 'Choose trial only when there is no organization size, employee count, website, agent task or source signal.',
      essentialRule: 'Essential is normally right for 1-10 employees, a simple chatbot, website and up to 2 integrations.',
      advancedRule: 'Advanced fits 11-50 employees, 3+ integrations, 4+ total sources, or clear automation/workflow/routing/escalation intent.',
      expertRule: 'Expert can only be recommended for about 50-100 employees and must also have complexity: 3+ integrations, 4+ sources, SLA/reporting or multiple teams.',
      customRule: 'Custom can only be recommended for 100+ employees or large/enterprise orgs, and should have 4+ integrations, 5+ sources or clear governance/volume needs.',
      reasonRule: 'reason must be one personal sentence explaining why the plan fits this specific user.',
      summaryRule: 'summary must be 1-2 short sentences referencing real signals: agent task, website, sources and employees/size when known.',
    },
    recommendation: {
      subjectFallback: 'your team',
      sourcesFallback: 'no connected sources yet',
      sources: '{{count}} sources',
      teamEmployees: '{{count}} employees from Brønnøysund',
      teamSize: {
        solo: 'as a solo organization',
        small: 'as a small team',
        medium: 'as a growing team',
        large: 'as a larger organization',
        enterprise: 'as an enterprise organization',
      },
      reasonWithGoal: '{{plan}} is the right choice for {{subject}}: you want {{goalPhrase}}, and we have {{sources}} as the answer base.',
      reasonWithoutGoal: '{{plan}} is the right choice for {{subject}} because you have {{sources}} as the knowledge base.',
      summaryWithGoal: 'For {{subject}}{{teamQualifier}}, {{plan}} makes the most sense. You described the need as "{{goal}}", and Velion can ground answers in {{sources}}. {{plan}} gives you {{fit}}.',
      summaryWithoutGoal: 'For {{subject}}{{teamQualifier}}, {{plan}} makes the most sense. Velion can ground answers in {{sources}}. {{plan}} gives you {{fit}}.',
      planFit: {
        trial: 'a safe 14 day start without a card',
        hobby: 'enough capacity to validate the agent on the first customer questions',
        standard: 'automation and more sources without needing Custom before volume grows',
        pro: 'more control, reporting and capacity when the agent must support more teams and sources',
        enterprise: 'custom volume, onboarding and governance for a larger setup',
      },
    },
  },
} as const

const MODEL_GATEWAY_URL = (
  process.env.MODEL_GATEWAY_URL ||
  process.env.AI_CORE_URL ||
  'http://model-plane-model-gateway-1:8080'
).replace(/\/+$/, '')
const MODEL_RECOMMENDATION_TIMEOUT_MS = 8_000
const PLANE_TOKEN_TIMEOUT_MS = 3_000

const MICROSOFT_CONNECTOR_ALIASES = new Set([
  'teams',
  'sharepoint',
  'onedrive',
  'outlook',
  'm365',
  'microsoft365',
  'microsoft-365',
])

function requestLocale(body: RequestBody): OnboardingLocale {
  return body.locale === 'en' ? 'en' : 'nb'
}

function formatText(
  template: string,
  values: Record<string, string | number>,
): string {
  return Object.entries(values).reduce(
    (out, [key, value]) =>
      out.replace(new RegExp(`{{\\s*${key}\\s*}}`, 'g'), String(value)),
    template,
  )
}

function formatNumber(value: number, locale: OnboardingLocale): string {
  return value.toLocaleString(locale === 'nb' ? 'nb-NO' : 'en-US')
}

export async function POST(request: NextRequest): Promise<NextResponse> {
  let body: RequestBody = {}
  try {
    body = (await request.json()) as RequestBody
  } catch {
    // Empty / malformed body — fall through to default recommendation.
  }

  // If the user gave us nothing, recommend trial without spending a
  // Model Plane call. Every non-empty onboarding signal gets a paid
  // recommendation floor below.
  if (!hasRecommendationSignal(body)) {
    return jsonRec(trialFallback(COPY[requestLocale(body)].noSources, requestLocale(body)))
  }

  let token: string
  try {
    token = await withTimeout(
      mintPlaneToken({ audience: 'model-plane', request }),
      PLANE_TOKEN_TIMEOUT_MS,
    )
  } catch (error) {
    if (error instanceof PlaneTokenError) {
      return jsonRec(
        signalFallbackRecommendation(body),
      )
    }
    return jsonRec(
      signalFallbackRecommendation(body),
    )
  }

  const prompt = buildPrompt(body)

  try {
    const response = await fetch(`${MODEL_GATEWAY_URL}/v1/invoke`, {
      method: 'POST',
      headers: {
        authorization: `Bearer ${token}`,
        'content-type': 'application/json',
      },
      body: JSON.stringify({
        model: process.env.ONBOARDING_REC_MODEL || 'claude-haiku-4-5',
        max_tokens: 320,
        response_format: {
          type: 'json_schema',
          schema: {
            type: 'object',
            required: ['planId', 'reason', 'summary'],
            properties: {
              planId: {
                type: 'string',
                enum: ['hobby', 'standard', 'pro', 'enterprise', 'trial'],
              },
              reason: { type: 'string', maxLength: 260 },
              summary: { type: 'string', maxLength: 320 },
            },
          },
        },
        messages: [
          {
            role: 'system',
            content: COPY[requestLocale(body)].system,
          },
          { role: 'user', content: prompt },
        ],
      }),
      cache: 'no-store',
      signal: AbortSignal.timeout(MODEL_RECOMMENDATION_TIMEOUT_MS),
    })

    if (!response.ok) {
      return jsonRec(
        signalFallbackRecommendation(body),
      )
    }

    const payload = (await response.json()) as {
      content?: unknown
      output?: unknown
      data?: unknown
    }
    const parsed = extractRecommendation(payload)
    if (!parsed) {
      return jsonRec(
        signalFallbackRecommendation(body),
      )
    }
    // Post-LLM guardrail. Business tier eligibility is deterministic:
    // Model Plane can make the copy better, but it cannot push a small
    // simple chatbot into Expert/Custom or under-sell a complex setup.
    const adjusted = applyDeterministicFloor(parsed, body)
    return jsonRec({
      ...adjusted,
      summary: adjusted.summary ?? buildRecommendationSummary(body, adjusted.planId),
      generatedAt: new Date().toISOString(),
    })
  } catch {
    return jsonRec(
      signalFallbackRecommendation(body),
    )
  }
}

function withTimeout<T>(promise: Promise<T>, timeoutMs: number): Promise<T> {
  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => {
      reject(new Error('operation timed out'))
    }, timeoutMs)

    promise
      .then((value) => {
        clearTimeout(timeout)
        resolve(value)
      })
      .catch((error: unknown) => {
        clearTimeout(timeout)
        reject(error)
      })
  })
}

/**
 * Override the LLM pick when it conflicts with the product tier rules.
 * Model Plane writes copy; deterministic rules own tier eligibility.
 *
 * Rules (ordered from strongest signal to weakest):
 *   - large org / 5+ connectors           → at least `enterprise`
 *   - medium org / 3+ connectors          → at least `pro`
 *   - small org / 1+ connector + website  → at least `standard`
 *   - any remaining signal                → at least `hobby`
 */
function applyDeterministicFloor(
  pick: Pick<Recommendation, 'planId' | 'reason' | 'summary'>,
  body: RequestBody,
): Pick<Recommendation, 'planId' | 'reason' | 'summary'> {
  const expectedPlan = selectSignalPlanId(body)
  if (pick.planId === expectedPlan) return pick
  return {
    planId: expectedPlan,
    reason: buildSignalReason(body, expectedPlan),
    summary: buildRecommendationSummary(body, expectedPlan),
  }
}

function buildPrompt(body: RequestBody): string {
  const copy = COPY[requestLocale(body)].prompt
  const lines: string[] = []
  if (body.organization?.name) {
    lines.push(`${copy.organization}: ${body.organization.name}`)
  }
  if (body.organization?.size) {
    lines.push(`${copy.size}: ${body.organization.size}`)
  }
  if (body.organization?.employeeCount != null) {
    lines.push(`${copy.employees}: ${body.organization.employeeCount}`)
  }
  if (body.website?.url) {
    lines.push(`${copy.website}: ${body.website.url}`)
  }
  if (body.website?.agentBrief) {
    lines.push(`${copy.goal}: ${body.website.agentBrief}`)
  }
  const connectors = normalizedConnectorPicks(body.connectors ?? [])
  if (connectors.length) {
    lines.push(`${copy.connectors}: ${connectors.map((c) => c.label).join(', ')}`)
  }
  lines.push(`${copy.sourceCount}: ${onboardingSourceCount(body)}`)
  lines.push('')
  lines.push(copy.choose)
  lines.push(copy.trial)
  lines.push(copy.hobby)
  lines.push(copy.standard)
  lines.push(copy.pro)
  lines.push(copy.enterprise)
  lines.push('')
  lines.push(copy.trialRule)
  lines.push(copy.essentialRule)
  lines.push(copy.advancedRule)
  lines.push(copy.expertRule)
  lines.push(copy.customRule)
  lines.push(copy.reasonRule)
  lines.push(copy.summaryRule)
  return lines.join('\n')
}

function extractRecommendation(
  payload: Record<string, unknown>,
): Pick<Recommendation, 'planId' | 'reason' | 'summary'> | null {
  // Different gateway shapes — be defensive.
  const candidates: unknown[] = [
    payload.content,
    payload.output,
    payload.data,
    (payload as { choices?: Array<{ message?: { content?: unknown } }> })
      .choices?.[0]?.message?.content,
  ]
  for (const candidate of candidates) {
    if (typeof candidate === 'object' && candidate !== null) {
      const obj = candidate as Record<string, unknown>
      if (
        typeof obj.planId === 'string' &&
        typeof obj.reason === 'string' &&
        isPlanId(obj.planId)
      ) {
        return {
          planId: obj.planId,
          reason: obj.reason,
          summary: typeof obj.summary === 'string' ? obj.summary : obj.reason,
        }
      }
    }
    if (typeof candidate === 'string') {
      try {
        const parsed = JSON.parse(candidate) as Record<string, unknown>
        if (
          typeof parsed.planId === 'string' &&
          typeof parsed.reason === 'string' &&
          isPlanId(parsed.planId)
        ) {
          return {
            planId: parsed.planId,
            reason: parsed.reason,
            summary:
              typeof parsed.summary === 'string' ? parsed.summary : parsed.reason,
          }
        }
      } catch {
        // Move on.
      }
    }
  }
  return null
}

function isPlanId(value: string): value is Recommendation['planId'] {
  return ['hobby', 'standard', 'pro', 'enterprise', 'trial'].includes(value)
}

function trialFallback(
  reason: string,
  locale: OnboardingLocale,
): Recommendation {
  return {
    planId: 'trial',
    reason,
    summary: COPY[locale].trialSummary,
    generatedAt: new Date().toISOString(),
  }
}

function signalFallbackRecommendation(body: RequestBody): Recommendation {
  const locale = requestLocale(body)
  const planId = selectSignalPlanId(body)
  if (planId === 'trial') {
    return trialFallback(COPY[locale].noSignals, locale)
  }
  return {
    planId,
    reason: buildSignalReason(body, planId),
    summary: buildRecommendationSummary(body, planId),
    generatedAt: new Date().toISOString(),
  }
}

function jsonRec(rec: Recommendation): NextResponse {
  return NextResponse.json({ recommendation: rec })
}

function hasRecommendationSignal(body: RequestBody): boolean {
  return (
    Boolean(body.website?.url?.trim()) ||
    Boolean(body.website?.agentBrief?.trim()) ||
    onboardingSourceCount(body) > 0 ||
    body.organization?.employeeCount != null ||
    Boolean(body.organization?.size)
  )
}

function onboardingSourceCount(body: RequestBody): number {
  const explicit =
    typeof body.sourceCount === 'number' && Number.isFinite(body.sourceCount)
      ? body.sourceCount
      : 0
  const connectorCount = uniqueConnectorCount(body.connectors ?? [])
  const websiteCount = body.website?.url?.trim() ? 1 : 0
  return Math.max(explicit, connectorCount + websiteCount)
}

function selectSignalPlanId(body: RequestBody): Recommendation['planId'] {
  if (!hasRecommendationSignal(body)) return 'trial'

  const connectorCount = uniqueConnectorCount(body.connectors ?? [])
  const sourceCount = onboardingSourceCount(body)
  const employeeCount = body.organization?.employeeCount
  const advancedIntent = hasAdvancedIntent(body.website?.agentBrief)
  const expertIntent = hasExpertIntent(body.website?.agentBrief)
  const customIntent = hasCustomIntent(body.website?.agentBrief)
  const sizeRank: Record<string, number> = {
    solo: 1,
    small: 2,
    medium: 3,
    large: 4,
    enterprise: 5,
  }
  const sizeScore = body.organization?.size
    ? sizeRank[body.organization.size] ?? 0
    : 0
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

function buildSignalReason(
  body: RequestBody,
  planId: Recommendation['planId'],
): string {
  const locale = requestLocale(body)
  const copy = COPY[locale].recommendation
  const context = buildPersonalRecommendationContext(body)
  const plan = planLabel(planId, locale)
  return formatText(
    context.goal ? copy.reasonWithGoal : copy.reasonWithoutGoal,
    {
      plan,
      subject: context.subject,
      goalPhrase: context.goal ? customerGoalPhrase(context.goal, locale) : '',
      sources: context.sourcesText,
    },
  )
}

function buildRecommendationSummary(
  body: RequestBody,
  planId: Recommendation['planId'],
): string {
  const locale = requestLocale(body)
  const copy = COPY[locale].recommendation
  const context = buildPersonalRecommendationContext(body)
  const plan = planLabel(planId, locale)
  return formatText(
    context.goal ? copy.summaryWithGoal : copy.summaryWithoutGoal,
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

function buildPersonalRecommendationContext(body: RequestBody): {
  goal?: string
  sourcesText: string
  subject: string
  teamText?: string
  teamQualifier: string
} {
  const locale = requestLocale(body)
  const copy = COPY[locale].recommendation
  const subject = body.organization?.name?.trim() || copy.subjectFallback
  const sourceNames = personalizedSourceNames(body)
  const sourceCount = onboardingSourceCount(body)
  const sourcesText =
    sourceNames.length > 0
      ? humanList(sourceNames, locale)
      : sourceCount > 0
        ? formatText(copy.sources, { count: sourceCount })
        : copy.sourcesFallback
  const employeeCount = body.organization?.employeeCount
  const teamText =
    employeeCount != null
      ? formatText(copy.teamEmployees, {
          count: formatNumber(employeeCount, locale),
        })
      : body.organization?.size
        ? organizationSizeLabel(body.organization.size, locale)
        : undefined

  return {
    goal: cleanGoal(body.website?.agentBrief),
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

function personalizedSourceNames(body: RequestBody): string[] {
  const names = normalizedConnectorPicks(body.connectors ?? []).map((c) => c.label)
  if (body.website?.url?.trim()) {
    names.unshift(websiteHost(body.website.url))
  }
  return names
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

function websiteHost(url: string): string {
  try {
    return new URL(url).hostname.replace(/^www\./, '')
  } catch {
    return url
  }
}

function planFitText(
  planId: Recommendation['planId'],
  locale: OnboardingLocale,
): string {
  return COPY[locale].recommendation.planFit[planId]
}

function organizationSizeLabel(
  size: OrganizationPayload['size'],
  locale: OnboardingLocale,
): string | undefined {
  if (!size) return undefined
  return COPY[locale].recommendation.teamSize[size]
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

function planLabel(
  id: Recommendation['planId'],
  locale: OnboardingLocale,
): string {
  switch (id) {
    case 'hobby':
      return 'Essential'
    case 'standard':
      return 'Advanced'
    case 'pro':
      return 'Expert'
    case 'enterprise':
      return 'Custom'
    case 'trial':
      return locale === 'nb' ? 'Gratis' : 'Free'
  }
}
