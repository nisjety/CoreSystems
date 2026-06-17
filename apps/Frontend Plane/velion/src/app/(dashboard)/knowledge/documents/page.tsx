import { redirect } from 'next/navigation';

// Wave 11: /knowledge/documents was the unified-list view. It's now
// split across /knowledge/files (uploaded docs) and /knowledge/text
// (pasted snippets). The Files page is the closer landing point for
// the legacy use-case.
export default function KnowledgeDocumentsRedirectPage() {
  redirect('/knowledge/files');
}
