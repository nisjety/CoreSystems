import { OrganicTrafficPage } from "@/features/services/organic-traffic-page";
import { organicTrafficSeoEn } from "@/features/services/service-seo-en";
import { createPageMetadata } from "@/lib/seo";

export const metadata = createPageMetadata({
  title: organicTrafficSeoEn.title,
  description: organicTrafficSeoEn.description,
  path: organicTrafficSeoEn.canonical,
  image: organicTrafficSeoEn.image,
  imageAlt: organicTrafficSeoEn.name,
  locale: "en",
});

export default function Page() {
  return <OrganicTrafficPage locale="en" />;
}
