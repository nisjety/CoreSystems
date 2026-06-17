import { FilesPaneClient } from '@/components/knowledge/panes/FilesPaneClient';
import { getKnowledgeDocuments } from '@/app/api/knowledge/_lib/knowledge-data';

export const dynamic = 'force-dynamic';

export default async function KnowledgeFilesPage() {
  // Wave 11 §2 — real read from documents-service. `getKnowledgeDocuments`
  // already swallows ECONNREFUSED (Phase 1), so an offline backend
  // surfaces as an empty list rather than a hard 500.
  const { documents } = await getKnowledgeDocuments({ limit: 500 });

  // Files = anything that isn't website-crawl / text / qa / integration.
  // documents-service stamps type='file' on uploads from /api/knowledge/documents.
  const fileDocs = documents.filter((doc) => {
    const isCrawl = doc.source.startsWith('website-crawl:');
    const isIntegration = doc.source.startsWith('integration:');
    return !isCrawl && !isIntegration && doc.type !== 'text' && doc.type !== 'qa';
  });

  return <FilesPaneClient documents={fileDocs} />;
}
