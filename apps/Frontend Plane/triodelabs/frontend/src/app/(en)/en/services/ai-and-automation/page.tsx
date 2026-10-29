import { AiAutomationPage } from "@/features/services/ai-automation-page";
import { aiAutomationSeoEn } from "@/features/services/service-seo-en";
import { createPageMetadata } from "@/lib/seo";

export const metadata = createPageMetadata({
  title: aiAutomationSeoEn.title,
  description: aiAutomationSeoEn.description,
  path: aiAutomationSeoEn.canonical,
  image: aiAutomationSeoEn.image,
  imageAlt: aiAutomationSeoEn.name,
  locale: "en",
});

export default function Page() {
  return <AiAutomationPage locale="en" />;
}
