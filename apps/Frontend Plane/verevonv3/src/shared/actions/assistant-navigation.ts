import { z } from 'zod'

/**
 * A local, user-initiated presentation/navigation action. This is not part of
 * the model-executable registry and grants no access to any destination.
 * Only the fixed internal routes may be shown or opened by the guide.
 */
export const assistantNavigationActionSchema = z.strictObject({
  type: z.literal('assistant.navigation.guide'),
  href: z.enum(['/dashboard', '/chat', '/spaces', '/support', '/engagement', '/agents', '/knowledge', '/insights']),
  mode: z.enum(['point', 'navigate']),
})

export type AssistantNavigationAction = z.infer<typeof assistantNavigationActionSchema>

/** The model can name search terms or refer to the current page. Browser
 * destinations come from the visible session or the search service. */
export const assistantTaskProposalSchema = z.union([
  assistantNavigationActionSchema,
  z.strictObject({ type: z.literal('assistant.browser.current') }),
  z.strictObject({
    type: z.literal('assistant.browser.search'),
    query: z.string().trim().min(2).max(160).regex(/^[\p{L}\p{N}\p{M} &'’.,()+-]+$/u),
  }),
  z.strictObject({ type: z.literal('unsupported') }),
])

export type AssistantTaskProposal = z.infer<typeof assistantTaskProposalSchema>
