import type { Metadata } from "next";
import { ContactPage } from "@/features/contact/contact-page";
import { contactSeo } from "@/features/contact/contact-page-content";
import { getContactSourceFromSlug } from "@/features/contact/contact-sources";
import { isContactConfigured } from "@/lib/contact/config";
import { createPageMetadata } from "@/lib/seo";

export const dynamic = "force-dynamic";

export const metadata: Metadata = createPageMetadata({
  ...contactSeo.en,
  path: "/kontakt",
  locale: "en",
});

export default async function Page({
  searchParams,
}: {
  searchParams: Promise<{ service?: string | string[] }>;
}) {
  const { service } = await searchParams;
  return (
    <ContactPage
      configured={isContactConfigured()}
      locale="en"
      source={getContactSourceFromSlug(service, "en")}
    />
  );
}
