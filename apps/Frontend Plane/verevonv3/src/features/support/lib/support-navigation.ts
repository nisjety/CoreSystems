export type SupportSurface = 'conversations' | 'tickets' | 'outbound' | 'remote'

export function parseSupportSurface(value: string | string[] | null | undefined): SupportSurface {
  if (Array.isArray(value)) return parseSupportSurface(value[0])
  if (value === 'tickets' || value === 'review') return 'tickets'
  if (value === 'outbound') return 'outbound'
  if (value === 'remote') return 'remote'
  return 'conversations'
}

export function supportSurfaceHref(surface: SupportSurface): string {
  if (surface === 'tickets') return '/support?surface=tickets'
  if (surface === 'outbound') return '/support?surface=outbound'
  if (surface === 'remote') return '/support?surface=remote'
  return '/support'
}
