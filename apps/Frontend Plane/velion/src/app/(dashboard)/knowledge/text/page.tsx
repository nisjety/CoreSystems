import { TextPaneClient } from '@/components/knowledge/panes/TextPaneClient';
import { getKnowledgeDocuments } from '@/app/api/knowledge/_lib/knowledge-data';

export const dynamic = 'force-dynamic';

export default async function KnowledgeTextPage() {
  const { documents } = await getKnowledgeDocuments({ limit: 500, type: 'text' });
  return <TextPaneClient documents={documents} />;
}
