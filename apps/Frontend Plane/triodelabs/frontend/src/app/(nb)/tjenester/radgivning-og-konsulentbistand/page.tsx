import { ConsultancyPage } from "@/features/services/consultancy-page";
import { consultancySeo } from "@/features/services/service-seo";
import { createPageMetadata } from "@/lib/seo";

export const metadata = createPageMetadata({
  title: consultancySeo.title,
  description: consultancySeo.description,
  path: consultancySeo.canonical,
  image: consultancySeo.image,
  imageAlt: consultancySeo.name,
});

export default function Page() {
  return <ConsultancyPage locale="nb" />;
}
