import { notFound } from 'next/navigation';
import { WikiPageEditor } from '@/components/knowledge/WikiPageEditor';
import { KNOWLEDGE_FEATURE_FLAGS } from '@/lib/knowledge/feature-flags';
import { resolveChatActor } from '@/app/api/chat/_lib/session-store';
import {
  getWikiPage,
  getCurrentVersion,
  getBacklinks,
  listSourceLogs,
} from '@/lib/knowledge/wiki-client';

export const dynamic = 'force-dynamic';

export default async function KnowledgeWikiDetailPage({
  params,
}: {
  params: Promise<{ id: string }>;
}) {
  if (!KNOWLEDGE_FEATURE_FLAGS.wiki) {
    notFound();
  }

  const { id } = await params;

  let actor;
  try {
    actor = await resolveChatActor();
  } catch {
    notFound();
  }

  const base = { orgId: actor.orgId, userId: actor.userId };
  const [page, version, backlinks, sourceLog] = await Promise.all([
    getWikiPage(id, base),
    getCurrentVersion(id, base),
    getBacklinks(id, base),
    listSourceLogs(id, base),
  ]);

  if (!page) {
    notFound();
  }

  return (
    <WikiPageEditor
      page={page}
      version={version}
      backlinks={backlinks}
      sourceLog={sourceLog}
    />
  );
}
