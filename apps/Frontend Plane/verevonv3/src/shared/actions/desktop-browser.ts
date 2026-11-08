import { z } from 'zod'

const target = z.string().regex(/^t[0-9a-f]{16}$/)

const desktopBrowserStep = z.discriminatedUnion('kind', [
  z.object({ kind: z.literal('navigate'), url: z.string().min(1).max(2000) }),
  z.object({ kind: z.literal('click'), target }),
  z.object({ kind: z.literal('fill'), target, text: z.string().min(1).max(500) }),
  z.object({ kind: z.literal('press'), key: z.literal('Enter') }),
  z.object({ kind: z.literal('scroll'), deltaY: z.number().int().min(-1800).max(1800) }),
  z.object({ kind: z.literal('wait'), ms: z.number().int().min(250).max(5000) }),
])

export const desktopBrowserSuggestionSchema = z.discriminatedUnion('done', [
  z.object({ done: z.literal(false), action: desktopBrowserStep }),
  z.object({
    done: z.literal(true),
    action: z.null(),
    outcome: z.enum(['completed', 'blocked', 'incomplete']),
    answer: z.string().trim().min(1).max(4000),
  }),
])
