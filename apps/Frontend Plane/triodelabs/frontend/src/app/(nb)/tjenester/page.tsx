import { ServicesPage } from "@/features/services/services-page";
import { servicesIndexSeo } from "@/features/services/service-seo";
import { createPageMetadata } from "@/lib/seo";

export const metadata = createPageMetadata({
  title: servicesIndexSeo.title,
  description: servicesIndexSeo.description,
  path: servicesIndexSeo.canonical,
  imageAlt: "TriodeLab tjenester",
});

export default function Page() {
  return <ServicesPage locale="nb" />;
}
