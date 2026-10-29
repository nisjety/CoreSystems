import { IntegrationsPage } from "@/features/services/integrations-page";
import { integrationsSeo } from "@/features/services/service-seo";
import { createPageMetadata } from "@/lib/seo";

export const metadata = createPageMetadata({
  title: integrationsSeo.title,
  description: integrationsSeo.description,
  path: integrationsSeo.canonical,
  image: integrationsSeo.image,
  imageAlt: integrationsSeo.name,
});

export default function Page() {
  return <IntegrationsPage locale="nb" />;
}
