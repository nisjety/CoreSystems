/** Solid variants of the rail's existing metaphors. Interior cutouts remain
 * readable instead of applying fill to Lucide's open outline paths. */
const paths: Record<string, string> = {
  overview: 'M12 2 2 10v2h2v9h6v-7h4v7h6v-9h2v-2L12 2Z',
  messages: 'M12 2a10 10 0 0 0-8.5 15.3L2 22l4.7-1.5A10 10 0 1 0 12 2ZM7 11a1 1 0 1 1 0 2 1 1 0 0 1 0-2Zm5 0a1 1 0 1 1 0 2 1 1 0 0 1 0-2Zm5 0a1 1 0 1 1 0 2 1 1 0 0 1 0-2Z',
  spaces: 'M9 2a4 4 0 1 0 0 8 4 4 0 0 0 0-8Zm9 2a3 3 0 1 0 0 6 3 3 0 0 0 0-6ZM2 20v-3a6 6 0 0 1 12 0v3H2Zm14 0v-3a8 8 0 0 0-1-4h3a4 4 0 0 1 4 4v3h-6Z',
  inbox: 'M5 3h14l3 9v7a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2v-7l3-9Zm1.5 2L4 12h5v3h6v-3h5l-2.5-7h-11Z',
  marketing: 'M3 2h18v15h-8v3l4 2h-4l-1-1-1 1H7l4-2v-3H3V2Zm2 3v9h14V5H5Z',
  agents: 'M7 3h10l2 7H5l2-7ZM2 11h20v2H2v-2Zm4 4a4 4 0 1 0 3.87 5H14.13A4 4 0 1 0 14 18h-4a4 4 0 0 0-4-3Zm0 2a2 2 0 1 0 0 4 2 2 0 0 0 0-4Zm12 0a2 2 0 1 0 0 4 2 2 0 0 0 0-4Z',
  studio: 'M3 3h7v7H3V3Zm11 1h7v7h-7V4ZM4 14h7v7H4v-7Z',
  social: 'M5 2h2v3h10V2h2v3h1a2 2 0 0 1 2 2v13a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2h1V2Zm0 8v2h14v-2H5Zm1 5v2h2v-2H6Zm5 0v2h2v-2h-2Zm5 0v2h2v-2h-2Z',
  ingestions: 'M12 2a10 10 0 1 0 10 10h-2a8 8 0 1 1-8-8V2Zm0 5a5 5 0 1 0 5 5h-2a3 3 0 1 1-3-3V7Zm1 4 8-8 1 1-8 8v1h-3v-3h2v1Z',
  knowledge: 'M2 3h3c3 0 5 1 6 3v16c-2-2-4-3-9-3V3Zm20 0h-3c-3 0-5 1-6 3v16c2-2 4-3 9-3V3Z',
  insights: 'M2 3h2v17h18v2H2V3Zm4 9h4v6H6v-6Zm6-7h4v13h-4V5Zm6 4h4v9h-4V9Z',
  settings: 'm10 1-1 3-3 1-3-1-2 4 2 2v4l-2 2 2 4 3-1 3 1 1 3h4l1-3 3-1 3 1 2-4-2-2v-4l2-2-2-4-3 1-3-1-1-3h-4Zm2 7a4 4 0 1 0 0 8 4 4 0 0 0 0-8Z',
  account: 'M12 2a10 10 0 1 0 0 20 10 10 0 0 0 0-20Zm0 3a3 3 0 1 0 0 6 3 3 0 0 0 0-6Zm-6 13h12a6 6 0 0 0-12 0Z',
}

export function FilledRailIcon(props: { name: string }) {
  return <svg class="core-rail-filled-icon" width="20" height="20" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path fill-rule="evenodd" clip-rule="evenodd" d={paths[props.name] ?? paths.messages} /></svg>
}
