import { OrganicTrafficPage } from "@/features/services/organic-traffic-page";
import { organicTrafficSeo } from "@/features/services/service-seo";
import { createPageMetadata } from "@/lib/seo";

export const metadata = createPageMetadata({
  title: organicTrafficSeo.title,
  description: organicTrafficSeo.description,
  path: organicTrafficSeo.canonical,
  image: organicTrafficSeo.image,
  imageAlt: organicTrafficSeo.name,
});

export default function Page() {
  return <OrganicTrafficPage locale="nb" />;
}
