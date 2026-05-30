import type { Metadata } from 'next'

export const metadata: Metadata = {
  title: 'Triodelab – Digital Transformasjon',
  description: 'Triodelab leverer digital transformasjon og teknisk rådgivning med AI-utvikling, systemarkitektur og moderne webapplikasjoner. Oslo, Norge.',
}

export default function Layout({ children }: { children: React.ReactNode }) {
  return <>{children}</>
}
