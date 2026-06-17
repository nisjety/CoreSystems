import { redirect } from 'next/navigation'

// Wave 11: /knowledge/data was a legacy stub. The overview page is now
// the canonical entry point — Files / Text / Website / Q&A live as
// dedicated sub-pages.
export default function KnowledgeDataPage() {
  redirect('/knowledge')
}
