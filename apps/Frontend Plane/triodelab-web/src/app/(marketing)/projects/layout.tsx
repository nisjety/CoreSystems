import type { Metadata } from 'next'

export const metadata: Metadata = {
  title: 'Prosjekter & Case Studies | Triodelab',
  description: 'Utforsk Triodelabs prosjektportefølje: QualAI for WCAG og lenkevalidering, Væro AI-værassistent, Agenci eiendomsplattform og Domain Tracking System.',
}

export default function Layout({ children }: { children: React.ReactNode }) {
  return <>{children}</>
}
