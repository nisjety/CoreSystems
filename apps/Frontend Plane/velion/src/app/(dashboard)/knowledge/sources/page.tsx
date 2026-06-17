import { redirect } from 'next/navigation';

// Wave 11: /knowledge/sources is now /knowledge/website. We keep this
// route as a permanent redirect for any external links / browser
// history pointing at the legacy path.
export default function KnowledgeSourcesRedirectPage() {
  redirect('/knowledge/website');
}
