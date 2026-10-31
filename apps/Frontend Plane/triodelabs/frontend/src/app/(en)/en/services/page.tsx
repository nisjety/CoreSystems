import { ServicesPage } from "@/features/services/services-page";
import { servicesIndexSeoEn } from "@/features/services/service-seo-en";
import { createPageMetadata } from "@/lib/seo";

export const metadata = createPageMetadata({
  title: servicesIndexSeoEn.title,
  description: servicesIndexSeoEn.description,
  path: servicesIndexSeoEn.canonical,
  imageAlt: "TriodeLab services",
  locale: "en",
});

export default function Page() {
  return <ServicesPage locale="en" />;
}
