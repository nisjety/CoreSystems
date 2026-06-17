import { requireEdgeUser } from '@/components/auth/lib/edge-session';

import { SearchPageClient } from './SearchPageClient';

type SearchPageProps = {
  searchParams?: Promise<{ q?: string }>;
};

export const dynamic = 'force-dynamic';

export default async function SearchPage({ searchParams }: SearchPageProps) {
  // G28: read user from edge-gate-stamped header instead of round-tripping
  // to auth-core again. requireEdgeUser falls back to getServerSession()
  // when the header is missing (gate fail-open path).
  await requireEdgeUser('/search');

  const resolvedSearchParams = searchParams ? await searchParams : undefined;
  const initialQuery = resolvedSearchParams?.q?.trim() || null;

  return <SearchPageClient key={initialQuery ?? 'empty'} initialQuery={initialQuery} />;
}
