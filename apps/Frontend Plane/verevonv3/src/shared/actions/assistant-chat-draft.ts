import { z } from 'zod'

const maxDraftLength = 2000

/** An explicit, local request to prepare editable text in Chat. */
export const assistantChatDraftActionSchema = z.strictObject({
  type: z.literal('assistant.chat.draft'),
  href: z.literal('/chat'),
  text: z.string().trim().min(1).max(maxDraftLength),
  newChat: z.boolean(),
})

export type AssistantChatDraftAction = z.infer<typeof assistantChatDraftActionSchema>

const norwegian = /^(?:(?:kan|kunne)\s+du\s+)?(?:åpne|(?:gå|ga|gaa)\s+(?:til|inn\s+i))\s+(?:(ny)\s+)?chat(?:ten)?\s+og\s+(?:skrive|skriv|taste|tast|lage\s+et\s+utkast|lag\s+et\s+utkast)(?=\s|:)[\s:]+(?:inn\s+)?(?:at\s+|dette:\s*)?([\s\S]+)$/iu
const english = /^(?:(?:can|could)\s+you\s+)?(?:open|go\s+to)\s+(?:(?:a|the)\s+)?(?:(new)\s+)?chat\s+and\s+(?:write|type|draft)(?=\s|:)[\s:]+(?:that\s+|this:\s*)?([\s\S]+)$/iu

export function resolveAssistantChatDraft(prompt: string): AssistantChatDraftAction | null {
  const input = prompt.trim()
  if (input.length > maxDraftLength) return null
  const match = norwegian.exec(input) ?? english.exec(input)
  if (!match) return null

  const result = assistantChatDraftActionSchema.safeParse({
    type: 'assistant.chat.draft',
    href: '/chat',
    text: match[2],
    newChat: Boolean(match[1]),
  })
  return result.success ? result.data : null
}
