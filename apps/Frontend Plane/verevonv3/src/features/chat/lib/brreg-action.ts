import type { ChatAction } from '@/shared/api/chat-client'

export const BRREG_LOOKUP_ACTION: ChatAction = {
  id: 'brreg.lookup_organization',
  name: 'Brreg organization lookup',
  kind: 'tool',
}

function normalizeForRegistryMatch(value: string): string {
  return value
    .trim()
    .toLowerCase()
    .replaceAll('ø', 'o')
    .replaceAll('ö', 'o')
    .replaceAll('å', 'a')
    .replaceAll('æ', 'ae')
}

export function shouldAttachBrregLookupAction(text: string): boolean {
  const normalized = normalizeForRegistryMatch(text)
  if (!normalized) return false

  return [
    'brreg',
    'bronnoysund',
    'enhetsregisteret',
    'foretaksregisteret',
    'organisasjonsnummer',
    'org.nr',
    'org nr',
    'org number',
    'orgnummer',
    'organization number',
    'organisation number',
    'norwegian business registry',
    'norwegian company registry',
    'norwegian organization registry',
    'norwegian organisation registry',
  ].some((token) => normalized.includes(token))
}

export function withBrregLookupAction(
  actions: readonly ChatAction[],
  text: string,
): ChatAction[] {
  const base = [...actions]
  if (
    shouldAttachBrregLookupAction(text) &&
    !base.some((action) => action.id === BRREG_LOOKUP_ACTION.id)
  ) {
    return [...base, BRREG_LOOKUP_ACTION]
  }
  return base
}
