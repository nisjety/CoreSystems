import { DocumentDrawerLauncher } from '@/components/knowledge/DocumentDrawerLauncher';

export const dynamic = 'force-dynamic';

// Wave 11 §5 — clicking a document row deep-links here. The drawer is
// a client component because edits are interactive; the page just
// renders a launcher that opens the drawer immediately.
export default async function KnowledgeDocumentDetailPage({
  params,
}: {
  params: Promise<{ id: string }>;
}) {
  const { id } = await params;
  return <DocumentDrawerLauncher documentId={id} />;
}
