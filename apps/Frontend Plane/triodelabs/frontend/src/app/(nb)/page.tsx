import type { Metadata } from "next";
import { HomePage } from "@/features/home/home-page";
import { createPageMetadata, siteCopy } from "@/lib/seo";

export const metadata: Metadata = createPageMetadata({
  title: siteCopy.nb.title,
  description: siteCopy.nb.description,
  path: "/",
});

export default function Page() {
  return <HomePage locale="nb" />;
}
