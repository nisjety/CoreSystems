import { redirect } from 'next/navigation';

// Wave 11: renamed to plain "Integrations" in the sub-nav.
export default function KnowledgeApiIntegrationsRedirectPage() {
  redirect('/knowledge/integrations');
}
