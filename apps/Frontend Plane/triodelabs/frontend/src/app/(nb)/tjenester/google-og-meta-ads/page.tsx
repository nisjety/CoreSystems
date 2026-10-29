import { PaidAdsPage } from "@/features/services/paid-ads-page";
import { paidAdsSeo } from "@/features/services/service-seo";
import { createPageMetadata } from "@/lib/seo";

export const metadata = createPageMetadata({
  title: paidAdsSeo.title,
  description: paidAdsSeo.description,
  path: paidAdsSeo.canonical,
  image: paidAdsSeo.image,
  imageAlt: paidAdsSeo.name,
});

export default function Page() {
  return <PaidAdsPage locale="nb" />;
}
