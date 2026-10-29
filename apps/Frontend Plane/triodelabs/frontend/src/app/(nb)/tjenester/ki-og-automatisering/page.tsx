import { AiAutomationPage } from "@/features/services/ai-automation-page";
import { aiAutomationSeo } from "@/features/services/service-seo";
import { createPageMetadata } from "@/lib/seo";

export const metadata = createPageMetadata({
  title: aiAutomationSeo.title,
  description: aiAutomationSeo.description,
  path: aiAutomationSeo.canonical,
  image: aiAutomationSeo.image,
  imageAlt: aiAutomationSeo.name,
});

export default function Page() {
  return <AiAutomationPage locale="nb" />;
}
