import { redirect } from 'next/navigation';
import { Suspense } from 'react';
import { LoadingState } from '@/components/overview/ui/LoadingState';
import { LeadsView } from '@/components/overview/LeadsView';

export const dynamic = 'force-dynamic';
export const revalidate = 20;

async function getSession() {
  const res = await fetch('http://localhost:3000/api/auth/session', { cache: 'no-store' }).catch(() => null);
  return res?.ok ? res.json() : null;
}

export default async function Page() {
  const session = await getSession();

  if (!session?.user?.id) {
    redirect('/auth/login');
  }

  return (
    <Suspense fallback={<LoadingState />}>
      <LeadsView userId={session.user.id} />
    </Suspense>
  );
}
