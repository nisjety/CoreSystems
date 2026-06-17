import { notFound } from 'next/navigation';
import { WikiListClient } from '@/components/knowledge/WikiListClient';
import { KNOWLEDGE_FEATURE_FLAGS } from '@/lib/knowledge/feature-flags';

export const dynamic = 'force-dynamic';

export default async function KnowledgeWikiListPage() {
  if (!KNOWLEDGE_FEATURE_FLAGS.wiki) {
    notFound();
  }
  return <WikiListClient />;
}
