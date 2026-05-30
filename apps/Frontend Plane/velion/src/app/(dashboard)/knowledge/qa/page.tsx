import { QnAPaneClient } from '@/components/knowledge/panes/QnAPaneClient';
import { listQnAEntries } from '@/components/knowledge/server/qa-store';

export const dynamic = 'force-dynamic';

export default async function KnowledgeQnAPage() {
  const entries = await listQnAEntries();
  return <QnAPaneClient entries={entries} />;
}
