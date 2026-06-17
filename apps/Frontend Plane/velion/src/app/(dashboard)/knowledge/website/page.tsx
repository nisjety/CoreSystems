import { WebsitePaneClient } from '@/components/knowledge/panes/WebsitePaneClient';
import { getKnowledgeSources } from '@/app/api/knowledge/_lib/knowledge-data';

export const dynamic = 'force-dynamic';

export default async function KnowledgeWebsitePage() {
  const { sources } = await getKnowledgeSources();
  return <WebsitePaneClient sources={sources} />;
}
