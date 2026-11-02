import type { Metadata } from "next";
import { HomePage } from "@/features/home/home-page";
import { createPageMetadata, siteCopy } from "@/lib/seo";

export const metadata: Metadata = createPageMetadata({
  title: siteCopy.en.title,
  description: siteCopy.en.description,
  path: "/",
  locale: "en",
});

export default function Page() {
  return <HomePage locale="en" />;
}
