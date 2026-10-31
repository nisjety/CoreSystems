import { ConsultancyPage } from "@/features/services/consultancy-page";
import { consultancySeoEn } from "@/features/services/service-seo-en";
import { createPageMetadata } from "@/lib/seo";

export const metadata = createPageMetadata({
  title: consultancySeoEn.title,
  description: consultancySeoEn.description,
  path: consultancySeoEn.canonical,
  image: consultancySeoEn.image,
  imageAlt: consultancySeoEn.name,
  locale: "en",
});

export default function Page() {
  return <ConsultancyPage locale="en" />;
}
