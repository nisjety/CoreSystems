/**
 * The honest fallback. It answers a message no scenario matches, an attachment
 * (not read in the demo) and a request for an image (not in the demo). It says
 * plainly that this is a demo with predefined answers, and offers the three
 * cases as links. The runtime turns a `#demo:<id>` link into a starter
 * (suggestions-live.ts), so the visitor still sees the real composer and send.
 */
import { connected, done, paced, titleFrame } from '../frames'
import { titleFromQuestion } from '../match'
import type { Scenario, TurnContext, TurnScript } from '../scenario-types'

export type FallbackReason = 'free-text' | 'attachments' | 'image'

const LEAD: Record<FallbackReason, string> = {
  'free-text': 'Dette er en demo med forhåndsdefinerte svar. Verevon er her vist med et oppdiktet firma, Fjordform AS, så jeg kan ikke svare fritt på alt. Prøv ett av eksemplene, så viser jeg hvordan Verevon jobber:',
  attachments: 'Vedlegg leses ikke i demoen. Dette er en demo med forhåndsdefinerte svar, så jeg kan ikke svare fritt her. Prøv ett av eksemplene, så viser jeg hvordan Verevon jobber:',
  image: 'Bildegenerering er ikke med i demoen. Dette er en demo med forhåndsdefinerte svar, så jeg kan ikke svare fritt her. Prøv ett av eksemplene, så viser jeg hvordan Verevon jobber:',
}

/** The fallback's markdown: the honest sentence, then one link per case. */
export function fallbackAnswer(reason: FallbackReason, offers: readonly Scenario[]): string {
  const links = offers.map((scenario) => `- [${scenario.label}](#demo:${scenario.id})`)
  return [LEAD[reason], '', ...links].join('\n')
}

export function fallbackTurn(input: {
  ctx: TurnContext
  reason: FallbackReason
  question: string
  offers: readonly Scenario[]
}): TurnScript {
  const { ctx, reason, question, offers } = input
  const answer = fallbackAnswer(reason, offers)
  const body = paced(900, answer, ctx.requestId)
  const end = body.at(-1)?.at ?? 900
  const chat = [
    connected(10, { requestId: ctx.requestId, threadId: ctx.threadId, model: ctx.model }),
    ...(ctx.firstTurn ? [titleFrame(20, { requestId: ctx.requestId, title: titleFromQuestion(question) })] : []),
    ...body,
    done(end + 60, { requestId: ctx.requestId, model: ctx.model }),
  ]
  return { chat, run: [], answer }
}
