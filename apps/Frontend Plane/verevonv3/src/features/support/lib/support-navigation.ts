export type SupportSurface = 'conversations' | 'tickets' | 'outbound'

export function parseSupportSurface(value: string | string[] | null | undefined): SupportSurface {
  if (Array.isArray(value)) return parseSupportSurface(value[0])
  if (value === 'tickets' || value === 'review') return 'tickets'
  if (value === 'outbound') return 'outbound'
  return 'conversations'
}

export function supportSurfaceHref(surface: SupportSurface): string {
  if (surface === 'tickets') return '/support?surface=tickets'
  if (surface === 'outbound') return '/support?surface=outbound'
  return '/support'
}
