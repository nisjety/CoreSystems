import type { Metadata } from "next";
import { AboutPage } from "@/features/about/about-page";
import { aboutSeo } from "@/features/about/about-seo";
import { createPageMetadata } from "@/lib/seo";

export const metadata: Metadata = createPageMetadata({
  ...aboutSeo.nb,
  path: "/om-oss",
});

export default function Page() {
  return <AboutPage locale="nb" />;
}
