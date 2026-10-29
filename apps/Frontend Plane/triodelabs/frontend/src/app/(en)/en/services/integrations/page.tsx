import { IntegrationsPage } from "@/features/services/integrations-page";
import { integrationsSeoEn } from "@/features/services/service-seo-en";
import { createPageMetadata } from "@/lib/seo";

export const metadata = createPageMetadata({
  title: integrationsSeoEn.title,
  description: integrationsSeoEn.description,
  path: integrationsSeoEn.canonical,
  image: integrationsSeoEn.image,
  imageAlt: integrationsSeoEn.name,
  locale: "en",
});

export default function Page() {
  return <IntegrationsPage locale="en" />;
}
