import type { Metadata } from "next";
import { ContactPage } from "@/features/contact/contact-page";
import { contactSeo } from "@/features/contact/contact-page-content";
import { getContactSourceFromSlug } from "@/features/contact/contact-sources";
import { isContactConfigured } from "@/lib/contact/config";
import { createPageMetadata } from "@/lib/seo";

export const dynamic = "force-dynamic";

export const metadata: Metadata = createPageMetadata({
  ...contactSeo.nb,
  path: "/kontakt",
});

export default async function Page({
  searchParams,
}: {
  searchParams: Promise<{ tjeneste?: string | string[] }>;
}) {
  const { tjeneste } = await searchParams;
  return (
    <ContactPage
      configured={isContactConfigured()}
      locale="nb"
      source={getContactSourceFromSlug(tjeneste, "nb")}
    />
  );
}
