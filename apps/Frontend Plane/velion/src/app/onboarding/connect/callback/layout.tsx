import type { Metadata } from 'next'

export const metadata: Metadata = {
  title: 'Kobler til konto – Triodelab',
  description: 'Behandler Microsoft OAuth-godkjenning for å koble kontoen din til Triodelab-plattformen. Du blir straks omdirigert.',
}

export default function Layout({ children }: { children: React.ReactNode }) {
  return <>{children}</>
}
