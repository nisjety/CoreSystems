import { CommercePage } from "@/features/services/commerce-page";
import { commerceSeo } from "@/features/services/service-seo";
import { createPageMetadata } from "@/lib/seo";

export const metadata = createPageMetadata({
  title: commerceSeo.title,
  description: commerceSeo.description,
  path: commerceSeo.canonical,
  image: commerceSeo.image,
  imageAlt: commerceSeo.name,
});

export default function Page() {
  return <CommercePage locale="nb" />;
}
