import type { Metadata } from "next";
import { AboutPage } from "@/features/about/about-page";
import { aboutSeo } from "@/features/about/about-seo";
import { createPageMetadata } from "@/lib/seo";

export const metadata: Metadata = createPageMetadata({
  ...aboutSeo.en,
  path: "/om-oss",
  locale: "en",
});

export default function Page() {
  return <AboutPage locale="en" />;
}
