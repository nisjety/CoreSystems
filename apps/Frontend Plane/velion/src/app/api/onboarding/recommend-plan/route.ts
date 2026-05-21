/**
 * Phase 1 onboarding · plan recommendation.
 *
 * Asks Model Plane (via `mintPlaneToken({ audience: 'model-plane' })`)
 * to pick one of `hobby | standard | pro | enterprise | trial` based
 * on what the user supplied in the wizard. Returns the LLM's pick plus
 * a 1-sentence reason rendered as the paywall copy.
 *
 * The route degrades open: every failure path returns the `trial`
 * recommendation so the user always sees the paywall, just without
 * the personalised gradient outline.
 */

import { NextRequest, NextResponse } from 'next/server'

import {
  mintPlaneToken,
  PlaneTokenError,
} from '@/lib/auth/plane-token'

interface OrganizationPayload {
  name?: string
  size?: 'solo' | 'small' | 'medium' | 'large' | 'enterprise'
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
}

interface Recommendation {
  planId: 'hobby' | 'standard' | 'pro' | 'enterprise' | 'trial'
  reason: string
  generatedAt: string
}

const MODEL_GATEWAY_URL = (
  process.env.MODEL_GATEWAY_URL ||
  process.env.AI_CORE_URL ||
  'http://model-plane-model-gateway-1:8080'
).replace(/\/+$/, '')

export async function POST(request: NextRequest): Promise<NextResponse> {
  let body: RequestBody = {}
  try {
    body = (await request.json()) as RequestBody
  } catch {
    // Empty / malformed body — fall through to default recommendation.
  }

  // Heuristic shortcut: if the user gave us nothing, recommend trial
  // without spending a Model Plane call.
  if (
    !body.website?.url &&
    (!body.connectors || body.connectors.length === 0)
  ) {
    return jsonRec(trialFallback('Du har ikke koblet til kilder ennå.'))
  }

  let token: string
  try {
    token = await mintPlaneToken({ audience: 'model-plane', request })
  } catch (error) {
    if (error instanceof PlaneTokenError) {
      return jsonRec(
        trialFallback('Sikker token utilgjengelig — vi gir deg en prøveperiode.'),
      )
    }
    return jsonRec(trialFallback('Anbefalingen er midlertidig utilgjengelig.'))
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
        max_tokens: 200,
        response_format: {
          type: 'json_schema',
          schema: {
            type: 'object',
            required: ['planId', 'reason'],
            properties: {
              planId: {
                type: 'string',
                enum: ['hobby', 'standard', 'pro', 'enterprise', 'trial'],
              },
              reason: { type: 'string', maxLength: 220 },
            },
          },
        },
        messages: [
          {
            role: 'system',
            content:
              'Du er en intern rådgiver i Velion. Du svarer KUN med JSON som matcher response_format. Bruk norsk i feltet "reason".',
          },
          { role: 'user', content: prompt },
        ],
      }),
      cache: 'no-store',
    })

    if (!response.ok) {
      return jsonRec(
        trialFallback(
          'Modellen er overbelastet — vi anbefaler å starte med prøveperioden.',
        ),
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
        trialFallback('Vi kunne ikke tolke svaret — start gjerne gratis.'),
      )
    }
    return jsonRec({ ...parsed, generatedAt: new Date().toISOString() })
  } catch {
    return jsonRec(
      trialFallback('Anbefalingen er midlertidig utilgjengelig.'),
    )
  }
}

function buildPrompt(body: RequestBody): string {
  const lines: string[] = []
  if (body.organization?.name) {
    lines.push(`Organisasjon: ${body.organization.name}`)
  }
  if (body.organization?.size) {
    lines.push(`Størrelse: ${body.organization.size}`)
  }
  if (body.website?.url) {
    lines.push(`Nettside: ${body.website.url}`)
  }
  if (body.website?.agentBrief) {
    lines.push(`Agentens oppgave: ${body.website.agentBrief}`)
  }
  if (body.connectors?.length) {
    lines.push(`Koblede kilder: ${body.connectors.map((c) => c.label).join(', ')}`)
  }
  lines.push('')
  lines.push(
    'Velg den planen som passer best for dette teamet. Mulige verdier:',
  )
  lines.push('- hobby (1 person, sideprosjekt)')
  lines.push('- standard (lite team som vokser)')
  lines.push('- pro (etablert team med flere kilder)')
  lines.push('- enterprise (51+ ansatte, eller flere connectorer)')
  lines.push('- trial (de bør teste først)')
  return lines.join('\n')
}

function extractRecommendation(
  payload: Record<string, unknown>,
): Pick<Recommendation, 'planId' | 'reason'> | null {
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
        return { planId: obj.planId, reason: obj.reason }
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
          return { planId: parsed.planId, reason: parsed.reason }
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

function trialFallback(reason: string): Recommendation {
  return {
    planId: 'trial',
    reason,
    generatedAt: new Date().toISOString(),
  }
}

function jsonRec(rec: Recommendation): NextResponse {
  return NextResponse.json({ recommendation: rec })
}
