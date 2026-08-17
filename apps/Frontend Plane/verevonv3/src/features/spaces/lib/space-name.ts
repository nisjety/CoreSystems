/**
 * The name a personal Space is provisioned with, server-side (Application
 * Plane's `ensurePersonalSpace`). It is stored data rather than UI copy, so it
 * arrives in English no matter which locale the reader has chosen.
 */
const PROVISIONED_PERSONAL_SPACE_NAME = 'Personal Space'

export type NamedSpace = {
  readonly name: string
  readonly kind: string
}

/**
 * A Space's name as it should read to this user.
 *
 * Only the untouched provisioning default is translated. The moment a name
 * differs from it — because a person named the room — those are their words and
 * are shown verbatim in every locale. Translating on `kind === 'personal'`
 * alone would silently replace a deliberate name with a generic one, which is
 * worse than leaving one English string on screen.
 *
 * Shared between the Space header and the Core Sidebar so a room cannot appear
 * under two different names in the same window.
 */
export function spaceDisplayName(
  space: NamedSpace,
  tr: (no: string, en: string) => string,
): string {
  const name = space.name?.trim() ?? ''
  if (space.kind === 'personal' && name === PROVISIONED_PERSONAL_SPACE_NAME) {
    return tr('Personlig rom', 'Personal Space')
  }
  return name
}
