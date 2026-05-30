import type { Metadata } from 'next'

export const metadata: Metadata = {
  title: 'Implementering & Utvikling | Triodelab',
  description: 'Triodelab implementerer skreddersydde digitale løsninger – fra AI-integrasjon og mikrotjenester til skalerbar systemarkitektur og moderne webutvikling.',
}

export default function Layout({ children }: { children: React.ReactNode }) {
  return <>{children}</>
}
