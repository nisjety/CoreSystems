import type { Metadata } from "next";
import { PlatformPage } from "@/features/platform/platform-page";
import { platformSeo } from "@/features/platform/platform-content";
import { createPageMetadata } from "@/lib/seo";

export const metadata: Metadata = createPageMetadata({
  ...platformSeo.nb,
  path: "/plattform",
});

export default function Page() {
  return <PlatformPage locale="nb" />;
}
