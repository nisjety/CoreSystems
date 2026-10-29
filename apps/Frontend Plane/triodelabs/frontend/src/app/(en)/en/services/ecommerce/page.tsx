import { CommercePage } from "@/features/services/commerce-page";
import { commerceSeoEn } from "@/features/services/service-seo-en";
import { createPageMetadata } from "@/lib/seo";

export const metadata = createPageMetadata({
  title: commerceSeoEn.title,
  description: commerceSeoEn.description,
  path: commerceSeoEn.canonical,
  image: commerceSeoEn.image,
  imageAlt: commerceSeoEn.name,
  locale: "en",
});

export default function Page() {
  return <CommercePage locale="en" />;
}
