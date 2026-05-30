import type { Metadata } from 'next';
import LoadingPage from '@/components/marketing/loading/LoadingPage';

export const metadata: Metadata = {
  title: 'Triodelab – Digital transformasjon som faktisk fungerer',
  description:
    'Triodelab leverer skreddersydde digitale løsninger innen AI-utvikling, systemarkitektur og automasjon.',
};

export default function RootPage() {
  return <LoadingPage />;
}
