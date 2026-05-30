import type { Metadata } from 'next'
import LoadingPage from '@/components/marketing/loading/LoadingPage';

export const metadata: Metadata = {
  title: 'Triodelab – Digital Transformasjon som Faktisk Fungerer',
  description: 'Trio dé Lab: din portal til Triodelabs digitale løsninger – Agenci AI chatbot, QualAI web management og skreddersydd digital transformasjon.',
}

export default function Loading() {
  return <LoadingPage />;
}
