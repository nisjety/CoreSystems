import type { Metadata } from 'next'

export const metadata: Metadata = {
  title: 'Digital Transformasjon som Faktisk Fungerer | Triodelab',
  description: 'Vi leverer digital transformasjon og teknisk rådgivning med hands-on erfaring og full åpenhet. Prosjekter, implementering, vekst og support.',
}

export default function Layout({ children }: { children: React.ReactNode }) {
  return <>{children}</>
}
