import { redirect } from 'next/navigation';
import type { Metadata } from 'next';

export const metadata: Metadata = {
  title: 'Aqencia',
  description: 'Aqencia workspace',
};

export default function RootPage() {
  redirect('/dashboard');
}
