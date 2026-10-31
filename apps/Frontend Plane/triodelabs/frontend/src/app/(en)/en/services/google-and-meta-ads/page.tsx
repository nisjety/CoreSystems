import { PaidAdsPage } from "@/features/services/paid-ads-page";
import { paidAdsSeoEn } from "@/features/services/service-seo-en";
import { createPageMetadata } from "@/lib/seo";

export const metadata = createPageMetadata({
  title: paidAdsSeoEn.title,
  description: paidAdsSeoEn.description,
  path: paidAdsSeoEn.canonical,
  image: paidAdsSeoEn.image,
  imageAlt: paidAdsSeoEn.name,
  locale: "en",
});

export default function Page() {
  return <PaidAdsPage locale="en" />;
}
