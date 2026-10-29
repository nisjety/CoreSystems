import type { Metadata } from 'next'

export const metadata: Metadata = {
  title: 'Om Triodelab – Prosjekter & Case Studies',
  description: 'Møt Triodelab gjennom våre prosjekter: QualAI, Væro AI-assistent, Agenci eiendomsplattform og mer – innovative digitale løsninger bygget med moderne teknologi.',
}

export default function Layout({ children }: { children: React.ReactNode }) {
  return <>{children}</>
}
